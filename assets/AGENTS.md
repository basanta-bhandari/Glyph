# Agent guide to Glyph

## Scope and first steps

This guide is for agents executing **inside Glyph**. Glyph is a Rust terminal
workspace running on a host OS. Your filesystem and process environment are the
Glyph sandbox, not the host machine. This guide explains the environment; it
does not grant access or permission to perform additional work.

1. Establish your current directory and inspect the assigned project.
2. Read the project's own applicable instructions before changing its files.
3. Stay within the user's task and the resources already exposed to you.
4. If something is unavailable, identify the missing resource or permission.
   Propose the smallest specific change for the user to apply through Glyph.

The default guide is available at `/AGENTS.md` and `/home/AGENTS.md`. Both are
read-only inside confined applications. `GLYPH_AGENT_GUIDE` identifies the root
guide. A user's edits to either guide are preserved across Glyph restarts.

## Filesystem map

| Path | Purpose | Agent access |
| --- | --- | --- |
| `/home` | Persistent Glyph files and linked projects | Work within the assigned task; respect individual link permissions |
| `/home/AGENTS.md` | Default workspace instructions | Read only |
| `/apps` | Application files, including custom applications | Read/execute; do not change installed applications |
| `/configs/init.lua` | Theme, bindings, commands, and integration settings | Read only |
| `/configs/packages.lua` | Selected package manager and command templates | Read only |
| `/configs/links` | Host-resource mappings and their access permissions | Read only; never expand your own access |
| `/configs/apps` | Application kinds, handlers, sandbox/network settings | Read only |
| `/configs/agents` | Agent names and their application definitions | Read only |
| `/configs/themes` | Lua theme customizations | Read only |
| `/configs/handlers` | Trusted launcher helpers | Read only |
| `/system/source` | User-editable Glyph source | Read only to agents; develop in an assigned `/home` project |
| `/system/docs` | Human guides and figures | Read only |
| `/system/builds` | Runtime candidates | Read only; activation is human-only |
| `/tmp` | Private temporary files | Writable, temporary; not a place for final deliverables |
| `/usr`, `/bin`, `/lib*` | Runtime executables and libraries | Read/execute only; availability does not imply host filesystem access |
| `/proc`, `/dev`, `/run`, `/etc` | Restricted process/device/runtime support | Use normally when required; do not probe them to bypass confinement |

`HOME` is `/home`. Glyph does not expose the host user's home, original project
paths, session bus, desktop sockets, or environment credentials automatically.
Do not infer the original host location of a Glyph file. Use its Glyph path.

## Host links and real writes

The user declares links in `/configs/links/*.lua`. A link maps an explicitly
chosen host resource into a Glyph path beneath `/home`.

- A **read** link is read-only. Do not remount it, copy back over its source, or
  seek another route to change the original.
- A **read-write** link changes the actual host files. Treat edits, renames, and
  deletions as real project changes, not disposable simulator operations.
- A **recursive** directory link exposes its descendants.
- A **nonrecursive** directory link exposes only immediate regular files. Nested
  directories are hidden. In launched apps, new files in that shallow directory
  are temporary; only already-mounted read-write files write through. Put new
  deliverables elsewhere under `/home` or request an appropriate recursive link.
- Symlinks and special files are rejected in configured linked trees. Do not
  create symlinks, hard-link aliases, special files, or alternate mounts to evade
  mapping rules or make protected content writable.

A missing path or permission-denied error is a boundary, not a reason to search
the host root, change mappings, or retry without the sandbox.

## Commands and application launching

At the **Glyph prompt**, these commands are available:

```text
help
pwd
ls /home
cd /home/project
cat /home/project/README.md
write /home/note "some text"
mkdir /home/output
copy /home/source /home/output/copy
move /home/source /home/renamed
rm /home/unneeded-file
apps
agents
links
plan /apps/tool.app argument
run /apps/tool.app argument
exec /apps/tool.app argument
agent configured-agent-name
packages
plan install package-name
install package-name
remove package-name
upgrade package-name
```

These are Glyph dispatcher commands. They are **not automatically executables
in an agent's Bash/sh tool**. In particular, shell `exec` is a shell builtin,
not the Glyph application handler. Use the actual interface supplied to you;
do not invent a Glyph RPC endpoint or assume a CLI bridge is installed. Your
agent's ordinary shell tools still operate inside the inherited sandbox.

Glyph's own file commands have no implicit host-shell fallback. `move` and `rm`
currently handle files, not directories. Glyph prompt pipelines and redirections
are not implemented; quoted punctuation is literal. Use your available shell or
file tools for task work, within the same access boundary.

A `.app` is a launchable **file**, not a directory bundle or a particular
programming language. Its payload may be a native executable, interpreted
script, AppImage, or another format with an installed handler. Glyph identifies
the payload or consults configured metadata and passes arguments separately.
An unknown format needs a configured handler/runtime; renaming a file does not
make an unsupported format executable.

`run` and Glyph's `exec` use the same launcher. `plan` inspects an application
without executing it. App dispatch must retain its sandbox and network policy.
Do not use a different launcher as a route to broader privileges.

## Package management

No agent CLI is preinstalled or copied from the host. A human uses `agent install opencode` or `agent install codex` to install the package inside Glyph, then Glyph registers the app and agent configuration only after success. `agent add NAME` registers a custom executable already installed in `/home/.local/bin`. Agents cannot install/register themselves or edit `/configs/agents`. Authentication is provisioned inside Glyph, never imported from host agent state.

Glyph is not tied to pip. The user configures one package manager in
`/configs/packages.lua`, using `glyph.package_manager` with install, remove, and
upgrade templates. The exact `{[pkg}]` placeholder becomes one package argument.
Templates may be argument tables or strings parsed into arguments; Glyph does
not evaluate shell substitutions or pipelines in those strings.

One package is accepted per command. `uninstall` aliases `remove`; `packages`
shows the configured manager rather than pretending to list installed packages.
`plan install package-name` previews a package operation. Respect the native
manager's prompts and the user's installation request.

Host pacman/apt/Homebrew/WinGet configurations need host execution and a human's
interactive approval. Agents cannot grant that approval. Never invoke a host
manager through another mechanism to avoid this rule. A sandboxed manager must
be configured for Glyph-local storage; there is no automatically provisioned
isolated distro or writable host package database. The pacman upgrade example
performs a full system upgrade along with the named package; do not describe it
as an isolated single-package update.

## Persistent storage and encryption

Glyph's default backend is a real FUSE-mounted virtual filesystem backed by
`workspace.glyphdisk`. File contents are stored in individually compressed 64 KiB
chunks; normal reads decompress the needed chunks, and writes commit through
SQLite transactions. There is no expanded whole-disk working copy. Optional
Argon2id/XChaCha20-Poly1305 encryption protects chunks and filesystem metadata.
Host links remain external, with real read/write effects outside snapshots.

`storage` and `key status` report paths and state. `key set` and `key change` are
user-only hidden-prompt operations. Never collect a passphrase or put it in
commands, configuration, logs, or source. Do not modify the backing disk,
lockfile, native runtime manifest, bootstrap profile, or recovery snapshots.

After an unclean exit, Glyph preserves unfinished work and opens its last safe
snapshot read-only in emergency mode. Recovery and runtime activation are human
operations. Snapshots do not replace independent backups or roll back linked
host files. The compatibility `container` backend expands and archives sessions;
it is explicitly selected and lacks mounted snapshot recovery.

The host-side `storage.lua` is read before unlocking and selects backend,
data/session locations, compression, and retention. It must contain no keys.
Native rebuild caches and activated runtimes sit outside the encrypted disk;
never store credentials or unrelated private data in source/build artifacts.

## Lua customizability

Glyph loads `init.lua`, then `packages.lua`, then Lua files in `themes`, `links`,
`apps`, and `agents`, in that order and alphabetically within each directory.
A human may use `reload` to validate and reload policy; agents cannot. Restart also loads changes. The user can customize:

```lua
glyph.theme { background = "#181818", foreground = "#e8d8b8",
              accent = "#d6a35d", border = "rounded" }
glyph.bind("Ctrl+Space", function() glyph.launcher.open() end)
glyph.command("cad", function(args)
    glyph.apps.launch("/apps/kat.app", args)
end)
glyph.handler { kind = "custom", command = {"/usr/bin/runtime", "{app}", "{args}"} }
glyph.app { path = "/apps/tool.app", kind = "custom", network = true }
glyph.agent { name = "codex", app = "/apps/codex.app" }
```

This example illustrates the API; it does not assert that the listed apps or
runtimes are installed. Lua also defines `glyph.link` and
`glyph.package_manager`. Its embedded VM has memory/instruction limits and no
host IO/OS/module-loading libraries. Lua configuration is user-controlled policy,
not an agent escape hatch. Offer proposed configuration snippets for the user;
do not apply them to running Glyph policy yourself.

## Things you must not change or bypass

- **Running Glyph's trust controls:** `/configs`, `/apps`, `/AGENTS.md`, and
  `/home/AGENTS.md`. Do not rewrite, remove, replace, change permissions on, or
  attempt to make aliases of these protected paths to modify them.
- **Your own access:** link definitions, recursion, read/write grants,
  sandbox/network flags, host-mode approval, or launcher helpers. Do not disable
  isolation, forge approval input, or restart yourself in a broader environment.
- **Host resources outside declared links:** do not mount, scan, or reconstruct
  access to the host filesystem, external process namespaces, devices, or session
  sockets. Network permission does not grant host-file permission.
- **Credentials:** do not search for or import host credentials, SSH agents,
  browser profiles, or provider tokens. Use only credentials intentionally
  provisioned for your task inside Glyph, and do not print their values.
- **Unrelated user work:** preserve changes and files outside your assigned task.
  A writable path is not blanket authorization for unrelated changes.

If the user asks for a configuration/access change from inside an agent session,
explain the required change and provide a reviewable snippet for the user to
apply through trusted Glyph controls. If you are explicitly developing Glyph's
source in a writable project, you may edit that project's source as requested;
that does not authorize installing a replacement runtime or relaxing your own
active sandbox.

## How the boundary works

The trusted Rust launcher reads configuration, normalizes Glyph paths, resolves
declared links, builds an argument-vector launch plan, and starts the child.
On Linux, Bubblewrap establishes mount/process isolation before execution.
Configuration, app files, and these default guides are read-only; Glyph `/home`
and declared read-write resources are writable. Child processes inherit the
same namespace. Environment credentials are cleared, and temporary storage is
private. Runtime files are exposed read-only to make programs executable.

Networking is isolated by default. `network = true` shares host networking for
an explicitly configured app; it does not broaden filesystem access. It also
does not authorize arbitrary external requests unrelated to the task. Host-mode
apps are user exceptions requiring human confirmation; agent launches cannot
select host mode. Sandbox failures never fall back to unrestricted execution.
`--agent` alone is a command-policy restriction, not an OS sandbox.

Instruction files are guidance. Read-only mounts and launch restrictions enforce
the boundary even if an application ignores the guide. Do not claim this
foundation has received an independent security audit.

## Current scope and reporting

This is a Linux alpha. The full local distribution includes the graphical
Chromium/Kitty browser, Kat, Pytris, Terminal Spiel, human guides, editor
configuration, and a searchable TUI catalog. Host network tools are optional
imports and require human host-mode approval. Catalog entries are metadata,
not installed programs or guarantees of compatibility. Native package recipes
currently cover Codex and OpenCode on Linux x86-64; the package-manager interface
is configurable. Non-Linux isolation, comprehensive browser parity, and Pydos
JSON-data migration are not implemented. Inspect actual installed files rather
than assuming the full local bundle is present in every source build.

Leave deliverables in the assigned project or an appropriate `/home` directory.
Report what changed, the relevant checks, and any concrete blocker. Distinguish
an unavailable tool from an access denial, and a planned capability from one you
actually verified. Do not change permissions merely to make a check pass.
