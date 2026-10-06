pub mod db;
pub mod toml;

use std::collections::BTreeMap;
use std::path::Component;

use crate::act::{self, Act, ActLog};
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

/// A catalog: metadata, every recorded path, and the owner act log.
///
/// The present state of a machine, addressed by path relative to the scan root
/// so the same catalog describes any machine whose home lives at a different
/// absolute path.
///
/// The act log travels *inside* the catalog (D20). It is what makes an owner's
/// decision durable — the scanner reads it rather than overwriting it — and
/// what makes it portable, since the catalog is the file a new machine imports.
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
    acts: ActLog,
}

impl Catalog {
    /// Every path is validated here, so a `Catalog` in memory never holds a
    /// path that could address a file outside the scan root. The same rule
    /// applies to the act log: a hand-edited catalog may name any path it likes,
    /// and a `teach` act naming one would otherwise smuggle it into a recipe.
    pub fn new(
        root: String,
        scanned_at: String,
        host: String,
        files: Vec<FileEntry>,
        acts: ActLog,
    ) -> Result<Self> {
        for e in &files {
            validate_path(&e.path)?;
        }
        for a in acts.acts() {
            validate_path(&a.path)?;
        }
        act::check_unique_seqs(acts.acts())?;
        let mut files = files;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Catalog {
            root,
            scanned_at,
            host,
            files,
            acts,
        })
    }

    /// The owner act log. Read by the scanner, appended to by the verbs.
    pub fn acts(&self) -> &ActLog {
        &self.acts
    }

    /// Record an owner act, stamping it with the next sequence number.
    ///
    /// Validates the path here for the same reason the constructor does: an act
    /// is as much a path a `restore` or `clean` will act on as an entry is, and
    /// a verb must not be able to write one the next load would refuse (#17).
    pub fn record(&mut self, act: Act) -> Result<Act> {
        validate_path(&act.path)?;
        Ok(self.acts.append(act))
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
    use crate::act::ActLog;
    use crate::model::{Category, Origin, Source, Verdict};

    fn entry(path: &str) -> FileEntry {
        FileEntry::new_unknown(path.into(), Some(Category::Document), 1, None)
    }

    fn cat(root: &str, files: Vec<FileEntry>) -> Result<Catalog> {
        Catalog::new(
            root.into(),
            "t".into(),
            "h".into(),
            files,
            ActLog::default(),
        )
    }

    #[test]
    fn by_path_finds_and_misses() {
        let mut c = cat("/home/u", vec![entry("b"), entry("a")]).unwrap();
        c.upsert(entry("c")).unwrap();
        assert!(c.by_path("a").is_some());
        assert!(c.by_path("b").is_some());
        assert!(c.by_path("c").is_some());
        assert!(c.by_path("missing").is_none());
    }

    #[test]
    fn constructor_sorts_by_path() {
        let c = cat("/r", vec![entry("zeta"), entry("alpha")]).unwrap();
        let paths: Vec<_> = c.files().iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec!["alpha", "zeta"]);
    }

    #[test]
    fn reinserting_existing_path_updates_in_place() {
        let mut c = cat("/r", vec![entry("a")]).unwrap();
        let upgraded = FileEntry::new_restorable(
            "a".into(),
            None,
            "git checkout -- A".into(),
            Source::Verified,
            Origin::Chive,
            3,
            None,
        );
        c.upsert(upgraded).unwrap();
        assert_eq!(c.files().len(), 1);
        assert_eq!(c.by_path("a").unwrap().verdict, Verdict::Restorable);
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
            let err = cat("/r", vec![entry(evil)]);
            assert!(err.is_err(), "{evil:?} must be refused");
        }
    }

    #[test]
    fn escape_refusal_names_the_reason() {
        let err = cat("/r", vec![entry("../outside.txt")]).unwrap_err();
        assert!(err.to_string().contains("escapes the scan root"));
    }

    #[test]
    fn ordinary_nested_paths_are_accepted() {
        cat(
            "/r",
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
        let mut c = cat("/r", vec![]).unwrap();
        assert!(c.upsert(entry("../evil")).is_err());
        assert!(c.files().is_empty(), "a refused entry is never stored");
    }

    #[test]
    fn an_escaping_act_path_is_refused_like_any_entry_path() {
        // A hand-edited act log is as much untrusted input as an entry list: a
        // `teach` naming `../escape` would smuggle the path into a recipe.
        let acts = ActLog::new(vec![Act::dispose(0, "../escape")], 1);
        assert!(Catalog::new("/r".into(), "t".into(), "h".into(), vec![], acts).is_err());
    }

    #[test]
    fn the_act_log_travels_with_the_catalog() {
        let acts = ActLog::new(vec![Act::dispose(0, "a")], 1);
        let c = Catalog::new("/r".into(), "t".into(), "h".into(), vec![], acts).unwrap();
        assert_eq!(
            c.acts().latest("a").unwrap().kind,
            crate::act::ActKind::Dispose
        );
    }

    #[test]
    fn record_appends_and_stamps_a_sequence_number() {
        let mut c = cat("/r", vec![]).unwrap();
        let recorded = c.record(Act::teach(0, "a", "echo a")).unwrap();
        assert_eq!(recorded.seq, 0);
        assert_eq!(c.acts().len(), 1);
        assert_eq!(c.acts().next_seq(), 1);
    }

    #[test]
    fn record_refuses_an_escaping_path() {
        let mut c = cat("/r", vec![]).unwrap();
        assert!(c.record(Act::teach(0, "../escape", "echo x")).is_err());
        assert!(
            c.acts().is_empty(),
            "a refused act is never stored, exactly like a refused entry"
        );
    }
}
