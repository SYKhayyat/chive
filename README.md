# chive

Reconstruction engine. For everything meaningful on your machine, chive keeps a recipe for getting it back. Move to a new machine — or lose files by accident — and chive re-creates what's missing from those recipes.

It does not recover bytes from disk. It re-derives: a package is reinstalled, a file is re-exported from git, a symlink is recreated. A file that already exists is never touched.

## Quick start

```bash
# scan your home directory
chive scan ~/

# see what can be restored and how
chive status --restorable

# preview a restore (nothing runs)
chive plan restore --root ~/

# restore one file
chive restore conf/emacs.d/init.el

# export the catalog (commit this to a repo off-box)
chive export --to catalog.toml
```

## Moving to a new machine

```bash
# on the old machine (before it's gone)
chive scan ~/
chive export --to catalog.toml
# commit catalog.toml to a repo, or copy it to removable storage

# on the new machine
chive import --from catalog.toml
chive plan restore --root ~/
chive restore --all
```

## Teaching chive

chive infers recipes from provenance: package ownership, git tracking, symlinks. For everything else, you teach it.

```bash
chive teach conf/emacs.d/init.el --method "git -C ~/dotfiles pull && cp ~/dotfiles/emacs.d/init.el '{dest}'"
```

The `{dest}` variable expands to the file's absolute path on the target machine. Taught recipes are stored in `~/.config/chive/recipes.toml` and can be edited by hand. A taught recipe overrules inference, so it works on any file whatever status chive assigned it.

Files without a recipe are `orphaned`. Files you know matter but can't rebuild yet: mark them so `clean` leaves them alone:

```bash
chive mark conf/manual.nef --status not-restorable
```

## Status

| Status | Meaning | What happens to it |
|--------|---------|-------------------|
| `restorable` | Has a recipe | Can be restored |
| `not-restorable` | Owner says it matters, no recipe yet | Protected from clean, surfaced in "teach me" view |
| `temporary` | Transient | Safe to clean |
| `orphaned` | No provenance, no taught recipe | Safe to clean unless protected |

## Cleaning

```bash
chive clean --dry-run    # preview what would be removed
chive clean              # confirm, then remove temporary + orphaned files
```

`clean` never touches `restorable` or `not-restorable`.

## Categories

`document · image · code · config · program · audio · video · archive · data`

Categories say what a file is. They don't decide whether it can be restored.

## The catalog

The catalog is plain text (TOML). It lives off-box — committed to a repo, or on storage that survives the machine it describes. SQLite is a derived working index rebuilt from the TOML.

## Commands

| Command | What it does |
|---------|-------------|
| `chive scan <path>` | Catalog the machine, infer recipes |
| `chive status [filter]` | Show what's restorable and how |
| `chive plan restore` | Preview restore without running |
| `chive restore <path...>` | Re-derive files by recipe |
| `chive restore --all` | Restore every restorable file |
| `chive teach <path> --method "<cmd>"` | Teach a recipe (overrules inference, any status) |
| `chive mark <path> --status <st>` | Set status explicitly: `not-restorable` \| `temporary` \| `orphaned` |
| `chive clean [--scope <t|o|both>] [--dry-run]` | Remove temporary + orphaned |
| `chive export [--to <file>]` | Write catalog as TOML |
| `chive import [--from <file>]` | Load catalog from TOML |
| `chive stats` | Catalog statistics |

## Building

```bash
cargo build --release
```

## Platform

Linux / NixOS · macOS · Windows

## License

All rights reserved.