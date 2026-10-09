//! The application context: the store plus the concrete runner, and the two
//! persistence operations that every command passes through.
//!
//! Commands do not touch files directly; they load a [`Catalog`], transform it,
//! and save it via [`App::save_catalog`]. That is the whole persistence path —
//! the TOML *is* the store, so D9 ("the TOML is the truth") is enforced by there
//! being nothing else to disagree with it.

use std::path::{Path, PathBuf};

use crate::catalog::Catalog;
use crate::catalog::toml;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::provenance::config::Table as AdapterTable;
use crate::provenance::package::PackageDetector;
use crate::runner::Real;
use crate::scan::{self, Scanner};
use crate::store::Store;

/// Everything a command needs that is not its arguments.
pub struct App {
    pub store: Store,
    pub dry_run: bool,
    runner: Real,
    package: PackageDetector,
}

impl App {
    /// Build the app. The package-adapter table is loaded from the built-in
    /// set plus any user adapters under `<store>/adapters`.
    pub fn new(store: Store, dry_run: bool) -> Result<App> {
        let adapters_dir = store.dir.join("adapters");
        let package = PackageDetector::new(AdapterTable::load(&adapters_dir)?)?;
        let runner = Real { dry_run };
        Ok(App {
            store,
            dry_run,
            runner,
            package,
        })
    }

    pub fn runner(&self) -> &dyn crate::runner::Runner {
        &self.runner
    }

    pub fn package(&self) -> &PackageDetector {
        &self.package
    }

    /// Load the current catalog. The TOML file is the only truth (D9): the
    /// derived index is never read as a fallback, because a stale index would
    /// then impersonate a catalog the owner has deleted or replaced. No TOML
    /// means no catalog — scan or import first.
    pub fn load_catalog(&self) -> Result<Catalog> {
        let truth = self.store.default_catalog_file();
        if truth.exists() {
            return toml::read(&truth);
        }
        Err(Error::MissingCatalog)
    }

    /// Persist a catalog by writing the TOML truth. That is the whole operation.
    ///
    /// There used to be a second step: rebuild a SQLite index over the same rows.
    /// It had no reader outside its own tests — `db::load` and the version gate
    /// were called from nowhere else — so every save paid a wholesale table
    /// rebuild for a store nothing read, and D9's own rationale ("fast queries
    /// (status, restorable-set)") named queries that do not exist: both readers
    /// filter `catalog.files()` in memory. Deleted, along with `rusqlite` and the
    /// five hand-written `FromStr` impls on the domain model that existed only to
    /// feed it. Shall has no index at 142k lines either (issue #10).
    pub fn save_catalog(&self, catalog: &Catalog) -> Result<()> {
        self.store.ensure()?;
        toml::write(catalog, &self.store.default_catalog_file())
    }

    /// Re-decide one path against the current evidence, and return the catalog
    /// with that entry replaced. The owner act log is untouched — this refreshes
    /// the *view*, not the record.
    /// The single owner of "what does this path look like now".
    ///
    /// Every verb routes through here: `scan` when it walks the tree, and
    /// `teach`/`dispose`/`withdraw` when they record an act. They used to be four
    /// copies of the rule, and they had already drifted — `teach` hardcoded
    /// `present: false` on an absent path while `withdraw` went through the
    /// scanner and got `present: true`. Same rule, two answers (issue #56).
    pub fn rederive(&self, catalog: &Catalog, rel: &str) -> Result<Catalog> {
        let root = Path::new(&catalog.root);
        let act = catalog.acts().latest(rel).cloned();

        // A binding act answers on the owner's word alone, so it needs no
        // evidence. Skipping the scanner also skips the per-manager package
        // probes and the Rhai compile that constructing one would cost a
        // `teach` for nothing.
        let entry = match act.as_ref().filter(|a| a.kind.is_binding()) {
            Some(act) => scan::entry_from_act(rel, act, &root.join(rel)),
            None => {
                let config = Config::load(&self.store.config_file())?;
                let no_extra: &[String] = &[];
                let scanner = Scanner::new(
                    &self.runner,
                    &self.package,
                    root,
                    &config,
                    no_extra,
                    config.compile_rules()?,
                );
                scanner.rederive(root, rel, act.as_ref())?
            }
        };
        let mut next = catalog.clone();
        next.upsert(entry)?;
        Ok(next)
    }

    /// Run a scan and return the catalog it produced (not yet persisted).
    ///
    /// The owner act log is read from the existing catalog and re-applied over
    /// the fresh evidence, then carried onto the result. This is the whole of
    /// #43's fix: the scanner is downstream of the owner's decisions, so a
    /// routine rescan cannot erase one (D20). A scan against no existing catalog
    /// simply starts with an empty log.
    ///
    /// The root is resolved to its absolute, symlink-free spelling *here*, so
    /// nothing downstream ever holds what the owner typed (issue #34).
    pub fn scan(&self, root: &Path, extra_ignore: &[String]) -> Result<Catalog> {
        let root = canonical_scan_root(root)?;
        let existing = self.load_catalog().ok();
        let acts = existing
            .as_ref()
            .map(|c| c.acts().clone())
            .unwrap_or_default();
        if let Some(c) = &existing {
            let recorded = Path::new(&c.root);
            // Compare canonicalized forms: the recorded root is already
            // canonical, but an older catalog may hold a spelling that is the
            // same directory by another name.
            if recorded != root.as_path()
                && std::fs::canonicalize(recorded).unwrap_or_else(|_| recorded.to_path_buf())
                    != root.as_path()
            {
                return Err(Error::Refused(format!(
                    "this catalog describes {}; scanning {} would replace it and \
                     every recipe it holds. Use a separate store (`--config-dir \
                     <dir>`) for a second tree",
                    c.root, root.display()
                )));
            }
        }
        let config = Config::load(&self.store.config_file())?;
        // The store, resolved, so chive never catalogs its own catalog. The
        // default store is inside the home it scans, so the walk would otherwise
        // record catalog.toml/config.toml/adapters as files of the machine and
        // `clean` would delete the archive it was reading (issue #33). Resolved
        // rather than matched by basename: a directory the *owner* named chive
        // is still theirs to catalog.
        let skip_dirs = [std::fs::canonicalize(&self.store.dir)
            .unwrap_or_else(|_| self.store.dir.clone())];
        let scanner = Scanner::new(
            &self.runner,
            &self.package,
            &root,
            &config,
            extra_ignore,
            config.compile_rules()?,
        )
        .with_skip_dirs(&skip_dirs);
        let files = scanner.scan(&root, &acts)?;
        Catalog::new(
            root.to_string_lossy().into_owned(),
            now_iso(),
            hostname(),
            files,
            acts,
        )
    }
}

/// The absolute, symlink-free scan root.
///
/// A relative or symlinked root is resolved once, at the only moment the real
/// filesystem path is known. Recording what the owner typed meant every later
/// command resolved `.` against whatever cwd it happened to run from, so
/// `clean` from another directory reported removals that never happened and
/// silently dropped live entries (issue #34).
///
/// `canonicalize` fails only when the path does not exist — which is the case
/// that must never scan as an empty catalog and overwrite a real one
/// (issue #32). The refusal names that consequence rather than just "not found",
/// because the two halves of #32 have one shared cause.
fn canonical_scan_root(root: &Path) -> Result<PathBuf> {
    std::fs::canonicalize(root).map_err(|e| {
        Error::Refused(format!(
            "cannot scan {}: {e}; an unresolvable root would scan as an empty \
             catalog and overwrite the existing one, and that catalog is the archive",
            root.display()
        ))
    })
}

/// Returns an RFC 3339 / ISO 8601 UTC timestamp for the catalog meta.
fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Best-effort hostname; never fails.
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use crate::act::{Act, ActLog};
    use crate::model::{Origin, Source, Verdict};

    fn store() -> Store {
        let dir = std::env::temp_dir().join(format!(
            "chive-app-test-{}-{}",
            std::process::id(),
            rand_suffix()
        ));
        Store::at(dir)
    }

    /// A short unique suffix so parallel test threads get distinct stores.
    fn rand_suffix() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        N.fetch_add(1, Ordering::SeqCst)
    }

    #[test]
    fn save_then_load_round_trips_through_toml_truth() {
        let app = App::new(store(), false).unwrap();
        let entry = crate::model::FileEntry::new_restorable(
            "a.txt".into(),
            None,
            "echo {dest}".into(),
            Source::Verified,
            Origin::Chive,
            1,
            None,
        );
        let c = Catalog::new(
            "/root".into(),
            "t".into(),
            "h".into(),
            vec![entry],
            ActLog::default(),
        )
        .unwrap();
        app.save_catalog(&c).unwrap();
        let back = app.load_catalog().unwrap();
        assert_eq!(back, c);
    }

    #[test]
    fn load_with_nothing_anywhere_is_missing_catalog() {
        let app = App::new(store(), false).unwrap();
        // a fresh store has neither TOML nor DB
        let err = app.load_catalog();
        assert!(matches!(err, Err(Error::MissingCatalog)));
    }

    #[test]
    fn scan_produces_catalog_metadata_and_entries() {
        let app = App::new(store(), true).unwrap(); // dry_run: no subprocesses
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.md"), "x").unwrap();
        let c = app.scan(dir.path(), &[]).unwrap();
        assert_eq!(c.root, dir.path().to_string_lossy());
        assert!(!c.scanned_at.is_empty());
        assert_eq!(c.files().len(), 1);
        assert_eq!(c.files()[0].path, "notes.md");
        // No package manager is present in dry-run and there is no .git, so
        // the file is a hole: it matters, and chive cannot rebuild it.
        assert_eq!(c.files()[0].verdict, Verdict::Unknown);
    }

    #[test]
    fn a_scan_re_applies_the_owner_act_log_it_read() {
        // Issue #43's chain, end to end: dispose a path, then rescan. The
        // verdict must survive, because the scanner reads the log rather than
        // replacing it.
        let app = App::new(store(), true).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("junk.nef"), "x").unwrap();

        let first = app.scan(dir.path(), &[]).unwrap();
        let mut first = first;
        first.record(Act::dispose(0, "junk.nef")).unwrap();
        app.save_catalog(&first).unwrap();

        let second = app.scan(dir.path(), &[]).unwrap();
        let e = second.by_path("junk.nef").expect("the path is still there");
        assert_eq!(
            e.verdict,
            Verdict::Disposable,
            "a rescan must not erase the owner's decision"
        );
        assert_eq!(e.verdict_source, Origin::Owner);
    }

    #[test]
    fn user_adapter_dir_is_loaded() {
        let s = store();
        let adapters = s.dir.join("adapters");
        std::fs::create_dir_all(&adapters).unwrap();
        std::fs::write(
            adapters.join("user.toml"),
            "[[manager]]\nname = \"carv\"\nos = [\"any\"]\nrestore = \"carv i {pkg}\"\n",
        )
        .unwrap();
        let app = App::new(s, true).unwrap();
        // 'any' OS managers survive; carv must be loaded after built-ins.
        assert!(app.package().manager_names().contains(&"carv"));
    }
}
