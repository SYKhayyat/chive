//! The package-provenance runtime: run the declared adapters against a file.
//!
//! The scan asks [`PackageDetector::detect`] "which installed package owns this
//! file?" The detector consults each adapter in the table (order is
//! load-bearing), and the first that returns a package produces a reinstall
//! recipe. Probes go through the [`Runner`] seam so tests script the answers.
//!
//! Detection is *best effort and never fatal*: a manager whose probe fails
//! (missing tool, non-zero exit, unparseable output) is skipped, and chive
//! falls through to the next source (git, symlink, orphan). A single failing
//! adapter therefore cannot sink a whole scan.

use std::path::Path;

use crate::error::Result;
use crate::model::{Category, Source};
use crate::provenance::Recipe;
use crate::provenance::config::{Manager, Table, compile_name_match};
use crate::runner::Runner;

/// Answers "which package owns this file" from a table of declared adapters.
pub struct PackageDetector {
    /// Managers, in priority order, filtered to the current OS.
    managers: Vec<Compiled>,
}

/// A manager with its name regex already compiled, so per-file detection never
/// recompiles.
struct Compiled {
    spec: Manager,
    name_match: regex::Regex,
}

impl PackageDetector {
    pub fn new(table: Table) -> Result<Self> {
        let mut compiled = Vec::new();
        for spec in table.for_current_os().managers {
            let pattern = spec
                .name_match
                .clone()
                .unwrap_or_else(|| r"^([^ ]+)".to_string());
            let name_match = compile_name_match(&pattern, &spec.name)?;
            compiled.push(Compiled { spec, name_match });
        }
        Ok(PackageDetector { managers: compiled })
    }

    /// Probe `abs_path`, consulting only the managers `available` on this
    /// machine. Callers hoist availability out of the per-file loop (issue
    /// #20): a `--version` probe per adapter per file is a scan-long storm of
    /// subprocesses — O(files × managers) instead of O(managers).
    pub fn detect(
        &self,
        runner: &dyn Runner,
        available: &[String],
        abs_path: &Path,
    ) -> Option<Recipe> {
        for c in &self.managers {
            if let Some(program) = &c.spec.program {
                // argv-probe manager: only try it if its binary is present.
                if !available.iter().any(|a| a == program) {
                    continue;
                }
            }
            // A path-probe manager (no program) runs regardless.
            if let Some(pkg) = probe(runner, &c.spec, &c.name_match, abs_path) {
                return Some(Recipe {
                    restore_method: fill(&c.spec.restore, &pkg, ""),
                    source: Source::Verified,
                    category: category_for(&c.spec.name),
                });
            }
        }
        None
    }

    /// Which probe programs are present on this machine — the hoisted answer
    /// to "which managers can be consulted at all?" (issue #20: asked once per
    /// scan, never per file). Returns *program* names (`xbps-query`), the same
    /// key `detect` gates on, not manager names (`xbps`); the two differ for
    /// managers whose probe binary is not the manager itself.
    pub fn available(&self, runner: &dyn Runner) -> Vec<&str> {
        self.managers
            .iter()
            .filter_map(|c| match &c.spec.program {
                Some(prog) if runner.exists(prog) => Some(prog.as_str()),
                Some(_) => None,
                None => None, // path-probe manager, always conceptually present
            })
            .collect()
    }

    /// The names of every loaded manager, in priority order.
    pub fn manager_names(&self) -> Vec<&str> {
        self.managers.iter().map(|c| c.spec.name.as_str()).collect()
    }
}

fn probe(
    runner: &dyn Runner,
    spec: &Manager,
    name_match: &regex::Regex,
    abs_path: &Path,
) -> Option<String> {
    // Path probe: no command to run, derive the package from the path itself.
    if let Some(prefix) = &spec.path_prefix {
        let path = abs_path.to_string_lossy();
        return match_path(name_match, &path, prefix);
    }

    // The caller has already hoisted the exists() probe (issue #20); the
    // per-file probe only runs the ownership command itself.
    let program = spec.program.as_deref()?;
    let args = spec
        .detect_args
        .as_deref()?
        .iter()
        .map(|a| fill(a, "", &abs_path.to_string_lossy()))
        .collect::<Vec<_>>();
    let out = runner
        .run_argv(
            program,
            &args.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .ok()?;
    if !out.success() {
        return None;
    }
    // Match against the trimmed answer: probes end their line with a newline,
    // and an anchored `name_match` is written against the line, not the bytes.
    let name = name_match
        .captures(out.stdout.trim())
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())?;
    Some(name)
}

/// Apply a regex to a path and require it to fall under `prefix`.
fn match_path(re: &regex::Regex, path: &str, prefix: &str) -> Option<String> {
    if !path.starts_with(prefix) {
        return None;
    }
    re.captures(path)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Fill the `{pkg}` and `{path}` tokens in a template. `{dest}` is left intact
/// here; it is only substituted at restore time on a specific target.
fn fill(template: &str, pkg: &str, path: &str) -> String {
    let mut s = template.to_string();
    if !path.is_empty() {
        s = s.replace("{path}", path);
    }
    s.replace("{pkg}", pkg)
}

/// Best-effort category from the manager name. Package-owned files are usually
/// programs or configs; leave the extension classifier to decide the fine type.
fn category_for(manager: &str) -> Option<Category> {
    match manager {
        "brew" | "dpkg" | "pacman" | "rpm" | "dnf" | "apk" | "xbps" => Some(Category::Program),
        _ => None,
    }
}

#[cfg(test)]
mod package_detector_tests {
    use super::*;
    use crate::error::Result;

    fn detector() -> PackageDetector {
        PackageDetector::new(Table::builtin().unwrap()).unwrap()
    }

    /// The available-managers list for tests: names passed the way the scan
    /// passes them after hoisting (#20).
    fn available(_mock: &crate::runner::Mock, names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn argv_probe_yields_reinstall_recipe() -> Result<()> {
        let mut mock = crate::runner::Mock::default();
        mock.register_program("dpkg");
        mock.on_argv(|program, args| {
            if program == "dpkg" && args == ["-S", "/bin/ls"] {
                Some(crate::runner::Mock::ok("coreutils: /bin/ls"))
            } else {
                None
            }
        });
        let d = detector();
        let r = d.detect(&mock, &available(&mock, &["dpkg"]), Path::new("/bin/ls"));
        let r = r.expect("dpkg owns /bin/ls");
        assert_eq!(
            r.restore_method,
            "sudo apt-get install --reinstall coreutils"
        );
        assert_eq!(r.source, Source::Verified);
        Ok(())
    }

    #[test]
    fn manager_not_listed_as_available_is_skipped() -> Result<()> {
        // dpkg not in the hoisted available list -> no recipe, and crucially
        // no per-file exists() probe (the mock would record nothing either way;
        // the hoisting is asserted by the harness probe-count test).
        let mock = crate::runner::Mock::default();
        let d = detector();
        assert!(d.detect(&mock, &[], Path::new("/bin/ls")).is_none());
        Ok(())
    }

    #[test]
    fn non_zero_probe_is_skipped() -> Result<()> {
        let mut mock = crate::runner::Mock::default();
        mock.register_program("pacman");
        mock.on_argv(|_, _| {
            Some(crate::runner::Output {
                stdout: String::new(),
                stderr: "error".into(),
                code: Some(1),
            })
        });
        let d = detector();
        assert!(
            d.detect(
                &mock,
                &available(&mock, &["pacman"]),
                Path::new("/usr/bin/x")
            )
            .is_none()
        );
        Ok(())
    }

    #[test]
    fn pacman_owns_file_from_is_owned_by_line() -> Result<()> {
        let mut mock = crate::runner::Mock::default();
        mock.register_program("pacman");
        mock.on_argv(|program, args| {
            if program == "pacman" && args.first() == Some(&"-Qo") {
                Some(crate::runner::Mock::ok(
                    "/usr/bin/jq is owned by jq 1.7.1-1",
                ))
            } else {
                None
            }
        });
        let d = detector();
        let r = d
            .detect(
                &mock,
                &available(&mock, &["pacman"]),
                Path::new("/usr/bin/jq"),
            )
            .expect("owns jq");
        assert_eq!(r.restore_method, "sudo pacman -S jq");
        Ok(())
    }

    #[test]
    fn available_lists_probe_programs_not_manager_names() -> Result<()> {
        // xbps probes via `xbps-query`, not `xbps`: the hoisted list must use
        // the program name (what detect() gates on) or the manager would
        // silently never run.
        let mut mock = crate::runner::Mock::default();
        mock.register_program("xbps-query");
        let d = detector();
        let avail = d.available(&mock);
        assert!(
            avail.contains(&"xbps-query"),
            "available() must list probe programs, got: {avail:?}"
        );
        assert!(!avail.contains(&"xbps"));
        Ok(())
    }

    #[test]
    fn brew_path_probe_needs_no_command() -> Result<()> {
        let mock = crate::runner::Mock::default(); // no programs registered
        let d = detector();
        // Path-probe managers need no availability entry: they run regardless.
        let r = d.detect(
            &mock,
            &[],
            Path::new("/opt/homebrew/Cellar/jq/1.7.1/bin/jq"),
        );
        // brew is os=macos; filtered out on linux, so if we're not on macos
        // this returns None. Guard the assertion by platform.
        if cfg!(target_os = "macos") {
            assert_eq!(r.unwrap().restore_method, "brew reinstall jq");
        } else {
            assert!(r.is_none());
        }
        Ok(())
    }
}
