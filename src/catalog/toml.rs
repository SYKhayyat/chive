//! The TOML catalog — the versionable artifact.
//!
//! The TOML file is the source of truth (decision D9); SQLite is a derived
//! index rebuilt from it. Serde stays out of the public model: these DTO types
//! mirror the schema in `docs/spec/target-state.md` exactly, and
//! [`from_catalog`]/[`to_catalog`] are the only boundary the model crosses,
//! keeping the trim of optional fields in one place.
//!
//! The act log lives here rather than in a sidecar file, because the catalog is
//! what travels to a new machine: a judgement kept elsewhere would stay behind
//! with the old machine (D20).

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::act::{Act, ActLog};
use crate::catalog::Catalog;
use crate::error::{Error, Result};
use crate::model::{Category, FileEntry, Origin, Source, Verdict};

#[derive(Debug, Serialize, Deserialize)]
struct TomlCatalog {
    meta: TomlMeta,
    #[serde(default)]
    files: Vec<TomlFile>,
    #[serde(default)]
    acts: Vec<Act>,
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlMeta {
    root: String,
    scanned_at: String,
    host: String,
    /// The next owner-act sequence number. Written so a hand-edited catalog can
    /// append an act without inventing a number that already exists.
    #[serde(default)]
    next_seq: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlFile {
    path: String,
    verdict: Verdict,
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
    verdict_source: Origin,
    #[serde(default = "default_present")]
    present: bool,
    size: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    modified: Option<String>,
}

/// A file entry always describes a path the scan walked, so an absent `present`
/// key in a hand-written catalog means "present".
fn default_present() -> bool {
    true
}

impl TomlFile {
    fn from_entry(e: &FileEntry) -> Self {
        TomlFile {
            path: e.path.clone(),
            verdict: e.verdict,
            category: e.category,
            restore_method: e.restore_method.clone(),
            source: e.source,
            verdict_source: e.verdict_source,
            present: e.present,
            size: e.size,
            modified: e.modified.clone(),
        }
    }

    fn into_entry(self, file_index: usize, catalog: &Path) -> Result<FileEntry> {
        let at = format!("{}: files[{file_index}]", catalog.display());
        let name = self.path.clone();
        let valid = self.size >= 0 && self.verdict_is_consistent(&self.verdict);
        let entry = FileEntry {
            path: self.path,
            verdict: self.verdict,
            category: self.category,
            restore_method: self.restore_method,
            source: self.source,
            verdict_source: self.verdict_source,
            present: self.present,
            size: self.size,
            modified: self.modified,
        };
        if !valid {
            return Err(Error::Catalog(format!(
                "entry {name:?} ({at}) has an invalid size or verdict/recipe combination"
            )));
        }
        Ok(entry)
    }

    /// A `restorable` entry must carry a recipe; anything else must not, because
    /// a verdict that says "cannot rebuild this" beside a recipe is a
    /// contradiction the archive would then have to resolve silently.
    fn verdict_is_consistent(&self, verdict: &Verdict) -> bool {
        match verdict {
            Verdict::Restorable => self.restore_method.is_some(),
            Verdict::Unknown | Verdict::Disposable => self.restore_method.is_none(),
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
        .enumerate()
        .map(|(i, f)| f.into_entry(i, path))
        .collect::<Result<Vec<_>>>()?;
    Catalog::new(
        doc.meta.root,
        doc.meta.scanned_at,
        doc.meta.host,
        files,
        ActLog::new(doc.acts, doc.meta.next_seq),
    )
}

/// Serialize a catalog to TOML text.
pub fn to_catalog_string(catalog: &Catalog) -> String {
    let doc = TomlCatalog {
        meta: TomlMeta {
            root: catalog.root.clone(),
            scanned_at: catalog.scanned_at.clone(),
            host: catalog.host.clone(),
            next_seq: catalog.acts().next_seq(),
        },
        files: catalog.files().iter().map(TomlFile::from_entry).collect(),
        acts: catalog.acts().acts().to_vec(),
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
            Origin::Chive,
            2048,
            Some("2026-08-15T10:00:00Z".into()),
        );
        let u =
            FileEntry::new_unknown("Pictures/photo.nef".into(), Some(Category::Image), 25, None);
        let mut acts = ActLog::default();
        acts.append(Act::dispose(0, "Documents/notes.pdf"));
        Catalog::new(
            "/home/user".into(),
            "2026-09-04T12:00:00Z".into(),
            "desktop".into(),
            vec![e, u],
            acts,
        )
        .unwrap()
    }

    #[test]
    fn round_trips_metadata_files_and_acts() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("catalog.toml");
        let c = sample_catalog();
        write(&c, &p).unwrap();
        let back = read(&p).unwrap();
        assert_eq!(back, c, "the act log must survive the round trip");
        assert_eq!(back.acts().len(), 1);
        assert_eq!(back.acts().next_seq(), 1);
    }

    #[test]
    fn serialized_shape_matches_docs_vocabulary() {
        let text = to_catalog_string(&sample_catalog());
        assert!(text.contains("root = \"/home/user\""));
        assert!(text.contains("verdict = \"restorable\""));
        assert!(text.contains("verdict = \"unknown\""));
        assert!(text.contains("source = \"verified\""));
        assert!(text.contains("verdict_source = \"chive\""));
        assert!(text.contains("restore_method ="));
        assert!(text.contains("category = \"config\""));
        assert!(text.contains("[[files]]"));
        assert!(text.contains("[[acts]]"));
        assert!(text.contains("kind = \"dispose\""));
    }

    #[test]
    fn an_unknown_entry_leaves_the_recipe_absent() {
        let text = to_catalog_string(&sample_catalog());
        assert_eq!(text.matches("restore_method =").count(), 1);
    }

    #[test]
    fn rejects_a_restorable_entry_without_a_method() {
        let text = r#"
[meta]
root = "/x"
scanned_at = "t"
host = "h"
[[files]]
path = "a"
verdict = "restorable"
verdict_source = "owner"
size = 0
"#;
        assert!(from_str(text, Path::new("t.toml")).is_err());
    }

    #[test]
    fn rejects_a_recipe_beside_a_verdict_that_says_it_cannot_be_rebuilt() {
        let text = r#"
[meta]
root = "/x"
scanned_at = "t"
host = "h"
[[files]]
path = "a"
verdict = "disposable"
verdict_source = "owner"
restore_method = "echo hi"
size = 0
"#;
        assert!(from_str(text, Path::new("t.toml")).is_err());
    }

    #[test]
    fn refuses_the_retired_status_spellings() {
        // No legacy reader: an old-format catalog is an error, not a guess.
        for gone in ["temporary", "orphaned", "not-restorable"] {
            let text = format!(
                "[meta]\nroot = \"/x\"\nscanned_at = \"t\"\nhost = \"h\"\n\
                 [[files]]\npath = \"a\"\nstatus = \"{gone}\"\nsize = 0\n"
            );
            assert!(
                from_str(&text, Path::new("t.toml")).is_err(),
                "{gone} must not parse"
            );
        }
    }

    #[test]
    fn a_hand_edited_act_is_as_valid_as_a_written_one() {
        // The catalog is the interface, not a dump (D20).
        let text = r#"
[meta]
root = "/x"
scanned_at = "t"
host = "h"
next_seq = 2
[[files]]
path = "a"
verdict = "disposable"
verdict_source = "owner"
size = 0
[[acts]]
seq = 1
path = "a"
kind = "dispose"
"#;
        let c = from_str(text, Path::new("t.toml")).unwrap();
        assert_eq!(
            c.acts().latest("a").unwrap().kind,
            crate::act::ActKind::Dispose
        );
        assert_eq!(c.acts().next_seq(), 2);
    }

    #[test]
    fn a_teach_act_carries_its_recipe() {
        // Issue #45: the recipe lives in the act, so it reaches a new machine
        // through export with no join against a sidecar file.
        let text = r#"
[meta]
root = "/x"
scanned_at = "t"
host = "h"
[[acts]]
seq = 1
path = "future/x"
kind = "teach"
method = "echo built > '{dest}'"
"#;
        let c = from_str(text, Path::new("t.toml")).unwrap();
        assert_eq!(
            c.acts().active_recipes().get("future/x").copied(),
            Some("echo built > '{dest}'")
        );
    }

    #[test]
    fn an_escaping_act_path_is_refused_like_any_entry_path() {
        let text = r#"
[meta]
root = "/x"
scanned_at = "t"
host = "h"
[[acts]]
seq = 1
path = "../escape"
kind = "dispose"
"#;
        assert!(from_str(text, Path::new("t.toml")).is_err());
    }
}
