-- Glyph configuration. This file is user-controlled and read-only inside apps.
glyph.theme {
    background = "#181818",
    foreground = "#e8d8b8",
    accent = "#d6a35d",
    border = "rounded",
}

glyph.bind("Ctrl+Space", function()
    glyph.launcher.open()
end)

glyph.command("hello", function(args)
    glyph.apps.launch("/apps/hello.app", args)
end)

-- Files in themes/, links/, apps/, and agents/ are loaded in that order.
-- Example /configs/links/projects.lua:
-- glyph.link {
--     source = "/absolute/host/path/to/projects",
--     target = "/home/projects",
--     recursive = true,
--     access = "read-write",
-- }

-- Example custom runtime (no fixed format allowlist):
-- glyph.handler { kind = "ruby", command = {"/usr/bin/ruby", "{app}", "{args}"} }
-- glyph.app { path = "/apps/my-tool.app", kind = "ruby" }

-- An explicit user configuration can request host execution. Each launch
-- still requires interactive confirmation and is unavailable to --agent.
-- glyph.app { path = "/apps/desktop-tool.app", sandbox = false }

-- No agents are preinstalled. At the Glyph prompt, use agent install codex
-- or agent install opencode. Installation happens inside Glyph; only a
-- successful installation creates /apps/NAME.app and /configs/agents/NAME.lua.
-- For a custom Glyph-local installation, use agent add NAME afterwards.
-- Equivalent registration example (after installation):
-- glyph.app { path = "/apps/codex.app", network = true }
-- glyph.agent { name = "codex", app = "/apps/codex.app" }
