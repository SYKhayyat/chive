//! Taught recipes — the extension file (`recipes.toml`).
//!
//! `chive teach` writes a shell recipe for a file chive would not otherwise
//! rebuild. Recipes live as data here (never in program code), so teaching a
//! new restore source never requires recompiling — the same lesson Shall's
//! adapter system teaches (see `docs/spec/why.md`).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// A single taught entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Taught {
    pub path: String,
    /// The recipe shell. `{dest}` is substituted at restore time.
    pub method: String,
}

/// The recipes file contents. Kept as a `BTreeMap` keyed by path so a file has
/// exactly one taught recipe and lookups are fast.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Recipes {
    map: BTreeMap<String, String>,
}

impl Recipes {
    /// Load from a file, or return an empty set if it does not exist.
    pub fn load(path: &Path) -> Result<Recipes> {
        if !path.exists() {
            return Ok(Recipes::default());
        }
        let text = std::fs::read_to_string(path).map_err(Error::Io)?;
        let doc: Doc =
            toml::from_str(&text).map_err(|e| Error::parse(path.to_path_buf(), e.to_string()))?;
        let map = doc.recipe.into_iter().map(|t| (t.path, t.method)).collect();
        Ok(Recipes { map })
    }

    /// Set the recipe for `path` and persist to `into`, atomically.
    pub fn teach(&mut self, path: &str, method: &str, into: &Path) -> Result<()> {
        self.map.insert(path.to_string(), method.to_string());
        self.write(into)
    }

    /// Look up a taught recipe by relative path.
    pub fn get(&self, path: &str) -> Option<&str> {
        self.map.get(path).map(String::as_str)
    }

    /// Number of taught recipes.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterate (path, method) pairs, sorted by path.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().map(|(p, m)| (p.as_str(), m.as_str()))
    }

    fn write(&self, path: &Path) -> Result<()> {
        let doc = Doc {
            recipe: self
                .map
                .iter()
                .map(|(path, method)| Taught {
                    path: path.clone(),
                    method: method.clone(),
                })
                .collect(),
        };
        let text = toml::to_string(&doc).expect("recipes must serialize");
        atomic_write(path, text.as_bytes())
    }
}

#[derive(Deserialize, Serialize)]
struct Doc {
    recipe: Vec<Taught>,
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let stem = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("recipes");
    let tmp = dir.join(format!(".{stem}.tmp.{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(Error::Io)?;
    std::fs::rename(&tmp, path).map_err(Error::Io)
}

#[cfg(test)]
mod recipes_tests {
    use super::*;

    #[test]
    fn missing_file_is_empty() {
        let r = Recipes::load(Path::new("/definitely/absent/recipes.toml")).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn teach_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("recipes.toml");
        let mut r = Recipes::default();
        r.teach(
            "conf/init.el",
            "git -C ~/dotfiles pull && cp ~/dotfiles/init.el '{dest}'",
            &f,
        )
        .unwrap();
        let back = Recipes::load(&f).unwrap();
        assert_eq!(
            back.get("conf/init.el"),
            Some("git -C ~/dotfiles pull && cp ~/dotfiles/init.el '{dest}'")
        );
        assert_eq!(back.len(), 1);
    }

    #[test]
    fn teaching_same_path_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("recipes.toml");
        let mut r = Recipes::default();
        r.teach("a", "one", &f).unwrap();
        r.teach("a", "two", &f).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r.get("a"), Some("two"));
    }

    #[test]
    fn iter_is_sorted_by_path() {
        let mut r = Recipes::default();
        r.map.insert("zeta".into(), "z".into());
        r.map.insert("alpha".into(), "a".into());
        let paths: Vec<_> = r.iter().map(|(p, _)| p).collect();
        assert_eq!(paths, vec!["alpha", "zeta"]);
    }

    #[test]
    fn malformed_file_errors() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("recipes.toml");
        std::fs::write(&f, "not: [valid").unwrap();
        assert!(Recipes::load(&f).is_err());
    }
}
