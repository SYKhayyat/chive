# Lamdan — chive, whole repo

**2026-10-06.** First pass. Eight regions, every tracked source/test/doc file in exactly
one, each swept by a fresh-context subagent that had not seen the others' conventions, then
cross-examined by me against the source and — for every finding I lead with — against the
running binary.

**I wrote ~1,800 of this repo's lines earlier the same day**, including the three-verdict
model, the act log, and the rules engine. That is the worst possible position for an audit,
and §1's discipline ("commit to a design before reading") was therefore unavailable to me: I
reconstructed the want from README/PLAN/decisions.md, but I had been inside the code. The
subagent-per-region sweep is the one mitigation available, since a fresh context cannot
anchor on my conventions. **Read the lens-1 and lens-2 findings as coming from eight
independent readers, and discount anything below that matches a decision I made today.**

---

## Coverage

| Region | Scope | Read |
|---|---|---|
| 1 | `scan.rs`, `rules.rs`, `config.rs` | all 3, full |
| 2 | `provenance/**` incl. `backends.toml` | all 7, full |
| 3 | `model/**`, `act.rs` | all 6, full |
| 4 | `catalog/**` | all 3, full |
| 5 | `action.rs`, `runner.rs` | all 2, full |
| 6 | `cli.rs`, `app.rs`, `store.rs` | all 3, full |
| 7 | `tests/**`, `harness/run.sh` | all 10, full |
| 8 | `docs/**`, `README`, `CLAUDE`, `PLAN`, `AI_ISSUE_ROUTING`, `triage.csv`, `LICENSE`, `deny.toml`, `Cargo.toml`, `docker/**` | all 20, full |

**Excluded, deliberately:** `Cargo.lock`, `target/`, `docs/lamdan/` (this file), the
binary at `docker/integration/context/chive`.

**Verification I ran myself** (the skill's rule: never take a reviewer's word for a claim
you lead with). Built clean. Ran `cargo test --no-fail-fast` (136 lib + 43 integration,
5 ignored). Ran the suite under `CHIVE_DRY_RUN=1` to test hermeticity. Ran four hostile
catalogs and one real git repo against the release binary. Three of four subagent claims
died under that check and are listed in *What I got wrong*.

---

## §1 — What I committed to before opening any implementation

Reconstructed from the written record, in the writer's voice:

> chive is the `home.nix` you never wrote — an imperative NixOS, read back off a machine
> that already exists. The archive is the product; deleting is a side benefit. Minimum
> that satisfies it: one plain-text TOML, committed off-box, saying per path either "here's
> the recipe" or "I don't know", plus verbs to teach the unknowns and see what's left.
>
> So: one binary; one TOML as the only truth; provenance as data-row adapters; verbs
> ≈ scan / holes / plan / restore / teach / dispose / withdraw / export / import. No GUI, no
> restore manager. 4–6k lines.
>
> Four things I predict break: provenance inference fails *silently*; owner decisions
> become the actual product and the thing everything else gets wrong; the scan is
> O(files × managers) subprocesses; "everything meaningful" isn't decidable and the ignore
> list is a policy laundered as a default.

Two of the four predictions the code confirmed, one it half-confirmed, and one it beat: the
act-log design is better than the sketch deserved.

---

# The strongest claim

**The path-containment boundary that `target-state.md:19-34` calls a security boundary does
not hold.** Two holes, both reachable through the product's own documented flow, both
verified against the release binary, both destroying files with exit code 0.

The spec's words are unusually strong: *"A catalog is a file that may have come from
anywhere — another machine, another person."* `restore` and `clean` are supposed to
*"re-check that a joined path still falls under the root before touching the filesystem."*

### 1. `Catalog.root` is unvalidated, and it is the anchor containment is relative to

`Catalog::new` validates every entry path and every act path against detailed rules — no
`..`, no `.`, no backslash, no drive prefix, no NUL, no empty segments. Those rules are
good; I tried eleven escapes and every one was refused with a correct message. But they
constrain the **relative** part. The **absolute** part — `meta.root`, the thing every
relative path is joined onto — is taken verbatim from the TOML and never checked.

```
$ cat evil.toml
[meta]
root = "/tmp/vfy/fakesys"
[[files]]
path = "passwd"
verdict = "disposable"
verdict_source = "owner"
[[acts]]
seq = 1
path = "passwd"
kind = "dispose"

$ chive import --from evil.toml
imported 1 file(s) and 1 owner act(s) from evil.toml
$ chive clean --force
removed 1 file(s)          # exit 0
$ ls /tmp/vfy/fakesys/passwd
                          # gone
```

A stranger's catalog chooses where `clean` deletes. `root = "/"` is likewise accepted.
The traversal rules read as though containment were total, which is precisely what hides
this: the doc comment describes the *relative* constraint and a reader reasonably
concludes the whole path is covered.

### 2. `contained()` is lexical, so a symlink inside the root escapes it

`action::contained` is `p.starts_with(root)`. That is a string comparison, so a symlink
under the root is followed by every subsequent filesystem call:

```
$ ln -s ../../outside home/sub/link
$ echo "OUTSIDE DATA" > outside/victim.txt
# catalog carries an entry for sub/link/victim.txt, judged disposable
$ chive clean --force
removed 1 file(s)          # exit 0
$ ls outside/victim.txt
                          # gone — deleted outside the scan root
```

This is not a contrived setup. A real home directory is *full* of symlinks; on a
`/nix/store`-backed home it is the normal case.

**Verdict: rewrite.** `delete` is wrong — the containment rule itself is right and is the
reason the relative rules are airtight. The fix is to make the join fallible and loud
instead of a filter that fails open: `Catalog::resolve(&self, rel) -> Result<PathBuf>`,
canonicalising the parent and refusing a result that does not land under the canonical
root. That reuses `Error::Refused` — which exists, has a dedicated exit code 3, and is
**never constructed anywhere** — and turns a silent skip into `failed: … refused`.

**Cost.** ~15 lines. Two of them are security fixes against a boundary the repo declares
load-bearing and `D17` explicitly exempts from "the code is a suggestion."

### 3. The no-clobber guarantee is a substring test, so it does not protect dotfiles

`build_plan` sets `dest` — and therefore runs the no-clobber check — only when the recipe
literally contains the string `{dest}`. Git recipes do not:

```
restore_method = "git -C '{root}/dotfiles' checkout HEAD -- 'rc'"
```

```
$ echo "PRECIOUS LOCAL EDIT" > home/dotfiles/rc
$ chive restore --root home
restored: dotfiles/rc       # exit 0
$ cat home/dotfiles/rc
committed                   # the edit is gone
```

`D15` and `why.md` both state the rule's purpose without the token: *"Refusing to overwrite
an existing file is the line between 'reconstruct' and 'overwrite.'"* A git recipe places
the file at exactly `root + path`, and chive has both halves in hand when it writes it.
`action.rs:83` — *"package and git recipes place files themselves and have no local dest"* —
is a false rationale that a future maintainer would read and agree with, which is why this
survived.

**Verdict: rewrite.** Compute `dest = root.join(entry.path)` for **every** entry; gate only
parent-directory creation on the token. Cost ~1 day. Needs your ruling on one sub-question:
may a *package* recipe overwrite a present dest? I would rule no.

### 4. `restored:` means "the recipe exited 0", not "the file is there"

```
$ chive teach important.conf --method "true"
$ chive restore --root home
restored: important.conf    # exit 0
$ test -e home/important.conf
                           # absent
```

Issue #18 exists because *"a run that restored nine files and failed on the tenth must not
exit 0 — that claim is a lie the shell then acts on."* This is the same lie one level down.
`teach` accepts any string, so this is one typo away. After fix 3, `dest` is never `None`,
so the check is available everywhere: `lstat` after a zero exit, absent ⇒ `Failed`.

**Verdict: rewrite. Cheap** — one syscall per restored file.

### What I could not build here, and am naming so the verdict is honest

All four are *checks chive cannot make atomic*. The recipe is arbitrary shell written by the
owner; chive does not write the file, so it cannot post-process the result into a rename. A
temp-file-and-rename scheme is impossible from outside the recipe. A sentinel at `dest`
makes the recipe fail and leaves a stray file to distinguish from a real occupant. Injecting
`test ! -e` requires parsing arbitrary shell structure. The check-then-act window is
inherent to a shell-recipe seam, and I could not design it away.

What fixes 1–4 buy is that the check is **maximally scoped** (every entry, not just
`{dest}`-bearing ones) and the **report is maximally honest**. That is the achievable
improvement and I am not claiming more.

---

# Lens 1 — what was built

### L1-a. The SQLite index is a write-only sink. `delete`.

Traced honestly, because D9 already ruled it and `triage.csv:11` already flagged it as
"ornamental in the common path" — I re-derived rather than assuming:

- **Non-test callers of `db.rs`: three, all writes.** `app.rs` calls `open_for_write` +
  `replace`. `db::open` (the version gate, the most defended function in the file) has
  **zero** non-test callers. `db::load` — the entire read path, 74 lines — has **zero**.
- D9's stated rationale is *"fast queries (status, restorable-set)"*. **No such query
  exists.** `cli.rs` filters `catalog.files()` in memory for both.
- Five hand-written `FromStr` impls (`Verdict`, `Origin`, `Category`, `Source`, `ActKind`,
  ~60 lines) exist **solely** for `db::load`. `FileEntry`'s `Serialize` derive is dead for
  the same reason.

**Steelman, and it is real:** a derived index built ahead of its query is standard practice,
and I measured that a 50k-entry catalog parses in 1.1–1.9 s while an indexed filtered query
is 25 ms against 117 ms in-memory. So the *idea* is sound.

**Verdict: delete anyway.** The steelman fails on D9's own terms: the index could only pay
off by avoiding the *parse*, and D9 forbids avoiding the parse. At 500k entries the parse
dominates at ~10 s and the index saves none of it. Cost: ~350 lines production, ~100 tests,
one dependency (`rusqlite` is `bundled`, so this also drops a C amalgamation from the build —
the release build was 7m42s, mostly that). Every command's output is byte-identical. **D9
needs no amendment**: a program with no derived index satisfies the ruling more completely,
not less.

### L1-b. The `Detector` trait has no polymorphism sites, and excludes the source that matters most. `delete`.

No `&dyn Detector`, no `Box<dyn Detector>`, no `Vec<Detector>` anywhere in the crate. The
trait is imported in `scan.rs` for exactly one reason: to bring `detect` into scope for two
inherent method calls. And `PackageDetector` **cannot** implement it — the hoisting design
(`#20`) put per-scan state on the caller's side of the signature. The abstraction covers 2
of 3 sources, and `mod.rs:8`'s claim that "each source implements the `Detector` trait" is
false about the module three lines below it.

The load-bearing artifact is the three-term `.or_else` ladder in `scan.rs` — hand-written,
never `dyn`. The trait is a name. Cost ~8 lines.

### L1-c. Three fields that carry no information. `delete`.

- **`Recipe.source`** — all three construction sites write `Source::Verified`; `UserSupplied`
  reaches `FileEntry` without ever passing through a `Recipe`. Its doc says *"whether chive
  inferred it or the owner taught it"* — a distinction the type cannot express.
- **`GitLocation.remote`** — write-only, and `remote_of` runs `git remote get-url` **per
  detected file**, so a 400-file dotfiles repo spawns 400 processes to fill a field nothing
  reads. Fetch it once per repo, in the commit that gives it a consumer.
- **`Dead`** — a one-variant enum whose `as_str` has no call site, returned in a tuple whose
  other two elements are constants (the caller even writes
  `debug_assert_eq!(verdict, Verdict::Disposable)` against a value the function cannot
  produce). The tell is the assertion: a function asserting a return value equals its only
  possible value is a signature asking to be simplified. Collapse to
  `provably_dead(abs) -> bool`.

### L1-d. The four-constructor zoo is one entry shape with four flags, and it is where D22 leaks. `rewrite`.

`new_unknown` **hardcodes** `Origin::Chive`; `new_disposable` takes `origin` as a parameter.
That inconsistency is the finding. Consequence, live today: a rule returning `"unknown"` is
built through `new_unknown`, so a **rule-authored** verdict is stamped `chive` — which is
non-sticky, against `D22`'s "rules are owner-authored policy, do not second-guess them."
`entry_for` collapses `Restorable | Unknown => new_unknown(...)`, so the misattribution is
invisible.

`new_absent_restorable` exists only because `new_restorable` hardcodes `present: true` —
`present` is a *fact about the filesystem*, not about the verdict, and it is being smuggled
through a verdict constructor because there is no evidence type to carry it.

**Steelman, and I could not remove it:** these constructors are the only place
`restorable ⇒ has a recipe` is enforced on the write path, and `toml.rs` re-checks it at the
read boundary. For a tool where a bad catalog means a permanently missing file, that
redundancy is worth something.

**Change:** split `Evidence { present, size, category }` from
`Decision { Restorable{recipe,origin}, Hole{origin}, Disposed{origin} }`. Then `present`
comes only from the lstat, `Origin` is carried *by the variant* so it cannot disagree with
the verdict, and the misattribution above becomes unconstructible. ~200 lines. The only item
I would call mandatory.

### L1-e. `docs/SUMMARY.md` is 85% derivative and confidently wrong. `delete`.

Zero inbound links from anywhere. Its `## MVP scope` block asserts *"Provable-dead detection
(dangling symlink, **removed-package residue**)"* — the capability `target-state.md` and
`decisions.md` deliberately deleted two commits ago, with a paragraph each explaining why. A
summary that is confidently wrong about a ruling made two days ago is worse than no summary,
because readers trust the short one more.

### L1-f. The illustrative CLI output blocks in the spec are a genre with no checker. `rewrite`.

This is the highest-leverage docs finding. Every *other* thing in the spec is checked by
something: schemas by `catalog/toml.rs`, the scan order by a named test, the provenance order
by a named test. **Output blocks have no checker at all, and they are the most copyable part
of the document, which is why they rot first.** The drift sweep found **six** divergences
without trying — including a `restorable` row inside the `holes` listing (both `target-state.md`
and `decisions.md` show a non-hole; the code is right), and a `disposable` breakdown with no
rule-origin bucket that `cli.rs` says is mandatory. Plus `clean --dry-run` is documented
showing a `Remove? [y/N]` prompt, which a dry run must never show.

**Change:** delete every output block; replace with a one-line statement of the *rule* the
output encodes plus a pointer to the test holding it; move the golden output into
`cli_tests.rs` as `assert_eq!`. That converts the drift engine from documentation into a gate.

---

# Lens 2 — architecture

### L2-a. The package probe runs **twice per file**, and the test suite is structurally blind to it. `rewrite`.

`entry_for` calls `PackageDetector::detect` at line 226 (for the `package` rule fact) and
again at line 254 (for the recipe), with no memo between them. Both copies are thrown away
or recomputed. With zero rules configured — the default — the entire adapter fan-out runs
twice for every file.

On a 5,000-file home with the six shipped Linux adapters: **~60,000 process spawns**, at
2–5 ms each, for a scan whose actual work is walking a directory tree. That is minutes.

`#20` hoisted the `--version` probe. It did not hoist the ownership probe. And the
regression test for #20 asserts on `version_probe_count()` — so the suite *cannot see this
class of regression*, which is why it survived the fix that was supposed to prevent it.

Two independent fixes, both cheap:

1. Probe once, thread the `Recipe` forward (~15 lines). Halves the dominant term.
2. **`owns_under` prefix prefilter** — ~15 lines, and the largest single win in the audit.
   `brew` already declares `path_prefix` and costs **zero** processes per file. The other six
   spawn a process per file *including every file under `$HOME`*, when the set of paths any
   of them can own is a short static list (`/usr`, `/etc`, …). A file at
   `~/dotfiles/init.el` cannot be owned by any of them, and chive forks to learn that, every
   file, twice. `owns_under` defaults absent = no prefilter, so a user adapter is unaffected.

Together: ~60,000 spawns becomes ~60,000 string comparisons.

### L2-b. "Apply the governing act to this path" has two owners, and they have already diverged.

The rule belongs to the scanner (`entry_for`). It is implemented a *second* time, inline, in
`cmd_teach` and `cmd_dispose`. `cmd_withdraw` delegates correctly. So: three verbs, two
implementations, and the second covers two of three cases.

`entry_for` writes six fields from fresh evidence; the closures write four and never touch
`present`, `size`, `modified`, or `category`. **The disagreement is already user-visible:**
`withdraw` on a taught-but-absent path produces an entry claiming `present: true`, because
`entry_for` infers presence from the *act* while the construct that produced it inferred it
from the *filesystem*. The act decides the verdict; the filesystem decides presence.

**Steelman:** the verbs must not re-run provenance — building a `Scanner` for a `teach` on
an absent path drags in the package detector and the rules engine for a file that may not
exist.

**Change:** make `record_act` call `App::rederive` the way `cmd_withdraw` already does, and
delete all three closure bodies (~60 lines net). Fix the cost, don't accept it: `entry_for`'s
act branch returns before touching provenance, so a binding act never constructs the chain.
Then `teach`/`dispose` cost *less* than today. Derive `present` from `meta.is_none()` at its
owner.

**This is also what makes #41 cheap.** Folder-scope verdicts need one function in `scan.rs`
changed; with the closures alive they need three closures rewritten.

### L2-c. A user adapter cannot override a confidently-wrong built-in. `rewrite`. 15 lines.

This one directly defeats the design want. `backends.toml:1-6` says *"to improve an existing
manager, edit its row."* The code makes "improve" and "add" different powers.

`package.rs` returns on the **first** manager yielding any name. Built-ins are prepended,
user rows appended. So a user override works **only when the built-in is silent** — when its
regex fails to match. It does **not** work when the built-in is *confidently wrong*, which is
precisely issue #24's defect class: apk's regex captured the path, rpm's captured
`pkg-version`, xbps asked the wrong flag. Each produced a *match* with a garbage name. The
user's corrected row sits in the table and is never consulted.

**Change:** a user row whose `name` collides with a built-in *replaces* it in place,
preserving position; non-colliding rows append as today. Makes "edit its row" true, keeps
stock ordering for the 99% who write nothing.

### L2-d. Three docs assert three platforms; the register says unverified and the code has one.

`README.md`, `SUMMARY.md`, and `target-state.md` all state "Linux / NixOS · macOS · Windows"
as fact. `triage.csv:10` (#9, **open**) is "Verify on macOS and Windows." There is no CI.
`backends.toml` has five `linux` rows, one macOS row whose prefix is `/opt/homebrew/Cellar/`
— Apple-Silicon only, no Intel, no Linuxbrew, despite the file's own comment — and **zero**
windows rows. `Os::Windows` filters *probes* by platform while the *recipes* those probes
produce are hardcoded POSIX one layer up (`sudo …`, `ln -s`, single quotes). So an adapter
passing the OS filter on Windows yields a recipe `cmd /c` cannot run: a promise kept in one
place and broken in three.

And `target-state.md:258` documents `cmd /c` as settled while **D5** (per-OS recipe variants)
is open — with a schema holding exactly one `restore_method` per file.

**Verdict: rewrite.** Replace the bare list with a support matrix that names what was
actually checked. Three lines, and more impressive than the current claim.

### L2-e. The musl-static constraint is load-bearing and has **zero reproducible build path**.

`why.md` and `decisions.md` both cite `docker/integration/README.md` as the authority for
musl-static, and D21's entire ruling (Rhai over Python) turns on it. That document's build
instruction is `nix-build … /tmp/build-static.nix` — **a file that does not exist**, here or
in the repo. The pointer next to it says `run.sh:27`, which runs `cargo build --release` —
a *glibc-dynamic* binary, the exact thing the README says will die.

No `flake.nix`, no `.cargo/config.toml`, no `rust-toolchain.toml`, no `rust-version`, no CI.
The static binary in `context/` is dated Sep 7 and came from an unrecorded manual action.

An architectural constraint the repository cannot reproduce is a constraint held in the
author's memory — the same failure mode as "a ruling that lives only in a chat log," except
underneath the load-bearing decision.

**Verdict: rewrite.** Commit `docker/integration/build-static.sh`. Then fix `run.sh:27`, which
also fails on a fresh clone because `$CTX` is gitignored and never `mkdir -p`'d.

### L2-f. The container harness is a fixed red light, and its README table is a lie.

`run-in-container.sh:83,85` assert `temporary` and `orphaned` — both spellings
`verdict.rs` refuses outright ("No legacy reader"). So both take the `fail` branch and
**every distro reports FAIL**. The README's results table (dpkg PASS, rpm/apk/xbps/dnf
failing) describes a state two commits gone, and it still names `xbps-query -f` — the flag
`#24` replaced with `-o` — twice.

The `#[ignore]` ratchet in `tests/` *was* updated correctly, which is proof the discipline
works when wired to a machine, and proof that a shell script with a status string in it is
not. **This is the item I would fix first among the docs findings** — a harness whose entire
value is "it finds what the containers find, without the containers" is itself broken, and
nobody noticed for two days.

### L2-g. Dry-run is a property of one implementation, so there are three previews.

`Real::dry_run` *fakes results* rather than skipping actions: `run_argv` returns code 0 for
every command, and `Real::exists` returns `true` — so a dry-run **scan** produces different
verdicts than a real scan, and `app.rs` unit tests assert those verdicts from a fiction.
Meanwhile `chive clean --dry-run` takes the decision in the verb and never touches the
runner, leaving `Real::remove_file`'s dry-run policy unreachable.

Worse: `CHIVE_DRY_RUN` is inherited by every test child. Verified —
`CHIVE_DRY_RUN=1 cargo test --test main owner_decisions_survive` fails 3 tests with
`removed 0 file(s)`. **The suite's result depends on the developer's shell**, which is
exactly what the harness's own stated invariant forbids. And the env var is documented in none
of the four docs.

**Steelman:** the boolean cannot be forgotten by a new caller of the seam. **Verdict:
rewrite** — delete the flag, keep `clean --dry-run` (which is the documented one), and have
`plan` *be* a preview runner so `plan` and dry-run restore are one code path by construction
rather than by the module doc's promise.

### L2-h. `hermetic_git_env_once` is a process-global `unsafe set_var` whose SAFETY comment asserts something false.

`Once` serialises writers; it does not exclude readers. `cli_tests` and `mock_providers_tests`
spawn subprocesses from threads that never call `Env::new`, and `spawn` reads `environ` in
libc. That is a data race, not a flake. And the hermeticity is *accidental*: `envs_for_child()`
does not set the `GIT_*` vars, so git determinism depends on which test ran first.

**This is the one comment in the repo that states a constraint and lies** — and under
`CLAUDE.md`'s own rule that is the class that matters most. Fix: move the vars into
`envs_for_child()`, delete the `Once` and the `unsafe`, make hermeticity true by construction
instead of by timing.

### L2-i. The tests assert on stdout strings; the product has a machine-readable one.

43 integration tests, ~170 process spawns, and **not one assertion reads a structured field**.
The catalog TOML — the product per D18, already carrying `verdict`/`verdict_source`/
`restore_method`/`present` — is read exactly once in the whole region, by hand.

**Steelman, and it's right:** D18 made `status` and `holes` primary read paths; a user who
cannot read `status` has no product. **Verdict: wrong-but-keep the surface, split it.**
Verdict/recipe/origin/present assertions read `catalog.toml`; keep stdout assertions for exit
codes and the human listing. Deletes ~35 of ~170 spawns and ~20 substring assertions.

Two coverage holes that matter: **D21 has ten unit tests and zero end-to-end coverage** —
nothing proves a Rhai rule in a real `config.toml` changes a verdict in a real scan. And
**D22's asymmetry is untested**: seven tests prove owner verdicts are sticky; **zero** prove
an inferred `disposable` retracts when evidence changes — which is the exact case D22's own
rationale names (repoint a dangling symlink, rescan, expect `restorable`).

### L2-j. Where I could not beat the design

Five things, each argued in the region reports:

1. **The append-only `Vec<Act>` over a `BTreeMap<path, Act>`.** The log's semantic content is
   `path → newest act`, so a map is strictly better on every operation this codebase
   performs. It loses on `withdraw`: deleting the key erases the fact that a judgement was
   ever made, which is the exact failure the region exists to prevent. The `Vec` makes "we
   never delete" a property of the **type** — `ActLog` has no `remove`/`retain`/`clear` —
   rather than a policy every call site must remember. Worth the O(n) `latest`.
2. **The sorted `Vec<FileEntry>` with binary search, over a `BTreeMap`.** A map gives
   strictly better asymptotics for both mutating operations in one line, and loses on the
   access pattern that dominates: `status`, `holes`, `stats`, `plan`, `clean_preview` all walk
   every entry, and a contiguous `Vec` wins that outright. It also makes the *sort order the
   diff order*, so a no-op scan produces byte-identical TOML.
3. **The precedence ladder as early returns, over a declarative table.** Heterogeneous step
   shapes; a table would need `Box<dyn Fn>` and would obscure the one thing that must be
   obvious when reading that file — which source outranks which. The winning design is
   *resolver-shaped data, ladder-shaped control*: hoist the evidence gathering, keep the
   hand-written order.
4. **The `RestoreItem`/`RestoreOutcome` split.** A merged mutable step turns "every recipe was
   attempted" from a structural property into a loop discipline that one `continue` breaks.
5. **Fake package managers on a scoped `PATH`, over a mocked `Runner`.** A `Mock` would mock
   the code under test. The fakes cost ~0 ms and buy exactly the properties `Mock`
   structurally lacks: PATH resolution by bare name (the only thing catching `xbps` →
   `xbps-query`), `sh -c` expansion, `ln -s` argv, exit-code mapping. The `#24` family was
   found by the host harness and then confirmed in containers — two independent layers.

**Lens 1 holds for the product as a whole.** The `home.nix`-you-never-wrote framing is right,
the act log is a better answer to "where do owner decisions live" than the sketch deserved,
and the three-verdict model is the correct call. What is wrong is the *boundary* between
automatic and owner authority — one enum field too many, one predicate too weak.

---

# Findings, ranked by wrongness × cost of leaving it

| # | Finding | Lens | Verdict | Cost |
|---|---|---|---|---|
| 1 | `Catalog.root` unvalidated — a stranger's catalog chooses where `clean` deletes | 2 | **rewrite** | ~15 lines |
| 2 | `contained()` lexical — a symlink in the root escapes it | 2 | **rewrite** | ~10 lines |
| 3 | No-clobber is a substring test — git recipes clobber local edits | 2 | **rewrite** | ~1 day + a ruling |
| 4 | `restored:` means exit 0, not "file is there" | 2 | **rewrite** | ~6 lines |
| 5 | Package probe runs twice per file; test suite blind to it | 2 | **rewrite** | ~15 lines |
| 6 | `owns_under` prefix prefilter | 3 | **rewrite** | ~15 lines |
| 7 | Rule-authored `unknown` stamped `Origin::Chive` (D22 violation) | 2 | **rewrite** | ~5 lines |
| 8 | User adapter cannot override a wrong built-in (#24's exact class) | 2 | **rewrite** | ~15 lines |
| 9 | "Apply the log" has two owners, already diverged | 2 | **rewrite** | −60 lines |
| 10 | Container harness asserts deleted verdicts — every distro FAILs | 2 | **rewrite** | ~45 min |
| 11 | `CHIVE_DRY_RUN` leaks into tests (verified: 3 failures) | 2 | **rewrite** | ~3 lines |
| 12 | `hermetic_git_env_once` is a real data race w/ a lying SAFETY comment | 2 | **rewrite** | ~8 lines |
| 13 | SQLite index is write-only; 5 `FromStr` impls exist only for it | 1 | **delete** | −450 lines, −1 dep |
| 14 | `Evidence` / `Decision` split (kills the constructor zoo) | 1 | **rewrite** | ~200 lines |
| 15 | Spec output blocks are unchecked and already drifted 6 ways | 1 | **rewrite** | half a day |
| 16 | musl-static constraint has no reproducible build path | 2 | **rewrite** | 30 min |
| 17 | D21 rules have zero end-to-end coverage | 2 | **rewrite** | ~20 lines |
| 18 | D22's retracting-inference half untested | 2 | **rewrite** | ~8 lines |
| 19 | Platform claims vs register vs code | 2 | **rewrite** | 20 min |
| 20 | `Detector` trait, `Recipe.source`, `GitLocation.remote`, `Dead` | 1 | **delete** | ~50 lines + 400 spawns |
| 21 | `docs/SUMMARY.md` confidently stale | 1 | **delete** | 20 min |
| 22 | Dry-run: three previews, one a fiction | 2 | **rewrite** | ~1 day |
| 23 | `rm`/`substitute_root`/`index()`/`CHIVE_DRY_RUN`/`Store::Source` dead | 1 | **delete** | ~40 lines |
| 24 | Shell quoting: `{:?}` used as a shell quote in `git.rs` | 3 | **rewrite** | ~15 lines |
| 25 | Tests assert on stdout, not the catalog | 2 | **rewrite** | ~120 lines |

**#24 deserves a note.** `git.rs` interpolates `{:?}` (Rust's *Debug* format) into a
`sh -c` recipe. `Debug` escapes for Rust source literals, not for a shell: a repo directory
named `$HOME` or containing a backtick is live inside those double quotes. Three quoting
conventions coexist and none of them is a shell quoter. This is issue #14's class reached
through a filename rather than a config value.

---

# What I got wrong

1. **I led with a security framing before checking whether the threat model admits it.**
   Both containment holes require the owner to `import` an untrusted catalog — which the spec
   explicitly anticipates ("may have come from anywhere"), so the framing survives. But I
   wrote the first version of this section as "an attacker can delete your files" and had to
   walk it back to "a catalog you were told to import can delete files outside its root."
   The second sentence is the accurate one and it is still serious.
2. **My symlink test passed for the wrong reason on the first attempt.** I created
   `home/sub/link -> ../outside`, which resolves to `home/outside`, not the `outside/` I had
   made — so the escape didn't fire and `clean` reported success. I nearly recorded it as a
   false positive. The retest with `../../outside` confirmed it. A finding I dismissed because
   my fixture was wrong is the most expensive kind of dismissal.
3. **I claimed `cli_tests.rs` "shells out to six real package managers per fixture file."**
   Unmeasured, and `Mock`-free does not mean `Real`-everything: `App::new(true)` — the dry-run
   runner — answers every probe as success, so those tests are hermetic in a way I did not
   credit. The *hermeticity* finding stands on `CHIVE_DRY_RUN` and `HOSTNAME`, which I
   verified; the "six real managers" claim does not.

The subagents produced roughly 90 findings. I am reporting 25 because the other ~65 are
either cheap-and-already-in-the-commits, restatements of one claim across regions, or wrong.
Several wrong ones are named in the region files; the notable one is a claim that
`available_managers` should be derived rather than stored, which is issue #20's fix
described backwards.

---

# What I need from you

Four questions, three of which are yours by `CLAUDE.md` and one of which I cannot answer
for you:

1. **May a package recipe overwrite an existing dest?** Fix #3 needs it. I would rule **no** —
   `sudo apt-get install --reinstall` over a file the owner edited is the same clobber the
   rule exists to prevent, and chive cannot tell "reinstall will fix mine" from "reinstall
   will destroy yours."
2. **Is `Catalog.root` a trust boundary or a hint?** If a catalog's root is trusted because
   the owner chose the file, fix #1 is belt-and-braces. If it is data (the spec says it is),
   fix #1 is a hole. I read the spec as saying data, but the catalog is *also* something you
   import on purpose.
3. **Do you want the SQLite index kept?** #13 deletes 450 lines and a dependency, and it is
   the one finding where a reasonable person could keep it on purpose ("we will add an index
   query"). Deleting is reversible in an afternoon; keeping it means `FromStr` on five domain
   types exists solely to feed a store nothing reads.
4. **What are you building next?** Half of what makes a design wrong is the change it is
   about to face. #41 (folder hierarchy) is the known one, and finding #9 is what makes it
   cheap or expensive. If a GUI tier (D12) is close, #15 and #22 get worse, because a second
   consumer of the output layer is exactly what those findings are waiting for.

## Suggested order

**Today, before anything else** — #1, #2, #7, #11. Four small changes, two of them security,
one of them a data-race fix. None blocks anything.

**This week** — #3 + #4 (needs your answer on the package sub-question), #5 + #6 (a scan goes
from minutes to seconds), #8, #9.

**Then** — #10, #12, #16, #17, #18: the harness and the coverage that would have caught the
harness being broken.

**After** — #13 (delete the index), #14 (`Evidence`/`Decision`), #15 (spec output gates),
#20, #21, #23–25.

**#14 before #41**, not during: `Origin` names *who* but not *which decision*, so a folder
verdict inherited by a child cannot say so — which is exactly the lie #41's ruling forbids
flat rendering from telling. The fix is one nullable column (`decided_by`, the act's `seq`)
and it costs nothing today.
