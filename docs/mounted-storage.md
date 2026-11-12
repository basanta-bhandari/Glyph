# Mounted virtual disk

Implemented as the default `backend = "mounted"`: `workspace.glyphdisk` stores SQLite metadata and compressed file chunks, exposed through a user-owned FUSE mount. `disk.rs`, `mount.rs`, and `storage.rs` implement it. It is an actual mounted virtual filesystem, not a guest OS, loop-mounted ext4 image, or expanded archive session.

Files use 64 KiB chunks. Frequently updated working metadata uses fast gzip; file chunks and retained snapshot trees use the configured compression level. Writes create compressed objects and transactionally update the working tree; reads decompress only needed objects. Metadata and chunks are authenticated and encrypted when a passphrase is enabled. The random disk key is wrapped with an Argon2id-derived key, allowing passphrase changes without invalidating snapshots.

Snapshot trees refer to immutable chunks. Retention keeps recent points plus a pinned last-safe tree; garbage collection removes unreachable objects. Unclean startup preserves the working tree as an unsafe snapshot, then exposes the last safe tree read-only. Restoration selects a tree and exits for restart. A stale mount marker supports cleanup after SIGKILL without global FUSE settings.

FUSE3 `/dev/fuse`, `fusermount3`, and user mount permission are required. Missing support fails explicitly; it never silently expands the disk or runs apps on the host. The compatibility `container` backend must be deliberately selected and does not provide mounted recovery features. Existing archive containers migrate into the mounted store while preserving the old archive as a backup.

Snapshots share the same physical disk file. Keep an independent closed-disk backup. Native bootable runtimes and rebuild caches remain outside disk encryption. Linked host resources remain external to storage and rollback.
