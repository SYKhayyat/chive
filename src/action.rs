//! The operations that *do* something: plan, restore, clean.
//!
//! These are pure functions over a [`Catalog`] plus a [`Runner`], so they are
//! fully unit-testable without a machine. Restore and clean both route their
//! side effects through the [`Runner`] seam, which is how `--dry-run` and the
//! test double intercept them: what you preview with [`plan`] is exactly what
//! [`restore`] would run.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::catalog::Catalog;
use crate::error::Result;
use crate::model::{FileEntry, Status};
use crate::runner::Runner;

/// Defense in depth behind [`crate::catalog::validate_path`]: prove that `p`,
/// joined onto `root`, still names a file inside `root`. Restore and clean call
/// this before they touch the filesystem, so even a caller that bypassed the
/// catalog boundary cannot make chive write or delete outside the root.
fn contained(root: &Path, p: &Path) -> bool {
    p.starts_with(root)
}

/// One resolvable restore step, shared by `plan` and `restore`.
#[derive(Debug, Clone)]
pub struct RestoreItem<'a> {
    pub path: &'a str,
    /// The recipe with `{dest}` already resolved against the target root.
    pub command: String,
    /// The concrete file that would appear, if the recipe targets one.
    pub dest: Option<PathBuf>,
}

/// Substitute `{dest}` in a recipe with a concrete target path. Only `{dest}`
/// is replaced here; other tokens are the recipe's own and stay untouched.
pub fn substitute(recipe: &str, dest: &Path) -> String {
    recipe.replace("{dest}", &dest.to_string_lossy())
}

/// Whether `p` is present on disk, in the no-clobber sense. This must be an
/// lstat, not `exists()`: a dangling symlink is a real occupant of the path
/// (issue #21) and overwriting it would clobber whatever it is about to name.
pub fn present(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

/// The restorable entries selected by include/exclude. No includes means every
/// restorable entry; includes narrows; excludes subtracts from whatever is left.
pub fn select_restorable<'a>(
    catalog: &'a Catalog,
    includes: &[String],
    excludes: &[String],
) -> Vec<&'a FileEntry> {
    let excludes: HashSet<&str> = excludes.iter().map(String::as_str).collect();
    let includes: Option<HashSet<&str>> = if includes.is_empty() {
        None
    } else {
        Some(includes.iter().map(String::as_str).collect())
    };
    catalog
        .files()
        .iter()
        .filter(|e| e.is_restorable())
        .filter(|e| {
            includes
                .as_ref()
                .is_none_or(|s| s.contains(e.path.as_str()))
        })
        .filter(|e| !excludes.contains(e.path.as_str()))
        .collect()
}

/// The exact restore steps for the selected entries under `root`, in path order.
/// A step targets a concrete `dest` only when its recipe references `{dest}`;
/// package and git recipes place files themselves and have no local dest.
pub fn build_plan<'a>(
    catalog: &'a Catalog,
    root: &Path,
    includes: &[String],
    excludes: &[String],
) -> Vec<RestoreItem<'a>> {
    let mut items: Vec<RestoreItem<'a>> = select_restorable(catalog, includes, excludes)
        .into_iter()
        .map(|entry| {
            let method = entry.restore_method.as_deref().unwrap_or_default();
            let dest = method
                .contains("{dest}")
                .then(|| root.join(&entry.path))
                .filter(|d| contained(root, d));
            RestoreItem {
                path: &entry.path,
                command: match &dest {
                    Some(d) => substitute(method, d),
                    None => method.to_string(),
                },
                dest,
            }
        })
        .collect();
    items.sort_by(|a, b| a.path.cmp(b.path));
    items
}

/// One file's restore outcome.
#[derive(Debug)]
pub enum RestoreOutcome {
    Restored(String),
    SkippedExists(String),
    Failed(String, String),
}

/// Execute restore steps in order. A step whose `dest` already exists is
/// refused without running (the no-clobber rule, decision D15). Before a
/// `{dest}` recipe runs, its parent directory is created (issue #26): a fresh
/// machine has none of the directories the source layout implies.
pub fn restore(runner: &dyn Runner, items: &[RestoreItem]) -> Vec<RestoreOutcome> {
    items
        .iter()
        .map(|item| {
            let Some(dest) = &item.dest else {
                return run_recipe(runner, item);
            };
            if present(dest) {
                return RestoreOutcome::SkippedExists(item.path.to_string());
            }
            if !runner.ensure_parent_dir(dest) {
                return RestoreOutcome::Failed(
                    item.path.to_string(),
                    format!("could not create parent directory for {}", dest.display()),
                );
            }
            run_recipe(runner, item)
        })
        .collect()
}

fn run_recipe(runner: &dyn Runner, item: &RestoreItem) -> RestoreOutcome {
    match runner.run_recipe(&item.command) {
        Ok(out) if out.success() => RestoreOutcome::Restored(item.path.to_string()),
        Ok(out) => RestoreOutcome::Failed(
            item.path.to_string(),
            describe_failure(&item.command, &out.stderr, out.code),
        ),
        Err(e) => RestoreOutcome::Failed(item.path.to_string(), e.to_string()),
    }
}

fn describe_failure(command: &str, stderr: &str, code: Option<i32>) -> String {
    let code = code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".to_string());
    let stderr = stderr.trim();
    if stderr.is_empty() {
        format!("exit code {code}: {command}")
    } else {
        format!("exit code {code}: {stderr}")
    }
}

/// Which cleanable files to remove. Restorable and not-restorable are never
/// removable (this is the safety guarantee).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanScope {
    Temporary,
    Orphaned,
    Both,
}

impl CleanScope {
    fn selects(self, status: Status) -> bool {
        match self {
            CleanScope::Both => status.is_cleanable(),
            CleanScope::Temporary => status == Status::Temporary,
            CleanScope::Orphaned => status == Status::Orphaned,
        }
    }
}

/// The paths clean would remove — a read-only preview, deletes nothing.
pub fn clean_preview<'a>(
    catalog: &'a Catalog,
    root: &Path,
    scope: CleanScope,
) -> Vec<(&'a str, PathBuf)> {
    catalog
        .files()
        .iter()
        .filter(|e| scope.selects(e.status))
        .map(|e| (e.path.as_str(), root.join(&e.path)))
        .filter(|(_, abs)| contained(root, abs))
        .collect()
}

/// Remove the selected cleanable files, through the runner seam, and return the
/// catalog with the removed entries stripped.
pub fn clean_execute(
    runner: &dyn Runner,
    catalog: &Catalog,
    root: &Path,
    scope: CleanScope,
) -> Result<(Catalog, Vec<String>)> {
    let rows = clean_preview(catalog, root, scope);
    let mut removed: Vec<String> = Vec::new();
    for (rel, abs) in rows {
        if runner.remove_file(&abs) {
            removed.push(rel.to_string());
        }
    }
    let removals: HashSet<&str> = removed.iter().map(String::as_str).collect();
    let files: Vec<FileEntry> = catalog
        .files()
        .iter()
        .filter(|e| !removals.contains(e.path.as_str()))
        .cloned()
        .collect();
    let next = Catalog::new(
        catalog.root.clone(),
        catalog.scanned_at.clone(),
        catalog.host.clone(),
        files,
    )?;
    Ok((next, removed))
}

#[cfg(test)]
mod action_tests {
    use super::*;
    use crate::model::{Category, Source};

    fn restorable(path: &str, method: &str) -> FileEntry {
        FileEntry::new_restorable(
            path.into(),
            Some(Category::Config),
            method.into(),
            Source::Verified,
            1,
            None,
        )
    }

    fn sample() -> Catalog {
        let mut files = vec![
            restorable("a.txt", "cp ~/seed/a.txt '{dest}'"),
            restorable("b.txt", "cp ~/seed/b.txt '{dest}'"),
            FileEntry::new_orphaned("junk.tmp".into(), None, 1, None),
        ];
        files.push(FileEntry {
            path: "temp~".into(),
            status: Status::Temporary,
            category: None,
            restore_method: None,
            source: None,
            not_restorable_reason: None,
            size: 1,
            modified: None,
        });
        Catalog::new("/root".into(), "t".into(), "h".into(), files).unwrap()
    }

    fn c_at(root: &Path, files: Vec<FileEntry>) -> Catalog {
        Catalog::new(root.to_string_lossy().into(), "t".into(), "h".into(), files).unwrap()
    }

    #[test]
    fn substitute_only_replaces_dest() {
        let d = Path::new("/root/a.txt");
        assert_eq!(substitute("cp x '{dest}'", d), "cp x '/root/a.txt'");
        assert_eq!(substitute("no token", d), "no token");
    }

    #[test]
    fn select_defaults_to_all_then_respects_include_exclude() {
        let c = sample();
        assert_eq!(select_restorable(&c, &[], &[]).len(), 2);
        let inc = select_restorable(&c, &["a.txt".into()], &[]);
        assert_eq!(&inc[0].path, "a.txt");
        let exc = select_restorable(&c, &[], &["b.txt".into()]);
        assert_eq!(&exc[0].path, "a.txt");
    }

    #[test]
    fn plan_sets_dest_only_for_dest_recipes() {
        let c = sample();
        let plan = build_plan(&c, Path::new("/target"), &[], &[]);
        // Both restorable entries reference {dest}, so both resolve to /target.
        assert_eq!(plan.len(), 2);
        assert!(plan.iter().any(|i| i.path == "a.txt"));
        assert!(plan.iter().all(|i| i.dest.is_some()));
        assert!(plan.iter().all(|i| i.command.contains("/target/")));
    }

    #[test]
    fn recipe_without_dest_has_no_clobber_target() {
        let c = c_at(
            Path::new("/root"),
            vec![restorable("pkg.txt", "sudo apt --reinstall x")],
        );
        let plan = build_plan(&c, Path::new("/root"), &[], &[]);
        assert_eq!(plan[0].dest, None);
        assert_eq!(plan[0].command, "sudo apt --reinstall x");
    }

    #[test]
    fn restore_refuses_existing_dest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "present").unwrap();
        let c = c_at(root, vec![restorable("a.txt", "echo '{dest}'")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mock = crate::runner::Mock::default();
        let outcomes = restore(&mock, &plan);
        assert!(matches!(outcomes[0], RestoreOutcome::SkippedExists(_)));
        assert!(
            mock.recorded().is_empty(),
            "no recipe ran for a clobbered dest"
        );
    }

    #[test]
    #[cfg(unix)]
    fn a_dangling_symlink_dest_counts_as_present() {
        // Issue #21: exists() follows the link and reports absence; the
        // no-clobber check must not.
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("dangling");
        std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();
        assert!(!link.exists(), "fixture must be a *dangling* link");
        assert!(present(&link), "lstat must report the link as present");

        let c = c_at(
            dir.path(),
            vec![restorable("dangling", "echo X > '{dest}'")],
        );
        let plan = build_plan(&c, dir.path(), &[], &[]);
        let mock = crate::runner::Mock::default();
        let outcomes = restore(&mock, &plan);
        assert!(matches!(outcomes[0], RestoreOutcome::SkippedExists(_)));
        assert!(
            mock.recorded().is_empty(),
            "a dangling occupant must not be clobbered"
        );
    }

    #[test]
    fn restore_runs_recipe_for_missing_dest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = c_at(root, vec![restorable("hi.txt", "echo hi")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mut mock = crate::runner::Mock::default();
        mock.on_recipe(|_| Some(crate::runner::Mock::ok("hi")));
        let outcomes = restore(&mock, &plan);
        assert!(matches!(outcomes[0], RestoreOutcome::Restored(_)));
        assert_eq!(mock.recorded(), vec!["recipe: echo hi"]);
    }

    #[test]
    fn restore_creates_the_dest_parent_dir_before_the_recipe_runs() {
        // Issue #26: a fresh machine has none of the parent directories the
        // source layout implies; restore must make them, in order, and only
        // after the no-clobber check passes.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = c_at(
            root,
            vec![restorable("conf/emacs.d/init.el", "echo x > '{dest}'")],
        );
        let plan = build_plan(&c, root, &[], &[]);
        let mock = crate::runner::Mock::default();
        let outcomes = restore(&mock, &plan);
        assert!(matches!(outcomes[0], RestoreOutcome::Restored(_)));
        let dest = root.join("conf/emacs.d/init.el").display().to_string();
        assert_eq!(
            mock.recorded(),
            vec![
                format!("mkdir {dest}"),
                format!("recipe: echo x > '{dest}'"),
            ]
        );
    }

    #[test]
    fn restore_skips_clobber_before_creating_any_directory() {
        // The no-clobber check precedes parent creation: a refused restore
        // must not leave directories behind.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("conf/emacs.d")).unwrap();
        std::fs::write(root.join("conf/emacs.d/init.el"), "present").unwrap();
        let c = c_at(
            root,
            vec![restorable("conf/emacs.d/init.el", "echo x > '{dest}'")],
        );
        let plan = build_plan(&c, root, &[], &[]);
        let mock = crate::runner::Mock::default();
        let outcomes = restore(&mock, &plan);
        assert!(matches!(outcomes[0], RestoreOutcome::SkippedExists(_)));
        assert!(
            mock.recorded().is_empty(),
            "a refused restore runs nothing and creates nothing"
        );
    }

    #[test]
    fn recipes_without_dest_create_no_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = c_at(root, vec![restorable("pkg.txt", "sudo apt --reinstall x")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mock = crate::runner::Mock::default();
        let outcomes = restore(&mock, &plan);
        assert!(matches!(outcomes[0], RestoreOutcome::Restored(_)));
        assert_eq!(mock.recorded(), vec!["recipe: sudo apt --reinstall x"]);
    }

    #[test]
    fn failing_recipe_is_a_failure_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = c_at(root, vec![restorable("f.txt", "false")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mut mock = crate::runner::Mock::default();
        mock.on_recipe(|_| {
            Some(crate::runner::Output {
                stdout: String::new(),
                stderr: "boom".into(),
                code: Some(1),
            })
        });
        let outcomes = restore(&mock, &plan);
        assert!(matches!(outcomes[0], RestoreOutcome::Failed(_, _)));
    }

    #[test]
    fn clean_scope_filters_and_removes_through_runner() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("junk.tmp"), "x").unwrap();
        std::fs::write(root.join("temp~"), "x").unwrap();
        let c = sample();
        let both = clean_preview(&c, root, CleanScope::Both);
        assert_eq!(both.len(), 2);
        let temp = clean_preview(&c, root, CleanScope::Temporary);
        assert_eq!(temp.len(), 1);
        assert_eq!(temp[0].0, "temp~");

        let mock = crate::runner::Mock::default();
        let (next, removed) = clean_execute(&mock, &c, root, CleanScope::Both).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(next.by_path("junk.tmp").is_none());
        assert!(next.by_path("temp~").is_none());
        assert!(
            next.by_path("a.txt").is_some(),
            "restorable must survive clean"
        );
    }
}
