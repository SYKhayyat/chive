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
`rpm -qf`, `apk info -W`, `xbps-query -f`. No mocks.

```
./docker/integration/run.sh                 # ubuntu arch fedora alpine [void]
DISTROS="ubuntu fedora" ./docker/integration/run.sh
BUILD_ONLY=1 ./docker/integration/run.sh
```

The container binary must be **statically linked** (a Nix/glibc binary dies in a
foreign image because its interpreter lives under `/nix/store`). Build it with
`nix-build --arg pkgs import /tmp/build-static.nix`-style derivation (see
`run.sh`) or any musl static build, and place it at
`docker/integration/context/chive`.

### What it has found (open issues)

| | manager | distro | result | issue |
|---|---|---|---|---|
| `dpkg` | dpkg | ubuntu | **PASS** | — |
| `pacman` | pacman | arch | **PASS** | — |
| `rpm` | rpm/dnf | fedora | recipe `reinstall <pkg>-<ver>` (name_match grabs version) | #24 |
| `apk` | apk | alpine | recipe `add --upgrade <path>` (name_match grabs path) | #24 |
| `xbps` | xbps | void | xbps image install blocked by rootless TLS on this box; regex provably broken via fake | #24 |
| `dnf` | dnf | fedora | **unreachable** — `rpm` is checked first and always matches | #24 |

`void`'s image build is blocked on this box by the rootless Docker network
rewriting TLS (`SSL certificate subject does not match ... repo.voidlinux.org`);
the xbps adapter bug is independently shown by the host fake.

The git-nested provenance failure (#19) is confirmed by **every** distro run
(reported as a soft signal, not a hard fail, until #19 is fixed).