//! Package-manager adapters, declared as data so new managers (and user ones)
//! arrive without recompiling — the same lesson Shall's `adapters/` teaches.
//!
//! An adapter answers the two questions in the name: **where** does this file
//! come from (how to probe ownership), and **how** is it put back (the restore
//! recipe). Both are data; the runtime in `package.rs` interprets them.
//!
//! Two probe kinds exist:
//!
//! - **argv** — run a program with `{path}` filled, then extract the package
//!   name from stdout with a regex (`dpkg -S`, `pacman -Qo`, `rpm -qf`).
//! - **path** — a file under a prefix is owned by a package derived from the
//!   path itself (Homebrew's Cellar: package = the directory right under the
//!   prefix). No external command is needed.
//!
//! A restore template uses `{pkg}` and optionally `{dest}`. Template *fields*
//! are filled by chive; the rest of the string is whatever the package manager
//! needs (`sudo apt-get install --reinstall {pkg}`).

use std::path::Path;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The OS-family a manager is valid on. Filters so an adapter never probes the
/// wrong kind of system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    MacOs,
    Windows,
    Any,
}

impl Os {
    /// Whether this `os` value applies to the current target. `any` always does.
    pub fn current_matches(self) -> bool {
        if self == Os::Any {
            return true;
        }
        let current = if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        };
        self == current
    }
}

/// An adapter row, deserialized from TOML.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manager {
    pub name: String,
    #[serde(default)]
    pub os: Vec<Os>,
    /// argv probe: program + argument template (`{path}`).
    pub program: Option<String>,
    pub detect_args: Option<Vec<String>>,
    /// path probe: files under this prefix are owned by the package named by
    /// `name_match`, applied to the path.
    pub path_prefix: Option<String>,
    /// Where to find the package name: a regex against probe stdout (for argv)
    /// or against the path (for path). Capture group 1 is the package name.
    pub name_match: Option<String>,
    /// The restore recipe shell, with `{pkg}` (and `{dest}` when relevant).
    pub restore: String,
}

#[derive(Default)]
pub struct Table {
    pub managers: Vec<Manager>,
}

impl Table {
    /// Parse a TOML document into a table.
    pub fn from_str(text: &str, path: &Path) -> Result<Table> {
        let doc: TableDoc =
            toml::from_str(text).map_err(|e| Error::parse(path.to_path_buf(), e.to_string()))?;
        Ok(Table {
            managers: doc.manager,
        })
    }

    /// The shipped adapter set (the data in `backends.toml`).
    pub fn builtin() -> Result<Table> {
        Table::from_str(
            include_str!("backends.toml"),
            Path::new("backends.toml (built-in)"),
        )
    }

    /// Load the built-in set plus every user adapter under `adapters_dir`.
    ///
    /// User tables append to the built-ins, so a user adapter can add a manager
    /// chive has never heard of. Order respects that a user's very own manager
    /// is consulted after the stock ones (the stock adapters win ties), which
    /// keeps built-in behaviour predictable.
    pub fn load(adapters_dir: &Path) -> Result<Table> {
        let mut table = Table::builtin()?;
        let entries: Vec<std::path::PathBuf> = match std::fs::read_dir(adapters_dir) {
            Ok(read) => read
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|e| e == "toml"))
                .map(|e| e.path())
                .collect(),
            Err(_) => return Ok(table), // no adapters dir yet
        };
        for path in entries {
            let text = std::fs::read_to_string(&path).map_err(Error::Io)?;
            let t = Table::from_str(&text, &path)?;
            table.managers.extend(t.managers);
        }
        Ok(table)
    }

    /// Keep only managers valid for this OS, in declaration order.
    pub fn for_current_os(self) -> Table {
        Table {
            managers: self
                .managers
                .into_iter()
                .filter(|m| m.os.is_empty() || m.os.iter().any(|o| o.current_matches()))
                .collect(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TableDoc {
    #[serde(rename = "manager")]
    manager: Vec<Manager>,
}

/// Compile a manager's `name_match` regex once, failing fast on a bad pattern.
pub fn compile_name_match(pattern: &str, manager: &str) -> Result<Regex> {
    Regex::new(pattern)
        .map_err(|e| Error::parse(manager.into(), format!("invalid name_match regex: {e}")))
}

#[cfg(test)]
mod package_config_tests {
    use super::*;

    #[test]
    fn parses_an_argv_adapter() {
        let doc = r#"
[[manager]]
name = "dpkg"
os = ["linux"]
program = "dpkg"
detect_args = ["-S", "{path}"]
name_match = '^([^: ]+)[: ]'
restore = "sudo apt-get install --reinstall {pkg}"
"#;
        let t = Table::from_str(doc, Path::new("t.toml")).unwrap();
        assert_eq!(t.managers.len(), 1);
        let m = &t.managers[0];
        assert_eq!(m.name, "dpkg");
        assert_eq!(m.restore, "sudo apt-get install --reinstall {pkg}");
    }

    #[test]
    fn unknown_field_is_rejected() {
        let doc = r#"[[manager]]
name = "x"
restore = "echo {pkg}"
bogus = 1
"#;
        assert!(Table::from_str(doc, Path::new("t.toml")).is_err());
    }

    #[test]
    fn os_filter_keeps_matching_os() {
        let doc = r#"
[[manager]]
name = "brew"
os = ["macos"]
restore = "brew reinstall {pkg}"
[[manager]]
name = "anyone"
os = ["any"]
restore = "true"
"#;
        let table = Table::from_str(doc, Path::new("t.toml"))
            .unwrap()
            .for_current_os();
        // brew appears only on macOS; the adapter behind this test runs on
        // whatever platform CI is, so "anyone" must always be present.
        assert_eq!(table.managers.len(), 1);
        assert_eq!(table.managers[0].name, "anyone");
    }

    #[test]
    fn os_enum_any_always_matches() {
        assert!(Os::Any.current_matches());
    }
}
