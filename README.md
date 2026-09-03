# chive

Declarative file lifecycle management. Scan your filesystem, categorize every file, declare its state, and act on it.

## What is chive?

chive crawls your filesystem, extracts metadata, categorizes files, and stores everything in a searchable database. It then tells you what each file is — restorable, not-restorable, temporary, orphaned, or user-supplied — and how to restore it. It knows how to restore on its own, or you can supply the method yourself (marked `verified` or `user-supplied`). You can clean orphans, restore files, and export the catalog as a versionable state file.

**Key principle**: The catalog is the truth. Every file gets a category and a status. chive tells you how to restore each one, and you can teach it. You decide what to keep, what to clean, and what to restore.

## Features

- **Filesystem Scanner**: Crawls directories, extracts metadata (size, dates, permissions, type)
- **File Categorization**: Automatically classifies files into categories (documents, images, code, config, programs, audio, video, archives, data)
- **Status Declaration**: Each file is classified as restorable, not-restorable, temporary, orphaned, user-supplied, or verified
- **State Persistence**: SQLite for fast queries, with TOML export for versioning and cross-device sync
- **Restore**: Restores files to their declared state from the catalog
- **Clean**: Removes orphaned files (dry-run first, confirm before deleting)
- **CLI + GUI**: Full CLI and a Tauri-based GUI, both backed by a Rust core
- **Cross-platform**: Linux, NixOS, macOS, Windows

## Status types

| Status | Meaning |
|--------|---------|
| `restorable` | This file can be restored. The catalog records how. |
| `not-restorable` | This file cannot be restored. The catalog explains why. |
| `temporary` | Temporary file. Safe to clean. |
| `orphaned` | File has no parent directory or is unreferenced. Candidate for cleanup. |
| `user-supplied` | Restore method provided by the user, not yet verified. |
| `verified` | Restore method verified by chive. |

Every file also carries a `restore_method` field describing how to restore it, and a `source` field indicating whether the method is `verified` or `user_supplied`.

## Categories

| Category | Examples |
|----------|----------|
| `document` | pdf, doc, docx, txt, md, rtf, odt, xls, xlsx, ppt, pptx |
| `image` | png, jpg, jpeg, gif, svg, webp, bmp, tiff |
| `code` | rs, py, ts, js, go, cpp, c, h, java, rb, lua |
| `config` | toml, yaml, yml, json, xml, conf, ini, env |
| `program` | deb, rpm, exe, appimage, snap |
| `audio` | mp3, wav, flac, aac, ogg, m4a |
| `video` | mp4, mkv, avi, webm, mov |
| `archive` | zip, tar, gz, rar, 7z, bz2, xz |
| `data` | csv, sql, db, bin |

## State file format

chive supports two storage backends, selectable by the user:

- **SQLite** (default): Fast indexed queries for large filesystems.
- **TOML file**: Human-readable, versionable, git-friendly. Export with `chive export`, import with `chive import`.

Example TOML state entry:

```toml
[[files]]
path = "/home/user/doc.pdf"
category = "document"
status = "restorable"
restore_method = "apt reinstall poppler-utils"
source = "verified"
size = 1048576
modified = "2024-01-15"
```

## Commands

```bash
# Scan a directory
chive scan ~/

# Show files by status
chive status --orphaned
chive status --restorable

# Restore a file
chive restore <file-id>

# Clean orphaned files (dry-run first)
chive clean --dry-run
chive clean

# Export catalog as TOML
chive export --to catalog.toml

# Import from TOML
chive import --from catalog.toml

# View statistics
chive stats
```

## GUI

chive includes a Tauri-based GUI with a Rust backend. The GUI displays file categories, statuses, and restore methods, and provides controls for scan, restore, and clean operations. All heavy computation runs in Rust; the frontend is a web view.

## Getting Started

```bash
# Build
cargo build --release

# Scan your home directory
chive scan ~/

# See what's orphaned
chive status --orphaned --dry-run

# Export as versionable TOML
chive export --to ~/chive-state.toml
```

## Project Structure

```
chive/
├── Cargo.toml
├── Cargo.lock
├── deny.toml
├── CLAUDE.md
├── src/
│   ├── main.rs
│   ├── scanner.rs
│   ├── categorize.rs
│   ├── state.rs
│   ├── cli.rs
│   └── gui.rs
├── web/                    # Tauri frontend
├── tests/
│   └── main.rs             # autotests = false, manual mod list
├── docs/
│   ├── SUMMARY.md
│   └── spec/
│       ├── target-state.md
│       ├── why.md
│       └── decisions.md
└── LICENSE
```

## Platform support

- Linux / NixOS
- macOS
- Windows

## License

No license — all rights reserved.
