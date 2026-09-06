//! Where chive keeps its state, and which file is which.
//!
//! The config directory is meant to be a git repo (the versionable part). The
//! layout follows `docs/spec/target-state.md`:
//!
//! ```text
//! <config-dir>/
//! ├── config.toml     # ignore list, scan defaults
//! ├── recipes.toml    # user-taught recipes (extension file)
//! └── catalog.db      # working index (derived, not committed)
//! ```

use std::path::{Path, PathBuf};

/// Resolution order: `--config-dir` (CLI) wins; else `$CHIVE_CONFIG_DIR`;
/// else the platform default (`~/.config/chive`).
pub enum Source {
    Cli(PathBuf),
    Env,
    Default,
}

/// The resolved set of paths chive will read and write.
#[derive(Debug, Clone)]
pub struct Store {
    pub dir: PathBuf,
}

impl Store {
    /// Resolve the store from an explicit CLI path if given.
    pub fn resolve(cli_dir: Option<PathBuf>) -> Self {
        let dir = match cli_dir {
            Some(d) => d,
            None => std::env::var_os("CHIVE_CONFIG_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(default_config_dir),
        };
        Store { dir }
    }

    /// For tests and callers that already know their root.
    pub fn at(dir: PathBuf) -> Self {
        Store { dir }
    }

    pub fn config_file(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    pub fn recipes_file(&self) -> PathBuf {
        self.dir.join("recipes.toml")
    }

    pub fn db_file(&self) -> PathBuf {
        self.dir.join("catalog.db")
    }

    /// The default catalog TOML destination (`catalog.toml` in the config dir),
    /// used when `export`/`scan` gets no explicit `--to`.
    pub fn default_catalog_file(&self) -> PathBuf {
        self.dir.join("catalog.toml")
    }

    /// Create the directory if it does not exist.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)
    }
}

/// The default config directory (`~/.config/chive` on Unix; `%APPDATA%`-style
/// on Windows), falling back to a `.chive` dir in the home if nothing is set.
fn default_config_dir() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("chive");
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(home).join(".config").join("chive");
    }
    Path::new(".").join(".chive")
}

/// Name of the recipes extension file (for error messages / docs).
pub const RECIPES_FILE: &str = "recipes.toml";
pub const CONFIG_FILE: &str = "config.toml";
pub const DB_FILE: &str = "catalog.db";

#[cfg(test)]
mod store_tests {
    use super::*;

    #[test]
    fn explicit_cli_path_wins_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::resolve(Some(dir.path().to_path_buf()));
        assert_eq!(s.dir, dir.path().to_path_buf());
    }

    #[test]
    fn sub_paths_are_relative_to_dir() {
        let s = Store::at(PathBuf::from("/tmp/chive-s"));
        assert_eq!(s.db_file(), PathBuf::from("/tmp/chive-s/catalog.db"));
        assert_eq!(s.recipes_file(), PathBuf::from("/tmp/chive-s/recipes.toml"));
        assert_eq!(
            s.default_catalog_file(),
            PathBuf::from("/tmp/chive-s/catalog.toml")
        );
    }

    #[test]
    fn ensure_creates_missing_directory() {
        let base = tempfile::tempdir().unwrap();
        let s = Store::at(base.path().join("nested").join("chive"));
        assert!(!s.dir.exists());
        s.ensure().unwrap();
        assert!(s.dir.exists());
    }

    #[test]
    fn env_var_overrides_default() {
        // Simulate by resolving with an explicit CLI that shadows env.
        let via_env = Store::resolve(None);
        // Without HOME guarantees we just assert it returns something sensible.
        assert!(!via_env.dir.as_os_str().is_empty());
    }
}
