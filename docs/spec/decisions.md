# Decisions — chive

Every open question lives here with a status. Do not answer one in code.

| ID | Question | Status |
|----|----------|--------|
| D1 | TOML schema design for state export | open |
| D2 | Category extension lists vs MIME-type detection | open |
| D3 | Tauri frontend structure (single window vs multi-panel) | open |
| D4 | How to handle `.tar.gz` and other compound extensions | open |
| D5 | Whether `chive clean` should prompt for confirmation or have a `--force` flag | open |
| D6 | Whether the SQLite DB and TOML file should be kept in sync automatically or independently | open |
| D7 | Whether to use `egui`/`iced` instead of Tauri | open |
| D8 | Project license | open |

## Decision record

### D0 — Framing: file lifecycle manager, not recovery tool
- **Status**: decided
- **Why**: The user removed deleted-file detection and added GUI + declarative state. The product is a catalog and lifecycle manager, not a recovery tool.
- **Ruling**: All docs and future code must frame chive as a file lifecycle manager.

### D1 — Language: Rust
- **Status**: decided
- **Why**: User chose Rust with AI assistance. Shall already has the full Rust pipeline.
- **Ruling**: Rust only. No Python, Java, or Go.

### D2 — GUI framework: Tauri
- **Status**: decided
- **Why**: Tauri provides native desktop apps on all three platforms with a Rust backend. User approved.
- **Ruling**: Tauri for GUI. All slow parts in Rust.

### D3 — Storage: SQLite + TOML export/import
- **Status**: decided
- **Why**: User wants both database performance and versionable state files. Shall uses TOML for config.
- **Ruling**: SQLite as working state, TOML for export/import.
