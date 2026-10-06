//! The scanner: walk the tree and decide what every file is.
//!
//! This is where verdicts are assigned, and the order is the whole contract
//! (`docs/spec/target-state.md`):
//!
//! 1. **ignored dir** — not cataloged at all.
//! 2. **owner act** — the newest act for this path governs. Teach makes it
//!    `restorable`, dispose makes it `disposable`, withdraw releases it to
//!    re-derive. A rescan *reads* this and re-applies it, so it can never
//!    erase or reorder a decision (D20, issue #43).
//! 3. **owner rule** — the owner's own policy, if a `[[rules]]` script has an
//!    opinion.
//! 4. **provenance chain** — package → git → symlink → `restorable` (verified).
//! 5. **provable-dead** — a symlink whose target no longer resolves, or a
//!    removed package's residue → `disposable`. The only automatic source of a
//!    cleanable verdict (D19).
//! 6. otherwise **`unknown`** — a hole. Never cleanable.
//!
//! Provenance runs lazily per file (each likely candidate is probed once and
//! both package and git are only reached when the earlier sources miss), so a
//! scan is no heavier than the provenance it actually needs.

use std::path::Path;

use walkdir::{DirEntry, WalkDir};

use crate::act::{ActKind, ActLog};
use crate::config::Config;
use crate::error::Result;
use crate::model::{Category, FileEntry, Origin, Source, Verdict};
use crate::provenance::dead;
use crate::provenance::git::GitDetector;
use crate::provenance::package::PackageDetector;
use crate::provenance::symlink::SymlinkDetector;
use crate::provenance::{Detector, Recipe};
use crate::rules::{self, Rules};
use crate::runner::Runner;

/// The provenance chain, resolved once per scan and reused for every file.
/// `available_managers` is the hoisted per-machine answer to "which package
/// managers are installed?" — asking per file would spawn a `--version`
/// subprocess per manager per file (issue #20).
pub struct Provenance<'a> {
    runner: &'a dyn Runner,
    package: &'a PackageDetector,
    available_managers: Vec<String>,
    git: GitDetector<'a>,
    symlink: SymlinkDetector,
}

impl<'a> Provenance<'a> {
    pub fn new(runner: &'a dyn Runner, package: &'a PackageDetector, scan_root: &'a Path) -> Self {
        let available_managers = package
            .available(runner)
            .into_iter()
            .map(String::from)
            .collect();
        Provenance {
            runner,
            package,
            available_managers,
            git: GitDetector::new(runner, Some(scan_root)),
            symlink: SymlinkDetector,
        }
    }

    /// The package recipe for this file, or `None`. Run **once** per file and
    /// threaded forward to both consumers.
    ///
    /// It used to be computed twice: once for the rule's `package` fact and once
    /// for the provenance chain, each calling `PackageDetector::detect` with
    /// identical arguments. With zero rules configured -- the default -- the
    /// whole adapter fan-out therefore ran twice per file, for every file in the
    /// tree including every file under `$HOME` (issue #50).
    fn package_recipe(&self, abs: &Path) -> Option<Recipe> {
        self.package
            .detect(self.runner, &self.available_managers, abs)
    }

    /// git, then symlink. The package rung is supplied by the caller because it
    /// was already probed for the rule facts.
    fn git_then_symlink(&self, abs: &Path) -> Option<Recipe> {
        self.git.detect(abs).or_else(|| self.symlink.detect(abs))
    }
}

/// Scans a root into a set of catalog entries.
pub struct Scanner<'a> {
    provenance: Provenance<'a>,
    config: &'a Config,
    /// Extra ignore patterns from the CLI, beyond the config file.
    extra_ignore: &'a [String],
    rules: Rules,
}

impl<'a> Scanner<'a> {
    pub fn new(
        runner: &'a dyn Runner,
        package: &'a PackageDetector,
        scan_root: &'a Path,
        config: &'a Config,
        extra_ignore: &'a [String],
        rules: Rules,
    ) -> Self {
        Scanner {
            provenance: Provenance::new(runner, package, scan_root),
            config,
            extra_ignore,
            rules,
        }
    }

    /// Walk `root` and produce one entry per path, with the owner act log
    /// re-applied over the fresh evidence.
    pub fn scan(&self, root: &Path, acts: &ActLog) -> Result<Vec<FileEntry>> {
        let root_abs = root.to_path_buf();
        let governing = acts.governing();
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
            out.push(self.entry_for(&root_abs, &rel, governing.get(rel.as_str()).copied())?);
        }

        // A taught recipe for a path that is not on this machine is still worth
        // recording — planning a new machine is the core workflow, and dropping
        // it here is what made #45 lose recipes at export (issue #45).
        for (path, act) in &governing {
            if act.kind != ActKind::Teach {
                continue;
            }
            if out.iter().any(|e| e.path == *path) {
                continue;
            }
            let Some(method) = &act.method else { continue };
            out.push(FileEntry::new_absent_restorable(
                (*path).to_string(),
                classify(path),
                method.clone(),
            ));
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    /// Re-examine one path and return the verdict chive can justify now, with
    /// the owner act for that path re-applied.
    ///
    /// Used by `withdraw`: taking back a judgement means "you decide, chive
    /// re-examines", and the evidence a previous `dispose` cleared has to be
    /// regenerated rather than left stale until the next full scan.
    pub fn rederive(
        &self,
        root: &Path,
        rel: &str,
        act: Option<&crate::act::Act>,
    ) -> Result<FileEntry> {
        self.entry_for(root, rel, act)
    }

    /// Decide what a single file becomes, given its relative path and the owner
    /// act that governs it (if any).
    fn entry_for(
        &self,
        root: &Path,
        rel: &str,
        act: Option<&crate::act::Act>,
    ) -> Result<FileEntry> {
        let abs = root.join(rel);
        let meta = std::fs::symlink_metadata(&abs).ok();
        let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
        let modified = meta
            .and_then(|m| m.modified().ok())
            .map(iso_timestamp)
            .unwrap_or_default();
        let category = classify(rel);

        // 2. The owner's newest act governs, and it beats every automatic answer
        //    below. A rescan re-applies the log rather than replacing it.
        match act.map(|a| a.kind) {
            Some(ActKind::Teach) => {
                let method = act.and_then(|a| a.method.clone()).unwrap_or_default();
                return Ok(FileEntry::new_restorable(
                    rel.to_string(),
                    category,
                    method,
                    Source::UserSupplied,
                    Origin::Owner,
                    size,
                    modified,
                ));
            }
            Some(ActKind::Dispose) => {
                return Ok(FileEntry::new_disposable(
                    rel.to_string(),
                    category,
                    Origin::Owner,
                    size,
                    modified,
                ));
            }
            // A withdraw releases the path: fall through and re-derive.
            Some(ActKind::Withdraw) | None => {}
        }

        // One probe, two readers. The package rung of the provenance chain and the
        // rule's `package` fact are the same question, asked once.
        let package_recipe = self.provenance.package_recipe(&abs);

        // 3. The owner's own policy, if a rule has an opinion about this path.
        let facts = rules::facts_for(
            &abs,
            rel,
            size,
            package_recipe.as_ref().and_then(|r| r.package.as_deref()),
        );
        if let Some((verdict, _label)) = self.rules.evaluate(&facts)? {
            // The origin is the RULE's on every branch. Routing `unknown` through
            // `new_unknown` stamped it `Origin::Chive`, which made a rule-authored
            // verdict non-sticky and contradicted D22 -- and the inconsistency
            // with `new_disposable`, which takes the origin as a parameter, is
            // what made it a bug rather than a choice.
            return Ok(match verdict {
                Verdict::Disposable => FileEntry::new_disposable(
                    rel.to_string(),
                    category,
                    Origin::Rule,
                    size,
                    modified,
                ),
                // A rule claiming restorable must supply a recipe to be useful,
                // and chive has none; the honest reading of "I know how to
                // rebuild this" without a recipe is a hole. Still the rule's
                // claim about it, so still rule-origin.
                Verdict::Restorable | Verdict::Unknown => {
                    FileEntry::new_hole(rel.to_string(), category, Origin::Rule, size, modified)
                }
            });
        }

        // 4. Provenance: becomes restorable if any source explains the file.
        if let Some(Recipe {
            restore_method,
            source,
            category: found,
            ..
        }) = package_recipe.or_else(|| self.provenance.git_then_symlink(&abs))
        {
            return Ok(FileEntry::new_restorable(
                rel.to_string(),
                found.or(category),
                restore_method,
                source,
                Origin::Chive,
                size,
                modified,
            ));
        }

        // 5. Provably dead: the only automatic source of `disposable`.
        if let Some((verdict, origin, _why)) = dead::detect(&abs) {
            debug_assert_eq!(verdict, Verdict::Disposable);
            return Ok(FileEntry::new_disposable(
                rel.to_string(),
                category,
                origin,
                size,
                modified,
            ));
        }

        // 6. Nothing explains it. It matters, and chive cannot rebuild it.
        Ok(FileEntry::new_unknown(
            rel.to_string(),
            category,
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
    use crate::act::{Act, ActLog};
    use crate::rules::Rule;
    use crate::runner::Mock;

    fn build_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join(".git/config"), "").unwrap();
        dir
    }

    /// Owned pieces each test calls `Scanner::new` with.
    fn parts() -> (Config, PackageDetector, Rules) {
        (
            Config::default(),
            PackageDetector::new(crate::provenance::config::Table::builtin().unwrap()).unwrap(),
            Rules::compile(&[]).unwrap(),
        )
    }

    fn no_acts() -> ActLog {
        ActLog::default()
    }

    #[test]
    fn ignored_dir_is_not_cataloged() {
        let dir = build_dir();
        std::fs::create_dir(dir.path().join("node_modules")).unwrap();
        std::fs::write(dir.path().join("node_modules/junk.js"), "x").unwrap();
        std::fs::write(dir.path().join("real.txt"), "x").unwrap();
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &no_acts()).unwrap();
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
    fn an_unexplained_file_is_a_hole_and_never_cleanable() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("photo.nef"), "").unwrap();
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &no_acts()).unwrap();
        let e = files.iter().find(|e| e.path == "photo.nef").unwrap();
        assert_eq!(e.verdict, Verdict::Unknown);
        assert!(!e.verdict.is_cleanable());
        assert_eq!(e.category, Some(Category::Image));
    }

    #[test]
    fn the_retired_temporary_heuristic_no_longer_guesses() {
        // D19: a name that used to mean "safe to clean" is now just a hole. The
        // owner states that policy as a rule instead.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("wip~"), "").unwrap();
        std::fs::write(dir.path().join("#auto#"), "").unwrap();
        std::fs::write(dir.path().join("scratch.tmp"), "").unwrap();
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &no_acts()).unwrap();
        for e in &files {
            assert_eq!(
                e.verdict,
                Verdict::Unknown,
                "{} must not be guessed disposable",
                e.path
            );
        }
    }

    #[test]
    fn a_rule_authored_verdict_keeps_rule_origin_and_its_stickiness() {
        // Issue #54: a rule returning `unknown` used to be stamped
        // `Origin::Chive`, which made it non-sticky and contradicted D22. D22's
        // table says rule verdicts are owner policy and are not second-guessed.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("thing.conf"), "x").unwrap();
        let rules = Rules::compile(&[Rule {
            name: "hold".into(),
            script: r#""unknown""#.into(),
        }])
        .unwrap();
        let mock = Mock::default();
        let (cfg, pkg, _) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);

        for _ in 0..2 {
            let files = s.scan(dir.path(), &no_acts()).unwrap();
            let e = files.iter().find(|e| e.path == "thing.conf").unwrap();
            assert_eq!(e.verdict, Verdict::Unknown);
            assert_eq!(
                e.verdict_source,
                Origin::Rule,
                "the rule decided this, so it must be recorded as the decider"
            );
            assert!(
                e.verdict_source.is_sticky(),
                "D22: rule verdicts are owner policy and survive a rescan"
            );
        }
    }

    #[test]
    fn a_rule_replaces_the_heuristic_it_retired() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pkg.apk"), "").unwrap();
        std::fs::write(dir.path().join("notes.md"), "").unwrap();
        let rules = Rules::compile(&[Rule {
            name: "apk".into(),
            script: r#"if path.ends_with(".apk") { "disposable" } else { () }"#.into(),
        }])
        .unwrap();
        let mock = Mock::default();
        let (cfg, pkg, _) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &no_acts()).unwrap();
        let apk = files.iter().find(|e| e.path == "pkg.apk").unwrap();
        assert_eq!(apk.verdict, Verdict::Disposable);
        assert_eq!(apk.verdict_source, Origin::Rule);
        let md = files.iter().find(|e| e.path == "notes.md").unwrap();
        assert_eq!(md.verdict, Verdict::Unknown);
    }

    #[test]
    fn an_owner_teach_overrules_inference_and_survives_the_scan() {
        let dir = build_dir();
        std::fs::write(dir.path().join("conf.txt"), "").unwrap();
        let mut acts = ActLog::default();
        acts.append(Act::teach(0, "conf.txt", "cp ~/seed/conf.txt '{dest}'"));
        let mock = Mock::default(); // no provenance programs
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &acts).unwrap();
        let e = files.iter().find(|e| e.path == "conf.txt").unwrap();
        assert_eq!(e.verdict, Verdict::Restorable);
        assert_eq!(e.source, Some(Source::UserSupplied));
        assert_eq!(e.verdict_source, Origin::Owner);
        assert_eq!(
            e.restore_method.as_deref(),
            Some("cp ~/seed/conf.txt '{dest}'")
        );
    }

    #[test]
    fn a_dispose_overrules_a_older_teach_for_the_same_path() {
        // Issue #43's chain, in its pure form: the log is the only place order
        // is recorded, and the newest act must win.
        let dir = build_dir();
        std::fs::write(dir.path().join("conf.txt"), "").unwrap();
        let mut acts = ActLog::default();
        acts.append(Act::teach(0, "conf.txt", "echo one"));
        acts.append(Act::dispose(0, "conf.txt"));
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &acts).unwrap();
        let e = files.iter().find(|e| e.path == "conf.txt").unwrap();
        assert_eq!(e.verdict, Verdict::Disposable);
        assert_eq!(e.verdict_source, Origin::Owner);
        assert_eq!(e.restore_method, None);
    }

    #[test]
    fn a_withdraw_releases_the_path_to_re_derive() {
        // A plain tree: build_dir() plants a .git, and then git provenance would
        // (correctly) make the path restorable regardless of the withdrawal.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("conf.txt"), "").unwrap();
        let mut acts = ActLog::default();
        acts.append(Act::dispose(0, "conf.txt"));
        acts.append(Act::withdraw(0, "conf.txt"));
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &acts).unwrap();
        let e = files.iter().find(|e| e.path == "conf.txt").unwrap();
        assert_eq!(
            e.verdict,
            Verdict::Unknown,
            "withdrawn: no owner verdict, and nothing here is restorable"
        );
        assert_eq!(e.verdict_source, Origin::Chive);
    }

    #[test]
    fn a_taught_recipe_for_an_absent_path_is_still_recorded() {
        // Issue #45: planning a new machine is the core workflow, so the entry
        // must survive with present = false rather than being dropped.
        let dir = build_dir();
        std::fs::write(dir.path().join("here.txt"), "").unwrap();
        let mut acts = ActLog::default();
        acts.append(Act::teach(0, "not/here/yet.conf", "echo built"));
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &acts).unwrap();
        let e = files
            .iter()
            .find(|e| e.path == "not/here/yet.conf")
            .expect("an absent taught path must be kept");
        assert!(e.is_restorable());
        assert!(!e.present);
        assert_eq!(e.restore_method.as_deref(), Some("echo built"));
    }

    #[test]
    fn a_disposed_absent_path_is_not_invented() {
        let dir = build_dir();
        let mut acts = ActLog::default();
        acts.append(Act::dispose(0, "not/here.nef"));
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &acts).unwrap();
        assert!(
            files.iter().all(|e| e.path != "not/here.nef"),
            "there is nothing to clean, so there is nothing to record"
        );
    }

    #[test]
    fn symlink_becomes_restorable_verified() {
        #[cfg(unix)]
        {
            let dir = tempfile::tempdir().unwrap();
            std::fs::write(dir.path().join("target.txt"), "x").unwrap();
            std::os::unix::fs::symlink(dir.path().join("target.txt"), dir.path().join("link.txt"))
                .unwrap();
            let mock = Mock::default();
            let (cfg, pkg2, rules) = parts();
            let s = Scanner::new(&mock, &pkg2, dir.path(), &cfg, &[], rules);
            let files = s.scan(dir.path(), &no_acts()).unwrap();
            let e = files.iter().find(|e| e.path == "link.txt").unwrap();
            assert_eq!(e.verdict, Verdict::Restorable);
            assert_eq!(e.source, Some(Source::Verified));
            assert!(e.restore_method.as_deref().unwrap().starts_with("ln -s"));
        }
    }

    #[test]
    #[cfg(unix)]
    fn a_dangling_symlink_is_disposable_not_a_useless_restore_step() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(dir.path().join("collected"), dir.path().join("dead-link"))
            .unwrap();
        let mock = Mock::default();
        let (cfg, pkg, rules) = parts();
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
        let files = s.scan(dir.path(), &no_acts()).unwrap();
        let e = files.iter().find(|e| e.path == "dead-link").unwrap();
        assert_eq!(e.verdict, Verdict::Disposable);
        assert_eq!(e.verdict_source, Origin::Chive);
        assert_eq!(
            e.restore_method, None,
            "`ln -s` would recreate the same broken link"
        );
        assert!(e.verdict.is_cleanable());
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
            let (cfg, pkg, rules) = parts();
            let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &[], rules);
            let files = s.scan(dir.path(), &no_acts()).unwrap();
            let e = files.iter().find(|e| e.path == "link.txt").unwrap();
            assert_eq!(e.verdict, Verdict::Restorable);
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
        let (cfg, pkg, rules) = parts();
        let ignore = ["vendor".to_string()];
        let s = Scanner::new(&mock, &pkg, dir.path(), &cfg, &ignore, rules);
        let files = s.scan(dir.path(), &no_acts()).unwrap();
        assert!(!files.iter().any(|e| e.path.contains("vendor")));
    }
}
