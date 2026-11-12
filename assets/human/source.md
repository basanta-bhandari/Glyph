# Yes, you can change the computer.

`/system/source` contains the Rust source used to build Glyph, the Lua defaults, bundled game sources, and these guides. `source edit src/ui.rs` opens a file in your configured editor. Read it, change it, make it a little stranger if you want. This is your software; having the source should mean more than a link at the bottom of a page.

`source build` uses the host Rust toolchain read-only inside Glyph's Bubblewrap sandbox. Dependencies and build output go into a dedicated native rebuild cache. It runs library checks, compiles release executables, then runs the candidate health check inside the sandbox. It needs Rust, a C compiler, and network access for uncached crates. A build failure leaves the selected runtime alone.

A successful candidate lands in `/system/builds/candidate`. `source activate` saves a disk snapshot and selects that executable for the next launch. Use `start.sh` or the bundled `glyph-bootstrap` launcher to get runtime selection and crash fallback. Starting a `glyph` executable directly skips supervisor selection.

The supervisor keeps the bundled runtime plus three previous runtime references. If an activated runtime crashes during an interactive session, the next attempt uses the bundled runtime in emergency mode. This catches crashes and failed starts, not infinite loops, deliberate corruption, or every possible programming mistake. Editing your own system is a real power; automatic snapshots don't make every experiment harmless.

Activated executables and the build cache live outside the encrypted disk because something must run before the disk is unlocked. They contain code you chose to build. Keep private data out of source literals if you expect the disk passphrase to protect it.

Agents cannot edit `/system/source`, activate builds, or rewrite running policy. If you're asking an agent to develop Glyph, give it a separate project under `/home`, review its changes, then apply the result through your human controls. The apprentice can help build the lock. It doesn't need the master key.
