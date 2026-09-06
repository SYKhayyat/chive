//! A single seam through which every external command is launched.
//!
//! Both provenance detection (package/git probes) and restore (user recipes)
//! run external commands. Routing them all through [`Runner`] means scanning
//! and restoration can be tested without shelling out, and `--dry-run` is one
//! option on the real runner instead of a flag scattered through callers.
//!
//! Two calling conventions exist, and this module distinguishes them:
//!
//! - **argv** ([`Runner::run_argv`]) launches an exact program + arguments with
//!   no shell — used for provenance probes like `dpkg -S <path>`. No shell
//!   means no injection from the path being probed.
//! - **recipe** ([`Runner::run_recipe`]) runs the user's `{dest}`-expanded
//!   recipe string through the shell (`sh -c "<recipe>"` on Unix). Recipes are
//!   written as command strings in the catalog, so they are *by definition*
//!   shell; this is the one place a shell is used.

use std::process::{Command, Stdio};

use crate::error::Result;

/// The outcome of a single external command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: Option<i32>,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
}

/// The execution seam. Each implementation owns a different *how* not *whether*.
pub trait Runner {
    /// Run `program args...` with no shell, capturing stdout/stderr.
    fn run_argv(&self, program: &str, args: &[&str]) -> Result<Output>;

    /// Run a full recipe string through the shell. A non-zero exit is returned,
    /// not an error — the caller decides how to report a failed restore.
    fn run_recipe(&self, recipe: &str) -> Result<Output>;

    /// Whether `program` is present on `PATH`.
    fn exists(&self, program: &str) -> bool;
}

/// Executes commands for real. `dry_run` makes every command a no-op that
/// reports success without touching the machine.
#[derive(Debug, Clone, Default)]
pub struct Real {
    pub dry_run: bool,
}

/// A stub that records which commands chive *intends* to run without running
/// any. Tests inject it and then assert on the recorded argv/recipes.
///
/// Because scans run sequentially over a single thread, a `RefCell` would be
/// enough; a `Mutex` also permits a future parallel scan without rework.
#[derive(Debug, Default)]
pub struct Mock {
    programs: Vec<String>,
    recorded: std::sync::Mutex<Vec<String>>,
}

impl Real {
    /// The shell pair for executing recipe strings on this platform.
    fn recipe_invocation() -> (&'static str, &'static str) {
        if cfg!(windows) {
            ("cmd", "/C")
        } else {
            ("sh", "-c")
        }
    }
}

impl Runner for Real {
    fn run_argv(&self, program: &str, args: &[&str]) -> Result<Output> {
        if self.dry_run {
            return Ok(Output {
                stdout: String::new(),
                stderr: String::new(),
                code: Some(0),
            });
        }
        let out = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .output()?;
        Ok(capture(out))
    }

    fn run_recipe(&self, recipe: &str) -> Result<Output> {
        if self.dry_run {
            return Ok(Output {
                stdout: String::new(),
                stderr: String::new(),
                code: Some(0),
            });
        }
        let (shell, flag) = Self::recipe_invocation();
        let out = Command::new(shell)
            .arg(flag)
            .arg(recipe)
            .stdin(Stdio::null())
            .output()?;
        Ok(capture(out))
    }

    fn exists(&self, program: &str) -> bool {
        if self.dry_run {
            return true;
        }
        // Presence means the program responds to --version; a missing binary
        // yields an io error. Deliberately not `which` to stay cheap and pure.
        Command::new(program)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok()
    }
}

impl Mock {
    pub fn register_program(&mut self, program: impl Into<String>) {
        self.programs.push(program.into());
    }

    fn record(&self, line: String) {
        self.recorded.lock().unwrap().push(line);
    }

    /// Every recorded command, in the order chive intended them.
    pub fn recorded(&self) -> Vec<String> {
        self.recorded.lock().unwrap().clone()
    }
}

impl Runner for Mock {
    fn run_argv(&self, program: &str, args: &[&str]) -> Result<Output> {
        self.record(format!("{program} {}", args.join(" ")));
        Ok(Output {
            stdout: String::new(),
            stderr: String::new(),
            code: Some(0),
        })
    }

    fn run_recipe(&self, recipe: &str) -> Result<Output> {
        self.record(format!("recipe: {recipe}"));
        Ok(Output {
            stdout: String::new(),
            stderr: String::new(),
            code: Some(0),
        })
    }

    fn exists(&self, program: &str) -> bool {
        self.programs.iter().any(|p| p == program)
    }
}

fn capture(out: std::process::Output) -> Output {
    let code = out.status.code();
    Output {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        code,
    }
}

#[cfg(test)]
mod runner_tests {
    use super::*;

    #[test]
    fn real_exists_checks_version_probe() {
        let r = Real::default();
        // `true` is always on PATH on Unix; `--version` may exit 0 or 2 but
        // either way the program ran, so exists() is true.
        assert!(r.exists("true"), "true should be found on PATH");
        assert!(!r.exists("definitely-not-a-real-binary-xyz"));
    }

    #[test]
    fn dry_run_recipe_reports_success_without_running() {
        let r = Real { dry_run: true };
        assert_eq!(r.run_recipe("rm -rf /").unwrap().code, Some(0));
    }

    #[test]
    fn recipe_shell_is_sh_dash_c_on_unix() {
        if !cfg!(windows) {
            assert_eq!(Real::recipe_invocation(), ("sh", "-c"));
        }
    }

    #[test]
    fn mock_records_argv_and_recipes_and_exists() {
        let mut m = Mock::default();
        m.register_program("dpkg");
        assert!(m.exists("dpkg"));
        assert!(!m.exists("curl"));

        m.run_argv("dpkg", &["-S", "/bin/ls"]).unwrap();
        m.run_recipe("git checkout HEAD -- init.el").unwrap();

        let rec = m.recorded();
        assert_eq!(rec[0], "dpkg -S /bin/ls");
        assert_eq!(rec[1], "recipe: git checkout HEAD -- init.el");
    }

    #[test]
    fn output_success_reads_code() {
        let ok = Output {
            stdout: "x".into(),
            stderr: String::new(),
            code: Some(0),
        };
        let bad = Output {
            stdout: String::new(),
            stderr: "boom".into(),
            code: Some(1),
        };
        assert!(ok.success());
        assert!(!bad.success());
    }
}
