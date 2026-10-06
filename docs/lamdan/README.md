# lamdan audits

Whole-repo design critiques, committed per `CLAUDE.md:57`.

An audit argues about whether the code *should* exist and whether it is the *right shape* —
not whether it has bugs. A bug is a one-line note inside an audit; `/code-review` owns the
rest.

Three lenses, asked in order, and a finished argument has been through all three:

1. **What was built** — is this the right piece of software at all? Verdicts: `don't-build`, `delete`.
2. **Architecture** — right software, wrong shape? Verdicts: `rewrite`, `wrong-but-keep`.
3. **Implementation** — right shape, wrong code inside it? Verdicts: `rewrite`, usually cheap.

Lens 1 and 2 findings are the expensive ones and the ones worth reading. Lens 3 findings
are cheap and often wrong.

Every finding carries four things: a steelman of the strongest case that the current design
is *correct*, a verdict, the concrete change, and the cost. An audit with no
`wrong-but-keep` verdict has stopped weighing things and started performing.

## Format

`whole-repo-YYYY-MM-DD.md`. One per run. A run that reproduces the previous run's top
findings is a coverage bug, not corroboration — each run records what it read and what it
skipped, so the next run can tell the difference.
