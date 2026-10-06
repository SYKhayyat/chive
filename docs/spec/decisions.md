# Decisions — chive

Every open question lives here with a status. Do not answer an open question in code. When a question is answered, the ruling ships in the same commit — into `decisions.md`, and into `target-state.md` plus `why.md` if it is a rule rather than a detail.

## Open questions

| ID | Question | Status | Notes |
|----|----------|--------|-------|
| D1 | Implementation language — Rust or Common Lisp | **open** | Rust is the reality on disk (7k lines, `Cargo.toml`, musl-static integration targets), so this is a register question rather than a build question. Recorded against #8. CL's REPL advantage is real but is the only thing CL still buys; the code has no use for it. |
| D2 | Restore manager: call restore periodically or when N files go missing | **open** | Non-goal for MVP. The catalog data model does not preclude a manager later. |
| D3 | What counts as "absent" on a target machine | **open** | MVP: restore operates on named files or all restorable. Absence detection is D2 territory. |
| D4 | Where the off-box catalog lives | **open** | MVP: user brings their own git/storage. chive produces the TOML. |
| D5 | Cross-platform recipes: per-OS variants or one portable form | **open** | MVP: one recipe string per file. Per-OS variants can be added later. |
| D7 | Category taxonomy: extension lists vs MIME-type detection | **open** | MVP: extension lists. MIME can be added without changing the catalog format. |
| D12 | GUI framework: Tauri vs egui/iced | **open** | MVP: CLI only. GUI is phase 2. |
| D13 | Project license | **ruled 10-06: MIT** | Was open; `Cargo.toml` said MIT while `README.md` and `docs/SUMMARY.md` said all-rights-reserved and no LICENSE file existed. MIT wins because it is the only value a machine reads. LICENSE file added. |

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
- **Ruling**: The TOML catalog file is the source of truth, and it is the *only* store. The TOML is what gets committed off-box.
- **Amended 10-06**: the derived index is deleted. It was written on every save and read by nothing — `db::load` and the schema-version gate had zero non-test callers, and the "fast queries (status, restorable-set)" the ruling names do not exist. Deleting it *satisfies* D9 rather than contradicting it: an index can only pay off by avoiding the parse, and the parse is what D9 forbids skipping. What went with it was not optional — five `FromStr` impls on the domain model existed only for its reader. See `why.md`, "One store, not two".

### D10 — Clean confirmation

- **Status**: ruled
- **Why**: Deletion must be previewed and confirmed. This mirrors Shall's removal guard (U26 rule).
- **Ruling**: `chive clean` requires confirmation unless `--force`. `--dry-run` prints what would be removed.

### D11 — TOML schema

- **Status**: ruled for MVP
- **Why**: An AI cannot build without a concrete schema. The schema is defined in `target-state.md` and can evolve.
- **Ruling**: The TOML schema in `target-state.md` is the MVP schema. Fields may be added later; fields are never removed (forward-compatible).

### D14 — Status assignment: orphaned vs not-restorable

- **Status**: superseded by D19
- **Why**: Without this split, `clean` is either too aggressive or too conservative. Separating them makes `clean` safe by default.
- **Ruling**: `orphaned` is the automatic default for files with no provenance. `not-restorable` is only assigned by explicit owner action (`chive protect`). Clean removes orphaned; clean never touches not-restorable.
- **Superseded because**: the split was answering a question chive should never have been asked. See D19.

### D15 — Restore never clobbers

- **Status**: ruled
- **Why**: Reconstruction fills gaps. Overwriting existing files is a different operation with different risks.
- **Ruling**: If `{dest}` already exists, chive refuses to restore that file. The user must delete or move it first.

### D16 — Taught recipes overrule inference

- **Status**: ruled, amended by D20
- **Why**: The owner's explicit recipe is stronger evidence of intent than any automatic provenance detection. Teaching is a statement of *how to rebuild this file*; a scan-time inference is only a guess. Rules 2–4 of the scan order (temporary blob, provenance, orphaned) are all guesses about a file chive has not been told about.
- **Ruling**: A user-taught recipe (`recipes.toml`, via `chive teach`) wins over every automatic classification for that path. Teaching works on a file in any of the four statuses and always promotes it to `restorable`, source `user_supplied`. `chive mark --status <not-restorable|temporary|orphaned>` is the unified verb for explicit status changes, folding the original `protect` command; marking clears a recipe.
- **Amended by**: D20 removes "every automatic classification" — an owner verdict and an owner recipe are two entries in one ordered log, so the later one governs rather than one class outranking another.

### D17 — The existing code is provisional

- **Status**: ruled
- **Why**: The implementation was scaffolded before the product's purpose was settled, and it encodes a model the purpose no longer supports: four statuses designed around `clean`, a flat catalog with no hierarchy, and a `mark` verb that does not survive a rescan. Treating that code as a contract would preserve the mistakes; treating it as untouchable would make the correct fixes (#38, #41, #43) strictly harder than the wrong ones. A rewrite that keeps reverse-engineering intent from provisional code is not a rewrite.
- **Ruling**: Every line of `src/` is a **suggestion, not a contract**, and an AI working here should not be afraid to change it drastically. The status model and its names, the CLI surface and its flags, the catalog TOML schema, the store layout, the module structure and the provenance/scan internals may be rewritten, deleted, or replaced from scratch without asking. A large honest rewrite that lands the purpose is the preferred outcome over a small safe patch that preserves the wrong model. Specifically not wanted: minimal additive or defensive changes made out of a fear of breaking things, preserved signatures and schema shapes because something already depends on them, tests that pin behaviour nobody ruled on, and fixes contorted to fit the existing architecture instead of reshaping the architecture to fit the fix.
- **Not part of the ruling** — this is permission, not a waiver. Real defects are still fixed in the same change with a resolving test (#32, #33, #34 and #43 destroy data through ordinary commands). Security boundaries are not suggestions (path containment, #14). The verify chain still runs (`CLAUDE.md:46-48`). Rewrite boldly, then verify hard.
- **Stated in**: `PLAN.md` ("The code is a suggestion") and `docs/spec/why.md`.
- **Note 10-06**: D18–D23 have since removed the three symptoms this ruling named (four statuses → D19; the erased `mark` → D20). The flat catalog and missing hierarchy (#41) remain, and the ruling still governs how they are addressed.

### D18 — The archive is the product

- **Status**: ruled (closes #40)
- **Why**: chive's value is answering "what can I rebuild, and how" on a machine that is not the one it was born on. Every design choice that did not serve that question had drifted toward serving `clean`, and the evidence was in the register itself: D10 spent a ruling on a clean confirmation prompt while the archive's headline flow was broken (#35, #44), D14 shaped the status model around `clean`, and `stats` led with percentages rather than holes. Cleaning is a consumer of a verdict, not the reason the verdict exists.
- **Ruling**: chive is the `home.nix` nobody wrote: an imperative NixOS configuration, read back off a machine that already exists. The catalog is the product — a portable, versionable, plain-text record of how to re-derive what matters. Deleting is a side benefit that happens when the owner has already decided something is disposable.
- **Consequence**: any status, verb, or view that does not answer "can this be rebuilt, and how" earns its place only by serving `clean`. `holes` (#46) is the primary read path; `stats` leads with the hole count and demotes percentages.

### D19 — Three verdicts, and only the owner may call a file disposable

- **Status**: ruled (closes #38, supersedes D14)
- **Why**: The four statuses answered two different questions with one enum, so "chive cannot explain this file" and "this file is safe to delete" collapsed into the same value. A file chive failed to explain — a package probe that missed, a repo it could not reach, an unrecognised extension — was recorded as `orphaned`, which is cleanable. The scan's inability to explain something was being treated as permission to delete it, and nothing in the enum distinguished "I checked and it's junk" from "I have no idea what this is."
- **Ruling**: an entry is in exactly one of three states.

  | Verdict | Meaning | Clean |
  |---------|---------|-------|
  | `restorable` | A recipe exists; chive can re-derive it. | never |
  | `unknown` | It matters (assumed) and chive cannot rebuild it. A hole. | never |
  | `disposable` | The owner has judged it a known gap; safe to delete. | yes |

  Two consequences, both load-bearing:

  1. **Protection is not a state.** Under the old model `not-restorable` was "protected," which implied a separate act to achieve safety. Here a file with no recipe is already `unknown`, and `unknown` is never cleanable — so safety is the default and `mark --status not-restorable` has nothing left to change. It is deleted as a verb, not deprecated.
  2. **No automatic answer may produce `disposable`.** See D21 for what may, and D22 for what that means when evidence changes.

  `temporary` and `orphaned` are removed, along with every config key, status spelling, test, and doc line that named them. There is no reader for the old values (see `CLAUDE.md` "No legacy").

### D20 — Owner intent is one ordered log in the catalog; the last act wins

- **Status**: ruled (closes #43, amends D16)
- **Why**: #43 is a data-loss chain, not a correctness bug: `mark` a file protected → routine rescan → the verdict is gone and an older taught recipe re-applies → `clean` deletes it. Two defects composed. `mark` wrote only to the catalog the scanner then replaced (so the verdict never survived a scan), and scan rule 2 re-applied any taught recipe over any verdict (so even a surviving verdict was overruled). Both halves are the same mistake: owner intent was modelled as mutable state of the *scan* rather than as a durable, ordered record the scan *reads*.
- **Ruling**: owner acts — `teach`, `dispose`, `withdraw` — are entries in one ordered log. Each is stamped with a monotonic sequence number and lives **in the catalog**, which is the chive file that travels off-box. Consequences:

  - **Last act wins.** Teach then dispose leaves the file `disposable`; dispose then teach leaves it `restorable`. One rule, no precedence table, and it matches the owner's mental model: the most recent thing I said.
  - **A rescan never reorders or erases the log.** It reads it and re-applies the newest act per path. This is what makes the verdict durable, and it is why verdicts cannot live in `config.toml`: the catalog is what a new machine imports, so a verdict kept in config stays behind with the old machine (this is also the write-path half of #44 — a verdict in the catalog is carried by `export` with no extra plumbing).
  - **Withdrawal is a first-class act.** `withdraw` clears the log entry for a path; the entry then reverts to whatever chive can prove, which is `restorable` if a recipe remains and `unknown` if none does. This needs no separate rule — it falls out of "recipe present ⇒ restorable, absent ⇒ unknown."
  - Verbs are conveniences, not the interface. The catalog is hand-editable and a hand-written verdict is exactly as valid as one written by a verb.

### D21 — Verdict rules are user config, in Rhai

- **Status**: ruled (lands with #38)
- **Why**: `temporary` did one job — it made `clean` useful on day one by auto-classifying thousands of files chive would otherwise have to ask about. Removing it (D19) leaves the owner with no way to say "`.apk` files are disposable" except writing three thousand entries by hand. The old heuristic did this by guessing from filenames, which is the exact failure D19 forbids; the fix is to let the owner state the rule instead of chive guessing it, in a language where a rule can see what a filename cannot (owner, size, realpath, whether the symlink resolves).
- **Ruling**: `config.toml` gains a `rules` list. Each rule is a Rhai script returning a verdict or nothing:

  ```toml
  [[rules]]
  name = "installer packages are disposable"
  script = '''
    if path.ends_with(".apk") { "disposable" } else { () }
  '''
  ```

  A rule may return any of the three verdicts. Rules are evaluated in order, first non-nothing result wins, and are compiled once per scan rather than once per file. A script that does not compile is refused at *load* — a rule that silently stops matching would quietly change verdicts mid-scan. **Rhai, not Python:** chive ships a musl-static binary (`docker/integration/README.md`) and its Alpine/Void integration targets depend on that; embedded Python needs glibc's `libpython` and would break the static build. Rhai is also sandboxed with no filesystem or process access unless granted, which matters while #14 (chive fabricating shell from untrusted data) is open — a rule language that could spawn a process would re-open that whole class.
- **Narrowed 10-06 while implementing**: two provable-dead cases were dropped rather than shipped. *Removed-package residue* would need chive to remember that a package once owned the file; the check is easy but the memory is a guess wearing a fact's clothes, which is the inference D19 removes. *A declared directory* needs no new mechanism — a rule matching the path prefix says the same thing, so a built-in would be a second way to say one thing. Both are available to the owner as rules, where the owner is the one making the claim.

### D22 — Owner verdicts are sticky; chive's own are not

- **Status**: ruled
- **Why**: The question "what happens when evidence changes underneath a standing verdict?" only arises for verdicts chive *inferred*. For owner verdicts there is nothing to rule on — an owner act is the newest word and only the owner may supersede it. The asymmetry matters because D21's evidence rule only pays off if chive can retract its own inference: a dangling symlink gets repointed, and a recomputing scan promotes it to `restorable` via `ln -s`. An inferred `disposable` that outranked that fresh evidence would let `clean` delete a link the owner just repaired.
- **Ruling**: stickiness is a per-verdict setting, defaulted per origin:

  | Origin | Default | Meaning |
  |--------|---------|---------|
  | Owner (`teach`/`dispose`/`withdraw`, or hand-edited) | sticky | Survives rescan until the owner changes it. |
  | Chive-inferred (provable-dead, provenance) | recompute | Re-derived from evidence every scan. |
  | Rule (`[[rules]]`) | sticky | Owner-authored policy, so treated as owner intent — but withdrawable by editing or removing the rule. |

  **Sticky-inferred is configurable and is a footgun.** It re-opens #43 through config: a stale `disposable` becomes permanent, because a verdict that nothing may revise is a verdict nothing may correct. It is a legitimate setting (an owner who has audited a subtree may want it frozen) and its hazard is recorded in `why.md` next to the rule.

### D23 — `chive holes` is the work list

- **Status**: ruled (closes #46's missing-verb half)
- **Why**: The loop the product is built around — see what has no recipe, teach it — had no verb. `status` was a flat dump, `stats` led with percentages, and `README.md:61` documented a "teach me" view that does not exist. Under D18 the backlog of unknowns is the thing the owner most wants to see, so it gets a verb rather than a flag.
- **Ruling**: `chive holes` lists `unknown` entries, largest first, each row carrying the act that would close it:

  ```
  14,002 holes (312 MB) — nothing chive can rebuild yet

    250 MB  Pictures/2024/                    teach a recipe, or mark disposable
     38 MB  Documents/                        teach a recipe, or mark disposable
    2.1 MB  conf/emacs.d/init.el              taught: restorable
  ```

  `stats` keeps its counts but leads with the hole count and moves percentages below. `status` remains the flat by-verdict listing.

### D24 — How much may `restore` overwrite (owner ruling 10-06)

- **Status**: ruled
- **Why**: The lamdan audit (2026-10-06) found that D15's no-clobber guarantee is a substring test: `build_plan` computes `dest` only when the recipe literally contains `{dest}`, so no git-tracked file is ever protected. Verified — `restore` on a dotfiles repo destroyed a local edit and reported `restored:`, exit 0. The deferred half of issue #21 asked whether *package* recipes may overwrite a present dest; it was never decided, and the audit found the answer was the wrong way round: a git recipe places the file at exactly `root + path`, so it is not exempt at all.
- **Sibling precedent (Shall).** Shall's rule is `is_deployed_shim` (`src/app/shim_manager.rs:90`): overwrite only what you can prove is yours — a redeploy of Shall's own shim proceeds, an unmanaged same-named file is refused. Its `copy_over` additionally removes the old entry before making the new one, and `config init` / `module create` / `export` all refuse without `--force`. So the answer is *refuse*, with a named escape hatch — never a silent overwrite.
- **Ruling**: three levels, in `config.toml`, safe by default:

  | `restore.overwrite` | Meaning |
  |---------------------|---------|
  | `refuse` (default) | An existing `{dest}` is never written. D15 as written. |
  | `backup` | The existing file is copied to `<dest>.chive-backup` first, then replaced. Shall's `copy_over` shape. |
  | `overwrite` | Replace without asking. The escape hatch, written where the decision lives (Shall's `ExecTrust::Warn`). |

  This applies to **every** entry, not only `{dest}`-bearing recipes — a package recipe's dest is still `root + path`. The setting changes what happens *after* chive knows a file is there; it never removes the knowledge.

### D25 — How much is an imported catalog's root trusted (owner ruling 10-06)

- **Status**: ruled
- **Why**: The audit found `Catalog.root` is stored verbatim and never validated, while every containment check is relative to it. Verified — a catalog with `root = "/tmp/elsewhere"` imports, then `clean --force` deletes that file, exit 0. `root = "/"` is accepted. The entry-path traversal rules are sound and constrain the *relative* part; the absolute anchor is unchecked, and the doc comment describing the relative rule reads as though the whole path were covered.
- **Sibling precedent (Shall).** Shall asks this question of data it did not author with `ExecTrust` (`src/config/config.rs:520`): `owner-only` / `not-world-writable` (default) / `warn`, where the escape hatch is "written where the decision lives." Shall also gates untrusted paths with `safe_relative` (`src/model/vendor.rs:153`), which drops `..`, roots, and drive prefixes outright.
- **Ruling**: two parts, and the split is the point.
  - **Unconditional, no setting**: an empty, relative, or `/` root is refused at `Catalog::new` (exit 3, `Error::Refused`). No configuration turns this off — it is a malformed catalog, not a judgement call. This is `safe_relative`'s half.
  - **Judgement call, in `config.toml`**: whether an imported catalog may name a root outside your own home.

  | `catalog.root_scope` | Meaning |
  |----------------------|---------|
  | `home-only` (default) | An imported root must live inside the current user's home. Otherwise refuse. |
  | `warn` | Accept any absolute root, naming it loudly on import. For a deliberate `chive scan /etc`. |
  | `any` | Accept anything. |
