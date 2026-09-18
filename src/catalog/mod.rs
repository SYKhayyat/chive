pub mod db;
pub mod toml;

use std::collections::BTreeMap;
use std::path::Component;

use crate::error::{Error, Result};
use crate::model::FileEntry;

/// The containment rule every catalog path must satisfy. A path is relative to
/// the scan root, `/`-separated, and names nothing outside the root: no `..`,
/// no `.` segments, no empty segments, no absolute form, no drive prefix, no
/// backslash (a backslash is a legal filename character on Unix, but it is how
/// a Windows-authored catalog would smuggle a second separator past a `join`).
///
/// `Catalog` only ever holds validated paths, so `restore` and `clean` can
/// join a path onto a root without re-proving containment each time. This is
/// the boundary behind issue #17: a catalog is a file a stranger may hand you,
/// and `root.join("..")` is how it would write outside everything chive owns.
pub fn validate_path(path: &str) -> Result<()> {
    let refuse = |why: &str| {
        Err(Error::Catalog(format!(
            "entry path {path:?} escapes the scan root: {why}"
        )))
    };
    if path.is_empty() {
        return refuse("paths are non-empty");
    }
    if path.contains('\\') {
        return refuse("catalog paths are /-separated");
    }
    if path.contains('\0') {
        return refuse("paths never contain NUL");
    }
    // Textual segment rule: `.` and `..` are refused even where `Path`
    // normalization would quietly swallow them.
    for seg in path.split('/') {
        match seg {
            "" => return refuse("segments are non-empty (no `//`)"),
            "." => return refuse("no `.` segments"),
            ".." => return refuse("`..` cannot be joined safely"),
            _ => {}
        }
    }
    let first = path.split('/').next().unwrap_or("");
    let bytes = first.as_bytes();
    let drive_prefix = bytes.len() == 2 && bytes[1] == b':' && bytes[0].is_ascii_alphabetic();
    if drive_prefix {
        return refuse("no drive-letter prefixes");
    }
    for c in std::path::Path::new(path).components() {
        if !matches!(c, Component::Normal(_)) {
            return refuse("paths are relative to the scan root");
        }
    }
    Ok(())
}

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
    /// Every path is validated here, so a `Catalog` in memory never holds a
    /// path that could address a file outside the scan root.
    pub fn new(
        root: String,
        scanned_at: String,
        host: String,
        files: Vec<FileEntry>,
    ) -> Result<Self> {
        for e in &files {
            validate_path(&e.path)?;
        }
        let mut files = files;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Catalog {
            root,
            scanned_at,
            host,
            files,
        })
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

    /// Replace one entry by path, keeping sorted order. The same containment
    /// rule as the constructor applies; an invalid path is refused.
    pub fn upsert(&mut self, entry: FileEntry) -> Result<()> {
        validate_path(&entry.path)?;
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
        )
        .unwrap();
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
        )
        .unwrap();
        let paths: Vec<_> = c.files().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["alpha", "zeta"]);
    }

    #[test]
    fn reinserting_existing_path_updates_in_place() {
        let mut c = Catalog::new("/r".into(), "t".into(), "h".into(), vec![entry("a")]).unwrap();
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

    #[test]
    fn escapes_are_refused_at_construction() {
        for evil in [
            "../outside.txt",
            "a/../../outside.txt",
            "./hidden",
            "a/./b",
            "/etc/passwd",
            "a\\win",
            "C:/Users/x",
            "",
            "a/../b",
            "a//b",
        ] {
            let err = Catalog::new("/r".into(), "t".into(), "h".into(), vec![entry(evil)]);
            assert!(err.is_err(), "{evil:?} must be refused");
        }
    }

    #[test]
    fn escape_refusal_names_the_reason() {
        let err = Catalog::new(
            "/r".into(),
            "t".into(),
            "h".into(),
            vec![entry("../outside.txt")],
        )
        .unwrap_err();
        assert!(err.to_string().contains("escapes the scan root"));
    }

    #[test]
    fn ordinary_nested_paths_are_accepted() {
        Catalog::new(
            "/r".into(),
            "t".into(),
            "h".into(),
            vec![
                entry("conf/emacs.d/init.el"),
                entry("my docs/file name.txt"),
                entry("dot.file"),
            ],
        )
        .unwrap();
    }

    #[test]
    fn upsert_validates_like_the_constructor() {
        let mut c = Catalog::new("/r".into(), "t".into(), "h".into(), vec![]).unwrap();
        assert!(c.upsert(entry("../evil")).is_err());
        assert!(c.files().is_empty(), "a refused entry is never stored");
    }
}
