# chive — Project Summary

## What chive is

chive is the `home.nix` you never wrote: an imperative NixOS configuration, read
back off a machine that already exists. It scans a machine, decides what
matters, and for each meaningful file records a recipe for re-deriving it. The
collection of recipes is the catalog. When you move to a new machine or lose
files by accident, chive rebuilds what's missing by running those recipes.

chive does not recover bytes. It re-derives.

**The archive is the product.** Deleting is a side benefit, and it happens only
where the owner has already judged something disposable (D18).

## Core concept

The catalog is the product. It is a versionable TOML file that lives off-box (committed to a repo or on removable storage). On a new machine, you import the catalog and run `chive restore` to rebuild everything that's missing.

## How it works

1. `chive scan ~/` — crawls the filesystem, runs provenance detection (package ownership, git tracking, symlinks), evaluates owner rules, re-applies the owner act log, and infers recipes.
2. `chive holes` — what cannot be rebuilt yet, largest first, each row carrying the act that closes it. The primary read path.
3. `chive plan restore` — previews every restore action without running anything.
4. `chive restore <path>` — re-derives the file by running its recipe.
5. `chive export --to catalog.toml` — writes the versionable catalog for off-box use.

## Teaching chive

Owner acts are entries in one ordered log in the catalog, newest per path
governs, and a rescan re-applies the log without reordering it:

```bash
chive teach <path> --method "<shell command>"   # -> restorable
chive dispose <path>                            # -> disposable (clean may remove it)
chive withdraw <path>                           # -> back to what chive can prove
```

Taught recipes use `{dest}` to reference the file's absolute path on the target machine. The catalog is hand-editable, so an act written by hand is as valid as one written by a verb.

Policy chive cannot infer goes in `config.toml` as Rhai rules:

```toml
[[rules]]
name = "installer packages are disposable"
script = '''
  if path.ends_with(".apk") { "disposable" } else { () }
'''
```

## Verdicts

| Verdict | Meaning |
|---------|---------|
| `restorable` | Has a recipe (verified or user-supplied) |
| `unknown` | Matters and chive cannot rebuild it — a hole. Never cleaned. |
| `disposable` | The owner judged it a known gap. Clean may remove it. |

Only the owner (or an owner rule) can produce `disposable`; chive's own authority
stops at *provable-dead* — a symlink whose target no longer resolves, a removed
package's residue, a declared directory. Nothing is cleanable merely because
chive failed to explain it.

## Store layout

```
~/.config/chive/
├── config.toml      # ignore list, verdict rules, defaults
├── catalog.toml     # the catalog: entries + the owner act log (versionable)
└── catalog.db       # working index (derived from TOML)
```

The catalog TOML is the source of truth. SQLite is a derived index.

## MVP scope

- CLI with: scan, status, holes, plan, restore, teach, dispose, withdraw, clean, export, import, stats
- Provenance detection: package ownership, git work-tree, symlinks
- Three verdicts (restorable / unknown / disposable) with owner-only disposal
- Ordered owner-act log in the catalog, re-applied (never reordered) by rescan
- Rhai verdict rules in `config.toml`
- Provable-dead detection (dangling symlink, removed-package residue)
- TOML catalog format with concrete schema
- SQLite working index
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

MIT — see `LICENSE`.