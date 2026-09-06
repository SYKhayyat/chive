//! The application context: the store plus the concrete runner, and the two
//! persistence operations that every command passes through.
//!
//! Commands do not touch files directly; they load a [`Catalog`], transform it,
//! and save it via [`App::save_catalog`], which writes the TOML source of truth
//! *and* rebuilds the derived SQLite index in one step. That keeps decision D9
//! (TOML is the truth; SQLite is derived) enforced in a single place.

use std::path::Path;

use crate::catalog::Catalog;
use crate::catalog::db;
use crate::catalog::toml;
use crate::config::Config;
use crate::error::{Error, Result};
use crate::provenance::config::Table as AdapterTable;
use crate::provenance::package::PackageDetector;
use crate::recipes::Recipes;
use crate::runner::Real;
use crate::scan::Scanner;
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

    /// Load the current catalog. TOML source of truth first; fall back to the
    /// derived index (a machine that has scanned but not exported yet), else an
    /// explicit "nothing yet" error.
    pub fn load_catalog(&self) -> Result<Catalog> {
        let truth = self.store.default_catalog_file();
        if truth.exists() {
            return toml::read(&truth);
        }
        let db_file = self.store.db_file();
        if db_file.exists() {
            let conn = db::open(&db_file)?;
            return db::load(&conn);
        }
        Err(Error::MissingCatalog)
    }

    /// Persist a catalog: write the TOML truth, then rebuild the derived index.
    pub fn save_catalog(&self, catalog: &Catalog) -> Result<()> {
        self.store.ensure()?;
        let truth = self.store.default_catalog_file();
        toml::write(catalog, &truth)?;
        let mut conn = db::open(&self.store.db_file())?;
        db::replace(&mut conn, catalog)
    }

    /// Run a scan and return the catalog it produced (not yet persisted).
    pub fn scan(&self, root: &Path, extra_ignore: &[String]) -> Result<Catalog> {
        let config = Config::load(&self.store.config_file())?;
        let recipes = Recipes::load(&self.store.recipes_file())?;
        let scanner = Scanner::new(&self.runner, &self.package, &recipes, &config, extra_ignore);
        let files = scanner.scan(root)?;
        Ok(Catalog::new(
            root.to_string_lossy().into_owned(),
            now_iso(),
            hostname(),
            files,
        ))
    }
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
    use crate::model::{Source, Status};

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
            1,
            None,
        );
        let c = Catalog::new("/root".into(), "t".into(), "h".into(), vec![entry]);
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
        // the file is orphaned.
        assert_eq!(c.files()[0].status, Status::Orphaned);
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
