//! `config.toml` — user-editable settings.
//!
//! Today it holds the ignore list and scan defaults. Unknown keys are a hard
//! error (`deny_unknown_fields`), the same policy Shall uses, so a typo cannot
//! silently change how chive scans.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The directories never scanned. Matching is on the exact directory basename,
/// so `.git` matches any `.git` dir at any depth. This is the default; the
/// file overrides it wholesale.
pub const DEFAULT_IGNORE: &[&str] = &[
    ".git",
    ".svn",
    "node_modules",
    "target",
    "__pycache__",
    ".cache",
    "dist",
    "build",
    ".next",
    ".nuxt",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Directory basenames to skip during scan.
    #[serde(default = "default_ignore_list")]
    pub ignore: Vec<String>,
}

fn default_ignore_list() -> Vec<String> {
    DEFAULT_IGNORE.iter().map(|s| (*s).to_string()).collect()
}

impl Config {
    pub fn load(path: &Path) -> Result<Config> {
        if !path.exists() {
            // No config yet: defaults apply.
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(path).map_err(Error::Io)?;
        toml::from_str(&text).map_err(|e| Error::parse(path.to_path_buf(), e.to_string()))
    }

    /// Whether a directory with the given basename should be skipped.
    pub fn is_ignored(&self, basename: &str) -> bool {
        self.ignore.iter().any(|i| i == basename)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            ignore: default_ignore_list(),
        }
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    fn tmp(p: &str) -> std::path::PathBuf {
        tempfile::NamedTempFile::new()
            .unwrap()
            .into_temp_path()
            .parent()
            .unwrap()
            .join(p)
    }

    #[test]
    fn missing_config_yields_defaults() {
        let c = Config::load(Path::new("/definitely/absent/config.toml")).unwrap();
        assert!(c.is_ignored(".git"));
        assert!(c.is_ignored("node_modules"));
        assert!(!c.is_ignored("src"));
    }

    #[test]
    fn parses_ignore_list() {
        let path = tmp("chive-config-a.toml");
        std::fs::write(&path, "ignore = [\".git\", \"vendor\"]\n").unwrap();
        let c = Config::load(&path).unwrap();
        assert!(c.is_ignored(".git"));
        assert!(c.is_ignored("vendor"));
        assert!(!c.is_ignored("node_modules"));
    }

    #[test]
    fn empty_file_uses_default_ignore() {
        // `ignore = []` is stored; a totally empty file means "no key" -> default.
        let c = Config::default();
        assert!(c.is_ignored("target"));
    }

    #[test]
    fn unknown_key_is_rejected() {
        let path = tmp("chive-config-unknown.toml");
        std::fs::write(&path, "ignore = []\nwrong_key = 1\n").unwrap();
        let err = Config::load(&path).unwrap_err();
        assert!(matches!(err, Error::Parse { .. }));
    }
}
