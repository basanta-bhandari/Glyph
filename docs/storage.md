# Where the files actually go.

The default is a mounted virtual filesystem. `workspace.glyphdisk` is a SQLite-backed disk image containing directory metadata and individually compressed 64 KiB file chunks. FUSE mounts it during the session. Apps get normal files; the storage layer reads and decompresses the chunks they need. There isn't an expanded mirror of the whole disk sitting in a cache directory.

This is why searching works. A search program asks for a file; Glyph serves its contents. Compression happens underneath. Searching all your files still means reading all the relevant chunks (compression doesn't become a psychic index).

`storage` prints the exact locations. On Linux the defaults follow XDG: data under `~/.local/share/glyph`, session mount points and rebuild cache under `~/.cache/glyph`, and the bootstrap profile at `~/.config/glyph/storage.lua`. Windows and macOS path conventions are represented in the configuration layer, but this alpha's real mount and sandbox backends are Linux-only. Don't mistake a configured Windows path for a finished Windows port.

The host-side `storage.lua` is read before unlocking. It contains locations and preferences, never keys:

```lua
glyph.storage {
    backend = "mounted",
    data_dir = "~/GlyphData",
    session_dir = "~/GlyphSessions",
    compression_level = 6,
    snapshots = 5,
}
```

`--root DIRECTORY` makes a portable store and puts its profile there. `--storage-config FILE` selects another profile. Relative paths are relative to that file. Compression accepts 1–9, snapshot retention 2–20. The explicit `container` backend keeps the older expand-on-open/archive-on-close behavior for compatibility; it doesn't offer mounted snapshot recovery. There's no silent backend fallback. The compatibility archive accepts regular files and directories, and rejects symlinks/special files; plugin trees such as NvChad's can require the mounted backend.

`key set` enables encryption. It asks twice at a hidden terminal prompt; use at least 12 characters. `key change` changes the passphrase later. Argon2id derives the wrapping key; XChaCha20-Poly1305 protects chunks and filesystem metadata. Changing the passphrase rewraps the random disk key, so old snapshots remain usable with the new passphrase. Lose the passphrase and Glyph can't recover it for you.

While unlocked, apps can read the files they're authorized to read, and plaintext exists in memory. Native rebuild caches and activated executable runtimes sit outside the encrypted disk so the supervisor can boot them. They may contain your modified source or compiled code. Host links remain external files and aren't encrypted or snapshotted by Glyph. Encryption protects the managed disk at rest; it doesn't encrypt the rest of your computer by association.

Writes commit transactionally as you work, and a clean exit records a safe snapshot. Copy the closed `workspace.glyphdisk` to make an independent backup. Don't copy just one live SQLite file in the middle of a write and hope for the best. Hope is a poor storage protocol.
