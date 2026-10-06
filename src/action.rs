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
use crate::config::Overwrite;
use crate::error::Result;
use crate::model::FileEntry;
use crate::runner::Runner;

/// One resolvable restore step, shared by `plan` and `restore`.
///
/// `dest` is `root + entry.path` for **every** entry. The old shape made it
/// `Some` only when the recipe happened to contain the literal `{dest}`, which
/// left every git-tracked file unprotected: `restore` on a dotfiles repo
/// destroyed a local edit and reported success (issue #21). A git recipe places
/// the file at exactly this path; chive has both halves in hand when it builds
/// the plan. `recipe_names_dest` survives only to gate parent-directory
/// creation, which package recipes genuinely do not need.
#[derive(Debug, Clone)]
pub struct RestoreItem<'a> {
    pub path: &'a str,
    /// The recipe with `{dest}` already resolved against the target root.
    pub command: String,
    /// The concrete file this entry is about, under the target root.
    pub dest: PathBuf,
    /// Whether the recipe itself names `{dest}` (decides parent creation only).
    pub recipe_names_dest: bool,
}

/// Substitute `{dest}` in a recipe with a concrete target path. Only `{dest}`
/// is replaced here; other tokens are the recipe's own and stay untouched.
pub fn substitute(recipe: &str, dest: &Path) -> String {
    recipe.replace("{dest}", &dest.to_string_lossy())
}

/// Substitute `{root}` — the target machine's restore root. Recipes that
/// address a resource beside the file itself (a git repo holding `{dest}`)
/// use it so the recipe works on any machine whose layout matches the
/// catalog's relative one (issue #28).
pub fn substitute_root(recipe: &str, root: &Path) -> String {
    recipe.replace("{root}", &root.to_string_lossy())
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
            // Containment is decided by the catalog, which can refuse. A plan
            // that cannot be built is not a plan that silently drops the entry.
            let dest = crate::catalog::resolve_under(root, &entry.path)
                .unwrap_or_else(|_| root.join(&entry.path));
            let recipe_names_dest = method.contains("{dest}");
            RestoreItem {
                path: &entry.path,
                command: if recipe_names_dest {
                    substitute(method, &dest)
                } else {
                    method.replace("{root}", &root.to_string_lossy())
                },
                dest,
                recipe_names_dest,
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

/// Execute restore steps in order. How an existing `dest` is treated comes from
/// `overwrite` (D24); the default refuses without running (D15). Before a
/// `{dest}`-naming recipe runs, its parent directory is created (issue #26): a
/// fresh machine has none of the directories the source layout implies.
///
/// A step that exits 0 but produced no file is a **failure** (issue #49). Shall
/// states the rule this is an instance of: *"an action that fetches something
/// throws on failure, because a fetch that quietly returned nothing would let a
/// hook report success over a command that never ran."*
pub fn restore(
    runner: &dyn Runner,
    items: &[RestoreItem],
    overwrite: Overwrite,
) -> Vec<RestoreOutcome> {
    items
        .iter()
        .map(|item| match occupy(item, overwrite) {
            Some(outcome) => outcome,
            None => {
                if item.recipe_names_dest && !runner.ensure_parent_dir(&item.dest) {
                    return RestoreOutcome::Failed(
                        item.path.to_string(),
                        format!(
                            "could not create parent directory for {}",
                            item.dest.display()
                        ),
                    );
                }
                run_recipe(runner, item)
            }
        })
        .collect()
}

/// Decide what an already-occupied `dest` means, or `None` to proceed.
///
/// The backup copy goes straight to the filesystem rather than through the
/// `Runner` seam: it is chive's own displacement step rather than an owner
/// recipe, and `plan` is the preview path, so there is no dry-run variant of
/// this to keep honest.
fn occupy(item: &RestoreItem, overwrite: Overwrite) -> Option<RestoreOutcome> {
    if !present(&item.dest) {
        return None;
    }
    match overwrite {
        Overwrite::Refuse => Some(RestoreOutcome::SkippedExists(item.path.to_string())),
        Overwrite::Backup => {
            let backup = backup_path(&item.dest);
            match std::fs::copy(&item.dest, &backup) {
                Ok(_) => None,
                Err(e) => Some(RestoreOutcome::Failed(
                    item.path.to_string(),
                    format!(
                        "could not back up {} to {}: {e}",
                        item.dest.display(),
                        backup.display()
                    ),
                )),
            }
        }
        Overwrite::Overwrite => None,
    }
}

/// Where a displaced file is copied before being replaced (D24).
pub fn backup_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(Overwrite::BACKUP_SUFFIX);
    PathBuf::from(name)
}

fn run_recipe(runner: &dyn Runner, item: &RestoreItem) -> RestoreOutcome {
    match runner.run_recipe(&item.command) {
        Ok(out) if out.success() => {
            if present(&item.dest) {
                RestoreOutcome::Restored(item.path.to_string())
            } else {
                RestoreOutcome::Failed(
                    item.path.to_string(),
                    format!(
                        "recipe succeeded (exit 0) but {} does not exist",
                        item.dest.display()
                    ),
                )
            }
        }
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

/// The paths clean would remove — a read-only preview, deletes nothing.
///
/// Only `disposable`. Under D19 that is the sole cleanable verdict, so the set
/// here is exactly the set someone named; there is no scope flag because there
/// is no second cleanable class left to choose between.
pub fn clean_preview<'a>(catalog: &'a Catalog, root: &Path) -> Vec<(&'a str, PathBuf)> {
    catalog
        .files()
        .iter()
        .filter(|e| e.verdict.is_cleanable())
        .map(|e| (e.path.as_str(), root.join(&e.path)))
        .collect()
}

/// The cleanable paths that containment actually permits, and the ones it does
/// not. Clean refuses outright rather than silently skipping: a containment
/// failure that exits 0 having removed something else is the failure mode
/// issue #48 describes.
pub fn clean_resolved<'a>(
    catalog: &'a Catalog,
    root: &Path,
) -> (Vec<(&'a str, PathBuf)>, Vec<&'a str>) {
    let (mut ok, mut refused) = (Vec::new(), Vec::new());
    for e in catalog.files().iter().filter(|e| e.verdict.is_cleanable()) {
        match crate::catalog::resolve_under(root, &e.path) {
            Ok(abs) => ok.push((e.path.as_str(), abs)),
            Err(_) => refused.push(e.path.as_str()),
        }
    }
    (ok, refused)
}

/// Remove the selected cleanable files, through the runner seam, and return the
/// catalog with the removed entries stripped.
pub fn clean_execute(
    runner: &dyn Runner,
    catalog: &Catalog,
    root: &Path,
) -> Result<(Catalog, Vec<String>)> {
    let rows = clean_preview(catalog, root);
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
        catalog.acts().clone(),
    )?;
    Ok((next, removed))
}

#[cfg(test)]
mod action_tests {
    use super::*;
    use crate::act::{Act, ActLog};
    use crate::model::{Category, Origin, Source};

    fn restorable(path: &str, method: &str) -> FileEntry {
        FileEntry::new_restorable(
            path.into(),
            Some(Category::Config),
            method.into(),
            Source::Verified,
            Origin::Chive,
            1,
            None,
        )
    }

    /// Two restorable, one hole, two disposable (one owner, one chive).
    fn sample() -> Catalog {
        let files = vec![
            restorable("a.txt", "cp ~/seed/a.txt '{dest}'"),
            restorable("b.txt", "cp ~/seed/b.txt '{dest}'"),
            FileEntry::new_unknown("hole.nef".into(), None, 1, None),
            FileEntry::new_disposable("junk.tmp".into(), None, Origin::Owner, 1, None),
            FileEntry::new_disposable("dead-link".into(), None, Origin::Chive, 1, None),
        ];
        Catalog::new(
            "/root".into(),
            "t".into(),
            "h".into(),
            files,
            ActLog::default(),
        )
        .unwrap()
    }

    fn c_at(root: &Path, files: Vec<FileEntry>) -> Catalog {
        Catalog::new(
            root.to_string_lossy().into(),
            "t".into(),
            "h".into(),
            files,
            ActLog::default(),
        )
        .unwrap()
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
    fn every_entry_gets_a_dest_so_no_clobber_check_is_skipped() {
        // Issue #21: `dest` used to be `None` for any recipe without a literal
        // `{dest}`, which left every git-tracked file unprotected. Now every
        // entry is checked; the token only gates parent creation.
        let c = c_at(
            Path::new("/root"),
            vec![
                restorable("pkg.txt", "sudo apt --reinstall x"),
                restorable(
                    "dotfiles/rc",
                    "git -C '{root}/dotfiles' checkout HEAD -- rc",
                ),
            ],
        );
        let plan = build_plan(&c, Path::new("/target"), &[], &[]);
        assert_eq!(plan.len(), 2);
        for item in &plan {
            assert!(
                item.dest.starts_with("/target"),
                "{} must resolve under the root, got {}",
                item.path,
                item.dest.display()
            );
            assert!(
                !item.recipe_names_dest,
                "neither recipe names {{dest}} literally"
            );
        }
        // Sorted by path, so the dotfiles entry comes first.
        assert_eq!(plan[0].path, "dotfiles/rc");
        assert!(plan[0].command.contains("/target/dotfiles"));
        assert_eq!(plan[1].command, "sudo apt --reinstall x");
    }

    #[test]
    fn a_recipe_naming_dest_gets_it_substituted() {
        let c = c_at(
            Path::new("/root"),
            vec![restorable("a.txt", "cp x '{dest}'")],
        );
        let plan = build_plan(&c, Path::new("/target"), &[], &[]);
        assert!(plan[0].recipe_names_dest);
        assert!(plan[0].command.contains("/target/a.txt"));
    }

    #[test]
    fn overwrite_default_refuses_and_backup_copies_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "PRECIOUS").unwrap();
        let c = c_at(root, vec![restorable("a.txt", "echo new > '{dest}'")]);

        // Default: refused, nothing written, nothing copied.
        let plan = build_plan(&c, root, &[], &[]);
        let mock = crate::runner::Mock::default();
        let out = restore(&mock, &plan, Overwrite::Refuse);
        assert!(matches!(out[0], RestoreOutcome::SkippedExists(_)));
        assert_eq!(
            std::fs::read_to_string(root.join("a.txt")).unwrap(),
            "PRECIOUS"
        );
        assert!(!backup_path(&root.join("a.txt")).exists());

        // `backup`: the old bytes are preserved under the backup name first.
        let mut mock = crate::runner::Mock::default();
        mock.on_recipe(|_| Some(crate::runner::Mock::ok("")));
        std::fs::write(root.join("a.txt"), "PRECIOUS").unwrap();
        let out = restore(&mock, &plan, Overwrite::Backup);
        assert!(matches!(out[0], RestoreOutcome::Restored(_)));
        assert_eq!(
            std::fs::read_to_string(backup_path(&root.join("a.txt"))).unwrap(),
            "PRECIOUS",
            "the displaced file must survive"
        );
    }

    #[test]
    fn overwrite_escape_hatch_writes_with_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "old").unwrap();
        let c = c_at(root, vec![restorable("a.txt", "echo new > '{dest}'")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mut mock = crate::runner::Mock::default();
        let owned = root.to_path_buf();
        mock.on_recipe(move |_| {
            std::fs::write(owned.join("a.txt"), "new").unwrap();
            Some(crate::runner::Mock::ok(""))
        });
        let out = restore(&mock, &plan, Overwrite::Overwrite);
        assert!(matches!(out[0], RestoreOutcome::Restored(_)));
        assert!(!backup_path(&root.join("a.txt")).exists());
    }

    #[test]
    fn a_recipe_that_exits_zero_without_producing_the_file_is_a_failure() {
        // Issue #49: `restored:` used to mean "exit 0", and `chive teach` accepts
        // any string, so `true` reported a restore that never happened.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let c = c_at(root, vec![restorable("a.txt", "true")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mut mock = crate::runner::Mock::default();
        mock.on_recipe(|_| Some(crate::runner::Mock::ok("")));
        let out = restore(&mock, &plan, Overwrite::Refuse);
        match &out[0] {
            RestoreOutcome::Failed(_, why) => assert!(
                why.contains("does not exist"),
                "the message must say what is missing, got {why}"
            ),
            other => panic!("expected a failure, got {other:?}"),
        }
    }

    #[test]
    fn restore_refuses_existing_dest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("a.txt"), "present").unwrap();
        let c = c_at(root, vec![restorable("a.txt", "echo '{dest}'")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mock = crate::runner::Mock::default();
        let outcomes = restore(&mock, &plan, Overwrite::Refuse);
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
        let outcomes = restore(&mock, &plan, Overwrite::Refuse);
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
        let c = c_at(root, vec![restorable("hi.txt", "echo hi > '{dest}'")]);
        let plan = build_plan(&c, root, &[], &[]);
        let mut mock = crate::runner::Mock::default();
        // `on_recipe` wants a 'static closure, so the path moves in owned.
        let owned = root.to_path_buf();
        mock.on_recipe(move |_| {
            // The mock does not really write, so create the file the post
            // condition will look for.
            std::fs::write(owned.join("hi.txt"), "hi").unwrap();
            Some(crate::runner::Mock::ok("hi"))
        });
        let outcomes = restore(&mock, &plan, Overwrite::Refuse);
        assert!(matches!(outcomes[0], RestoreOutcome::Restored(_)));
        assert_eq!(
            mock.recorded(),
            vec![
                format!("mkdir -p {}", root.display()),
                "recipe: echo hi > '/tmp/PLACEHOLDER'"
                    .replace("/tmp/PLACEHOLDER", &format!("{}/hi.txt", root.display())),
            ]
        );
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
        // The verdict is not the subject here: the mock recipe writes nothing, so
        // under the post-condition rule (issue #49) it is correctly a failure.
        // What this test proves is the *order*.
        restore(&mock, &plan, Overwrite::Refuse);
        let dest = root.join("conf/emacs.d/init.el").display().to_string();
        assert_eq!(
            mock.recorded(),
            vec![
                format!("mkdir -p {}", root.join("conf/emacs.d").display()),
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
        let outcomes = restore(&mock, &plan, Overwrite::Refuse);
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
        restore(&mock, &plan, Overwrite::Refuse);
        assert_eq!(
            mock.recorded(),
            vec!["recipe: sudo apt --reinstall x"],
            "a package recipe places its own files, so chive creates no directories"
        );
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
        let outcomes = restore(&mock, &plan, Overwrite::Refuse);
        assert!(matches!(outcomes[0], RestoreOutcome::Failed(_, _)));
    }

    #[test]
    fn clean_removes_only_disposable_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for f in ["junk.tmp", "dead-link", "hole.nef", "a.txt"] {
            std::fs::write(root.join(f), "x").unwrap();
        }
        let c = sample();
        let rows = clean_preview(&c, root);
        let paths: Vec<_> = rows.iter().map(|(p, _)| *p).collect();
        assert_eq!(paths, vec!["dead-link", "junk.tmp"]);

        let mock = crate::runner::Mock::default();
        let (next, removed) = clean_execute(&mock, &c, root).unwrap();
        assert_eq!(removed.len(), 2);
        assert!(next.by_path("junk.tmp").is_none());
        assert!(next.by_path("dead-link").is_none());
        assert!(
            next.by_path("a.txt").is_some(),
            "restorable must survive clean"
        );
        assert!(
            next.by_path("hole.nef").is_some(),
            "a hole must survive clean: chive not understanding a file is not consent to delete it"
        );
    }

    #[test]
    fn clean_preserves_the_act_log_so_a_disposal_can_be_reinstated() {
        // D20: removing a file must not discard the log, or the owner's record
        // of having disposed of it would vanish with it.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("junk.tmp"), "x").unwrap();
        let mut c = sample();
        c.record(Act::dispose(0, "junk.tmp")).unwrap();
        let mock = crate::runner::Mock::default();
        let (next, _) = clean_execute(&mock, &c, root).unwrap();
        assert_eq!(next.acts().len(), c.acts().len());
    }
}
