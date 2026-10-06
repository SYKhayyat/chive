# PLAN — chive (greenfield, ds-era scaffold; safety before features)

Worker loop: top unchecked item only, fix + test, commit, check off, stop.
Note: all 11 commits are ds-era (scaffolded 09-05/06) — review as new code, not fixes.

## The code is a suggestion

Every line of `src/` is **provisional**. It was scaffolded before the purpose was
settled (#40), against a model the purpose no longer supports: four statuses
designed to make `clean` safe, a flat per-file catalog with no hierarchy, and a
`mark` verb that does not survive a rescan. None of that is a contract, and no
worker should reverse-engineer intent from it.

**Freely changeable, without asking:** the status model and its names, the CLI
surface and its flags, the catalog TOML and SQLite schemas, the store layout,
the module structure, and any test that pins *provisional behaviour* rather than
a rule in `target-state.md`.

**Not excused by this:**

- **Real defects are still real defects.** #32, #33, #34 and #43 destroy data
  through ordinary commands with no error anywhere. Being provisional does not
  make them tolerable, and "the code was only a suggestion" is never a reason to
  leave one open.
- **Security boundaries are not suggestions** — path containment
  (`target-state.md:19-34`) and the shell-fabrication gap in #14.
- **The verify chain still runs** (`CLAUDE.md:46-48`).
- **No shim preserves an unruled shape.** If a thing is replaced, the old thing
  goes with it, including its tests (`CLAUDE.md:32-34`).

Provisional design is not optional correctness. If the correct fix is easier
because you may reshape the model, reshape it — but the defect still gets fixed
and still gets a resolving test in the same change.

Ruled as **D17**; rationale in `docs/spec/why.md`.

## Phase 0 — Framing (ruling, no code — not worker-loop work)
- [ ] #40 chive is the home.nix you never wrote: an imperative NixOS, read back from the machine. The archive is the product; cleaning is a side benefit. **Owner ruling** (CLAUDE.md:11-16). Lands first so #38/#39/#41/#42 cite it instead of re-deriving it. Evidence of misallocated attention: D10 ruled a `clean` confirmation prompt while the archive's own headline flow stayed broken (#35); D14 designed the status model around `clean`; `stats` leads with percentages instead of holes.

## Phase 1 — Safety foundations (nothing restores until these land)
- [ ] #32 scan silently clobbers the existing catalog: a nonexistent root scans as an EMPTY catalog (walk error swallowed), a subroot scan replaces the whole-HOME catalog — both exit 0. (Critical) — audit 09-19, sandbox-verified.
- [ ] #33 chive catalogs its own store files (store dir never excluded from scan); `clean --scope orphaned` deletes catalog.db/catalog.toml, resurrected only by the trailing save. (High) — audit 09-19, sandbox-verified.
- [ ] #34 relative scan root recorded as-is ("."): plan/restore/clean resolve it against the invocation cwd — clean from elsewhere reports removals that never happened and silently drops live entries. (High) — audit 09-19, sandbox-verified.
- [ ] #43 an owner verdict is erased by the next scan and silently overruled by an older taught recipe: `mark` writes only to the catalog the scanner replaces, and never touches recipes.toml (scan rule 2 re-applies the recipe). **Data-loss chain: mark a file protected → routine rescan → it reverts to cleanable → `clean` deletes it.** Also no `teach --remove` / `mark --clear`, so a verdict cannot be withdrawn. (Critical) — owner ruling needed on whether a later `mark` beats an earlier `teach` (amends D16). Blocked-by #32; do not weaken #32's root guard when merging.
- [x] #17 path escape: import/restore/clean can write/delete outside scan root → normalize+contain. (Critical) — FIXED: containment rule enforced in `Catalog` construction + action-time re-checks; spec rule V-path-containment.
- [x] #18 restore exits 0 on failure → real exit codes (note: #13 is the same bug, DUP — work once). (High) — FIXED: restore exits 1 when any recipe fails (all still run + report); clean exits 1 when any removal fails; Real::remove_file treats an absent path as the wanted end state.
- [x] #12 provenance order contract (docs say pkg→git→symlink, code runs reverse) → pick one, enforce. (High) — FIXED: code now runs the documented package→git→symlink order; pinned by provenance_order_is_package_then_git_then_symlink_tests.
- [x] #10 SQLite-vs-TOML truth decision → record, then fix #22 full reparse/rebuild. (Medium) — FIXED (D9 already ruled TOML-truth): `load_catalog` no longer falls back to reading the derived SQLite index — a stale index cannot impersonate a deleted/replaced catalog. Every command reparses the TOML (the "rebuild" #22 asked for is the only read path).

## Phase 2 — Correctness
- [ ] #35 README migration flow restores into the OLD machine's root: `restore --all` without `--root` expands `{dest}` against the imported stale root; imported orphans restore invisibly. (High) — audit 09-19, sandbox-verified.
- [ ] #36 catalog records host = "unknown": `hostname()` reads only `$HOSTNAME`, unset in non-interactive shells. (Medium) — audit 09-19, sandbox-verified.
- [ ] #38 the four statuses answer two questions at once, so "chive cannot explain this file" is recorded as "safe to delete" — three verdicts (restorable / unknown hole / disposable known-gap), unknown never cleanable. (High) — REOPENS D14: **owner ruling required before any code** (CLAUDE.md:11-16). Fixes the root of #15.
- [ ] #44 a verdict never reaches the archive: `save_catalog` always writes the store default and has no destination parameter, so `export --to` is a separate manual step and every state change after the last export is invisible to a rebuild. Also `target-state.md:230` promises `scan` writes "directly to a path the user specifies" — the code has no such flag. (High) — per-file half of #41's ruling.
- [ ] #45 a recipe taught for a path absent from this machine is stored only in recipes.toml, so it is dropped by export/import and never reaches a new machine. **Revises #37**: the absent-path case is legitimate (planning a new machine), so rejecting the path is the wrong fix — the catalog needs to express "I know how to rebuild this, it is not here". (High)
- [x] #19/#29 nested git basename (same root — work once). (High) — FIXED: detector probes the repo-relative path instead of the basename; unit + harness tests pin it.
- [x] #21 no-clobber brittle + TOCTOU. (High) — FIXED: no-clobber uses lstat (`action::present`) so a dangling symlink counts as present; TOCTOU documented as inherent to the check-then-act seam (see why.md).
- [x] #26 no mkdir -p of dest parents. (High) — FIXED: restore creates the `{dest}` parent directory through the Runner seam (no-op under dry-run), after the no-clobber check; migration test no longer needs `mkdir -p` in the taught recipe.
- [x] #28 absolute recipes kill portable restore. (High) — FIXED: git recipes address the repo with a `{root}` token (scan-root-relative), expanded at restore time from `--root`; a repo outside the scan root stays absolute (honestly machine-bound). Pinned by a two-machine end-to-end test.
- [x] #20/#30 exists storm (same root as #24/#25 — adapters fixed separately) — FIXED: manager availability hoisted to once per scan (program names, not manager names); per-file loop consults the hoisted list.
- [x] #24/#25 apk/rpm/xbps adapters fixed (dnf audited; nix removed) — FIXED: rpm/dnf probe `--queryformat %{NAME}` (exact bare name, no regex guessing); apk regex anchored on real path-first output; xbps probes `-o` (ownership) not `-f` (file listing) and parses `pkg-ver_rel:`; probe answers trimmed before matching; nix adapter removed — its recipe could never succeed and its comment promised orphan-fallback.

## Phase 3 — Decisions + hygiene (D1–D13, in dependency order)
- [ ] #8 D1 language ruling → #4 D5 cross-platform → #2 D3 absence → #1 D2 manager → #3 D4 off-box sync → #5 D7 taxonomy → #6 D12 GUI → #7 D13 license (MIT vs All-rights-reserved + missing LICENSE file).
- [x] #31 schema_version dead — FIXED: `replace` stamps the index version it writes; `open` refuses a version row it cannot parse as a number (was: any read failure silently coerced to "current", disabling the gate); writers use `open_for_write` so a wrongly-versioned index can always be repaired by rescanning.
- [x] #27 --all parity — FIXED: `restore --all` (and `plan restore --all`) is the documented explicit spelling of the default; conflicts with explicit paths (exit 2). Pinned by CLI tests. Remaining open: #23 dead-code sweep, #15/#16 defaults drift, #9 matrix, and the Phase-3 decision line.

## Phase 4 — Quality (docs + UX)
- [ ] #37 teach accepts a nonexistent path and reports success (exit 0) while mark rejects it — sibling validation drift. (Low) — audit 09-19, sandbox-verified. **NARROWED 10-06:** the absent-path half is wrong-by-framing and is owned by #45 (that case is legitimate, not drift); #37 keeps only the honest half — malformed or root-escaping paths must be rejected by both verbs.
- [ ] #46 no work list: the "see what has no recipe → teach it" loop has no verb. `status` is a flat dump, `stats` leads with percentages, and README.md:61 documents a "teach me" view that does not exist. `not_restorable_reason` is in the model, the TOML, the SQLite index and the spec, and **every construction site writes None** — dead column in a committed schema. (Medium) — blocked by #38.

## Phase 5 — Features (the archive is the product; cleaning is a side benefit)
Dependency chain — **#38 → #39 → #41 → #42**, with **#43** feeding #41 (it is the per-file half of #41's "the catalog carries owner decisions" ruling; a verdict that is not durable cannot live in the catalog). They share one root (a folder is not a first-class thing) and were split only because the model, the write path, and the read path are separately verifiable. An AI picking up any of them reads all five first.

- [ ] #39 teach/archive at folder scope: one line per file *or folder*, expanding to per-file entries underneath; `restore` runs a folder recipe once, not once per descendant. (Medium) — extends D16 from per-file to folder scope; the `{dest}`-per-file-vs-per-folder question is **owner-only**. *Write path.*
- [ ] #41 the archive has no hierarchy: a folder cannot carry a verdict that cascades to what's inside it, and nothing groups by folder (flat `[[files]]`, no parent column, no path-shaped filter anywhere in the CLI). (Medium) — *Model + read path.* Hard prerequisite for #42. **RULED 10-06:** folder verdicts live in the **catalog**, not config.toml (the chive file is what travels off-box, so a verdict kept in config stays behind with the old machine); precedence is **deepest wins** (file > nearer folder > root default); `restorable` outranks `disposable` at every level; **both nested and flat renderings must work everywhere** (status/stats/export/GUI) — one model, two views, flat must still show inherited verdicts and their origin so it cannot lie. Still open: whether the committed TOML is grouped-canonical (my reading: yes, flat is a derived sort).
- [ ] #42 end goal: a WizTree-style treemap of the machine, boxes colored by the three verdicts, top pane showing the verdict + the resolved recipe, fixable in place. (Medium) — **D12 framework stays open**; layout/IA/interaction only, no code. **RULED 10-06:** unaccounted boxes are drawn muted and **hover explains why** (status bar, as WizTree does); zoom is inherited from WizTree (double-click to zoom, `..`/`\` to leave, F10/F11); inherit bidirectional treemap↔list selection, accessible palette presets, and outline-as-separate-channel for holes. Last item, not the second.

## Routing rule for new issues
Any AI opening an issue here MUST put safety/boundary items in Phase 1 and decisions in Phase 3 order — never append a feature above #17/#18. Duplicates of one root cause get folded into the existing line. See AI_ISSUE_ROUTING.md.
