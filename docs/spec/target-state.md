# Target State — chive

This document is canonical. If code disagrees with this document, the code is wrong.

## What chive is

chive scans a machine, decides what matters, and for each meaningful file records a recipe for re-deriving it. The collection of recipes is the catalog. When you move to a new machine or lose files by accident, chive rebuilds what's missing by running those recipes.

chive does not recover bytes. It re-derives: a package is reinstalled, a file is re-exported from git, a symlink is recreated. A file that already exists is never touched.

## The catalog is the point

Everything chive does serves one outcome: on a new or damaged machine, being able to see exactly what can be rebuilt and to rebuild it.

- Files are addressed by relative path, so the same catalog describes any machine.
- The catalog is plain text (TOML) that can be committed to git.
- The catalog must live off-box to be useful: it is committed to a repo or kept on storage that survives the box.

## Path containment (security boundary)

A catalog is a file that may have come from anywhere — another machine, another
person. Every entry path in a catalog is therefore constrained, and the
constraint is enforced in `Catalog` construction so an invalid path cannot exist
in memory:

- relative to the scan root — never absolute, never a drive prefix;
- `/`-separated, no empty segments, no `.` or `..` segments;
- no backslash (a backslash is a legal Unix filename character, but it is how a
  Windows-authored catalog would smuggle a second separator past a join);
- no NUL.

`chive import` refuses a catalog whose entries break the rule; `restore` and
`clean` re-check that a joined path still falls under the root before touching
the filesystem (defense in depth). A refused import leaves the store untouched.

## Path containment (security boundary)

A catalog is a file that may have come from anywhere — another machine, another
person. Every entry path in a catalog is therefore constrained, and the
constraint is enforced in `Catalog` construction so an invalid path cannot exist
in memory:

- relative to the scan root — never absolute, never a drive prefix;
- `/`-separated, no empty segments, no `.` or `..` segments;
- no backslash (a backslash is a legal Unix filename character, but it is how a
  Windows-authored catalog would smuggle a second separator past a join);
- no NUL.

`chive import` refuses a catalog whose entries break the rule; `restore` and
`clean` re-check that a joined path still falls under the root before touching
the filesystem (defense in depth). A refused import leaves the store untouched.

## Catalog root

Every catalog entry is addressed by `path` relative to the scan root. The scan root is the path passed to `chive scan` (e.g. `~/`). The catalog records the root at scan time.

When restoring on a new machine, the user supplies the equivalent root via `--root`. Recipes that place files use `{dest}` which expands to `<root>/<path>` at restore time.

## Statuses

Every file in the catalog has exactly one status:

| Status | Meaning |
|--------|---------|
| `restorable` | This file can be re-derived. The catalog has a recipe. |
| `not-restorable` | This file cannot be re-derived. The catalog records why. The owner can teach it a recipe to promote it to restorable. |
| `temporary` | Transient file. Safe to clean. Never a restore target. |
| `orphaned` | File present on disk but with no provenance and no taught recipe. Candidate for cleanup. |

### How status is assigned during scan

The decision is mechanical and deterministic, and one order governs it:

1. If the file is in an ignored directory (see Ignore list below) — not cataloged at all.
2. If a user-taught recipe matches — `restorable`, source `user_supplied`. **A taught recipe is the owner's explicit word and overrules every automatic answer**, including an inferred recipe and even a temporary bloom (see D16).
3. If the file matches a temporary-file heuristic — `temporary`.
4. If provenance is detected (see Provenance detection below) — `restorable`, source `verified`.
5. Otherwise — `orphaned`.

`not-restorable` is never assigned automatically. It is only assigned when the owner explicitly marks a file: `chive mark <path> --status not-restorable` moves an `orphaned` file to `not-restorable`, meaning "this file matters, I don't know how to rebuild it, don't clean it." This is the starting point for teaching.

### Status and clean

`chive clean` removes only `temporary` and `orphaned` files (with confirmation unless `--force`). It never touches `restorable` or `not-restorable`.

## Source

Every `restorable` entry has a `source`:

- `verified` — chive inferred the recipe (provenance detection).
- `user_supplied` — the owner supplied the recipe via `chive teach`.

`source` is about recipe provenance, not file provenance. It stays separate from `status`.

## Provenance detection (verified recipes)

When chive scans a file, it checks provenance sources in this order. The first
match wins. This order is load-bearing and pinned by tests
(`provenance_order_is_package_then_git_then_symlink_tests`):

1. **Package ownership**: The file is owned by an installed package.
   - Linux: `dpkg -S <abs>`, `pacman -Qo <abs>`, `rpm -qf --queryformat %{NAME} <abs>`, `apk info -W <abs>`, `xbps-query -o <abs>`
   - macOS: check if file is under a known brew/cellar path
   - Probes are answered per manager's real output; the probe answer is trimmed before the `name_match` regex runs against it. rpm/dnf are asked for the bare name (`--queryformat %{NAME}`) rather than parsed out of `name-version-release`.
   - Nix store files are deliberately not claimed: no package-manager verb re-derives a file from a derivation path, so they fall through to git/symlink/orphan.
   - Recipe: reinstall the owning package. For deb: `sudo apt-get install --reinstall <pkg>`. For pacman: `sudo pacman -S <pkg>`. For brew: `brew reinstall <pkg>`.
   - No `{dest}` substitution needed — the package manager places the file.

2. **Git work-tree**: The file is tracked by a git repo.
   - Detect: `git -C <dir> ls-files --error-unmatch <relpath>` walks up.
   - Recipe: `git -C <repo> checkout HEAD -- <relpath>`.
   - `{dest}` is the file path. The repo must exist on the target machine (chive records the repo URL if available from remote config).

3. **Symlink**: The file is a symbolic link.
   - Recipe: `ln -s <target> <dest>`.
   - `{dest}` is the link path. `<target>` is the original symlink target.

4. **No provenance found** → not restorable via verification. Falls through to orphaned (unless taught).

## Taught recipes (user_supplied)

The owner teaches chive a recipe via:

```bash
chive teach <path> --method "<shell command>"
```

This writes a recipe to the extension file (see Store layout below). The entry becomes `restorable`, source `user_supplied`.

A taught recipe works on a file in **any** of the four statuses: teaching an
`orphaned`, `not-restorable`, `temporary`, or already-`restorable` (inferred)
file promotes it to `restorable` with `user_supplied`, overriding whatever chive
would otherwise have inferred. This is the owner's explicit word, and it wins
over automatic detection (see D16).

The recipe is a shell command. `{dest}` expands to the file's absolute path on the target (root + relative path). Example:

```bash
chive teach conf/emacs.d/init.el --method "git -C ~/dotfiles pull && cp ~/dotfiles/emacs.d/init.el '{dest}'"
```

Taught recipes are also editable by hand in the extension file (TOML format).

## Marking files (the unified verb)

`chive mark <path> --status <not-restorable|temporary|orphaned>` sets a file's
status explicitly. It is the single verb that covers what was once two separate
actions and more:

- `--status not-restorable` — the old `protect`: "this matters, don't clean it."
- `--status temporary` — "this is transient, safe to clean."
- `--status orphaned` — "no recipe, no protection; cleanable."

Marking always clears any existing recipe (a marked file is never restorable).
Use `mark not-restorable` then `teach` to turn a protected file into a
rebuildable one.

## Recipes and restore

A recipe is an executable shell command. During `chive restore`, each recipe is run via:

- Unix: `sh -c '<recipe>'`
- Windows: `cmd /c '<recipe>'`

Variables in the recipe are substituted before execution:

- `{dest}` — the absolute path where the file should appear on the target (`root + path`).

Exit code 0 means success. Non-zero means failure; chive reports it and continues with other files. A restore run where any recipe failed exits non-zero (1) — every file is still attempted and reported, but the process must not claim success. `clean` follows the same rule: any path that could not be removed makes the exit non-zero.

### Restore behavior

- `chive restore <path...>` — restore named files.
- `chive restore --all` — restore every `restorable` file.
- Before executing, chive checks if `{dest}` already exists. If it does, chive **refuses to act** on that file (no clobber). The user must delete or move it first.
- `chive plan restore` — preview every restore action without executing. Prints the recipe and `{dest}` for each file. This is the "see what can be restored and how" view.

## Categories

Each file is classified into one category by extension. Categories are flavor — they describe what a file is, not what to do with it. A category never implies restorability.

| Category | Extensions |
|----------|-----------|
| `document` | pdf, doc, docx, txt, md, rtf, odt, xls, xlsx, ppt, pptx, tex |
| `image` | png, jpg, jpeg, gif, svg, webp, bmp, tiff, tif |
| `code` | rs, py, ts, js, go, cpp, c, h, java, rb, lua, sh, zsh, bash, el, nix |
| `config` | toml, yaml, yml, json, xml, conf, ini, env |
| `program` | deb, rpm, exe, appimage, snap, dmg |
| `audio` | mp3, wav, flac, aac, ogg, m4a |
| `video` | mp4, mkv, avi, webm, mov |
| `archive` | zip, tar, gz, bz2, xz, rar, 7z |
| `data` | csv, sql, db, bin, parquet |

Compound extensions: match on the last extension but special-case known compounds: `.tar.gz` → archive, `.tar.xz` → archive, `.tar.bz2` → archive, `.jpeg` → image (alias for jpg).

## Ignore list

Directories matching these patterns are never scanned:

- `.git`
- `.svn`
- `node_modules`
- `target`
- `__pycache__`
- `.cache`
- `dist`
- `build`
- `.next`
- `.nuxt`

The ignore list is stored in the config file and is user-editable.

## Store layout

The config directory defaults to `~/.config/chive/`. It is a git-worthy directory (the versionable part).

```
~/.config/chive/
├── config.toml          # ignore list, scan defaults
├── recipes.toml         # user-taught recipes (extension file)
└── catalog.db           # working index (derived, not committed)
```

The catalog TOML (the versionable artifact) is written by `chive export` or by `chive scan` directly to a path the user specifies. For cross-machine use, the user commits `catalog.toml` to a repo.

## Catalog TOML schema

```toml
# catalog metadata
[meta]
root = "/home/user"          # scan root
scanned_at = "2026-09-04T12:00:00Z"
host = "desktop"

# one entry per file
[[files]]
path = "conf/emacs.d/init.el"            # relative to root
status = "restorable"
category = "config"
restore_method = "git -C ~/dotfiles checkout HEAD -- conf/emacs.d/init.el"
source = "verified"
size = 2048
modified = "2026-08-15T10:00:00Z"

[[files]]
path = "Documents/notes.pdf"
status = "restorable"
category = "document"
restore_method = "apt-get install --reinstall poppler-utils"
source = "verified"
size = 1048576
modified = "2026-01-15T10:00:00Z"

[[files]]
path = "Pictures/photo.nef"
status = "orphaned"
category = "image"
restore_method = null
source = null
size = 25000000
modified = "2025-12-01T14:00:00Z"

[[files]]
path = "tmp/emacs-workfile-~"
status = "temporary"
category = null
restore_method = null
source = null
size = 0
modified = "2026-09-04T11:59:00Z"
```

Fields:
- `path` (string, required) — relative to root.
- `status` (string, required) — one of the four statuses.
- `category` (string or null) — the category, or null for temporary/unclassified files.
- `restore_method` (string or null) — the recipe. Null unless restorable.
- `source` (string or null) — `verified` or `user_supplied`. Null unless restorable.
- `size` (integer) — bytes, informational.
- `modified` (string or null) — ISO 8601, informational.
- `not_restorable_reason` (string or null) — present only if status is `not-restorable`.

## Extension file (recipes.toml)

User-taught recipes live here. `chive teach` writes to this file.

```toml
[[recipe]]
path = "conf/emacs.d/init.el"
method = "git -C ~/dotfiles pull && cp ~/dotfiles/emacs.d/init.el '{dest}'"
```

Fields:
- `path` (string, required) — relative to root.
- `method` (string, required) — the shell recipe. `{dest}` is substituted at restore time.

## SQLite schema (working index)

```sql
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE files (
    path                  TEXT PRIMARY KEY,  -- relative to root
    status                TEXT NOT NULL,      -- restorable|not-restorable|temporary|orphaned
    category              TEXT,               -- document|image|code|...|null
    restore_method        TEXT,               -- null unless restorable
    source                TEXT,               -- verified|user_supplied|null
    not_restorable_reason TEXT,               -- null unless not-restorable
    size                  INTEGER,
    modified              TEXT                -- ISO 8601
);
```

`meta` stores: `schema_version`, `root`, `scanned_at`, `host`.

## CLI reference

### chive scan

```bash
chive scan <path> [--ignore <pattern>...]
```

Crawls `<path>`, runs provenance detection, writes entries to the catalog and the catalog TOML.

Output:
```
Scanned 12,403 files:
  8,201 restorable (verified)
    312 restorable (user-supplied)
    847 not-restorable
  2,903 temporary
    140 orphaned
```

### chive status

```bash
chive status [--restorable | --not-restorable | --temporary | --orphaned]
```

Lists entries filtered by status.

Output:
```
restorable (verified):
  conf/emacs.d/init.el          config     git -C ~/dotfiles checkout HEAD -- conf/emacs.d/init.el
  Documents/notes.pdf           document   apt-get install --reinstall poppler-utils

restorable (user-supplied):
  conf/zshrc                    config     cp ~/dotfiles/zshrc '{dest}'
```

### chive plan

```bash
chive plan restore [--root <path>]
```

Preview every restore action without executing. Shows the resolved `{dest}` for each file.

Output:
```
Plan: 3 file(s) to restore

  git -C ~/dotfiles checkout HEAD -- conf/emacs.d/init.el
  -> /home/newuser/conf/emacs.d/init.el

  apt-get install --reinstall poppler-utils
  -> (placed by package manager)

  cp ~/dotfiles/zshrc /home/newuser/conf/zshrc
  -> /home/newuser/conf/zshrc
```

### chive restore

```bash
chive restore <path...> [--root <path>]
chive restore --all [--root <path>]
```

Runs each recipe. Refuses if `{dest}` already exists. Reports success/failure per file.

Output:
```
restored: conf/emacs.d/init.el
restored: Documents/notes.pdf
failed:   conf/zshrc — exit code 1
  cp: cannot stat '/home/newuser/dotfiles/zshrc': No such file or directory
```

### chive teach

```bash
chive teach <path> --method "<shell command>"
```

Writes a recipe to `recipes.toml`. The entry becomes `restorable`, source `user_supplied`.

Output:
```
taught: conf/emacs.d/init.el
  method: git -C ~/dotfiles pull && cp ~/dotfiles/emacs.d/init.el '{dest}'
```

### chive mark

```bash
chive mark <path> --status <not-restorable|temporary|orphaned>
```

Sets a file's status explicitly (the unified verb). `mark` clears any existing
recipe, so a marked file is never restorable.

```bash
chive mark Pictures/photo.nef --status not-restorable
```

Output:
```
marked: Pictures/photo.nef → not-restorable
```

### chive clean

```bash
chive clean [--scope <temporary|orphaned|both>] [--dry-run] [--force]
```

Removes `temporary` and/or `orphaned` files, scoped by `--scope` (default
`both`). Without `--force`, prompts for confirmation. `--dry-run` prints what
would be removed.

Output:
```
Would remove 3,043 file(s) (2,903 temporary, 140 orphaned):
  tmp/emacs-workfile-~
  .cache/thumbnails/...
  ...
Remove? [y/N]
```

### chive export

```bash
chive export [--to <file>]
```

Writes the catalog as TOML. Default: `catalog.toml` in the config directory.

### chive import

```bash
chive import [--from <file>]
```

Loads a catalog from TOML. Replaces the working index.

### chive stats

```bash
chive stats
```

Output:
```
Catalog: 12,403 files, 245 MB
  restorable:       8,513 (69%)
  not-restorable:     847 (7%)
  temporary:        2,903 (23%)
  orphaned:           140 (1%)
  verified:          8,201 (96% of restorable)
  user-supplied:       312 (4% of restorable)
```

## Non-goals

- Byte-level recovery of deleted or unlinked file content. chive re-derives from a source, it does not recover raw bytes.
- Block/FS forensics (inode or MFT parsing) for unlinked data.
- Cloud sync of the catalog itself (user brings their own version control or storage).
- The restore manager (periodic/triggered restore when files go missing). This is open question D2, deliberately not part of the MVP.

## Platform support

- Linux / NixOS
- macOS
- Windows

## Implementation note

This spec is language-independent. The current working assumption is Rust (pending decision D1), but the data model, CLI interface, and catalog format are defined here and do not depend on the implementation language.