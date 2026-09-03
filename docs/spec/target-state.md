# Target State — chive

This document is canonical. If code disagrees with this document, the code is wrong.

## What chive is

chive is a declarative file lifecycle manager. It scans your filesystem, categorizes every file, declares its state, and lets you act on it.

## Commands

| Command | Description |
|---------|-------------|
| `chive scan <path>` | Crawl a directory, extract metadata, categorize files, update state |
| `chive status [filter]` | Show files filtered by status (restorable, not-restorable, temporary, orphaned) |
| `chive restore <id>` | Restore a file to its declared state |
| `chive clean [--dry-run]` | Remove orphaned files. Dry-run prints what would be removed. |
| `chive export [--to <file>]` | Export catalog as TOML |
| `chive import [--from <file>]` | Import catalog from TOML |
| `chive stats` | Show catalog statistics |

## Status types

Every file in the catalog has a `status`:

- `restorable` — This file can be restored.
- `not-restorable` — This file cannot be restored. The catalog explains why.
- `temporary` — Temporary file. Safe to clean.
- `orphaned` — File has no parent directory or is unreferenced. Candidate for cleanup.

Every file also has a `restore_method` (how to restore it) and a `source` (where the restore method came from):

- `source = "verified"` — chive determined the restore method.
- `source = "user_supplied"` — The user provided the restore method.

## Categories

Each file is classified into one category: `document`, `image`, `code`, `config`, `program`, `audio`, `video`, `archive`, `data`.

## State persistence

chive supports two storage backends:

- **SQLite** (default): Fast indexed queries. Used at runtime.
- **TOML file**: Human-readable, versionable, git-friendly. Used for export/import and cross-device sync.

The SQLite database is the working state. TOML export is a snapshot. Import loads a TOML file into the database.

## GUI

chive includes a Tauri-based GUI. The GUI shares the same Rust core as the CLI. All heavy computation (scanning, categorization, state management) runs in Rust. The frontend is a web view rendered by Tauri.

## Platform support

- Linux / NixOS
- macOS
- Windows

## Non-goals

- Block-level recovery of deleted files
- MFT/inode parsing for unlinked data
- Network/remote filesystem scanning (MVP)
- Cloud sync (MVP)
