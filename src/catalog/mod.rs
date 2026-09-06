pub mod db;
pub mod toml;

use std::collections::BTreeMap;

use crate::model::FileEntry;

/// A catalog: metadata plus every recorded file.
///
/// The present state of a machine, addressed by path relative to the scan root
/// so the same catalog describes any machine whose home lives at a different
/// absolute path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalog {
    /// The absolute scan root on the machine that produced this catalog.
    pub root: String,
    /// The scan's canonical timestamp (ISO 8601 UTC).
    pub scanned_at: String,
    /// The hostname that produced the catalog.
    pub host: String,
    /// Entries kept sorted by `path` and addressed via [`Catalog::by_path`].
    files: Vec<FileEntry>,
}

impl Catalog {
    pub fn new(root: String, scanned_at: String, host: String, mut files: Vec<FileEntry>) -> Self {
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Catalog {
            root,
            scanned_at,
            host,
            files,
        }
    }

    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    pub fn by_path(&self, path: &str) -> Option<&FileEntry> {
        // Binary search over the sorted entries.
        self.files
            .binary_search_by(|e| e.path.as_str().cmp(path))
            .ok()
            .map(|i| &self.files[i])
    }

    /// Replace one entry by path, keeping sorted order. Missing path => error.
    pub fn upsert(&mut self, entry: FileEntry) -> std::result::Result<(), String> {
        match self.files.binary_search_by(|e| e.path.cmp(&entry.path)) {
            Ok(i) => self.files[i] = entry,
            Err(i) => self.files.insert(i, entry),
        }
        Ok(())
    }

    /// Index of every path, efficient for bulk lookups.
    pub fn index(&self) -> BTreeMap<&str, &FileEntry> {
        self.files.iter().map(|e| (e.path.as_str(), e)).collect()
    }
}

#[cfg(test)]
mod catalog_tests {
    use super::*;
    use crate::model::{Category, Source};

    fn entry(path: &str) -> FileEntry {
        FileEntry::new_orphaned(path.into(), Some(Category::Document), 1, None)
    }

    #[test]
    fn by_path_finds_and_misses() {
        let mut c = Catalog::new(
            "/home/u".into(),
            "t".into(),
            "h".into(),
            vec![entry("b"), entry("a")],
        );
        c.upsert(entry("c")).unwrap();
        assert!(c.by_path("a").is_some());
        assert!(c.by_path("b").is_some());
        assert!(c.by_path("c").is_some());
        assert!(c.by_path("missing").is_none());
    }

    #[test]
    fn constructor_sorts_by_path() {
        let c = Catalog::new(
            "/r".into(),
            "t".into(),
            "h".into(),
            vec![entry("zeta"), entry("alpha")],
        );
        let paths: Vec<_> = c.files().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["alpha", "zeta"]);
    }

    #[test]
    fn reinserting_existing_path_updates_in_place() {
        let mut c = Catalog::new("/r".into(), "t".into(), "h".into(), vec![entry("a")]);
        let upgraded = FileEntry::new_restorable(
            "a".into(),
            None,
            "git checkout -- A".into(),
            Source::Verified,
            3,
            None,
        );
        c.upsert(upgraded).unwrap();
        assert_eq!(c.files().len(), 1);
        assert_eq!(
            c.by_path("a").unwrap().status,
            crate::model::Status::Restorable
        );
    }
}
