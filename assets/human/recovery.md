# When the experiment bites back.

Glyph keeps recent disk snapshots and a pinned last-safe point. A validated boot establishes a usable baseline; a normal exit records the current valid filesystem. A failed configuration check doesn't replace the safe point.

After an unclean shutdown, Glyph preserves the unfinished session as an unsafe snapshot and mounts the last safe one read-only in emergency mode. The supervisor also returns to the bundled executable if a custom runtime crashes. It tries recovery once. An incorrect encryption passphrase doesn't trigger a pointless rollback.

In emergency mode, use `ls`, `cat`, `storage`, and `snapshot list`. The prompt explicitly says it's read-only. `snapshot restore ID` selects a retained snapshot and exits; restart to work normally. You can choose the preserved unfinished snapshot if it contains work you need, but restoring a broken configuration can fail again. Keep a closed-disk backup before experimenting with recovery.

`source rollback` selects the previous runtime for the next launch. Runtime rollback and filesystem restoration are separate controls: one changes the executable, the other changes managed files. Neither rolls back host files exposed through read-write links.

A snapshot is a recovery point inside the same disk. If the disk is lost or the underlying storage fails, all those snapshots can disappear together. Keep an independent backup. The emergency room is helpful; it is not an immortality department.
