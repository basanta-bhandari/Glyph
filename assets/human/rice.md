# Make it yours.

On first setup, Glyph copies supported host configuration into `/configs/user`: Kitty, Fish, fastfetch, neofetch, and the other supported editor/network-tool config directories. It copies ordinary files and safe in-tree file symlinks as independent files. It skips history and credential files. Changes in Glyph stay in Glyph; changes on the host stay on the host. A useful starting point, then two different lives.

`rice status` shows what was copied. `rice copy kitty` (or another single config-directory name) imports additional missing files later, without overwriting your changes. It does not run the host's startup scripts during import. Copied shell configuration is still code when you launch that shell, so inspect anything that expects paths or tools from the host. External includes and assets outside the selected config tree need copying explicitly; the importer reports skipped symlinks rather than keeping live links.

Use `edit /configs/init.lua` for Glyph colors, bindings, and commands. A small example:

```lua
glyph.theme {
    background = "#181818", foreground = "#e8d8b8",
    accent = "#d6a35d", border = "rounded",
}
glyph.bind("Ctrl+Space", function() glyph.launcher.open() end)
glyph.command("cad", function(args)
    glyph.apps.launch("/apps/kat.app", args)
end)
```

`reload` validates the configuration and applies it to the Glyph TUI. In Glyph's dedicated Kitty instance it also applies the current background, foreground, and cursor colors. Running apps keep the permissions they were launched with. Invalid Lua reports an error without replacing the loaded configuration. You can always restart after editing; the previous safe snapshot remains available if a later boot fails.

Kitty uses `/configs/user/kitty/kitty.conf` when launched through `start.sh` or `--kitty`. That is a separate Kitty process with a private control socket. Your other terminals and desktop wallpaper are unaffected. For the copied Kitty rice, colors win at startup; `reload` explicitly applies Glyph's Lua colors.

The wallpaper integration follows Zenith's on-demand idea: `wallpaper categories`, `wallpaper list retro`, then `wallpaper fetch retro FILE`. Only the selected image downloads into `/home/.local/share/wallpapers`. Apply it with `wallpaper set /home/.local/share/wallpapers/FILE`; `wallpaper clear` removes the selection. It changes the background of Glyph's Kitty instance. Downloaded images belong to their original artists; the collection index isn't a blanket license.

The default editor configuration is `/configs/editor.lua`. NvChad's starter is at `/configs/editor/nvim`, separate from the copied host Neovim rice. To use the copy instead:

```lua
glyph.editor { command = {"/usr/bin/nvim", "{file}"}, preset = "user", network = false }
```

Nano, Vim, and Vis work through the same command template. Emacs also has a terminal UI: use `{"emacs", "-nw", "{file}"}`. Replace the existing editor definition; don't stack two. Dependencies must exist in the runtime. Your editor choice shouldn't require a constitutional amendment.

Applications and agents receive `/configs` read-only. The human's `edit /configs/...` command deliberately grants the selected editor configuration write access. That's a trusted operation. Keep agent project edits under `/home`, rather than handing them the controls that determine their own access.
