# Why — chive

Every rule in `target-state.md` has a matching entry here explaining why it exists.

## Status types: restorable / not-restorable / temporary / orphaned

These four statuses cover every possible state a file can be in. A file either can be restored or not, is temporary or permanent, has a parent directory or doesn't. Adding more statuses inflates the model without adding value — the user decides via the `source` field whether a restore method is trusted.

## Source: verified vs user_supplied

Separating the restore method from its trust level keeps the status clean. A file is restorable regardless of whether chive or the user provided the method. The `source` field tells the user whether to trust the method.

## Dual storage (SQLite + TOML)

SQLite handles large catalogs with indexed queries. TOML handles versioning and cross-device sync. Both are necessary because a catalog of millions of files needs indexed access, and a catalog that can't be checked into git isn't versionable.

## Categories as types, not just extensions

Categories are first-class types with extension mappings. Extension-only matching is brittle (`.tar.gz`, `.jpeg`/`.jpg`). MIME-type detection is planned as a future improvement.

## Tauri GUI

Tauri provides a native desktop app with a Rust backend and web frontend. This is the best balance of performance and development speed for a GUI that must run on Linux, macOS, and Windows.

## No deleted-file detection

The user explicitly removed this scope. Block-level recovery of unlinked data is a different product. chive manages existing files, not deleted ones.
