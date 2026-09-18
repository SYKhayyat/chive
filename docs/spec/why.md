# Why — chive

Every rule in `target-state.md` has a matching explanation here. If a rule changed, this file changed in the same commit.

## Path containment

A catalog path is data from outside the machine — it arrives by `import`, and
`restore` and `clean` act on it with real filesystem writes and deletes. An
unvalidated `root.join(path)` is therefore a stranger's instruction to write
outside everything chive owns: `../..` in a crafted catalog is all it takes
(issue #17). The rule is enforced once, at `Catalog` construction, so no
invalid path can exist in memory and every later join is safe by construction;
restore/clean re-check containment as defense in depth. Fail closed at the
boundary (`import` refuses, store untouched) beats scrubbing paths mid-flight,
because a half-sanitized traversal is the bug that comes back.

## Catalog root and {dest} substitution

The catalog stores relative paths so it works on any machine. But recipes need to know where to write on the target. The `{dest}` variable solves both: the catalog is portable, and the recipe knows its destination at restore time. This mirrors how Shall's module grammar works — the data is relative, the machine resolves it. The root is supplied at restore time (`--root`), not baked into the catalog, because the same catalog describes machines with different home directories.

## Statuses: restorable / not-restorable / temporary / orphaned

Four statuses cover every file without forcing false certainty.

- `restorable` means "we know how to rebuild this."
- `not-restorable` means "this matters and we can't rebuild it — yet." It's a protected state: not cleaned, surfaced in the "teach me" view.
- `temporary` means "this was never meant to persist." It's safe to clean and never a restore target.
- `orphaned` means "we found this but can't explain it." It's the default for unknown files, safe to clean unless the owner protects it.

The split between `orphaned` and `not-restorable` is the key design choice. `orphaned` is the default for files with no provenance — it's safe to clean. `not-restorable` is an explicit owner action (`chive protect`) meaning "I know this matters, I can't rebuild it, don't touch it." This makes `chive clean` safe by default: it only removes things chive can't explain, and the owner must explicitly protect files that matter.

If both were one status, `clean` would either be too aggressive (deleting files that matter) or too conservative (refusing to clean files that are junk). Separating them makes `clean` safe without requiring the owner to curate every file.

## Source: verified vs user_supplied

Separating the recipe from its trust level keeps the status clean. A file is restorable regardless of who provided the recipe. The `source` flag tells the user whether to trust the recipe without chive's endorsement.

## Provenance detection order

Package ownership is checked first because it's the most reliable and portable: a package manager always knows where a file goes. Git is second because it's common for config and code, and the recipe is deterministic (checkout from HEAD). Symlinks are third because they're simple and self-describing.

The order matters: a file owned by a package that happens to be in a git repo should use the package recipe (more portable across machines) rather than the git recipe (requires the repo to exist).

The order was once reversed in code (symlink first, "because it's cheap") —
issue #12. The price of a probe is not the price of a recipe: symlink is still
the cheapest *probe*, but precedence decides which recipe the machine runs
next year, and a portable recipe is worth a few microseconds at scan time. The
documented order is now pinned by tests so the two can't drift apart again.

## Teach and protect

`chive teach` writes to an extension file (recipes.toml), never to program code. This is the direct lesson of Shall's adapter system: the tool is extended by writing the thing it reads. Teaching a new restore source never requires recompiling.

**A taught recipe overrules inference.** The owner's explicit recipe is stronger evidence of intent than any automatic provenance detection, because the owner is the one who says "rebuild it this way". Teaching therefore works on a file in any of the four statuses — including one chive already marked `restorable (verified)` or `temporary` — and always promotes it to `restorable (user_supplied)`. See D16.

`chive mark` is the unified verb for setting an explicit status (`not-restorable`, `temporary`, or `orphaned`), folding the old separate `protect` verb (and its siblings) into one command. Marking always clears a recipe, because a status that is not `restorable` and a recipe are contradictory claims. `mark --status not-restorable` then `teach` is the two-step path to rebuilding a file the scan could not explain.

## Clean semantics

`clean` removes only `temporary` and `orphaned` files. It never touches `restorable` or `not-restorable`. This is the safety guarantee: nothing that chive knows how to rebuild, and nothing the owner has explicitly protected, is ever removed.

Confirmation is required unless `--force` is passed. This mirrors Shall's removal guard (U26 rule): an action that deletes must be previewed and confirmed. `--dry-run` prints what would be removed without removing it.

## Package adapters parse what the manager actually prints

An adapter regex written against imagined output is a bug that only a real
machine can catch (issue #24: apk's output is path-first, xbps's `-f` flag
lists a package's files instead of answering ownership, rpm glued the version
into the recipe). Three disciplines keep the adapters honest:

- **Ask the tool for the exact answer when it can give one.** rpm and dnf take
  `--queryformat %{NAME}` and print the bare package name — no regex has to
  guess where a name ends and a version begins.
- **Anchor on the documented shape when it can't.** apk's "<path> is owned by
  <pkg>-<ver>-r<rev>" and xbps's "<pkgver>: <path> (<type>)" are parsed from
  their real, documented formats.
- **Trim before matching.** Probe answers end with a newline; a line-anchored
  regex is written against the line, not the raw bytes.

A probe that answers something no verb can restore from is worse than no
probe: the nix adapter named a `.drv` derivation path that only exists on the
source machine's store, so the honest behaviour is to claim nothing and let
the file fall through.

## Manager availability is asked once per scan

Package detection consults each adapter per file, and each argv adapter used to
re-ask `exists(program)` per file — a `--version` subprocess per manager per
file, O(files × managers) for an answer that cannot change mid-scan (issue
#20). Availability is hoisted: once per scan, then the per-file loop consults
the hoisted list. The list names *probe programs* (`xbps-query`), the same key
the per-file gate compares, not manager names (`xbps`) — a mismatch there
would silently disable a manager while looking fully correct in tests that
never spell out the difference.

## Failure is visible in the exit status

`chive restore` is run by scripts and by people; both read the exit code as the
claim "everything I asked for happened." A run that restored nine files and
failed on the tenth must not exit 0 — that claim is a lie the shell then acts
on (issue #18, dup #13). Every file is still attempted and reported — the
per-file reporting is the product — but the final status carries the worst
outcome. `clean` obeys the same honesty rule: a path it could not remove is a
failure, not a shrug.

## Restore creates the destination's parent

A catalog from machine A records `conf/emacs.d/init.el`; machine B has never
heard of `conf/emacs.d/`. Without parent creation, every taught recipe that
writes into a nested path fails on exactly the machine a restore is for —
unless the owner hand-pollutes the recipe with `mkdir -p`, which is
scaffolding, not a recipe (issue #26). chive creates the parent through the
Runner seam, after the no-clobber check (a refused restore must not leave
directories behind), and never for recipes without `{dest}` — package
managers place their own files.

## Restore: no clobber

Refusing to overwrite an existing file is the line between "reconstruct" and "overwrite." Reconstruction fills gaps; it does not replace what's already there. If the user wants to replace, they delete or move the existing file first. This is the same discipline as Shall's `sync` — check before acting.

"Existing" means *occupying the path*, which is an lstat question, not an
`exists()` question: a dangling symlink is a real occupant (issue #21), and
clobbering it would destroy a link the owner is about to point somewhere new.
The check-then-act gap (the path appearing between the lstat and the recipe's
own write) is inherent to a shell-recipe seam — chive cannot make the recipe
atomic — so the rule is checked at the last moment chive controls, and the
recipe itself remains the owner's contract.

## Recipes as shell commands

A recipe is a shell command, not a label. This means chive can restore anything that has a shell-invocable recipe, regardless of the source: package manager, git, curl, custom script. The recipe is the contract between the catalog and the machine.

## Categories as extension lists

Extension-based classification is the MVP because it's simple and deterministic. MIME-type detection is a future improvement (D7) that can be added without changing the catalog format — the `category` field is a string, not an enum in the storage.

Compound extensions (`.tar.gz`) are special-cased because they're common and the last-extension rule gets them wrong. The list is short and stable.

## Catalog TOML as truth

The TOML file is the versionable artifact meant to be committed off-box. SQLite is a derived working index rebuilt from the TOML. This makes the catalog portable (commit the TOML, import on the new machine) and avoids the sync ambiguity of dual stores. See D9.

## {dest} for package-owned files

Package-owned files don't use `{dest}` because the package manager decides where they go. The recipe is "install the package" and the file appears at the OS-determined location. This is simpler and more portable than trying to replicate the package manager's layout logic.

## Plan restore

`plan restore` exists because restore is the product, and a restore is an act that should always be previewable. This is the same read-then-act discipline as `shall --dry-run sync`. The "seeing what can be rebuilt and how" is half the feature.

## Non-goals

Byte recovery and inode forensics are a different product (see D0): chive promises a recipe, not resurrection of unlinked data. The restore manager (D2) is deliberately kept out of the MVP so the core restore loop is proven before a manager sits on top of it.