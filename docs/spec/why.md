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

## Verdicts: restorable / unknown / disposable

Three states cover every file without forcing false certainty.

- `restorable` means "we know how to rebuild this."
- `unknown` means "this matters and we can't rebuild it — yet." It is a hole in
  the archive, and it is the largest and most actionable number chive reports.
- `disposable` means "the owner judged this a known gap." Safe to clean.

The old four-status model answered two questions with one enum, and that is
where the damage came from. "Chive cannot explain this file" and "this file is
safe to delete" are unrelated claims, but `orphaned` asserted both at once — and
since a failed package probe or an unrecognised extension produced exactly that
value, chive's own blindness was recorded as permission to delete. A rule that
makes "I don't understand this" indistinguishable from "this is safe to remove"
will eventually delete something the owner valued, and the owner has no way to
audit the difference because the catalog does not record one.

Three verdicts fix it by removing the claim chive cannot make. `unknown` is
the resting state for anything unexplained and is never cleanable, so safety is
the *default* rather than something the owner has to remember to apply. Only
`disposable` is cleanable, and only an owner (or a rule the owner wrote) can
produce it. `clean` is now safe by construction: the set it may remove is
exactly the set someone named.

**Protection is not a state.** The old `not-restorable` existed to mean "don't
touch this," which implied protection was an act you had to perform. Under the
three-verdict model a file with no recipe is already `unknown` and already
untouchable, so the protection verb had nothing left to do and was deleted
rather than deprecated. A safety property that holds by default beats a safety
property the owner must apply.

## Provable-dead is the only automatic disposable

If nothing automatic may produce `disposable`, something must: otherwise the
`.apk` files and the dead symlinks pile up as holes forever and `clean` never
does anything on a fresh install. The line is drawn at *provable* — chive may
call a file disposable only when it can show the file is already
non-functional:

- a symlink whose target does not resolve. On a `/nix/store`-backed home
  directory this is not an edge case; a collected generation leaves dozens
  behind, and every one of them is a file that cannot do its job.
- a file whose owning package is no longer installed.
- a file inside a directory the owner declared.

Name-based heuristics are excluded on purpose. `*.tmp`, a trailing `~`,
`emacs-workfile-` — these were the `temporary` heuristic, and they are guesses
about a file nobody described. A guess that lands on "delete this" is the
failure mode D19 exists to remove. The same policy is still *available*, but
the owner states it as a rule in their own config, where "`.apk` files are
disposable" is an assertion about their machine rather than chive's guess about
every machine.

A dangling symlink is `disposable` rather than `restorable` for a specific
reason: `ln -s <target> <dest>` would faithfully recreate the same broken link.
A recipe that cannot produce a working file is not a recipe, and claiming
`restorable` would put a no-op in the restore plan and report success.

## Rules are the owner's policy, in a sandboxed language

The temporary heuristic existed to make `clean` useful on day one. Deleting it
with `temporary` would leave the owner no way to say "`.apk` is disposable"
except writing thousands of catalog entries by hand, so the capability moved to
`config.toml` as Rhai scripts rather than being dropped.

The language choice is constrained by what chive ships: a **musl-static**
binary, which its Alpine and Void integration targets depend on. Embedded Python
requires glibc's `libpython`, so it would break the static build outright.
Rhai is pure Rust and therefore survives it.

Rhai is also sandboxed, with no filesystem or process access unless explicitly
granted. That matters while issue #14 is open — chive fabricating shell from
untrusted data is a live security boundary, and a rule language that could
spawn a process would re-open that entire class of exposure through the config
file. Rules are a decision surface, not an execution surface.

## Owner intent is one ordered log (D20)

#43 is a data-loss chain assembled from two independent mistakes. `mark` a file
protected → rescan → the verdict is gone → an older taught recipe re-applies →
`clean` deletes it.

The two halves are the same mistake wearing different clothes. `mark` wrote only
to the catalog the scanner then replaced, so the verdict never survived a scan.
And scan rule 2 re-applied any taught recipe over any verdict, so even a verdict
that *had* survived was overruled by a decision the owner had already moved
past. Owner intent was modelled as mutable state *of the scan* rather than as a
durable record the scan *reads*, which is why two separate fixes were both
needed and why the bug had a data-loss tail rather than a wrong-output tail.

The log fixes both halves at once. Every owner act — `teach`, `dispose`,
`withdraw` — is an entry with a monotonic sequence number in the catalog. The
newest act for a path governs, so "last thing I said wins" is the whole
precedence rule; there is no table of which class of decision outranks which.
And a rescan reads the log and re-applies it, so it cannot erase or reorder an
act: a verdict survives rescan because the scanner is downstream of the record,
not upstream of it.

The log lives in the catalog rather than in `config.toml` for a reason that is
about the product, not the mechanics: the catalog is the file that travels
off-box. A verdict kept in config stays behind with the old machine, so a new
machine would rebuild a file the owner had already ruled disposable and would
have no record of the ruling. Putting acts in the catalog also settles the
write-path half of #44 for free — a verdict that is in the catalog is carried by
`export` with no extra plumbing to add or forget.

`withdraw` needs no rule of its own. Removing the act means the entry re-derives,
and re-deriving already says what it says: recipe present ⇒ `restorable`, no
recipe ⇒ `unknown`.

## Owner verdicts are sticky, chive's own are not

Stickiness is only a question for verdicts chive *inferred*. An owner verdict
has nothing to be sticky about — it is the newest word and only the owner may
supersede it, which is the whole of D20.

For chive's own inferences the asymmetry is load-bearing in one direction: a
recomputing scan can **retract** its inference, and that is what makes the
provable-dead rule honest. A dangling symlink gets repointed; the next scan
sees a resolving link and promotes it to `restorable` via `ln -s`. If inferred
verdicts were sticky, that stale `disposable` would outrank the fresh evidence
and `clean` would delete a link the owner had just repaired.

Sticky *inferred* verdicts are configurable, and the hazard is recorded here on
purpose: a verdict nothing may revise is a verdict nothing may correct. The
setting re-opens #43 through the config file — a wrong `disposable` becomes
permanent — so it is legitimate only for an owner who has audited a subtree and
wants it frozen. Rule-origin verdicts default to sticky for the opposite reason:
a rule is owner-authored policy, so chive should not second-guess it — but
editing or deleting the rule withdraws it, which is the escape hatch that
inferred verdicts lack.

## Owner verbs are conveniences, the catalog is the interface

The TOML is hand-editable and an act written by hand is exactly as valid as one
written by a verb: append to `[[acts]]` with a higher `seq`.

This is not a nicety. The archive is committed to a repo, so the owner will
eventually want to move a decision in a diff, drop a hundred acts that are no
longer relevant, or script a batch — and a format only reachable through a CLI
cannot be reviewed in a diff. The verbs exist because typing a TOML entry is
worse, not because the file is an implementation detail.

## The archive is the product (D18)

chive's value is answering "what can I rebuild, and how" on a machine that is
not the one it was born on. The register shows how much attention drifted the
other way: D10 spent a ruling on a clean confirmation prompt while the archive's
headline migration flow was broken (#35) and verdicts never reached the export
(#44), D14 shaped the status model around what `clean` could safely delete, and
`stats` led with percentages rather than holes.

Cleaning is a *consumer* of a verdict, not a reason for one. Under the
three-verdict model that stops being a philosophical position and becomes
mechanical: the only thing that makes a file cleanable is a judgement someone
made, and that judgement is the same judgement the archive is built to record.

So the read path follows the product: `holes` lists the backlog largest-first
with the act that would close each row, and `stats` leads with the hole count
because it is the only number on that screen the owner can do something about.
Percentages are still there, below.

## hole is the unit of work

A hole is a file the archive cannot rebuild. Naming it makes it a task with a
closing action rather than a status to be tolerated, which is why `holes` got a
verb instead of a flag: the loop the product exists to support — see what has no
recipe, teach it — had no way to be run. The old `README.md:61` documented a
"teach me" view that never existed, which is worse than having said nothing.

`not_restorable_reason` went the other way. It was in the model, both schemas,
and the spec, and every construction site wrote `None`: a committed column that
always reads NULL is a claim the schema makes and the code does not keep. Under
the three-verdict model it has nothing to record — `unknown` has no reason,
because "chive could not explain it" *is* the reason. The column is deleted
rather than filled in.


## Source: verified vs user_supplied

Separating the recipe from its trust level keeps the verdict clean. A file is restorable regardless of who provided the recipe. The `source` flag tells the user whether to trust the recipe without chive's endorsement. It is also separate from `verdict_source`, which records who decided the *verdict* — the two questions ("how do I rebuild it" and "why do I trust that") have different answers and different lifetimes, and collapsing them is how `restorable` came to mean "chive checked this."

## Provenance detection order

Package ownership is checked first because it's the most reliable and portable: a package manager always knows where a file goes. Git is second because it's common for config and code, and the recipe is deterministic (checkout from HEAD). Symlinks are third because they're simple and self-describing.

The order matters: a file owned by a package that happens to be in a git repo should use the package recipe (more portable across machines) rather than the git recipe (requires the repo to exist).

The order was once reversed in code (symlink first, "because it's cheap") —
issue #12. The price of a probe is not the price of a recipe: symlink is still
the cheapest *probe*, but precedence decides which recipe the machine runs
next year, and a portable recipe is worth a few microseconds at scan time. The
documented order is now pinned by tests so the two can't drift apart again.

## Teach, dispose, withdraw

`chive teach` writes a recipe the owner supplies, never program code. This is
the direct lesson of Shall's adapter system: the tool is extended by writing the
thing it reads. Teaching a new restore source never requires recompiling.

Teaching works on a path in any state, and it records the fact that the owner
said so rather than merely outranking the current answer. See the log rule above
for why that is now an ordered act rather than a precedence rule.

## Clean semantics

`clean` removes only `disposable` files. It never touches `restorable` or
`unknown`. This is the safety guarantee, and under the three-verdict model it is
structural rather than a convention: no automatic verdict is cleanable except
provable-dead, so the set `clean` may remove is exactly the set a person named.

Confirmation is required unless `--force` is passed. This mirrors Shall's
removal guard (U26 rule): an action that deletes must be previewed and
confirmed. `--dry-run` prints what would be removed without removing it.

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

## Recipes never embed the source machine's layout

A recipe is read on the machine it rebuilds, so an absolute path in it is a
fossil of wherever the catalog was born (issue #28): `git -C /home/alice/...
checkout` can only fail on machine B — or, worse, succeed against an unrelated
directory that happens to sit at that path. The repo is therefore addressed
relative to the scan root with a `{root}` token and resolved at restore time,
exactly like `{dest}`. A repo outside the scan root cannot be given a portable
relative address, so its recipe stays absolute — an honest machine-bound
recipe is better than one that silently lies about portability.

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

"Truth" is enforceable only if the derived store is never *read* as one. A
fallback from missing-TOML to the SQLite index lets the stalest copy win
exactly when the owner believes they have no catalog (issue #22): delete the
catalog, and the last index resurrects it — including entries for files long
gone. Every command therefore reparses the TOML; the index is written, never
consulted for truth. Delete the truth and chive says so (exit 4, "scan or
import first").

## {dest} for package-owned files

Package-owned files don't use `{dest}` because the package manager decides where they go. The recipe is "install the package" and the file appears at the OS-determined location. This is simpler and more portable than trying to replicate the package manager's layout logic.

## Plan restore

`plan restore` exists because restore is the product, and a restore is an act that should always be previewable. This is the same read-then-act discipline as `shall --dry-run sync`. The "seeing what can be rebuilt and how" is half the feature.

## Non-goals

Byte recovery and inode forensics are a different product (see D0): chive promises a recipe, not resurrection of unlinked data. The restore manager (D2) is deliberately kept out of the MVP so the core restore loop is proven before a manager sits on top of it.

## The code is a suggestion (D17)

The implementation predates the settled purpose, so it encodes a model chive no longer holds: four statuses designed to make `clean` safe, a flat catalog with no hierarchy, and a `mark` that a rescan erases. Those are not decisions anyone made — they are what happened while the shape was still being guessed.

That is why the ruling is permission to rewrite rather than a footnote. Reading the existing code as a contract preserves the accidents along with the intent, and holding it fixed makes the *correct* changes (#38's three verdicts, #41's hierarchy, #43's durable verdicts) harder than keeping the wrong ones. The spec is the contract; the source is scaffolding nobody is depending on.

The failure mode this guards against is not recklessness but **timidity**. An agent that makes minimal additive changes because breaking something feels risky produces a model accreted by defensive patches — four statuses nobody chose, a `mark` nobody tested past a reload, a flat catalog that never grew a parent column. Every one of those looks like reasonable caution and all of them are the disease. So: rewrite the module, change the schema, delete the test that pins an unruled shape. A large honest rewrite that lands the purpose beats a small safe patch, and beats a patch so careful it never breaks anything — which is precisely how the current model happened.

The limit is the same as before, and it is about *quality* rather than design: a defect that deletes a file the owner protected (#43) is not softened by the code around it being provisional, a containment check is not a suggestion because the module holding it is, and none of this excuses skipping the verify chain. Bold in the rewrite, rigorous in the verification.
