# Working in this repo

## Spec-driven development

`docs/spec/target-state.md` is the way in: it holds the canonical rules. Read it before changing behaviour. Every rule in `target-state.md` has a matching entry in `docs/spec/why.md` explaining why it exists. Do not change a rule without reading its why entry first.

Every open question lives in `docs/spec/decisions.md`, with a status. Do not answer one in code. A decision the register calls open is the owner's to make; a decision it marks *built, never ruled* is code that ran ahead of a ruling and is still theirs to reverse.

## Asking while building

Build without stopping for permission. Stop and ask only for these four:

1. Anything with an ID in the register — `D*`, `W*`, `K*`, `N*`, `T*`, `U*`.
2. Anything that changes behaviour a user would notice.
3. Anything that would remove a feature.
4. Anything where `target-state.md` looks wrong. Do not fix it yourself.

Do not stop for implementation detail, naming, file layout, test structure, or a choice between two options that is invisible from outside the program. Make the call and put the reasoning in the commit message.

When a question is answered, the ruling ships in that same commit — rewritten into `decisions.md`, and into `target-state.md` plus `why.md` if it is a rule rather than a detail. A ruling that lives only in a chat log is exactly the drift that made decisions unanswerable.

## Comments

A comment states a constraint the code can't show. Nothing else.

- Not what the line does — the line does that.
- Not where it came from — git does that.
- Not that it's good, or which spec paragraph blessed it — that's narration. A comment that cites `V.n` to explain a design is usually narration. A comment that says "this must run before the snapshot, or the rollback has nothing to revert to" is a constraint. Prefer the second.

If you can express the constraint in a name or a type instead of a comment, do that.

## No legacy

This is a rewrite, not a migration. There are **no compatibility shims, no old-format readers, no dual code paths kept "just in case"**. When a thing is replaced, the old thing is deleted in the same change — including the config keys, the docs, and the tests that named it. A green test suite over the old model is not progress; it means the old model still runs.

## Fix the whole family, not one instance

A bug you find is a representative of a family, not a lone instance. Before you call a fix done, find the siblings and fix them in the same change.

- The same bug class in the adjacent code path
- The parallel field or the other branch of the same enum
- Every layer that carries the same value (the in-code default and the config default)

Fix the family, not the finding. Say what you covered.

## Verify

`cargo build --all-targets` → `cargo test --no-fail-fast` → `cargo clippy --all-targets --all-features --locked -- -D warnings` → `cargo fmt -- --check`. Report honestly: unverified is not done, and a skipped step is a said-so, not a done.

## Test conventions

- Use `autotests = false` in `Cargo.toml`. List every test file manually in `tests/main.rs`.
- Name test files as sentences ending `_tests.rs`.
- Run `--no-fail-fast` always.
- Write the failing test first, watch it fail, then fix.

## Lamdan audits

Periodically run a whole-repo design critique (`lamdan` skill). Commit findings to `docs/lamdan/`. The most valuable findings come from lens 1 (was this the right software to build) and lens 2 (is this the right architecture). Lens 3 findings are cheap to fix and often wrong.
