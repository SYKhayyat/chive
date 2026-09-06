# chive — Project Summary

## What chive is

chive is a reconstruction engine. It scans a machine, decides what matters, and for each meaningful file records a recipe for re-deriving it. The collection of recipes is the catalog. When you move to a new machine or lose files by accident, chive rebuilds what's missing by running those recipes.

chive does not recover bytes. It re-derives.

## Core concept

The catalog is the product. It is a versionable TOML file that lives off-box (committed to a repo or on removable storage). On a new machine, you import the catalog and run `chive restore` to rebuild everything that's missing.

## How it works

1. `chive scan ~/` — crawls the filesystem, runs provenance detection (package ownership, git tracking, symlinks), classifies files, and infers recipes.
2. `chive status --restorable` — shows what can be rebuilt and the exact method for each file.
3. `chive plan restore` — previews every restore action without running anything.
4. `chive restore <path>` — re-derives the file by running its recipe.
5. `chive export --to catalog.toml` — writes the versionable catalog for off-box use.

## Teaching chive

For files chive can't infer a recipe for, the owner teaches it:

```bash
chive teach <path> --method "<shell command>"
```

Taught recipes use `{dest}` to reference the file's absolute path on the target machine. They're stored in `~/.config/chive/recipes.toml` and can be edited by hand.

## Statuses

| Status | Meaning |
|--------|---------|
| `restorable` | Has a recipe (verified or user-supplied) |
| `not-restorable` | Owner says it matters, no recipe yet |
| `temporary` | Transient, safe to clean |
| `orphaned` | No provenance, no recipe, safe to clean |

## Store layout

```
~/.config/chive/
├── config.toml      # ignore list, defaults
├── recipes.toml     # user-taught recipes
└── catalog.db       # working index (derived from TOML)
```

The catalog TOML is the source of truth. SQLite is a derived index.

## MVP scope

- CLI with: scan, status, plan, restore, teach, mark, clean, export, import, stats
- Provenance detection: package ownership, git work-tree, symlinks
- TOML catalog format with concrete schema
- SQLite working index
- Extension file for user-taught recipes
- Ignore list for skipped directories

## Non-goals (MVP)

- Byte-level recovery of deleted files
- Inode/FS forensics
- Cloud sync (user brings their own storage)
- Restore manager (open question D2)
- GUI (phase 2, pending D12)

## Platforms

Linux / NixOS · macOS · Windows

## License

All rights reserved.