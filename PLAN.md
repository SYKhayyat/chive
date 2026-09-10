# PLAN — chive (greenfield, ds-era scaffold; safety before features)

Worker loop: top unchecked item only, fix + test, commit, check off, stop.
Note: all 11 commits are ds-era (scaffolded 09-05/06) — review as new code, not fixes.

## Phase 1 — Safety foundations (nothing restores until these land)
- [ ] #17 path escape: import/restore/clean can write/delete outside scan root → normalize+contain. (Critical)
- [ ] #18 restore exits 0 on failure → real exit codes (note: #13 is the same bug, DUP — work once). (High)
- [ ] #12 provenance order contract (docs say pkg→git→symlink, code runs reverse) → pick one, enforce. (High)
- [ ] #10 SQLite-vs-TOML truth decision → record, then fix #22 full reparse/rebuild. (Medium)

## Phase 2 — Correctness
- [ ] #19/#29 nested git basename (same root — work once). (High)
- [ ] #21 no-clobber brittle + TOCTOU. (High)
- [ ] #26 no mkdir -p of dest parents. (High)
- [ ] #28 absolute recipes kill portable restore. (High)
- [ ] #25 apk/rpm/xbps adapters, #24 dnf/nix/rpm suspects, #20/#30 exists storm (same root — work once).

## Phase 3 — Decisions + hygiene (D1–D13, in dependency order)
- [ ] #8 D1 language ruling → #4 D5 cross-platform → #2 D3 absence → #1 D2 manager → #3 D4 off-box sync → #5 D7 taxonomy → #6 D12 GUI → #7 D13 license (MIT vs All-rights-reserved + missing LICENSE file).
- [ ] #31 schema_version dead, #27 --all parity, #23 dead code, #15 ignore defaults, #16 platform drift, #9 macOS/Windows matrix.

## Routing rule for new issues
Any AI opening an issue here MUST put safety/boundary items in Phase 1 and decisions in Phase 3 order — never append a feature above #17/#18. Duplicates of one root cause get folded into the existing line. See AI_ISSUE_ROUTING.md.
