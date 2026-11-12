# Hello, Glyph.

A small place for your tools. Files, games, an editor, a browser, a slightly unreasonable number of terminal applications... the point is that it feels like yours. The Rust core runs on your existing OS. You don't have to pretend it's a separate kernel to enjoy having a little computer inside your computer.

Start with `apps`, then `run hello`. Ctrl+Space opens the application launcher. `store` opens the searchable tool catalog; type a name, use the arrows, and press Enter to run an installed app. F2 previews an installation through your chosen package manager. A catalog entry doesn't mean its dependencies are magically present (we haven't solved magic yet).

Your files live in `/home`. Your applications are files in `/apps`: `/apps/browser.app`, `/apps/kat.app`, `/apps/pytris.app`, and `/apps/terminal-spiel.app` in the full local bundle. `run` and `exec` both launch them. The `.app` name hides format details, not missing dependencies: an EXE still needs Wine or another configured runtime.

`run browser` opens real Chromium-rendered pages inside Kitty. Ctrl+L opens the address bar, Ctrl+T makes a tab, Ctrl+Tab switches tabs, Ctrl+R reloads, and Ctrl+Q comes home. Click and scroll normally. This alpha isn't a replacement for every desktop-browser feature; it's already actual graphics, JavaScript, forms, cookies, and downloads, rather than a webpage translated into text.

`edit /home/notes.md` opens Neovim with its own NvChad starter. The first launch installs plugins into Glyph. `guide rice` explains customization. `guide storage`, `guide recovery`, `guide source`, and `guide network` explain the less decorative machinery.

Agents aren't preinstalled. Use `agent install opencode` or `agent install codex` when you want one. Glyph installs the package inside its sandbox, then registers it in `/configs/agents`. Authenticate from that agent's own interface inside Glyph. Your host agent credentials aren't copied. `agents` shows the entries that actually exist.

Normal quit saves a safe snapshot. Writes are already compressed and committed while you work. `snapshot list` shows recovery points, and `key set` enables encryption through a hidden passphrase prompt. Keep a separate backup of your disk file when Glyph is closed. Snapshots are useful; they aren't a second hard drive.

This is alpha software. Give it a small project first. If something breaks, tell it exactly what you did and keep the disk. There's usually more to learn from the broken thing than from pretending it never happened.
