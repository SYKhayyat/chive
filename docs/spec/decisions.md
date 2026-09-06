# Decisions — chive

Every open question lives here with a status. Do not answer an open question in code. When a question is answered, the ruling ships in the same commit — into `decisions.md`, and into `target-state.md` plus `why.md` if it is a rule rather than a detail.

## Open questions

| ID | Question | Status | Notes |
|----|----------|--------|-------|
| D1 | Implementation language — Rust or Common Lisp | **open** | Shall proves Rust can deliver the needed flexibility. But CL has genuine REPL advantages. Owner to decide. |
| D2 | Restore manager: call restore periodically or when N files go missing | **open** | Non-goal for MVP. The catalog data model does not preclude a manager later. |
| D3 | What counts as "absent" on a target machine | **open** | MVP: restore operates on named files or all restorable. Absence detection is D2 territory. |
| D4 | Where the off-box catalog lives | **open** | MVP: user brings their own git/storage. chive produces the TOML. |
| D5 | Cross-platform recipes: per-OS variants or one portable form | **open** | MVP: one recipe string per file. Per-OS variants can be added later. |
| D7 | Category taxonomy: extension lists vs MIME-type detection | **open** | MVP: extension lists. MIME can be added without changing the catalog format. |
| D12 | GUI framework: Tauri vs egui/iced | **open** | MVP: CLI only. GUI is phase 2. |
| D13 | Project license | **open** | Not blocking the build. |

## Ruled decisions

### D0 — Framing: reconstruction engine

- **Status**: ruled
- **Why**: The product's value is the restore. "Recovery tool" implied byte-level resurrection; "lifecycle manager" buried the point. Reconstruction matches the mechanism (recipe, not bytes).
- **Ruling**: chive is framed as a reconstruction engine. Byte-level recovery and inode forensics are non-goals.

### D1 (reopened) — Language

- **Status**: open (reopened)
- **Why reopened**: Rust was chosen early on the assumption of "extreme speed." After reading Shall's code, the flexibility argument for CL does not hold: Shall achieves its flexibility via data-file adapters and a REPL that is explicitly "a thin front end over the one parser" (`src/app/adapters.rs`, `src/app/repl.rs`). chive's recipes are the same shape. But CL's REPL is genuinely useful for interactive exploration. The tradeoff is real.
- **Considerations**: chive is Shall's sibling (may share code). Tauri forces Rust in the GUI tier. CL shipping a single binary to 3 platforms is harder. CL's REPL advantage can be replicated with a Rust REPL (or `eval | jq` pattern from Shall).
- **Ruling**: none yet. The spec is language-independent. Current working assumption: Rust.

### D6 — Restore addressing: by relative path

- **Status**: ruled
- **Why**: Opaque IDs are not portable across machines. Relative paths are human-readable, git-friendly, and match how the catalog is used (you see a path, you restore it).
- **Ruling**: All restore commands use relative paths. Internally the path is the primary key.

### D8 — Compound extensions

- **Status**: ruled
- **Why**: `.tar.gz` and `.jpeg` are common. The last-extension rule gets them wrong. The list is short and stable.
- **Ruling**: Special-case `.tar.gz`, `.tar.xz`, `.tar.bz2` as archive. `.jpeg` as alias for `.jpg`.

### D9 — Storage authority

- **Status**: ruled
- **Why**: Dual stores (SQLite + TOML) create ambiguity about which is the source of truth. Shall's lesson: the file is the truth.
- **Ruling**: The TOML catalog file is the source of truth. SQLite is a derived working index rebuilt by `import` or `scan`. The TOML is what gets committed off-box.

### D10 — Clean confirmation

- **Status**: ruled
- **Why**: Deletion must be previewed and confirmed. This mirrors Shall's removal guard (U26 rule).
- **Ruling**: `chive clean` requires confirmation unless `--force`. `--dry-run` prints what would be removed.

### D11 — TOML schema

- **Status**: ruled for MVP
- **Why**: An AI cannot build without a concrete schema. The schema is defined in `target-state.md` and can evolve.
- **Ruling**: The TOML schema in `target-state.md` is the MVP schema. Fields may be added later; fields are never removed (forward-compatible).

### D14 — Status assignment: orphaned vs not-restorable

- **Status**: ruled
- **Why**: Without this split, `clean` is either too aggressive or too conservative. Separating them makes `clean` safe by default.
- **Ruling**: `orphaned` is the automatic default for files with no provenance. `not-restorable` is only assigned by explicit owner action (`chive protect`). Clean removes orphaned; clean never touches not-restorable.

### D15 — Restore never clobbers

- **Status**: ruled
- **Why**: Reconstruction fills gaps. Overwriting existing files is a different operation with different risks.
- **Ruling**: If `{dest}` already exists, chive refuses to restore that file. The user must delete or move it first.

### D16 — Taught recipes overrule inference

- **Status**: ruled
- **Why**: The owner's explicit recipe is stronger evidence of intent than any automatic provenance detection. Teaching is a statement of *how to rebuild this file*; a scan-time inference is only a guess. Rules 2–4 of the scan order (temporary blob, provenance, orphaned) are all guesses about a file chive has not been told about.
- **Ruling**: A user-taught recipe (`recipes.toml`, via `chive teach`) wins over every automatic classification for that path. Teaching works on a file in any of the four statuses and always promotes it to `restorable`, source `user_supplied`. `chive mark --status <not-restorable|temporary|orphaned>` is the unified verb for explicit status changes, folding the original `protect` command; marking clears a recipe.