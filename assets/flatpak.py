#!/usr/bin/env python3
"""Host-mode adapter based on the user's ~/scripts/run.sh Flatpak dispatch."""
import configparser
import os
import shutil
import subprocess
import sys


def main():
    kind, filename, *arguments = sys.argv[1:]
    executable = shutil.which("flatpak")
    if not executable:
        raise RuntimeError("Flatpak is not installed")

    import gi
    gi.require_version("Flatpak", "1.0")
    from gi.repository import Flatpak, Gio

    if kind == "flatpak":
        bundle = Flatpak.BundleRef.new(Gio.File.new_for_path(filename))
        if bundle.get_kind() != Flatpak.RefKind.APP:
            raise RuntimeError("runtime bundles are not applications")
        ref = bundle.format_ref()
    else:
        data = configparser.ConfigParser(interpolation=None)
        data.read(filename)
        entry = data["Flatpak Ref"]
        if entry.getboolean("IsRuntime", fallback=False):
            raise RuntimeError("runtime references are not applications")
        ref = Flatpak.Ref.parse("app/{}/{}/{}".format(
            entry["Name"], entry.get("Arch", Flatpak.get_default_arch()),
            entry.get("Branch", "stable"),
        )).format_ref()

    installation = "--user"
    for scope in ("--user", "--system"):
        if subprocess.run([executable, "info", scope, ref],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0:
            installation = scope
            break
    else:
        print("This application requires a Flatpak installation.", flush=True)
        # Keep Flatpak's own interactive confirmation; never pass --assumeyes.
        option = "--bundle" if kind == "flatpak" else "--from"
        subprocess.run([executable, "install", "--user", option, filename], check=True)

    os.execv(executable, [executable, "run", installation, ref, *arguments])


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print("Glyph Flatpak:", error, file=sys.stderr)
        sys.exit(1)
