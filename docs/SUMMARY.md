# chive — Project Summary

## Project Overview

**chive** is a declarative file lifecycle manager that:
1. Scans your filesystem
2. Categorizes every file
3. Declares each file's state (restorable, not-restorable, temporary, orphaned)
4. Records how to restore each file (verified or user-supplied)
5. Lets you restore files and clean orphans
6. Exports the catalog as a versionable TOML file

### Core Components

1. **Scanner** — Crawls directories, extracts metadata, categorizes files
2. **State Manager** — SQLite for fast queries, TOML export for versioning
3. **CLI** — `clap`-based commands: scan, status, restore, clean, export, import, stats
4. **GUI** — Tauri-based frontend, Rust backend

## Status Types

| Status | Meaning |
|--------|---------|
| `restorable` | This file can be restored |
| `not-restorable` | This file cannot be restored |
| `temporary` | Temporary file, safe to clean |
| `orphaned` | No parent directory, candidate for cleanup |

Every file also has a `source` field: `verified` (chive-determined) or `user_supplied` (user-provided).

## Platforms

- Linux / NixOS
- macOS
- Windows

## License

MIT License.
