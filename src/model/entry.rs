use serde::Serialize;

use super::{Category, Source, Status};

/// One catalog entry: every field the catalog records about a single file.
///
/// The `path` is relative to the scan root (the primary key). `category`,
/// `source`, and `restore_method` are `Option` because the schema stores them
/// as nullable: neither an orphan nor a temporary file has a recipe or a
/// category. Serialization matches the schema in `docs/spec/target-state.md`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileEntry {
    pub path: String,
    pub status: Status,
    pub category: Option<Category>,
    pub restore_method: Option<String>,
    pub source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_restorable_reason: Option<String>,
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
        size: i64,
        modified: Option<String>,
    ) -> Self {
        FileEntry {
            path,
            status: Status::Restorable,
            category,
            restore_method: Some(restore_method),
            source: Some(source),
            not_restorable_reason: None,
            size,
            modified,
        }
    }

    pub fn new_orphaned(
        path: String,
        category: Option<Category>,
        size: i64,
        modified: Option<String>,
    ) -> Self {
        FileEntry {
            path,
            status: Status::Orphaned,
            category,
            restore_method: None,
            source: None,
            not_restorable_reason: None,
            size,
            modified,
        }
    }

    pub fn is_restorable(&self) -> bool {
        self.status == Status::Restorable
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
            100,
            Some("2026-01-01T00:00:00Z".into()),
        );
        assert!(e.is_restorable());
        assert_eq!(
            e.restore_method.as_deref(),
            Some("cp ~/dotfiles/init.el '{dest}'")
        );
        assert_eq!(e.source, Some(Source::Verified));
        assert_eq!(e.not_restorable_reason, None);
    }

    #[test]
    fn orphaned_entry_has_no_recipe() {
        let e = FileEntry::new_orphaned("tmp/thing".into(), None, 5, None);
        assert!(!e.is_restorable());
        assert_eq!(e.restore_method, None);
        assert_eq!(e.source, None);
        assert_eq!(e.category, None);
    }

    #[test]
    fn not_restorable_reason_is_preserved_and_optional() {
        let mut e =
            FileEntry::new_orphaned("Pictures/photo.nef".into(), Some(Category::Image), 10, None);
        e.not_restorable_reason = Some("teach me a recipe".into());
        assert_eq!(
            e.not_restorable_reason.as_deref(),
            Some("teach me a recipe")
        );
    }
}
