-- Host-side bootstrap configuration, read BEFORE opening Glyph's disk.
-- This file holds locations/preferences, never encryption keys or passphrases.
-- Glyph's ordinary /configs tree stays inside its encrypted disk.
glyph.storage {
    backend = "mounted", -- actual FUSE filesystem; "container" selects archive compatibility
    snapshots = 5,
    compression_level = 6, -- 1 = faster/larger, 9 = slower/smaller; gzip remains on

    -- Optional: location containing workspace.glyphdisk.
    -- data_dir = "~/GlyphData",

    -- Optional: parent of private mount-* directories (container backend uses session-*).
    -- session_dir = "~/GlyphSessions",
    -- Mount points contain no expanded copy with the mounted backend.
    -- session_dir = "/dev/shm/glyph",

    -- Windows path example (Lua long brackets avoid backslash escaping):
    -- data_dir = [[D:\GlyphData]],
}

-- Absolute paths are used directly. Relative paths are relative to this file.
-- ~/ expands to your home directory. No shell or environment expansion occurs.
-- --root DIR overrides data_dir; an explicit session_dir still takes effect.
-- Moving/changing data_dir selects a store; it does not move existing data.
-- Mounted-filesystem and VM backends are not implemented. Unsupported backend
-- names fail instead of silently choosing a different implementation.
