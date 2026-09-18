//! **A real home, run like a human.**
//!
//! The host harness runs the real `chive` binary against a scratch filesystem
//! laid out like a real home directory — real git repos (nested and flat), real
//! symlinks, real editor temp files, real ignored `node_modules`, orphaned
//! files, and package-owned files answered by real executable *fake* package
//! managers on a scoped `PATH` (`tests/mock_providers`).
//!
//! Nothing is mocked *inside* chive's `Runner`: the real `git`, `sh`, and `ln`
//! run. The only thing shadowed is the package-manager `PATH` — a harness must
//! test what `dpkg -S` *would* say without letting it touch the real host.
//!
//! The invariant a hermetic test needs (from Shall): a test that can see the
//! developer's real home or the repo's working dir is a test whose result means
//! nothing. Every run pins `CHIVE_CONFIG_DIR`, `HOME`/`USERPROFILE`, `PATH`
//! (fake managers first, real PATH after), and `current_dir` to the scratch
//! root, so `~` and `git -C` never escape.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::mock_providers::{Ownership, write_managers};

/// A hermetic, human-shaped machine for one test.
///
///   <root>/home   the scanned home (also `$HOME`)
///   <root>/store  `CHIVE_CONFIG_DIR`
///   <root>/bin    fake package managers, prepended to `PATH`
///   <root>/owners ownership fixtures read by the fakes
///   <root>/calls.log  recordings of every fake-manager invocation
pub struct Env {
    pub root: PathBuf,
    pub home: PathBuf,
    pub store: PathBuf,
    pub bin: PathBuf,
    pub owners: PathBuf,
    pub calls: PathBuf,
}

/// Make git run deterministically, deaf to the host's `~/.gitconfig` (a
/// signed-commit or absent-identity host would fail these).
fn hermetic_git_env_once() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for (k, v) in [
            ("GIT_AUTHOR_NAME", "chive-tests"),
            ("GIT_AUTHOR_EMAIL", "test@example.invalid"),
            ("GIT_COMMITTER_NAME", "chive-tests"),
            ("GIT_COMMITTER_EMAIL", "test@example.invalid"),
            ("GIT_CONFIG_GLOBAL", "absent-chive-gitconfig"),
            ("GIT_CONFIG_SYSTEM", "absent-chive-gitconfig"),
        ] {
            // SAFETY: ran once at test start; no other thread races the env.
            unsafe { std::env::set_var(k, v) };
        }
    });
}

impl Env {
    /// A fresh root under `CARGO_TARGET_TMPDIR`. Removed first, because that
    /// dir persists across runs and a fixture that only creates carries
    /// yesterday's state into today's assertion.
    pub fn new(name: &str) -> Env {
        hermetic_git_env_once();
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("chive-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let home = root.join("home");
        let store = root.join("store");
        let bin = root.join("bin");
        let owners = root.join("owners");
        let calls = root.join("calls.log");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&owners).unwrap();
        std::fs::write(&calls, "").unwrap();
        write_managers(&bin);
        Env {
            root,
            home,
            store,
            bin,
            owners,
            calls,
        }
    }

    /// The env a child `chive` must see. `PATH` pits the scratch `bin` first so
    /// fake managers win over real ones; the real PATH stays so `git`/`sh` run.
    pub fn envs_for_child(&self) -> Vec<(&'static str, PathBuf)> {
        let real = std::env::var_os("PATH").unwrap_or_default();
        let path = path_prefix(&self.bin, &real);
        vec![
            ("CHIVE_CONFIG_DIR", self.store.clone()),
            ("HOME", self.home.clone()),
            ("USERPROFILE", self.home.clone()),
            ("PATH", path),
            ("CHIVE_OWNERSHIP_FILE", self.owners.clone()),
            ("CHIVE_CALL_LOG", self.calls.clone()),
        ]
    }

    /// The real binary, pointed at this scratch machine, ready to run.
    pub fn cmd(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_chive"));
        c.current_dir(&self.root).stdin(Stdio::null());
        for (k, v) in self.envs_for_child() {
            c.env(k, v);
        }
        c
    }

    /// Run chive, return (stdout+stderr, exit code).
    pub fn run(&self, args: &[&str]) -> (String, i32) {
        let out = self.cmd().args(args).output().expect("chive should run");
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        (text, out.status.code().unwrap_or(-1))
    }

    /// Run and assert success (exit 0).
    pub fn ok(&self, args: &[&str]) -> String {
        let (out, code) = self.run(args);
        assert!(
            code == 0,
            "`chive {}` exited {code}, not 0:\n{out}",
            args.join(" ")
        );
        out
    }

    /// The `chive status` line for `rel`.
    pub fn status_line(&self, rel: &str) -> String {
        let (out, _) = self.run(&["status"]);
        out.lines()
            .find(|l| l.contains(rel))
            .unwrap_or_else(|| panic!("no status line for `{rel}` in:\n{out}"))
            .to_string()
    }

    // ---- tree builders -------------------------------------------------

    /// Write a file into the scanned home, creating parents. Returns its path.
    pub fn put(&self, rel: &str, body: &str) -> PathBuf {
        let p = self.home.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }

    /// A real git repo at `home/<rel>`, with the given files committed.
    pub fn git_repo(&self, rel: &str, _files: &[&str]) -> PathBuf {
        let repo = self.home.join(rel);
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let o = Command::new("git")
                .current_dir(&repo)
                .args(args)
                .output()
                .unwrap_or_else(|e| panic!("git {args:?} failed: {e}"));
            assert!(
                o.status.success(),
                "git {args:?} failed:\n{}",
                String::from_utf8_lossy(&o.stderr)
            );
        };
        git(&["init", "-q", "."]);
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=x",
            "-c",
            "user.email=y@z",
            "commit",
            "-q",
            "-m",
            "seed",
        ]);
        repo
    }

    /// A symlink at `home/<link_rel>` pointing at `target_abs`.
    pub fn symlink(&self, link_rel: &str, target_abs: &Path) -> PathBuf {
        let link = self.home.join(link_rel);
        std::fs::create_dir_all(link.parent().unwrap()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(target_abs, &link).unwrap();
        #[cfg(not(unix))]
        std::fs::write(&link, "").unwrap();
        link
    }

    /// Declare that `pkg` owns `home/<rel>` under fake manager `m`.
    pub fn own(&self, m: &str, rel: &str, pkg: &str) {
        let path = self.put(rel, "");
        self.own_at(m, &path, pkg);
    }

    /// Declare ownership of an arbitrary path (e.g. a symlink that already
    /// exists, which `own` cannot create because it writes a file).
    pub fn own_at(&self, m: &str, path: &Path, pkg: &str) {
        let mut o = self.read_ownership();
        o.add(m, pkg, path.to_string_lossy().as_ref());
        o.write_to(&self.owners);
    }

    /// Read back the current ownership fixtures (merging all managers).
    pub fn read_ownership(&self) -> Ownership {
        let mut o = Ownership::default();
        if let Ok(dir) = std::fs::read_dir(&self.owners) {
            for e in dir.flatten() {
                let m = e.file_name().to_string_lossy().into_owned();
                if let Ok(body) = std::fs::read_to_string(e.path()) {
                    for line in body.lines() {
                        if let Some((pkg, path)) = line.split_once(' ') {
                            o.add(&m, pkg, path);
                        }
                    }
                }
            }
        }
        o
    }

    /// How many times fake managers were asked `--version` — chive's `exists`
    /// probe, which is what the per-file package-detection perf bug exercises.
    pub fn version_probe_count(&self) -> usize {
        let body = std::fs::read_to_string(&self.calls).unwrap_or_default();
        body.lines().filter(|l| l.contains("--version")).count()
    }

    /// Total fake-manager invocations in the call log.
    pub fn call_count(&self) -> usize {
        let body = std::fs::read_to_string(&self.calls).unwrap_or_default();
        body.lines().count()
    }
}

/// Join a scratch dir onto the head of a real PATH, `:`-separated (Unix).
fn path_prefix(bin: &Path, real_path: &std::ffi::OsStr) -> PathBuf {
    let mut parts = vec![bin.to_path_buf()];
    parts.push(PathBuf::from(real_path));
    let joined = parts
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(":");
    PathBuf::from(joined)
}
