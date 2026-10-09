# PLAN — chive (greenfield, ds-era scaffold; safety before features)

Worker loop: top unchecked item only, fix + test, commit, check off, stop.
Note: all 11 commits are ds-era (scaffolded 09-05/06) — review as new code, not fixes.

## The code is a suggestion — change it drastically

The implementation predates the settled purpose (#40). It is not a foundation to
build on and not a shape to be careful with. **A drastic rewrite is the expected
outcome here, not a risk to be managed.** If the right fix is to delete a module
and write it again, delete it and write it again.

Do not be gentle with the existing code. Specifically, do not:

- make minimal, additive, defensive changes because breaking something feels
  risky — timid patching is exactly what produced a model nobody chose;
- preserve a signature, module boundary, flag name, table column or schema
  shape because something already depends on it;
- keep a test that pins behaviour nobody ruled on;
- contort a fix to fit the current architecture instead of reshaping the
  architecture to fit the fix;
- treat an existing file as if a human were depending on its current contents.
  Nobody is. It is scaffolding.

The spec is the contract, not the source. Nothing in `src/` needs an owner's
permission to be rewritten.

Two things are **not** provisional, and are not a licence to rewrite:

- **Real defects still get fixed**, in the same change, with a resolving test.
  #32, #33, #34 and #43 destroy data through ordinary commands and no error
  anywhere. "The code was only a suggestion" is never a reason to leave one open.
- **Security boundaries and the verify chain stay.** Path containment
  (`target-state.md:19-34`), the shell-fabrication gap in #14, and
  `CLAUDE.md:46-48`.

Rewrite boldly, then verify hard. A large honest rewrite that lands the purpose
beats a small safe patch that keeps the wrong model — and beats a timid patch
that is careful never to break anything, which is how the current model happened.

Ruled as **D17**; rationale in `docs/spec/why.md`.

## Phase 0 — Framing (RULED 10-06)
- [x] #40 chive is the home.nix you never wrote: an imperative NixOS, read back from the machine. The archive is the product; cleaning is a side benefit. — **RULED 10-06 as D18.** Lands first so #38/#39/#41/#42 cite it instead of re-deriving it. Evidence of misallocated attention: D10 ruled a `clean` confirmation prompt while the archive's own headline flow stayed broken (#35); D14 designed the status model around `clean`; `stats` leads with percentages instead of holes. Consequence now in the spec: `holes` is the primary read path, `stats` leads with the hole count.

## Phase 1 — Safety foundations (nothing restores until these land)

**From the 2026-10-06 lamdan audit** (`docs/lamdan/whole-repo-2026-10-06.md`). All four verified against the release binary; each destroys files and exits 0.

- [x] #47 `Catalog.root` was unvalidated, so an imported catalog chose where `clean` deleted — **FIXED (bcc7552) with D25.** Refused at `Catalog::new`; the judgement half is `policy.catalog.root_scope`. Verified: `root = "/"` refused; a root outside home exits 3 under `home-only`, and is accepted *and named* under `warn`.
- [x] #48 `contained()` was `starts_with`, so a symlink escaped — **FIXED (bcc7552).** `resolve_under` canonicalises the deepest existing ancestor and re-checks; `clean` refuses (exit 3) rather than skipping. Verified: a `sub/link -> ../../outside` entry is refused and the outside file survives.
- [x] #21 **REOPENED then FIXED (bcc7552) with D24** — no-clobber was a substring test, so no git-tracked file was protected. Every entry now gets a `dest`; `policy.restore.overwrite` decides (`refuse`/`backup`/`overwrite`). Verified: a local edit in a dotfiles repo survives `restore`, and `backup` preserves the old bytes at `<dest>.chive-backup`.
- [x] #49 `restored:` meant "exit 0" — **FIXED (bcc7552).** A zero exit that leaves no file is a failure naming the path, exit 1. Shall's rule: a fetch that quietly returned nothing would report success over a command that never ran.
- [x] #32 scan silently clobbers the existing catalog: a nonexistent root scans as an EMPTY catalog (walk error swallowed), a subroot scan replaces the whole-HOME catalog — both exit 0. (Critical) — **FIXED (55d0d02 + a60e080).** Both halves now refuse. The root is canonicalized before the walk in `App::scan` (#34), so an unresolvable path fails rather than scanning empty; and `App::scan` — which already read the existing catalog for its act log — now also compares that catalog's root against the target and refuses (exit 3) when they differ, leaving the file untouched. Same directory by another spelling (symlink, trailing slash) is accepted so a rescan never refuses for the same tree. Escape hatch is the existing global `--config-dir` (one root, one store) rather than a new `--force`. audit 09-19, sandbox-verified.
- [x] #33 chive catalogs its own store files (store dir never excluded from scan); `clean --scope orphaned` deletes catalog.db/catalog.toml, resurrected only by the trailing save. (High) — **FIXED (2c5673d).** `App::scan` resolves the store directory and hands it to the scanner as a *path*-based skip, not a basename in the ignore list — a directory the owner named `chive` is still theirs to catalog. **Re-audit 10-09 against the D19 model, as the 10-06 note asked for:** with three verdicts, `clean` only removes `disposable`, so the self-deletion half had shrunk to the provable-dead and owner-disposed paths. It is now closed off entirely rather than narrowed, because the store is never cataloged at all, so no verdict path can reach it. Verified by hand plus test. audit 09-19, sandbox-verified.
- [x] #34 relative scan root recorded as-is ("."): plan/restore/clean resolve it against the invocation cwd — clean from elsewhere reports removals that never happened and silently drops live entries. (High) — **FIXED (55d0d02).** The root is canonicalized inside `App::scan`, the one place the real filesystem path is known, so the catalog records an absolute, symlink-free path no cwd can change. `canonicalize` fails only for a nonexistent path, which also lands the first half of #32 (an unresolvable root refuses rather than scanning empty). Verified by hand and by test. The filed resolving test had a wrong entry path and was corrected rather than the code bent to fit it.
- [x] #43 an owner verdict is erased by the next scan and silently overruled by an older taught recipe. (Critical) — **FIXED 10-06 with D20:** verdicts and recipes are one ordered act log in the catalog (`[[acts]]`, monotonic `seq`), newest act per path wins, rescan re-applies and never reorders. `teach`/`dispose`/`withdraw` are its verbs. This dissolved the data-loss chain at its root rather than guarding one arm. Three defects found on the way: `Catalog::record` validated nothing (so `teach ../escape` exited 0 until the next load — #17's family), the dangling-symlink check was gating all provenance so a package-owned dangling link came back `disposable`, and an empty log bumped its counter so it did not round-trip. Pinned by `owner_decisions_survive_a_rescan_tests`.
- [x] #17 path escape: import/restore/clean can write/delete outside scan root → normalize+contain. (Critical) — FIXED: containment rule enforced in `Catalog` construction + action-time re-checks; spec rule V-path-containment.
- [x] #18 restore exits 0 on failure → real exit codes (note: #13 is the same bug, DUP — work once). (High) — FIXED: restore exits 1 when any recipe fails (all still run + report); clean exits 1 when any removal fails; Real::remove_file treats an absent path as the wanted end state.
- [x] #12 provenance order contract (docs say pkg→git→symlink, code runs reverse) → pick one, enforce. (High) — FIXED: code now runs the documented package→git→symlink order; pinned by provenance_order_is_package_then_git_then_symlink_tests.
- [x] #10 SQLite-vs-TOML truth decision → record, then fix #22 full reparse/rebuild. (Medium) — FIXED (D9 already ruled TOML-truth): `load_catalog` no longer falls back to reading the derived SQLite index — a stale index cannot impersonate a deleted/replaced catalog. Every command reparses the TOML (the "rebuild" #22 asked for is the only read path).

## Phase 2 — Correctness
- [x] #50 the package ownership probe ran **twice** per file — **FIXED (9ca1a1d).** Probed once and threaded forward; `owning_package` deleted. The blind spot: #30's regression test asserted only on `version_probe_count()`, so it could not see this class at any size.
- [x] #51 `owns_under` prefix prefilter — **FIXED (9ca1a1d).** Measured on 400 files: 400 ownership probes → 0, 2.58s → 0.076s. Two wrong versions came first (a raw `starts_with` skipped every adapter on a home scan; a tail match was wrong because a prefix names a directory).
- [x] #55 a user adapter could not override a *confidently wrong* built-in — **FIXED (d6da240).** A colliding `name` replaces in place and keeps its position; `read_dir` order sorted; PermissionDenied no longer swallowed as "no adapters dir". Not done: incoherent `Manager` rows still fail silently rather than at load.
- [x] #56 "apply the governing act to this path" has two owners (`scan.rs::entry_for` and inline in `cmd_teach`/`cmd_dispose`) and they have already diverged: `withdraw` on a taught-but-absent path claims `present = true`. Also two whole-catalog saves per `withdraw`, the first of which persists a catalog where the log says withdrawn and the view still says cleanable. **This is what makes #41 cheap.** — **FIXED (8cd1457).** `App::rederive` is now the single owner of "what does this path look like now"; `teach`/`dispose`/`withdraw` route through `record_act` → `rederive` → one save, and `entry_from_act` takes `present`/`size`/`modified` from the lstat rather than a constructor's hardcode. The three closure bodies (~60 lines) are deleted along with the duplicate save. Pinned by `one_rule_one_owner_tests`, which asserts the property (a rescan must not change what a verb wrote) rather than any one verb's behaviour.
- [x] #54 a rule-authored `unknown` was stamped `Origin::Chive` — **FIXED (e3e6084).** `new_hole` takes the origin; D22's stickiness now holds for rules on every verdict. Took the cheap option; narrowing D21's rule vocabulary would have been a register item.
- [x] #35 README migration flow restores into the OLD machine's root: `restore --all` without `--root` expands `{dest}` against the imported stale root; imported orphans restore invisibly. (High) — **FIXED (f29e50c).** One `default_restore_root` helper, shared by `plan restore` and `restore` (which had the expression inline): the catalog's root when it exists on this machine — so a catalog scanned from `/etc` still restores into `/etc` and the same-machine case is unchanged — and this machine's home when it does not, which is what an imported catalog always is. Verified by hand both ways. Second half closed by D19, not code: `orphaned` no longer exists, so there is no entry type to skip invisibly. README now says why `--root` is still shown, since it is no longer required. audit 09-19, sandbox-verified.
- [x] #36 catalog records host = "unknown": `hostname()` reads only `$HOSTNAME`, unset in non-interactive shells. (Medium) — **FIXED (2e6ce3c).** `gethostname(2)` through the `libc` already in the tree, no new dependency. `$HOSTNAME` kept as a fallback for a failing syscall, then `"unknown"` — a missing hostname must not stop a scan. Verified by hand with `HOSTNAME` unset and set to a wrong value. Takes the `HOSTNAME` half of #57 with it; `CHIVE_DRY_RUN` leaking into the suite is still open there. audit 09-19, sandbox-verified.
- [x] #38 the four statuses answer two questions at once, so "chive cannot explain this file" is recorded as "safe to delete" — three verdicts (restorable / unknown hole / disposable known-gap), unknown never cleanable. (High) — **FIXED 10-06 with D19:** `Status` -> `Verdict`, four -> three. Only the owner (or an owner rule) may produce `disposable`; chive's own authority is limited to provable-dead, now just dangling symlinks (see D21 note). `protect`/`not-restorable`/`mark` deleted as verbs, not deprecated — protection is the default rather than an act. Supersedes D14. Rules engine landed with it (D21, Rhai; musl-static forbids embedded Python). Implements the root of #15.
- [ ] #44 a verdict never reaches the archive: `save_catalog` always writes the store default and has no destination parameter, so `export --to` is a separate manual step and every state change after the last export is invisible to a rebuild. Also `target-state.md:230` promises `scan` writes "directly to a path the user specifies" — the code has no such flag. (High) — **per-file half RULED 10-06 as D20:** acts live in the catalog, so `export` carries them with no extra plumbing to add or forget. **Remaining:** implement `scan --to <path>`, which the spec promises and the code lacks.
- [x] #45 a recipe taught for a path absent from this machine is stored only in recipes.toml, so it is dropped by export/import and never reaches a new machine. **Revises #37**: the absent-path case is legitimate (planning a new machine), so rejecting the path is the wrong fix — the catalog needs to express "I know how to rebuild this, it is not here". (High) — **FIXED 10-06:** expressed as the `present = false` catalog field, and the recipe rides *in the act*, so it reaches a new machine through export with no join against a sidecar file. recipes.toml is gone (D20). Pinned by a two-machine test in `owner_decisions_survive_a_rescan_tests`.
- [x] #19/#29 nested git basename (same root — work once). (High) — FIXED: detector probes the repo-relative path instead of the basename; unit + harness tests pin it.
- [x] #21 no-clobber brittle + TOCTOU. (High) — FIXED: no-clobber uses lstat (`action::present`) so a dangling symlink counts as present; TOCTOU documented as inherent to the check-then-act seam (see why.md).
- [x] #26 no mkdir -p of dest parents. (High) — FIXED: restore creates the `{dest}` parent directory through the Runner seam (no-op under dry-run), after the no-clobber check; migration test no longer needs `mkdir -p` in the taught recipe.
- [x] #28 absolute recipes kill portable restore. (High) — FIXED: git recipes address the repo with a `{root}` token (scan-root-relative), expanded at restore time from `--root`; a repo outside the scan root stays absolute (honestly machine-bound). Pinned by a two-machine end-to-end test.
- [x] #20/#30 exists storm (same root as #24/#25 — adapters fixed separately) — FIXED: manager availability hoisted to once per scan (program names, not manager names); per-file loop consults the hoisted list.
- [x] #24/#25 apk/rpm/xbps adapters fixed (dnf audited; nix removed) — FIXED: rpm/dnf probe `--queryformat %{NAME}` (exact bare name, no regex guessing); apk regex anchored on real path-first output; xbps probes `-o` (ownership) not `-f` (file listing) and parses `pkg-ver_rel:`; probe answers trimmed before matching; nix adapter removed — its recipe could never succeed and its comment promised orphan-fallback.

## Phase 3 — Decisions + hygiene (D1–D13, in dependency order)
- [x] #10 the SQLite index was a **write-only sink** — **DELETED.** `src/catalog/db.rs` (345 lines), the `rusqlite` dependency, `Store::db_file`/`DB_FILE`, and five domain `FromStr` impls that existed only for `db::load`. Three non-test callers, all writes; `db::load` and the version gate had zero. D9 **amended, not contradicted**: an index can only pay off by avoiding the parse, and D9 forbids avoiding the parse. Also drops a bundled C amalgamation the 7m42s release build was mostly waiting on. The store is now one file. Shall has no index at 142k lines either.
- [ ] #8 D1 language ruling → #4 D5 cross-platform → #2 D3 absence → #1 D2 manager → #3 D4 off-box sync → #5 D7 taxonomy → #6 D12 GUI → #7 D13 license (MIT vs All-rights-reserved + missing LICENSE file).
- [x] #31 schema_version dead — FIXED: `replace` stamps the index version it writes; `open` refuses a version row it cannot parse as a number (was: any read failure silently coerced to "current", disabling the gate); writers use `open_for_write` so a wrongly-versioned index can always be repaired by rescanning.
- [x] #27 --all parity — FIXED: `restore --all` (and `plan restore --all`) is the documented explicit spelling of the default; conflicts with explicit paths (exit 2). Pinned by CLI tests. Remaining open: #23 dead-code sweep, #15/#16 defaults drift, #9 matrix, and the Phase-3 decision line.

## Phase 4 — Quality (docs + UX)
- [x] #52 the container harness asserted deleted verdict spellings, so every distro FAILed — **FIXED.** `work~` and `conf.md` now assert `unknown` (D19 retired the name heuristic), a dangling-symlink fixture asserts `disposable` so the container layer exercises provable-dead, and the nested-git check is hard because #19 is fixed. `soft()` had no callers left and is deleted. Verified all seven assertions against the real binary with the script's own awk helpers. The README results table is replaced with what the layer has caught plus its known limits, because a hand-kept table rotted once already.
- [x] #53 the musl-static constraint had no reproducible build path — **FIXED.** `docker/integration/build-static.sh` is committed, builds the musl target, and *asserts* the result is static rather than trusting the build. `rust-version` pinned at 1.87, which clippy's incompatible_msrv immediately enforced. `run.sh`'s recovery instruction pointed at a dynamic build into a gitignored dir. `harness/run.sh` now checks for `rg` and reports an `--ignored` PASS as "drop the #[ignore]" instead of the opposite.
- [ ] #61 spec CLI output blocks are unchecked by anything and have already drifted six ways, including `clean --dry-run` documented as prompting for confirmation. Convert to golden-output assertions.
- [ ] #59 `Evidence`/`Decision` split — four constructors are one entry shape with four flags, and the hardcoded origin in `new_unknown` is where #54 leaks. **Do before #41, not during**: `Origin` names *who* but not *which decision*, so a folder verdict inherited by a child cannot say so — exactly the lie #41 forbids flat rendering from telling.
- [ ] #57 `CHIVE_DRY_RUN` leaks into the test suite (verified: 3 failures), and `HOSTNAME` into every catalog. `Real::dry_run` fakes *results* rather than skipping actions, so a dry-run scan's verdicts differ from a real scan's — and app.rs's unit tests assert those from a fiction.
- [x] #58 `hermetic_git_env_once` was a real data race — **FIXED (e3e6084).** `GIT_ENV` is applied to every child *and* to `git_repo`; the `Once`, the `unsafe`, and the SAFETY comment asserting the race did not exist are deleted. SIGPIPE moved from `cli::run` into `main`.
- [ ]  asserts on stdout strings; the catalog TOML is the machine-readable product and is read exactly once, by hand. Plus #60 (D21 rules and D22's retracting half have zero end-to-end coverage).
- [ ] #62 `docs/SUMMARY.md` is 85% derivative with zero inbound links and asserts a capability deleted two commits ago.
- [ ] #37 teach accepts a nonexistent path and reports success (exit 0) while mark rejects it — sibling validation drift. (Low) — audit 09-19, sandbox-verified. **NARROWED 10-06:** the absent-path half is wrong-by-framing and is owned by #45 (that case is legitimate, not drift); #37 keeps only the honest half — malformed or root-escaping paths must be rejected by both verbs.
- [x] #46 no work list: the "see what has no recipe → teach it" loop has no verb. `status` is a flat dump, `stats` leads with percentages, and README.md:61 documents a "teach me" view that does not exist. `not_restorable_reason` is in the model, the TOML, the SQLite index and the spec, and **every construction site writes None** — dead column in a committed schema. (Medium) — **FIXED 10-06 with D23:** `chive holes` lists unknowns largest-first, each row carrying the act that closes it; `stats` leads with the hole count and demotes percentages; `not_restorable_reason` **deleted** (under D19 `unknown` has no reason — "chive could not explain it" is the reason).

## Phase 5 — Features (the archive is the product; cleaning is a side benefit)
Dependency chain — **#38 → #39 → #41 → #42**, with **#43** feeding #41 (it is the per-file half of #41's "the catalog carries owner decisions" ruling; a verdict that is not durable cannot live in the catalog). They share one root (a folder is not a first-class thing) and were split only because the model, the write path, and the read path are separately verifiable. An AI picking up any of them reads all five first.

- [ ] #39 teach/archive at folder scope: one line per file *or folder*, expanding to per-file entries underneath; `restore` runs a folder recipe once, not once per descendant. (Medium) — extends D16 from per-file to folder scope; the `{dest}`-per-file-vs-per-folder question is **owner-only**. *Write path.*
- [ ] #41 the archive has no hierarchy: a folder cannot carry a verdict that cascades to what's inside it, and nothing groups by folder (flat `[[files]]`, no parent column, no path-shaped filter anywhere in the CLI). (Medium) — *Model + read path.* Hard prerequisite for #42. **RULED 10-06:** folder verdicts live in the **catalog**, not config.toml (the chive file is what travels off-box, so a verdict kept in config stays behind with the old machine); precedence is **deepest wins** (file > nearer folder > root default); `restorable` outranks `disposable` at every level; **both nested and flat renderings must work everywhere** (status/stats/export/GUI) — one model, two views, flat must still show inherited verdicts and their origin so it cannot lie. Still open: whether the committed TOML is grouped-canonical (my reading: yes, flat is a derived sort).
- [ ] #42 end goal: a WizTree-style treemap of the machine, boxes colored by the three verdicts, top pane showing the verdict + the resolved recipe, fixable in place. (Medium) — **D12 framework stays open**; layout/IA/interaction only, no code. **RULED 10-06:** unaccounted boxes are drawn muted and **hover explains why** (status bar, as WizTree does); zoom is inherited from WizTree (double-click to zoom, `..`/`\` to leave, F10/F11); inherit bidirectional treemap↔list selection, accessible palette presets, and outline-as-separate-channel for holes. Last item, not the second.

- [ ] #63 **replace-heuristics as named callable strategies** — a recipe is one flat shell string per file (`src/provenance/mod.rs:34`), `cmd_teach` takes the command inline (`src/cli.rs:502`), and `Config` has no strategy surface at all (`src/config.rs:159-171`), so the same recipe is copy-pasted by hand per file and a change means editing N catalog entries. Both halves exist in other shapes: the package adapters already have a named, parameterized restore template that user adapters override by `name` (#55), and D21 put owner policy in `config.toml` as callable `[[rules]]`. The join is what is missing. **Owner call first** (D5 per-OS variants, D16 taught-recipes-overrule, and the D20 tension: a strategy named in the catalog lives in `config.toml`, which is not what travels off-box). (Medium) — filed 10-09.

## Audit of record

Whole-repo design critiques live in `docs/lamdan/`. The most recent is
`whole-repo-2026-10-06.md` (8 regions, 25 ranked findings, 4 of them verified
data-loss paths). It names four things it could not beat, and records three of
its own claims that died under verification.

**Read it before starting any Phase 1–4 item filed after 2026-10-06** — several
of those findings change what the fix should be, not just how hard it is.

**Still open from that audit:** #56 (apply-the-log has two owners and has diverged; this is
what makes #41 cheap), #59 (Evidence/Decision split — do before #41), #60 (the
suite asserts on stdout; D21 rules and D22's retracting half have no end-to-end
coverage), #61 (spec output blocks unchecked, six drifts), #62 (SUMMARY.md stale),
#10 (delete the write-only SQLite index — Shall has no index at 142k lines). Plus
one filed late: an incoherent `Manager` row still fails silently rather than at
load, which was part of #55 and deliberately not half-landed with it.

## Routing rule for new issues
Any AI opening an issue here MUST put safety/boundary items in Phase 1 and decisions in Phase 3 order — never append a feature above #17/#18. Duplicates of one root cause get folded into the existing line. See AI_ISSUE_ROUTING.md.
