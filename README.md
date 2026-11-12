# Glyph · 0.3.0-alpha.1

A cozy terminal workspace for files, tools, games, CAD, agents, and a graphical web browser. Rust at the core, Lua for customization, ordinary application files underneath. The Python Pydos implementation is preserved in `legacy/python`; the Rust runtime does not load Rubus.

This alpha targets **Linux x86-64** with FUSE3, Bubblewrap, and Kitty. The storage image is a real mounted filesystem, not a pretend path namespace or a whole-disk expanded cache.

```bash
./start.sh                  # packaged build, dedicated Glyph Kitty instance
./start.sh --root ./my-glyph # portable disk and bootstrap profile
```

Build from source with Rust 1.88+, a C compiler, FUSE3/fusermount3, and Bubblewrap:

```bash
cargo build --release --locked --bins
python3 tools/package.py
./start.sh
```

The browser also needs Chromium or Google Chrome installed as a read-only runtime. `tools/package.py --kat /path/to/forgetui` includes an existing Kat build; the supplied local alpha includes it. Packaging builds Pytris and Terminal Spiel `.app` zipapps from the included original game sources. Python 3 runs these apps; Glyph itself is Rust. No agent CLI is bundled or preinstalled.

```text
/AGENTS.md                 default agent guide (read-only to apps)
/configs/init.lua          colors, bindings, commands
/configs/packages.lua      install/remove/upgrade backend and {[pkg}] templates
/configs/editor.lua        Neovim/NvChad default; Nano, Vim, Vis, Emacs alternatives
/configs/links/            explicit host files/directories and permissions
/configs/apps/             app kinds, runtime handlers, network/sandbox policy
/configs/agents/           agent registrations, created after installation
/configs/user/             independent copies of supported host rice
/apps/*.app                application FILES, never directory bundles
/home/                     your persistent files
/system/source/            inspect, edit, and rebuild Glyph
/system/docs/              human guides and Kitty graphics
```

Inside Glyph, `help` lists commands. Try `apps`, `run pytris`, `run kat`, `run browser`, `edit /home/notes.md`, `store`, and `guide`. Ctrl+Space opens the launcher. The catalog imports Zenith's 100-tool metadata and capability aliases, with search and explicit installation previews. It does not install the entire catalog.

`agent install opencode` installs the package **inside Glyph's sandbox**, then adds `/apps/opencode.app` and `/configs/agents/opencode.lua` after success. Codex uses the same flow. Host authentication state is never copied. The default local package helper currently provides official Linux x86-64 binary recipes for these two CLIs. Choose another backend in Lua for other packages. Pacman, apt, brew, WinGet, and custom examples use the exact `{[pkg}]` placeholder, passed as one argument without shell evaluation.

Apps are confined by default using Bubblewrap. Child processes inherit the boundary. Agents cannot change running policy, approve host execution, alter installed app files, or activate runtimes. Explicit host-mode apps require a human to type `allow host` at every launch. nmtui, impala, and bluetui deliberately use that mode to operate real host services; setup never changes network connections or enables daemons.

First setup **copies** supported host rice into Glyph without overwriting later Glyph edits. A dedicated Kitty instance loads Glyph's copied config. `wallpaper` uses Zenith's category index and downloads only your selected image; controls target that Kitty instance rather than your desktop or other terminals. `reload` applies valid Lua edits to the running TUI.

Managed data lives in `workspace.glyphdisk`: a SQLite-backed FUSE filesystem with compressed chunks, optional authenticated encryption, and retained snapshots. `storage` reports the exact native data/cache/profile paths. `key set` and `key change` use hidden prompts. `snapshot list` shows recovery points. Linked host files are external and are neither encrypted nor rolled back by Glyph. See [the storage guide](assets/human/storage.md).

`source edit`, `source build`, `source activate`, and `source rollback` support changing Glyph itself. Source builds run inside the sandbox; the bootstrap launcher selects activated builds and falls back to the bundled runtime in emergency mode after crashes. The filesystem preserves unclean work and opens the last safe snapshot read-only. Build caches and bootable runtime binaries remain outside the encrypted disk. See [source](assets/human/source.md) and [recovery](assets/human/recovery.md).

The browser renders real HTML/CSS/JavaScript with Chromium and sends PNG frames through Kitty's graphics protocol. Mouse, scrolling, forms, tabs, history, reload, and downloads are implemented. This is an alpha viewport and input bridge, with known gaps in desktop-browser features, accessibility, audio, and performance; see [browser details](docs/browser.md).

Read the [human welcome](assets/human/welcome.md), [ricing guide](assets/human/rice.md), [network guide](assets/human/network.md), [agent guide](assets/AGENTS.md), [architecture](docs/architecture.md), and [validation report](docs/validation.md). [Third-party notices](THIRD_PARTY.md) identify the NvChad starter and Zenith catalog sources.

```bash
cargo test --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Alpha means real working features with limited platform coverage and validation, not a production security audit. Independent backups still matter. Source and defaults are open under the repository's MIT license.
