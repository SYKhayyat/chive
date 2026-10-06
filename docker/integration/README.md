# chive real-world harness

Two layers, mirroring Shall.

**Host harness (`tests/harness` + `tests/*_tests.rs`)** — hermetic, runs with
`cargo test`, no Docker. The real `chive` binary scans a scratch home laid out
like a real machine: real git repos (flat and nested), real symlinks, real temp
files, and package-owned files answered by *real executable fake* package
managers on a scoped `PATH` that replicate each manager's real output format.
Fast, CI-safe, and it already catches the same adapter bugs the containers do —
because the fakes emit real `dpkg`/`pacman`/`apk`/`rpm`/`xbps` output.

Open issues are encoded as `#[ignore]`d regression tests in
`tests/bug_regressions_document_open_issues_tests.rs`; the suite stays green and
`cargo test -- --ignored` shows each bug still present.

**Container harness (`docker/integration`)** — disposable Linux images, one per
native package manager. Each installs a real package (warming the ownership DB),
copies in a statically-linked `chive`, and scans a real `/usr/bin` + a fixture
tree. This is where the adapters face a **real** `dpkg -S`, `pacman -Qo`,
`rpm -qf`, `apk info -W`, `xbps-query -o`. No mocks.

```
./docker/integration/run.sh                 # ubuntu arch fedora alpine [void]
DISTROS="ubuntu fedora" ./docker/integration/run.sh
BUILD_ONLY=1 ./docker/integration/run.sh
```

The container binary must be **statically linked** (a Nix/glibc binary dies in a
foreign image because its interpreter lives under `/nix/store`). Build it with the
committed script and place it at `docker/integration/context/chive`:

```bash
./docker/integration/build-static.sh
```

A dynamically-linked `cargo build --release` will not work here, and that is not a
close call: the image has no `/nix/store` to find the interpreter in.

### What it has found

**This table is not maintained.** It rotted once already — it described the
pre-#24 adapter state for two days after #24 landed, because a hand-kept results
table for a test suite is a snapshot nobody re-runs. The run output is the record;
this file explains how to produce it.

What the container layer has caught that the host suite could not:

- **Real manager output.** The `apk` and `xbps` bugs of #24 were *confirmed* here
  after the host fakes proposed them: `apk info -W` is path-first, and `xbps-query
  -f` lists a package's files instead of answering ownership. Both are why the
  adapter table asks each manager for the exact bare name it can.
- **The static-link requirement itself.** Only a container notices that the binary
  links against a `/nix/store` interpreter.

Current known limits:

- `void`'s image build is blocked on this box by the rootless Docker network
  rewriting TLS (`SSL certificate subject does not match ... repo.voidlinux.org`).
  The xbps adapter is covered by the host fakes instead.
- `dnf` is unreachable in the container layer: `rpm` is checked first and always
  matches on Fedora. Its recipe is covered by a host fake.