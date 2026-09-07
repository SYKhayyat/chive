# The host harness

The hermetic layer. `cargo test --test main` runs the real `chive` binary
against a scratch, human-shaped home directory. See `harness/mod.rs` for the
`Env` (a real home, isolated `HOME`/`CHIVE_CONFIG_DIR`/`PATH`/cwd) and
`mock_providers/mod.rs` for the fake-but-executable package managers.

Why a *fake* manager on `PATH` instead of a mocked `Runner`? So the scan runs
the real `Runner::exists`/`run_argv`/`name_match` code paths — only the *other
end* of the process is the test's. The fakes emit each manager's true output
format, so a `name_match` regex that is wrong against reality is wrong here too
(the `apk`/`rpm`/`xbps` bugs appear without a 30 GB distro image).

What is real: `git` (repos are committed with git), `sh -c` recipe execution,
`ln -s`, the filesystem, symlinks, and the full CLI.

Layout of a green test file: build the `Env`, lay down files / a git repo / a
symlink / ownership rows, `scan`, then assert the `status` lines.

Run just the bug-regressions (documents open issues until fixed):

```
cargo test --test main -- --ignored bug_
```