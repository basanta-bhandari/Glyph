# Alpha verification

Verified on Linux x86-64 on November 12, 2026. This report separates automated checks and actual runtime checks from untested integrations. The packaged version is **0.3.0-alpha.1**.

**55 Rust tests and 2 game-save regression tests passed**, with none ignored. Clippy with warnings denied and formatting checks passed.

The Rust suite covers workspace persistence, Lua execution limits, handlers and argument forwarding, real Bubblewrap isolation, link access/recursion, protected guides, package templates, agent installation/registration boundaries, native storage paths, archive validation, encryption, mounted chunk operations, snapshots, crash recovery, and runtime activation/retention. Additional regression checks ensure failed mounted unlocks never invoke archive cleanup or change backing-file bytes, encrypted snapshot labels are not stored in plaintext, changed candidates cannot activate, and package removal preserves custom registration edits.

`cargo test --locked --all-targets`, `cargo clippy --locked --all-targets -- -D warnings`, `cargo fmt --check`, and the optimized multi-binary build are the release checks. Actual FUSE checks require `/dev/fuse` outside the development command sandbox; the optional `tools/qa-mounted.py` reproduces the disk/crash checks with explicitly chosen disposable paths.

Runtime checks performed:

- Mounted file write/reopen round-trip, actual FUSE app access, and protected read-only policy inside a confined process.
- SIGKILL after a write, last-safe read-only recovery, preserved unsafe snapshot restoration, and stale mount cleanup.
- Hidden passphrase setup, encrypted reopen, key rotation, rejected old key, successful replacement-key unlock, and no leftover mount marker.
- Fresh full-bundle setup with source, docs, NvChad starter, rice copies, games, Kat, network-tool app definitions, and **no preinstalled or registered agent CLI**.
- Real Chromium rendering plus typed text and a clicked button through the debugging-pipe input bridge, inside the mounted Glyph sandbox; PNG output inspected.
- Kat format command, Pytris menu/quit handoff, and Terminal Spiel start/normal quit.
- Separate native Kitty instance loading Glyph's copied config, private control socket, TUI store search, browser graphics handoff, normal close, mount cleanup, and byte-for-byte unchanged host Kitty config.
- Ctrl+C during a confined child, return to Glyph, and successful subsequent file save.
- Actual source build inside Bubblewrap with a read-only toolchain and dedicated cache, library checks, optimized candidate compilation, confined health check, activation, and subsequent supervisor selection of the rebuilt executable.
- Simulated activated-runtime crash followed by bundled emergency startup, read-only safe-file access, and runtime-manifest rollback.
- NvChad dependencies downloaded successfully inside the sandbox after fixing the host public CA-path mounts. The compatibility archive intentionally rejects plugin symlinks; normal editor persistence uses the mounted backend.

The first NvChad plugin population can take several minutes on the transactional FUSE store; warm editor launches avoid that download/import work.

No OpenCode/Codex package was preinstalled during verification. Registration and failed-install behavior use fixtures; no authenticated agent/model session or host agent credential import was performed. The native recipes require a user command and retain upstream runtime compatibility requirements.

nmtui, impala, and bluetui are imported when available and their host-mode plans are configured, but verification does not change real network connections, pair devices, enable daemons, or claim hardware behavior. The TUI catalog is metadata; arbitrary catalog packages were not installed. No existing Pydos saved data was migrated. No independent sandbox/cryptography audit or Windows/macOS runtime execution is claimed.

The graphical browser is an alpha viewport/input bridge. Desktop-browser parity, audio/camera permissions, extensions, accessibility integration, and high-frame-rate video are not implemented. Chromium updates remain the host runtime provider's responsibility. Snapshot recovery covers managed files, not external read-write links, native rebuild caches, independent disk failure, or deliberately malicious human-selected runtimes.
