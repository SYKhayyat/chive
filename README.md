# chive

The `home.nix` you never wrote — an imperative NixOS configuration, read back off a machine that already exists.

chive scans a machine and keeps a recipe for getting everything back. Move to a new machine — or lose files by accident — and chive re-creates what's missing from those recipes.

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

# see what you cannot rebuild yet, largest first
chive holes

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

The `{dest}` variable expands to the file's absolute path on the target machine. Taught recipes are stored in the catalog and can be edited by hand.

## Verdicts

Every entry is in exactly one of three states.

| Verdict | Meaning | What happens to it |
|---------|---------|-------------------|
| `restorable` | Has a recipe | Can be restored |
| `unknown` | Matters, and chive cannot rebuild it — a hole | Never cleaned. Listed by `chive holes` |
| `disposable` | You judged it a known gap | `clean` may remove it |

A file with no recipe is `unknown`, which is never cleanable. chive deleting
something is always a consequence of a judgement you made, never of chive
failing to understand a file.

To call a file disposable, say so:

```bash
chive dispose conf/manual.nef      # clean may now remove it
chive withdraw conf/manual.nef     # take the judgement back
```

Owner acts are an ordered log in the catalog, so the last thing you said wins
and a rescan never erases it. The catalog is hand-editable, so you can also just
edit it.

## Cleaning

```bash
chive clean --dry-run    # preview what would be removed
chive clean              # confirm, then remove disposable files
```

`clean` only ever touches `disposable` files.

## Settings

```bash
chive config init     # write a commented config.toml
chive config show     # what chive is actually using
```

Two judgement calls, both three-level, both defaulting to the answer that cannot
lose data:

```toml
[policy.restore]
# refuse (default) | backup | overwrite — what to do when the file already exists
overwrite = "refuse"

[policy.catalog]
# home-only | warn (default) | any — how far outside your home an imported
# catalog's root may point
root_scope = "warn"
```

`root_scope` defaults to `warn` rather than `home-only` because a catalog written
on another machine names *that* machine's root, so refusing outside-home would
refuse the migration chive exists for. chive names the root when it imports one,
which is the only moment you can notice it is wrong.

An empty, relative, or `/` root is refused regardless — no setting makes `/`
acceptable, because it would place every absolute path inside the root.

## Rules

Tell chive what is disposable in your own words, in `config.toml`:

```toml
[[rules]]
name = "installer packages are disposable"
script = '''
  if path.ends_with(".apk") { "disposable" } else { () }
'''
```

Rules are [Rhai](https://rhai.rs) scripts — sandboxed, no filesystem or process
access. A rule sees `path`, `size`, `extension`, `is_symlink`, `link_resolves`,
`package`, and `in_ignored_dir`, and returns a verdict or `()`.

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
| `chive config init` | Write a commented `config.toml` with every setting at its default |
| `chive config show` | Print the settings actually in effect |
| `chive holes` | List what cannot be rebuilt, largest first |
| `chive teach <path> --method "<cmd>"` | Teach a recipe |
| `chive dispose <path>` | Judge a file a known gap (cleanable) |
| `chive withdraw <path>` | Take your judgement back |
| `chive clean [--dry-run]` | Remove disposable files |
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

MIT — see [LICENSE](LICENSE).