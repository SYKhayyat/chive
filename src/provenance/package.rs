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

    /// Probe `abs_path`. Returns a reinstall recipe if a manager owns it.
    pub fn detect(&self, runner: &dyn Runner, abs_path: &Path) -> Option<Recipe> {
        for c in &self.managers {
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

    /// Which managers are present on this machine, for info/insight.
    pub fn available(&self, runner: &dyn Runner) -> Vec<&str> {
        self.managers
            .iter()
            .filter_map(|c| match &c.spec.program {
                Some(prog) if runner.exists(prog) => Some(c.spec.name.as_str()),
                Some(_) => None,
                None => None, // path-probe manager, always conceptually present
            })
            .collect()
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

    let program = spec.program.as_deref()?;
    if !runner.exists(program) {
        return None;
    }
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
    let name = name_match
        .captures(&out.stdout)
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
        let r = d.detect(&mock, Path::new("/bin/ls"));
        let r = r.expect("dpkg owns /bin/ls");
        assert_eq!(
            r.restore_method,
            "sudo apt-get install --reinstall coreutils"
        );
        assert_eq!(r.source, Source::Verified);
        Ok(())
    }

    #[test]
    fn manager_missing_binary_is_skipped() -> Result<()> {
        // dpkg not registered -> exists() false -> no recipe.
        let mock = crate::runner::Mock::default();
        let d = detector();
        assert!(d.detect(&mock, Path::new("/bin/ls")).is_none());
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
        assert!(d.detect(&mock, Path::new("/usr/bin/x")).is_none());
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
        let r = d.detect(&mock, Path::new("/usr/bin/jq")).expect("owns jq");
        assert_eq!(r.restore_method, "sudo pacman -S jq");
        Ok(())
    }

    #[test]
    fn brew_path_probe_needs_no_command() -> Result<()> {
        let mock = crate::runner::Mock::default(); // no programs registered
        let d = detector();
        let r = d.detect(&mock, Path::new("/opt/homebrew/Cellar/jq/1.7.1/bin/jq"));
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
