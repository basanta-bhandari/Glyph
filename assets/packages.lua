-- Select ONE package manager by uncommenting its glyph.package_manager block.
-- GLYPH_BUILTIN_DEFAULT: installs into /home/.local inside the sandbox.
glyph.package_manager {
    name = "glyph",
    sandbox = true,
    network = true,
    install = {"/apps/package-manager.app", "install", "{[pkg}]"},
    remove = {"/apps/package-manager.app", "remove", "{[pkg}]"},
    upgrade = {"/apps/package-manager.app", "upgrade", "{[pkg}]"},
}
-- To select another backend, replace the active definition above.
-- You can instead define your own manager here or in init.lua.
-- {[pkg}] is replaced by one package name/ID, passed as a single argument.
-- Commands may be strings or argument tables. Strings are split into arguments;
-- they are NOT executed by a shell (no pipes, substitutions, or && expansion).
-- sandbox=false uses the host manager and requires interactive user approval.
-- To manage packages inside Glyph, configure a suitable workspace-local manager
-- and leave sandbox=true; these host examples do not provision an isolated OS.

-- Arch Linux / pacman
-- glyph.package_manager {
--     name = "pacman",
--     sandbox = false,
--     network = true,
--     install = "sudo pacman -S '{[pkg}]'",
--     remove  = "sudo pacman -R '{[pkg}]'",
--     -- Arch upgrades must remain consistent: this upgrades the WHOLE SYSTEM
--     -- and installs/upgrades the named package in the same transaction.
--     upgrade = "sudo pacman -Syu '{[pkg}]'",
-- }

-- Debian / Ubuntu / apt
-- glyph.package_manager {
--     name = "apt",
--     sandbox = false,
--     network = true,
--     install = {"sudo", "apt", "install", "{[pkg}]"},
--     remove  = {"sudo", "apt", "remove", "{[pkg}]"},
--     upgrade = {"sudo", "apt", "install", "--only-upgrade", "{[pkg}]"},
-- }

-- Homebrew (macOS or Linux)
-- glyph.package_manager {
--     name = "brew",
--     sandbox = false,
--     network = true,
--     install = "brew install '{[pkg}]'",
--     remove  = "brew uninstall '{[pkg}]'",
--     upgrade = "brew upgrade '{[pkg}]'",
-- }

-- Windows Package Manager / winget (requires running Glyph on Windows)
-- glyph.package_manager {
--     name = "winget",
--     sandbox = false,
--     network = true,
--     install = {"winget", "install", "--id", "{[pkg}]", "--exact"},
--     remove  = {"winget", "uninstall", "--id", "{[pkg}]", "--exact"},
--     upgrade = {"winget", "upgrade", "--id", "{[pkg}]", "--exact"},
-- }

-- Example Glyph-local manager (replace its executable with your own tool):
-- glyph.package_manager {
--     name = "custom",
--     sandbox = true,
--     network = true,
--     install = {"/apps/package-tool.app", "install", "{[pkg}]"},
--     remove  = {"/apps/package-tool.app", "remove", "{[pkg}]"},
--     upgrade = {"/apps/package-tool.app", "upgrade", "{[pkg}]"},
-- }
