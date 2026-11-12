# Third-party and source notices

Glyph retains the repository's MIT license. Rust dependencies keep their own licenses recorded by Cargo metadata/lockfile; system runtime tools are not relicensed by this project.

`assets/nvchad-starter` comes from [NvChad/starter](https://github.com/NvChad/starter). Its Unlicense/public-domain dedication is preserved at `assets/nvchad-starter/LICENSE`. These starter configuration files retain that dedication; Glyph adapts the lazy lockfile location to writable application data and reports bootstrap download failures. NvChad and plugins are installed into the user's Glyph environment on demand and retain their own licenses.

`assets/tui-catalog.tsv` and `assets/capabilities.tsv` are adapted from Basanta Bhandari's [Tic / Zenith OS](https://github.com/basanta-bhandari/Tic), distributed under MIT. Wallpaper indexing follows that project's on-demand approach; it does not ship the image collection or modify Sway/host desktop configuration. Images fetched from [walls](https://github.com/basanta-bhandari/walls) belong to the credited original artists; that collection is not a blanket license grant.

The included Pytris source is copied from the author's local `project2` version; Terminal Spiel source is copied from the author's `Summer_of_Making` project. Save locations are adapted to Glyph's home. The optional Kat executable in the local alpha is built from the author's ForgeTUI project. The packaging script accepts an explicit Kat build path rather than downloading or installing it automatically.

OpenCode and Codex are not bundled or preinstalled. Their optional package recipes download official releases only after an explicit user command, and those applications retain their upstream licenses.
