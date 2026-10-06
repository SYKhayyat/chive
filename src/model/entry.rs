use serde::Serialize;

use super::{Category, Origin, Source, Verdict};

/// One catalog entry: every field the catalog records about a single path.
///
/// The `path` is relative to the scan root (the primary key). `category`,
/// `source`, and `restore_method` are `Option` because the schema stores them
/// as nullable: an `unknown` entry has no recipe. Serialization matches the
/// schema in `docs/spec/target-state.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub verdict: Verdict,
    pub category: Option<Category>,
    pub restore_method: Option<String>,
    pub source: Option<Source>,
    /// Who decided `verdict`. Drives stickiness on the next scan (D22).
    pub verdict_source: Origin,
    /// Whether the path exists on the machine that wrote this catalog. `false`
    /// means a recipe was taught for a file that is not here, which is
    /// legitimate when planning a new machine (issue #45).
    pub present: bool,
    pub size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modified: Option<String>,
}

impl FileEntry {
    pub fn new_restorable(
        path: String,
        category: Option<Category>,
        restore_method: String,
        source: Source,
        verdict_source: Origin,
        size: i64,
        modified: Option<String>,
    ) -> Self {
        FileEntry {
            path,
            verdict: Verdict::Restorable,
            category,
            restore_method: Some(restore_method),
            source: Some(source),
            verdict_source,
            present: true,
            size,
            modified,
        }
    }

    /// An entry for a recipe that exists but whose file does not. Keeps a taught
    /// recipe reachable on a machine where the file has not been created yet.
    pub fn new_absent_restorable(
        path: String,
        category: Option<Category>,
        restore_method: String,
    ) -> Self {
        FileEntry {
            path,
            verdict: Verdict::Restorable,
            category,
            restore_method: Some(restore_method),
            source: Some(Source::UserSupplied),
            verdict_source: Origin::Owner,
            present: false,
            size: 0,
            modified: None,
        }
    }

    /// A hole chive decided by itself. For a rule's claim, use [`Self::new_hole`]
    /// — the origin drives stickiness (D22), so hardcoding `Chive` here is how a
    /// rule-authored verdict became non-sticky (issue #54).
    pub fn new_unknown(
        path: String,
        category: Option<Category>,
        size: i64,
        modified: Option<String>,
    ) -> Self {
        Self::new_hole(path, category, Origin::Chive, size, modified)
    }

    /// A hole decided by `origin`. Named `hole` rather than `unknown` so a caller
    /// passing a non-`Chive` origin reads as deliberate.
    pub fn new_hole(
        path: String,
        category: Option<Category>,
        verdict_source: Origin,
        size: i64,
        modified: Option<String>,
    ) -> Self {
        FileEntry {
            path,
            verdict: Verdict::Unknown,
            category,
            restore_method: None,
            source: None,
            verdict_source,
            present: true,
            size,
            modified,
        }
    }

    pub fn new_disposable(
        path: String,
        category: Option<Category>,
        verdict_source: Origin,
        size: i64,
        modified: Option<String>,
    ) -> Self {
        FileEntry {
            path,
            verdict: Verdict::Disposable,
            category,
            restore_method: None,
            source: None,
            verdict_source,
            present: true,
            size,
            modified,
        }
    }

    pub fn is_restorable(&self) -> bool {
        self.verdict == Verdict::Restorable
    }
}

#[cfg(test)]
mod entry_tests {
    use super::*;

    #[test]
    fn restorable_entry_wires_all_recipe_fields() {
        let e = FileEntry::new_restorable(
            "conf/init.el".into(),
            Some(Category::Config),
            "cp ~/dotfiles/init.el '{dest}'".into(),
            Source::Verified,
            Origin::Chive,
            100,
            Some("2026-01-01T00:00:00Z".into()),
        );
        assert!(e.is_restorable());
        assert_eq!(
            e.restore_method.as_deref(),
            Some("cp ~/dotfiles/init.el '{dest}'")
        );
        assert_eq!(e.source, Some(Source::Verified));
        assert!(e.present);
    }

    #[test]
    fn a_hole_may_be_decided_by_anything_and_records_who() {
        let mine = FileEntry::new_unknown("a".into(), None, 1, None);
        let ruled = FileEntry::new_hole("b".into(), None, Origin::Rule, 1, None);
        let owned = FileEntry::new_hole("c".into(), None, Origin::Owner, 1, None);
        assert_eq!(mine.verdict_source, Origin::Chive);
        assert!(!mine.verdict_source.is_sticky());
        assert!(ruled.verdict_source.is_sticky());
        assert!(owned.verdict_source.is_sticky());
        for e in [&mine, &ruled, &owned] {
            assert_eq!(e.verdict, Verdict::Unknown);
            assert!(!e.verdict.is_cleanable());
        }
    }

    #[test]
    fn unknown_entry_has_no_recipe_and_is_never_cleanable() {
        let e = FileEntry::new_unknown("tmp/thing".into(), None, 5, None);
        assert!(!e.is_restorable());
        assert_eq!(e.restore_method, None);
        assert_eq!(e.source, None);
        assert_eq!(e.category, None);
        assert!(!e.verdict.is_cleanable());
    }

    #[test]
    fn an_absent_recipe_is_still_restorable() {
        // Issue #45: a recipe taught for a path not on this machine must reach
        // the archive, so the entry carries the recipe and records its absence.
        let e = FileEntry::new_absent_restorable("future/file".into(), None, "echo hi".into());
        assert!(e.is_restorable());
        assert!(!e.present);
        assert_eq!(e.restore_method.as_deref(), Some("echo hi"));
        assert_eq!(e.verdict_source, Origin::Owner);
    }

    #[test]
    fn owner_and_chive_disposals_differ_only_in_origin() {
        let mine = FileEntry::new_disposable("a".into(), None, Origin::Chive, 1, None);
        let theirs = FileEntry::new_disposable("b".into(), None, Origin::Owner, 1, None);
        assert!(mine.verdict.is_cleanable() && theirs.verdict.is_cleanable());
        assert!(!mine.verdict_source.is_sticky());
        assert!(theirs.verdict_source.is_sticky());
    }
}
