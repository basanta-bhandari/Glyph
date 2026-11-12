# 0.3.0-alpha.1

Rust Glyph replaces the active Python Pydos runtime; the old implementation remains in `legacy/python` and the legacy branch.

- Real mounted FUSE virtual disk with compressed chunks, optional encryption, configurable native paths, snapshots, and emergency last-safe recovery.
- Lua themes, bindings, commands, link permissions, extensible app handlers, editor settings, and package-manager templates.
- Files at `/apps/*.app`, a launcher, and a searchable Zenith-derived TUI catalog with capability aliases and installation previews.
- Actual Chromium-rendered graphical browser inside Kitty, Kat packaging, Pytris, and Terminal Spiel with Glyph-local saves.
- User-only source editing, confined rebuilds and candidate checks, explicit activation, retained runtime references, and supervisor crash fallback.
- Neovim/NvChad default with independent editor configuration and alternative terminal editors.
- Independent first-copy host rice, Glyph-specific Kitty configuration, and scoped on-demand wallpaper/color controls.
- Optional nmtui, impala, and bluetui host-control apps with explicit human approval.
- On-demand agent package installation INSIDE Glyph before registration; no preinstalled OpenCode/Codex or copied host credentials.
- Human guides with Kitty figures, protected default agent guides, packaging tools, and automated checks.

Known alpha limits are documented in README, browser details, and the validation report. Linux x86-64 is the working target; other OS path conventions do not constitute finished runtime ports.
