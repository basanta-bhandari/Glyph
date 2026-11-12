# Architecture

`main.rs` unlocks/opens storage, seeds a new workspace, validates Lua, and runs either Ratatui, the plain shell, or a command. `glyph-bootstrap` selects activated native runtimes and retries the bundled runtime once in emergency mode after an interactive crash. `session.rs` keeps the mounted disk in a parent controller while Glyph runs in its own Kitty instance. Its local private socket uses a random capability token; application environments do not inherit it.

`storage_config.rs` reads restricted host-side Lua before unlock. `storage.rs` manages locks, migration, mount lifecycle, key prompts, and session snapshots. `disk.rs` implements the chunk store, authenticated encryption, filesystem tree, snapshot retention, transactions, and recovery selection. `mount.rs` exposes that store through FUSE. No root-level loop device or host filesystem format command is required. The explicit archive backend remains for compatibility.

`workspace.rs` normalizes logical paths and validates declared links. Symlinks in host-linked trees and overlapping link targets are rejected. Shallow links expose existing immediate files individually; recursive links support ordinary persistent directory writes. The trusted UI accesses disk files directly; confined apps see only their namespace mounts.

`config.rs` embeds bounded Lua without host IO, OS execution, or module loading. It implements themes, callbacks, bindings, apps, handlers, agents, package-manager templates, and editor settings. `app.rs` produces argument-vector launch plans, detecting payloads or selecting configured handlers. `.app` is a file identity, not a fixed programming language or directory format.

`sandbox.rs` translates a plan into Bubblewrap mount/process namespaces, private temporary storage, a cleared environment, read-only runtime libraries, and optional shared networking. Policy, app files, and guides are read-only. Explicit human editor operations can write configs or system source; agent operations cannot. Host mode requires typed human approval and never serves as a fallback for failed confinement.

`packages.rs` expands exact `{[pkg}]` arguments. `package_install.rs` supplies optional, on-demand official Codex/OpenCode binary recipes inside `/home/.local/bin`, checks available release digests, writes receipts, and registers app/agent files only after successful user installation. It never imports host agent credentials. Other package systems are user-configurable.

`system.rs` seeds editable source, the NvChad starter, docs, and optional local tools. Rebuilds mount a Rust toolchain read-only with dedicated writable build cache. Candidates undergo confined health checks; user activation updates a native runtime manifest and preserves previous runtime references. Runtime selection is outside the encrypted disk because it must happen before unlocking.

`browser.rs` drives Chromium over private debugging pipes, renders its viewport as Kitty graphics, and translates keyboard/mouse events. Chromium's nested sandbox is disabled only inside Glyph's real outer confinement for the packaged browser. `rice.rs` performs first-copy configuration import, `appearance.rs` implements scoped wallpaper/color changes, and `catalog.rs` reads Zenith-derived tool/capability metadata. `ui.rs` owns terminal handoff, the launcher, catalog search, prompts, and plain-shell behavior.

Limits: Linux backends only, broad read-only `/usr` runtime exposure, no independent security audit, no built-in distro package root, no Pydos JSON-data migration, and no full desktop-browser parity. Tests exercise concrete boundaries; they do not establish protection against every kernel exploit, hostile same-user host process, or deliberately malicious replacement runtime chosen by the human.
