//! The TOML catalog — the versionable artifact.
//!
//! The TOML file is the source of truth (decision D9); SQLite is a derived
//! index rebuilt from it. Serde stays out of the public model: these DTO types
//! mirror the schema in `docs/spec/target-state.md` exactly, and
//! [`from_catalog`]/[`to_catalog`] are the only boundary the model crosses,
//! keeping the trim of optional fields in one place.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::model::{Category, FileEntry, Source, Status};

#[derive(Debug, Serialize, Deserialize)]
struct TomlCatalog {
    meta: TomlMeta,
    #[serde(default)]
    files: Vec<TomlFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlMeta {
    root: String,
    scanned_at: String,
    host: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlFile {
    path: String,
    status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    category: Option<Category>,
    #[serde(
        rename = "restore_method",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    restore_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<Source>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    not_restorable_reason: Option<String>,
    size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified: Option<String>,
}

impl TomlFile {
    fn from_entry(e: &FileEntry) -> Self {
        TomlFile {
            path: e.path.clone(),
            status: e.status,
            category: e.category,
            restore_method: e.restore_method.clone(),
            source: e.source,
            not_restorable_reason: e.not_restorable_reason.clone(),
            size: e.size,
            modified: e.modified.clone(),
        }
    }

    fn into_entry(self, path: &Path) -> Result<FileEntry> {
        let valid = self.size >= 0 && self.status_is_consistent(&self.status);
        match self {
            TomlFile { .. } if !valid => Err(Error::parse(
                path.to_path_buf(),
                format!("entry {path:?} has an invalid size or status/recipe combination"),
            )),
            TomlFile {
                path,
                status,
                category,
                restore_method,
                source,
                not_restorable_reason,
                size,
                modified,
            } => Ok(FileEntry {
                path,
                status,
                category,
                restore_method,
                source,
                not_restorable_reason,
                size,
                modified,
            }),
        }
    }

    fn status_is_consistent(&self, status: &Status) -> bool {
        let restorable = *status == Status::Restorable;
        // A restorable entry must carry a recipe and a source; a non-restorable
        // one must not.
        if restorable {
            self.restore_method.is_some() && self.source.is_some()
        } else {
            self.restore_method.is_none() && self.source.is_none()
        }
    }
}

/// Read a catalog from a TOML file on disk.
pub fn read(path: &Path) -> Result<Catalog> {
    let text = std::fs::read_to_string(path).map_err(Error::Io)?;
    from_str(&text, path)
}

/// Write a catalog to a TOML file, atomically.
pub fn write(catalog: &Catalog, path: &Path) -> Result<()> {
    let toml = to_catalog_string(catalog);
    atomic_write(path, toml.as_bytes())
}

/// Parse catalog text. A path is attached only for error reporting.
pub fn from_str(text: &str, path: &Path) -> Result<Catalog> {
    let doc: TomlCatalog =
        toml::from_str(text).map_err(|e| Error::parse(path.to_path_buf(), e.to_string()))?;
    let files = doc
        .files
        .into_iter()
        .map(|f| f.into_entry(path))
        .collect::<Result<Vec<_>>>()?;
    Ok(Catalog::new(
        doc.meta.root,
        doc.meta.scanned_at,
        doc.meta.host,
        files,
    ))
}

/// Serialize a catalog to TOML text.
pub fn to_catalog_string(catalog: &Catalog) -> String {
    let doc = TomlCatalog {
        meta: TomlMeta {
            root: catalog.root.clone(),
            scanned_at: catalog.scanned_at.clone(),
            host: catalog.host.clone(),
        },
        files: catalog.files().iter().map(TomlFile::from_entry).collect(),
    };
    toml::to_string(&doc).expect("catalog must serialize")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(format!(
        ".{}.tmp.{}",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("catalog"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes).map_err(Error::Io)?;
    std::fs::rename(&tmp, path).map_err(Error::Io)
}

#[cfg(test)]
mod toml_tests {
    use super::*;

    fn sample_catalog() -> Catalog {
        let e = FileEntry::new_restorable(
            "conf/init.el".into(),
            Some(Category::Config),
            "git -C ~/dotfiles checkout HEAD -- conf/init.el".into(),
            Source::Verified,
            2048,
            Some("2026-08-15T10:00:00Z".into()),
        );
        let o =
            FileEntry::new_orphaned("Pictures/photo.nef".into(), Some(Category::Image), 25, None);
        Catalog::new(
            "/home/user".into(),
            "2026-09-04T12:00:00Z".into(),
            "desktop".into(),
            vec![e, o],
        )
    }

    #[test]
    fn round_trips_metadata_and_files() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("catalog.toml");
        let c = sample_catalog();
        write(&c, &p).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn serialized_shape_matches_docs_vocabulary() {
        let text = to_catalog_string(&sample_catalog());
        assert!(text.contains("root = \"/home/user\""));
        assert!(text.contains("status = \"restorable\""));
        assert!(text.contains("source = \"verified\""));
        assert!(text.contains("restore_method ="));
        assert!(text.contains("category = \"config\""));
        assert!(text.contains("[[files]]"));
    }

    #[test]
    fn orphaned_entry_leaves_recipe_and_source_absent() {
        let c = sample_catalog();
        let text = to_catalog_string(&c);
        // The single orphaned entry (photo.nef) must not carry source or method.
        assert_eq!(text.matches("source = \"verified\"").count(), 1);
        assert_eq!(text.matches("restore_method =").count(), 1);
    }

    #[test]
    fn rejects_restorable_entry_without_a_method() {
        let text = r#"
[meta]
root = "/x"
scanned_at = "t"
host = "h"
[[files]]
path = "a"
status = "restorable"
size = 0
"#;
        let err = from_str(text, Path::new("t.toml")).unwrap_err();
        assert!(matches!(err, Error::Catalog(_) | Error::Parse { .. }));
    }

    #[test]
    fn parses_docs_example_schema() {
        // A replica of the `[[files]]` shapes from target-state.md, with every
        // nullable field exercised (category = null, missing recipe on a
        // temporary file), must deserialize.
        let hand = r#"
[meta]
root = "/home/user"
scanned_at = "2026-09-04T12:00:00Z"
host = "desktop"
[[files]]
path = "conf/emacs.d/init.el"
status = "restorable"
category = "config"
restore_method = "git -C ~/dotfiles checkout HEAD -- conf/emacs.d/init.el"
source = "verified"
size = 2048
modified = "2026-08-15T10:00:00Z"
[[files]]
path = "tmp/emacs-workfile-~"
status = "temporary"
size = 0
"#;
        let c = from_str(hand, Path::new("x.toml")).unwrap();
        assert_eq!(c.files().len(), 2);
        assert_eq!(c.files()[1].status, Status::Temporary);
        assert_eq!(c.files()[1].category, None);
    }
}
