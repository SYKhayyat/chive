# Target State — chive

This document is canonical. If code disagrees with this document, the code is wrong.

## What chive is

chive is the `home.nix` you never wrote: an imperative NixOS configuration, read
back off a machine that already exists.

It scans a machine, decides what matters, and for each meaningful file records a
recipe for re-deriving it. The collection of recipes is the catalog. When you
move to a new machine or lose files by accident, chive rebuilds what's missing
by running those recipes.

chive does not recover bytes. It re-derives: a package is reinstalled, a file is
re-exported from git, a symlink is recreated. A file that already exists is never
touched.

**The archive is the product.** Deleting is a side benefit: it happens where the
owner has already judged something disposable, and it is not a reason for a
verdict to exist (D18). Under the three-verdict model this is mechanical rather
than aspirational — the only thing that makes a file cleanable is a judgement
someone made, which is the same judgement the archive exists to record.

## The catalog is the point

Everything chive does serves one outcome: on a new or damaged machine, being able to see exactly what can be rebuilt and to rebuild it.

- Files are addressed by relative path, so the same catalog describes any machine.
- The catalog is plain text (TOML) that can be committed to git.
- The catalog must live off-box to be useful: it is committed to a repo or kept on storage that survives the box.
- Owner decisions live in the catalog too, not in local config, so a decision made on the old machine travels with it (D20).

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
`clean` re-check that a joined path still lands under the root before touching
the filesystem (defense in depth). A refused import leaves the store untouched.

Two things the re-check must do that a string prefix cannot:

- The check is against **canonicalised** paths. `starts_with` compares spellings,
  so a symlink under the root satisfies it while every later filesystem call
  follows the link out. `clean` *refuses* such an entry (exit 3) rather than
  skipping it, because a containment failure that exits 0 having removed something
  else is the failure this rule exists to prevent.
- The check is against **the root chive is about to act against**, not the root the
  catalog was written against. On a fresh machine those differ, and checking the
  catalog's own root would prove nothing about where the write lands.

`meta.root` itself is validated at construction: empty, relative, and `/` are
refused outright. `policy.catalog.root_scope` governs only the judgement call of
whether a root outside your home is acceptable.

## Policies (config.toml)

Two judgement calls live in `config.toml`, both three-level, both defaulting to
the answer that cannot lose data. `chive config init` writes a commented
template; `chive config show` prints what is actually in effect.

```toml
[policy.restore]
# What restore does when the file it is about to write already exists (D24).
#   refuse    — never overwrite. The default, and D15 as written.
#   backup    — copy the existing file to <dest>.chive-backup, then replace it.
#   overwrite — replace without asking. The escape hatch.
overwrite = "refuse"

[policy.catalog]
# How far outside your home an imported catalog's root may point (D25).
#   home-only — refuse a root outside your home.
#   warn      — accept it, and name it on import. The default.
#   any       — accept anything, silently.
root_scope = "warn"
```

`root_scope` defaults to `warn` rather than `home-only` because a catalog written
on another machine names *that* machine's root, which by construction is not your
home; refusing outside-home by default would refuse the migration chive exists
for. Independent of this setting, an empty, relative, or `/` root is refused at
construction — no configuration makes `/` acceptable, because it would place every
absolute path inside the root.

## Catalog root

Every catalog entry is addressed by `path` relative to the scan root. The scan root is the path passed to `chive scan` (e.g. `~/`). The catalog records the root at scan time.

When restoring on a new machine, the user supplies the equivalent root via `--root`. Recipes that place files use `{dest}` which expands to `<root>/<path>` at restore time.

## Verdicts

Every catalog entry is in exactly one of three states:

| Verdict | Meaning |
|---------|---------|
| `restorable` | The catalog holds a recipe. chive can re-derive this file. |
| `unknown` | It matters and chive cannot rebuild it. A **hole**. |
| `disposable` | The owner has judged it a known gap. Safe to delete. |

`unknown` is the resting state for anything chive cannot explain, and the
assumption is that it matters: a missed package probe, an unreachable repo, an
unfamiliar extension are all statements about *chive*, not about the file.

There is no "protected" verdict. A file with no recipe is already `unknown`, and
`unknown` is never cleanable, so a file cannot be deleted by chive failing to
understand it. See D19.

### What may assign a verdict

| Assigns | May return |
|---------|------------|
| Owner acts (`teach`, `dispose`, `withdraw`, or a hand-edited catalog) | all three |
| Provenance detection | `restorable` |
| Provable-dead detection | `disposable` |
| Owner `[[rules]]` | all three |

**Provable-dead** is the only automatic source of `disposable`, and it is
narrow on purpose. chive may call a file disposable only when it can *prove* the
file is already non-functional, never when it merely looks like junk:

- a symlink whose target does not resolve. This is not a corner case — a
  `/nix/store`-backed home directory is full of them once a generation is
  collected, and each one is a file that cannot do its job.

That is the whole list. Two further cases were considered and deliberately left
out:

- **Removed-package residue.** Proving it would mean remembering that a package
  once owned the file and checking it is gone; a remembered ownership guess is
  the same class of inference D19 removes. An owner who wants this states it as
  a rule.
- **A directory the owner has declared.** This is expressible as a rule, so the
  mechanism exists; making it a separate built-in would be the second way to say
  the same thing.

Name-based heuristics — `*.tmp`, a trailing `~`, `emacs-workfile-` — do **not**
qualify. Guessing from a filename is the inference D19 removed, so those are
demoted to rule suggestions the owner writes for themselves (see Rules below).

### How a verdict is assigned during scan

1. If the path is in an ignored directory — not cataloged at all.
2. If the owner log has an act for this path, the newest act governs (D20).
   A `teach` yields `restorable`; a `dispose` yields `disposable`; a
   `withdraw` means fall through and re-derive.
3. If an owner rule matches — the rule's verdict, marked as rule-origin.
4. If provenance is detected — `restorable`.
5. If the file is provably dead — `disposable`.
6. Otherwise — `unknown`.

Steps 4 and 5 are checked in that order, and the order is pinned by
`provenance_order_is_package_then_git_then_symlink_tests`: a live symlink is
`restorable` by `ln -s` even though a dangling one is `disposable`, and only a
symlink that fails to resolve reaches the provable-dead check.

### Verdicts and clean

`chive clean` removes only `disposable` files (with confirmation unless
`--force`). It can never touch `restorable` or `unknown`.

Because no automatic verdict is cleanable except provable-dead, `clean` is safe
by construction rather than by convention: the set it may remove is exactly the
set the owner (or a rule they wrote) has named.

## Rules

`config.toml` carries a `rules` list. Each rule is a Rhai script that returns a
verdict or nothing; rules run in order and the first non-nothing result wins.

```toml
[[rules]]
name = "installer packages are disposable"
script = '''
  if path.ends_with(".apk") { "disposable" } else { () }
'''
```

A script sees the file's `path`, `size`, `extension`, `is_symlink`,
`link_resolves`, `package`, and `in_ignored_dir`. It returns one of
`"restorable"`, `"unknown"`, `"disposable"`, or `()` for no opinion.

Rules are how the owner states the policy that `temporary` used to guess at. A
rule can see things a filename cannot — whether a symlink resolves, which
package owns the file — which is why the policy moved from a hardcoded
heuristic to the owner's config rather than being deleted with `temporary`.

A rule that matches assigns its verdict with `verdict_source = "rule"`, on every
verdict it can return — the origin drives stickiness (D22), and a rule is owner
policy, so a rule's `unknown` is as sticky as an owner's.

The engine is bounded in **time and space**: an operation cap, plus caps on
string, array and map size. The operation cap alone is not enough — a script can
stay inside its operation budget and still allocate gigabytes — so a runaway rule
is stopped and the failure names the bound it hit.

## Source

Every `restorable` entry has a `source`:

- `verified` — chive inferred the recipe (provenance detection).
- `user_supplied` — the owner supplied the recipe via `chive teach`.

`source` is about recipe provenance, not verdict provenance. The verdict's own
origin (owner / rule / chive) is recorded separately in `verdict_source`.

## Provenance detection (verified recipes)

When chive scans a file, it checks provenance sources in this order. The first
match wins. This order is load-bearing and pinned by tests
(`provenance_order_is_package_then_git_then_symlink_tests`):

1. **Package ownership**: The file is owned by an installed package.
   - Linux: `dpkg -S <abs>`, `pacman -Qo <abs>`, `rpm -qf --queryformat %{NAME} <abs>`, `apk info -W <abs>`, `xbps-query -o <abs>`
   - macOS: check if file is under a known brew/cellar path
   - Probes are answered per manager's real output; the probe answer is trimmed before the `name_match` regex runs against it. rpm/dnf are asked for the bare name (`--queryformat %{NAME}`) rather than parsed out of `name-version-release`.
   - Nix store files are deliberately not claimed: no package-manager verb re-derives a file from a derivation path, so they fall through to git/symlink/unknown.
   - Recipe: reinstall the owning package. For deb: `sudo apt-get install --reinstall <pkg>`. For pacman: `sudo pacman -S <pkg>`. For brew: `brew reinstall <pkg>`.
   - No `{dest}` substitution needed — the package manager places the file.

2. **Git work-tree**: The file is tracked by a git repo.
   - Detect: `git -C <dir> ls-files --error-unmatch <relpath>` walks up.
   - Recipe: `git -C '{root}/<repo-relative>' checkout HEAD -- <relpath>` when the
     repo sits under the scan root (portable); absolute `-C` when it does not
     (that recipe is honestly machine-bound).
   - `{dest}` is the file path. The repo must exist on the target machine (chive records the repo URL if available from remote config).

3. **Symlink**: The file is a symbolic link **and its target resolves**.
   - Recipe: `ln -s <target> <dest>`.
   - `{dest}` is the link path. `<target>` is the original symlink target.
   - A symlink whose target does *not* resolve is not claimed here. Re-running
     `ln -s` would recreate the same broken link, so the file falls through to
     the provable-dead check and becomes `disposable`.

4. **No provenance found** → `unknown`, unless an owner act or rule already
   decided the path.

## Owner acts

Owner acts are entries in one ordered log, stored in the catalog with a
monotonic sequence number. The newest act for a path governs, and a rescan
never reorders or erases the log — it reads it and re-applies it (D20).

```bash
chive teach <path> --method "<shell command>"   # -> restorable
chive dispose <path>                            # -> disposable
chive withdraw <path>                           # -> back to what chive can prove
```

### chive teach

```bash
chive teach conf/emacs.d/init.el --method "git -C ~/dotfiles pull && cp ~/dotfiles/emacs.d/init.el '{dest}'"
```

Records a recipe and appends a `teach` act for the path. The entry is
`restorable`, source `user_supplied`.

The recipe is a shell command. `{dest}` expands to the file's absolute path on
the target (root + relative path).

`teach` works on a path in any state, including one that is not on this machine
at all — teaching a recipe for a file you have not created yet is the core
planning workflow, so an absent path is accepted and the entry records that the
recipe exists but the file does not (see #45).

### chive dispose

```bash
chive dispose Pictures/photo.nef
```

Appends a `dispose` act: the owner has judged the file a known gap, so `clean`
may remove it. This is the only verb that makes a file cleanable, and it is the
replacement for the old `mark --status temporary` and `mark --status orphaned`
spellings.

### chive withdraw

```bash
chive withdraw Pictures/photo.nef
```

Removes the owner's act for the path. The entry then reverts to whatever chive
can prove: `restorable` if a recipe remains, `unknown` if none does. This needs
no rule of its own — it is what "recipe present ⇒ restorable, absent ⇒ unknown"
already says.

### The log is the interface

The verbs are conveniences. The catalog is hand-editable TOML, and an act
written by hand is exactly as valid as one written by a verb: append an entry to
`[[acts]]` with a higher `seq`. chive never requires the owner to use the CLI
to express a decision.

## Recipes and restore

A recipe is an executable shell command. During `chive restore`, each recipe is run via:

- Unix: `sh -c '<recipe>'`
- Windows: `cmd /c '<recipe>'`

Variables in the recipe are substituted before execution:

- `{dest}` — the absolute path where the file should appear on the target (`root + path`).
- `{root}` — the target machine's restore root. Recipes that address a resource
  beside the file itself (the git repo holding `{dest}`) use it, so the recipe
  never embeds the source machine's absolute layout.

Exit code 0 means success. Non-zero means failure; chive reports it and continues with other files. A restore run where any recipe failed exits non-zero (1) — every file is still attempted and reported, but the process must not claim success. `clean` follows the same rule: any path that could not be removed makes the exit non-zero.

### Restore behavior

- `chive restore <path...>` — restore named files.
- `chive restore --all` — restore every `restorable` file.
- Before executing, chive checks whether `{dest}` already exists, for **every** entry — the destination is `root + path`, not only for recipes that name `{dest}` literally. A git recipe places the file at exactly that path, so exempting it would leave every git-tracked file unprotected. If the file is there, `policy.restore.overwrite` decides: `refuse` (default) stops, `backup` copies it to `<dest>.chive-backup` first, `overwrite` replaces it.
- The check precedes any filesystem change, so a refused restore leaves no directories behind.
- A recipe that exits 0 but leaves no file at `{dest}` is a **failure**, reported with the missing path and a non-zero exit. A restore that did not happen is never reported as one.
- For a `{dest}` recipe, chive creates the destination's parent directory before running the recipe. A fresh machine has none of the directories the source layout implies; a recipe should describe how to re-derive the file, not the scaffolding around it.
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
├── config.toml          # ignore list, verdict rules, scan defaults
├── catalog.toml         # the catalog: entries + the owner act log (versionable)
└── catalog.db           # working index (derived, not committed)
```

`chive teach` writes to the catalog, not to a separate extension file. The
rationale is D20: recipes and verdicts are both owner intent, both must survive
a rescan, and both must travel to a new machine — splitting them across two
files is what let a verdict die while its recipe lived (issue #43).

The catalog TOML is the versionable artifact. It is written by `chive export`,
or by `chive scan --to <path>` directly to a path the user specifies.

## Catalog TOML schema

```toml
# catalog metadata
[meta]
root = "/home/user"          # scan root
scanned_at = "2026-09-04T12:00:00Z"
host = "desktop"
next_seq = 3                 # monotonic owner-act counter

# one entry per path
[[files]]
path = "conf/emacs.d/init.el"            # relative to root
verdict = "restorable"
category = "config"
restore_method = "git -C ~/dotfiles checkout HEAD -- conf/emacs.d/init.el"
source = "user_supplied"
verdict_source = "owner"
present = true                # false for a taught recipe with no file here
size = 2048
modified = "2026-08-15T10:00:00Z"

[[files]]
path = "Documents/notes.pdf"
verdict = "restorable"
category = "document"
restore_method = "apt-get install --reinstall poppler-utils"
source = "verified"
verdict_source = "chive"
present = true
size = 1048576
modified = "2026-01-15T10:00:00Z"

[[files]]
path = "Pictures/photo.nef"
verdict = "disposable"
category = "image"
restore_method = null
source = null
verdict_source = "owner"
present = true
size = 25000000
modified = "2025-12-01T14:00:00Z"

[[files]]
path = ".gtkrc-2.0"
verdict = "disposable"          # symlink target does not resolve
category = "config"
restore_method = null
source = null
verdict_source = "chive"
present = true
size = 30
modified = "2026-10-06T00:54:00Z"

# the ordered owner-act log. Newest `seq` for a path governs.
[[acts]]
seq = 1
path = "conf/emacs.d/init.el"
kind = "teach"                  # teach | dispose | withdraw

[[acts]]
seq = 2
path = "Pictures/photo.nef"
kind = "dispose"
```

Fields:
- `path` (string, required) — relative to root.
- `verdict` (string, required) — `restorable`, `unknown`, or `disposable`.
- `category` (string or null) — the category, or null when unclassified.
- `restore_method` (string or null) — the recipe. Null unless restorable.
- `source` (string or null) — `verified` or `user_supplied`. Null unless restorable.
- `verdict_source` (string) — `owner`, `rule`, or `chive`. Drives stickiness (D22).
- `present` (boolean) — whether the path exists on the machine that wrote this
  catalog. `false` means a recipe was taught for a file that is not here, which
  is legitimate when planning a new machine (#45).
- `size` (integer) — bytes, informational.
- `modified` (string or null) — ISO 8601, informational.

Act fields:
- `seq` (integer, required) — monotonic. `meta.next_seq` is the next value to use.
- `path` (string, required) — relative to root.
- `kind` (string, required) — `teach`, `dispose`, or `withdraw`.

A `withdraw` act does not carry a verdict; it removes the path's owner override
so the entry re-derives.

## SQLite schema (working index)

```sql
CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE files (
    path           TEXT PRIMARY KEY,  -- relative to root
    verdict        TEXT NOT NULL,     -- restorable|unknown|disposable
    category       TEXT,              -- document|image|code|...|null
    restore_method TEXT,              -- null unless restorable
    source         TEXT,              -- verified|user_supplied|null
    verdict_source TEXT NOT NULL,     -- owner|rule|chive
    present        INTEGER NOT NULL,  -- 0|1
    size           INTEGER,
    modified       TEXT               -- ISO 8601
);

CREATE TABLE acts (
    seq   INTEGER PRIMARY KEY,
    path  TEXT NOT NULL,
    kind  TEXT NOT NULL              -- teach|dispose|withdraw
);
```

`meta` stores: `schema_version`, `root`, `scanned_at`, `host`, `next_seq`.

## CLI reference

### chive scan

```bash
chive scan <path> [--to <file>] [--ignore <pattern>...]
```

Crawls `<path>`, runs provenance detection and owner rules, re-applies the owner
act log, and writes the catalog. With `--to`, the catalog TOML is written
directly to that path — the archive is updated by the scan itself, not by a
separate manual export (issue #44).

Output:
```
Scanned 12,403 files:
  8,201 restorable (verified)
    312 restorable (user-supplied)
  3,750 unknown (holes)
    140 disposable (owner 96, provably dead 44)
```

### chive status

```bash
chive status [--restorable | --unknown | --disposable]
```

Lists entries filtered by verdict.

Output:
```
restorable (verified):
  Documents/notes.pdf           document   apt-get install --reinstall poppler-utils

restorable (user-supplied):
  conf/emacs.d/init.el          config     git -C ~/dotfiles checkout HEAD -- conf/emacs.d/init.el
  conf/zshrc                    config     cp ~/dotfiles/zshrc '{dest}'
```

### chive holes

```bash
chive holes [--limit <n>]
```

The work list: every `unknown` entry, largest first, each row carrying the act
that would close it. This is the primary read path under D18 — the backlog of
things chive cannot rebuild is what the owner most wants to see.

Output:
```
14,002 holes (312 MB) — nothing chive can rebuild yet

  250 MB  Pictures/2024/            teach a recipe, or dispose
   38 MB  Documents/                teach a recipe, or dispose
  2.1 MB  conf/emacs.d/init.el      restorable (user-supplied)
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

Appends a `teach` act to the catalog. The entry becomes `restorable`, source
`user_supplied`, `verdict_source` `owner`. Accepts a path that is not present on
this machine.

Output:
```
taught: conf/emacs.d/init.el
  method: git -C ~/dotfiles pull && cp ~/dotfiles/emacs.d/init.el '{dest}'
```

### chive dispose

```bash
chive dispose <path>
```

Appends a `dispose` act. The entry becomes `disposable` and `clean` may remove
it. This is the only verb that makes a path cleanable.

Output:
```
disposed: Pictures/photo.nef
```

### chive withdraw

```bash
chive withdraw <path>
```

Removes the owner's act for the path, so the entry re-derives from evidence.

Output:
```
withdrew: Pictures/photo.nef -> unknown
```

### chive clean

```bash
chive clean [--dry-run] [--force]
```

Removes `disposable` files. Without `--force`, prompts for confirmation.
`--dry-run` prints what would be removed.

Output:
```
Would remove 140 file(s):
  Pictures/photo.nef
  .gtkrc-2.0              (dangling symlink)
  ...
Remove? [y/N]
```

### chive export

```bash
chive export [--to <file>]
```

Writes the catalog as TOML, including the owner act log. Default: `catalog.toml`
in the config directory.

### chive import

```bash
chive import [--from <file>]
```

Loads a catalog from TOML, act log included. Replaces the working index.

### chive stats

```bash
chive stats
```

Leads with the hole count, because that is the number the owner can act on
(D18). Percentages come after.

Output:
```
Catalog: 12,403 files, 245 MB

  holes (unknown):        3,891 (31%)     <- nothing chive can rebuild yet
  restorable:             8,372 (67%)
    verified:             8,201 (98% of restorable)
    user-supplied:          171 (2% of restorable)
  disposable:               140 (1%)
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