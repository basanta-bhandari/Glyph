-- Default editor. Run `editor setup` once to install NvChad into Glyph only.
glyph.editor {
    command = {"/usr/bin/nvim", "{file}"},
    preset = "nvchad",
    network = true,
}
-- Other examples (replace the active definition above):
-- glyph.editor { command = {"nano", "{file}"}, preset = "plain", network = false }
-- glyph.editor { command = {"vim", "{file}"}, preset = "user", network = false }
-- glyph.editor { command = {"vis", "{file}"}, preset = "plain", network = false }
-- Emacs has a terminal UI: -nw keeps it inside the terminal.
-- glyph.editor { command = {"emacs", "-nw", "{file}"}, preset = "user", network = false }
