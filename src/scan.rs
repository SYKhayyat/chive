//! The scanner: walk the tree and decide what every file is.
//!
//! This is where statuses are assigned. The decision per file is mechanical and
//! deterministic (see `docs/spec/target-state.md`). One order governs the
//! whole scan, and it matters:
//!
//! 1. **ignored dir** — not cataloged at all.
//! 2. **taught recipe** — the owner explicitly wrote one, so it wins over every
//!    automatic answer, including an inferred recipe and even a temporary
//!    bloom. This is how `teach` overrules inference.
//! 3. **temporary heuristic** — transient suffix/extension → `temporary`.
//! 4. **provenance chain** — package → git → symlink → `restorable` (verified).
//! 5. otherwise **orphaned**.
//!
//! Provenance runs lazily per file (each likely candidate is probed once and
//! both package and git are only reached when the earlier sources miss), so a
//! scan is no heavier than the provenance it actually needs.

use std::path::Path;

use walkdir::{DirEntry, WalkDir};

use crate::config::Config;
use crate::error::Result;
use crate::model::{Category, FileEntry, Source, Status};
use crate::provenance::git::GitDetector;
use crate::provenance::package::PackageDetector;
use crate::provenance::symlink::SymlinkDetector;
use crate::provenance::{Detector, Recipe};
use crate::recipes::Recipes;
use crate::runner::Runner;

/// The provenance chain, resolved once and reused for every file.
pub struct Provenance<'a> {
    runner: &'a dyn Runner,
    package: &'a PackageDetector,
    git: GitDetector<'a>,
    symlink: SymlinkDetector,
}

impl<'a> Provenance<'a> {
    pub fn new(runner: &'a dyn Runner, package: &'a PackageDetector) -> Self {
        Provenance {
            runner,
            package,
            git: GitDetector::new(runner),
            symlink: SymlinkDetector,
        }
    }

    /// Order: package → git → symlink, first match wins. The order is the
    /// documented, load-bearing contract: package recipes are the most
    /// portable across machines, git recipes second (they need the repo), and
    /// a plain `ln -s` last. Symlink is still the cheapest *probe*, so the
    /// per-file cost only pays for the deeper sources when the file actually
    /// is a link — the doc-comment speed argument lives in the probe cost,
    /// not the precedence.
    fn detect(&self, abs: &Path) -> Option<Recipe> {
        self.package
            .detect(self.runner, abs)
            .or_else(|| self.git.detect(abs))
            .or_else(|| self.symlink.detect(abs))
    }
}

/// Scans a root into a set of catalog entries.
pub struct Scanner<'a> {
    provenance: Provenance<'a>,
    recipes: &'a Recipes,
    config: &'a Config,
    /// Extra ignore patterns from the CLI, beyond the config file.
    extra_ignore: &'a [String],
}

impl<'a> Scanner<'a> {
    pub fn new(
        runner: &'a dyn Runner,
        package: &'a PackageDetector,
        recipes: &'a Recipes,
        config: &'a Config,
        extra_ignore: &'a [String],
    ) -> Self {
        Scanner {
            provenance: Provenance::new(runner, package),
            recipes,
            config,
            extra_ignore,
        }
    }

    /// Walk `root` and produce one entry per file.
    pub fn scan(&self, root: &Path) -> Result<Vec<FileEntry>> {
        let root_abs = root.to_path_buf();
        let mut out = Vec::new();
        let walker = WalkDir::new(&root_abs)
            .follow_links(false)
            .into_iter()
            .filter_entry(|e| self.include(e));
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(_) => continue, // unreadable entry: skip, never fatal
            };
            if !entry.file_type().is_file() && !entry.file_type().is_symlink() {
                continue; // directories and special files are not cataloged
            }
            let rel = rel(root, entry.path());
            if let Some(e) = self.entry_for(&root_abs, &rel) {
                out.push(e);
            }
        }
        Ok(out)
    }

    /// Decide what a single file becomes, given its relative path.
    fn entry_for(&self, root: &Path, rel: &str) -> Option<FileEntry> {
        let abs = root.join(rel);
        let meta = std::fs::symlink_metadata(&abs).ok()?;
        let size = meta.len() as i64;
        let modified = meta.modified().ok().map(iso_timestamp).unwrap_or_default();
        let name = rel.rsplit('/').next().unwrap_or(rel).to_string();

        // 2. A taught recipe is the owner's explicit word; it overrules every
        //    automatic classification for this path.
        if let Some(method) = self.recipes.get(rel) {
            return Some(FileEntry::new_restorable(
                rel.to_string(),
                classify(rel),
                method.to_string(),
                Source::UserSupplied,
                size,
                modified,
            ));
        }

        // 3. Transient file.
        if is_temporary(&name) {
            return Some(FileEntry {
                path: rel.to_string(),
                status: Status::Temporary,
                category: None,
                restore_method: None,
                source: None,
                not_restorable_reason: None,
                size,
                modified,
            });
        }

        // 4. Provenance: becomes restorable if any source explains the file.
        if let Some(Recipe {
            restore_method,
            source,
            category,
        }) = self.provenance.detect(&abs)
        {
            return Some(FileEntry::new_restorable(
                rel.to_string(),
                category.or_else(|| classify(rel)),
                restore_method,
                source,
                size,
                modified,
            ));
        }

        // 5. Nothing explains it.
        Some(FileEntry::new_orphaned(
            rel.to_string(),
            classify(rel),
            size,
            modified,
        ))
    }

    /// Whether a walk entry should descend/be visited (filters ignored dirs).
    fn include(&self, entry: &DirEntry) -> bool {
        // Symlinks are cataloged as files but never followed into.
        if entry.file_type().is_symlink() {
            return true;
        }
        let name = entry.file_name().to_string_lossy();
        // Skip directories whose basename is in the ignore list.
        if entry.file_type().is_dir()
            && (self.config.is_ignored(&name) || self.extra_ignore.iter().any(|i| *i == name))
        {
            return false;
        }
        true
    }
}

/// A file's path relative to `root`, using forward slashes so catalog paths are
/// portable to any target machine.
fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Whether a filename looks transient. Conservative: only unambiguous editor
/// and cache artifacts, never `.bak`, `.log`, or source files.
fn is_temporary(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with('~')
        || (lower.starts_with('#') && lower.ends_with('#'))
        || matches!(
            extension(lower.as_str()),
            Some("tmp") | Some("swp") | Some("swo") | Some("pyc") | Some("cache")
        )
}

fn extension(name: &str) -> Option<&str> {
    name.rsplit('.').next().filter(|e| !e.is_empty())
}

/// Classify a relative path into its category (extension-based).
fn classify(path: &str) -> Option<Category> {
    crate::model::category::classify(path)
}

fn iso_timestamp(t: std::time::SystemTime) -> Option<String> {
    let dt: chrono::DateTime<chrono::Utc> = t.into();
    Some(dt.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

#[cfg(test)]
mod scan_tests {
    use super::*;
    use crate::runner::Mock;

    fn build_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/config"), "").unwrap();
        dir
    }

    /// Owned pieces each test calls `Scanner::new` with.
    fn parts() -> (Config, PackageDetector, Recipes) {
        let cfg = Config::default();
        let pkg =
            PackageDetector::new(crate::provenance::config::Table::builtin().unwrap()).unwrap();
        (cfg, pkg, Recipes::default())
    }

    #[test]
    fn ignored_dir_is_not_cataloged() {
        let dir = build_dir();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules/junk.js"), "x").unwrap();
        std::fs::write(dir.path().join("real.txt"), "x").unwrap();
        let mock = Mock::default();
        let (cfg, pkg, recipes) = parts();
        let s = Scanner::new(&mock, &pkg, &recipes, &cfg, &[]);
        let files = s.scan(dir.path()).unwrap();
        let paths: Vec<_> = files.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"real.txt"));
        assert!(
            !paths.iter().any(|p| p.contains("node_modules")),
            "node_modules must be skipped"
        );
        assert!(
            !paths.iter().any(|p| p.contains(".git")),
            "git dir must be skipped"
        );
    }

    #[test]
    fn temp_named_file_is_temporary() {
        let dir = build_dir();
        std::fs::write(dir.path().join("wip~"), "").unwrap();
        std::fs::write(dir.path().join("#auto#"), "").unwrap();
        let mock = Mock::default();
        let (cfg, pkg, recipes) = parts();
        let s = Scanner::new(&mock, &pkg, &recipes, &cfg, &[]);
        let files = s.scan(dir.path()).unwrap();
        let temp: Vec<_> = files
            .iter()
            .filter(|e| e.status == Status::Temporary)
            .collect();
        assert_eq!(temp.len(), 2);
    }

    #[test]
    fn taught_recipe_overrules_inference() {
        let dir = build_dir();
        std::fs::write(dir.path().join("conf.txt"), "").unwrap();
        let mut recipes = Recipes::default();
        recipes
            .teach(
                "conf.txt",
                "cp ~/seed/conf.txt '{dest}'",
                &dir.path().join("recipes.toml"),
            )
            .unwrap();
        let mock = Mock::default(); // no provenance programs
        let (cfg, pkg2, _rec) = parts();
        let s = Scanner::new(&mock, &pkg2, &recipes, &cfg, &[]);
        let files = s.scan(dir.path()).unwrap();
        let e = files.iter().find(|e| e.path == "conf.txt").unwrap();
        assert_eq!(e.status, Status::Restorable);
        assert_eq!(e.source, Some(Source::UserSupplied));
        assert_eq!(
            e.restore_method.as_deref(),
            Some("cp ~/seed/conf.txt '{dest}'")
        );
    }

    #[test]
    fn ordinary_unclassified_file_is_orphaned() {
        // A plain tree with *no* .git so nothing is git-tracked.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("photo.nef"), "").unwrap();
        let mock = Mock::default();
        let (cfg, pkg, recipes) = parts();
        let s = Scanner::new(&mock, &pkg, &recipes, &cfg, &[]);
        let files = s.scan(dir.path()).unwrap();
        let e = files.iter().find(|e| e.path == "photo.nef").unwrap();
        assert_eq!(e.status, Status::Orphaned);
        assert_eq!(e.category, Some(Category::Image));
    }

    #[test]
    fn symlink_becomes_restorable_verified() {
        #[cfg(unix)]
        {
            // A plain tree: no .git (git would outrank the link under the
            // documented order) and no package managers on the mock.
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("target.txt"), "x").unwrap();
            std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt"))
                .unwrap();
            let mock = Mock::default();
            let (cfg, pkg2, recipes) = parts();
            let s = Scanner::new(&mock, &pkg2, &recipes, &cfg, &[]);
            let files = s.scan(dir.path()).unwrap();
            let e = files.iter().find(|e| e.path == "link.txt").unwrap();
            assert_eq!(e.status, Status::Restorable);
            assert_eq!(e.source, Some(Source::Verified));
            assert!(e.restore_method.as_deref().unwrap().starts_with("ln -s"));
        }
    }

    #[test]
    fn provenance_order_is_package_then_git_then_symlink() {
        #[cfg(unix)]
        {
            // A link inside a mock-tracked repo: the git recipe must win over
            // ln -s even though the symlink probe is cheaper.
            let dir = build_dir();
            std::fs::write(dir.path().join("target.txt"), "x").unwrap();
            std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt"))
                .unwrap();
            let mut mock = Mock::default();
            mock.on_argv(|program, args| {
                if program == "git" && args.contains(&"ls-files") {
                    Some(Mock::ok(""))
                } else {
                    None
                }
            });
            let (cfg, pkg, recipes) = parts();
            let s = Scanner::new(&mock, &pkg, &recipes, &cfg, &[]);
            let files = s.scan(dir.path()).unwrap();
            let e = files.iter().find(|e| e.path == "link.txt").unwrap();
            assert_eq!(e.status, Status::Restorable);
            assert!(
                e.restore_method
                    .as_deref()
                    .unwrap()
                    .contains("checkout HEAD"),
                "git outranks symlink under the documented order, got: {:?}",
                e.restore_method
            );
        }
    }

    #[test]
    fn extra_ignore_from_cli_prunes_dir() {
        let dir = build_dir();
        std::fs::create_dir(dir.path().join("vendor")).unwrap();
        std::fs::write(dir.path().join("vendor/lib.rs"), "").unwrap();
        let mock = Mock::default();
        let (cfg, pkg, recipes) = parts();
        let ignore = ["vendor".to_string()];
        let s = Scanner::new(&mock, &pkg, &recipes, &cfg, &ignore);
        let files = s.scan(dir.path()).unwrap();
        assert!(!files.iter().any(|e| e.path.contains("vendor")));
    }
}
