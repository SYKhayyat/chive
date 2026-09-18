# PLAN — chive (greenfield, ds-era scaffold; safety before features)

Worker loop: top unchecked item only, fix + test, commit, check off, stop.
Note: all 11 commits are ds-era (scaffolded 09-05/06) — review as new code, not fixes.

## Phase 1 — Safety foundations (nothing restores until these land)
- [x] #17 path escape: import/restore/clean can write/delete outside scan root → normalize+contain. (Critical) — FIXED: containment rule enforced in `Catalog` construction + action-time re-checks; spec rule V-path-containment.
- [x] #18 restore exits 0 on failure → real exit codes (note: #13 is the same bug, DUP — work once). (High) — FIXED: restore exits 1 when any recipe fails (all still run + report); clean exits 1 when any removal fails; Real::remove_file treats an absent path as the wanted end state.
- [x] #12 provenance order contract (docs say pkg→git→symlink, code runs reverse) → pick one, enforce. (High) — FIXED: code now runs the documented package→git→symlink order; pinned by provenance_order_is_package_then_git_then_symlink_tests.
- [x] #10 SQLite-vs-TOML truth decision → record, then fix #22 full reparse/rebuild. (Medium) — FIXED (D9 already ruled TOML-truth): `load_catalog` no longer falls back to reading the derived SQLite index — a stale index cannot impersonate a deleted/replaced catalog. Every command reparses the TOML (the "rebuild" #22 asked for is the only read path).

## Phase 2 — Correctness
- [x] #19/#29 nested git basename (same root — work once). (High) — FIXED: detector probes the repo-relative path instead of the basename; unit + harness tests pin it.
- [x] #21 no-clobber brittle + TOCTOU. (High) — FIXED: no-clobber uses lstat (`action::present`) so a dangling symlink counts as present; TOCTOU documented as inherent to the check-then-act seam (see why.md).
- [x] #26 no mkdir -p of dest parents. (High) — FIXED: restore creates the `{dest}` parent directory through the Runner seam (no-op under dry-run), after the no-clobber check; migration test no longer needs `mkdir -p` in the taught recipe.
- [x] #28 absolute recipes kill portable restore. (High) — FIXED: git recipes address the repo with a `{root}` token (scan-root-relative), expanded at restore time from `--root`; a repo outside the scan root stays absolute (honestly machine-bound). Pinned by a two-machine end-to-end test.
- [x] #20/#30 exists storm (same root as #24/#25 — adapters fixed separately) — FIXED: manager availability hoisted to once per scan (program names, not manager names); per-file loop consults the hoisted list.
- [x] #24/#25 apk/rpm/xbps adapters fixed (dnf audited; nix removed) — FIXED: rpm/dnf probe `--queryformat %{NAME}` (exact bare name, no regex guessing); apk regex anchored on real path-first output; xbps probes `-o` (ownership) not `-f` (file listing) and parses `pkg-ver_rel:`; probe answers trimmed before matching; nix adapter removed — its recipe could never succeed and its comment promised orphan-fallback.

## Phase 3 — Decisions + hygiene (D1–D13, in dependency order)
- [ ] #8 D1 language ruling → #4 D5 cross-platform → #2 D3 absence → #1 D2 manager → #3 D4 off-box sync → #5 D7 taxonomy → #6 D12 GUI → #7 D13 license (MIT vs All-rights-reserved + missing LICENSE file).
- [ ] #31 schema_version dead, #27 --all parity, #23 dead code, #15 ignore defaults, #16 platform drift, #9 macOS/Windows matrix.

## Routing rule for new issues
Any AI opening an issue here MUST put safety/boundary items in Phase 1 and decisions in Phase 3 order — never append a feature above #17/#18. Duplicates of one root cause get folded into the existing line. See AI_ISSUE_ROUTING.md.
