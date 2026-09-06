//! Git work-tree provenance: a file tracked by a repository has a deterministic
//! restore (checkout from `HEAD`).

use std::path::{Path, PathBuf};

use crate::model::Source;
use crate::provenance::Recipe;
use crate::runner::Runner;

/// Progress marker when walking up to find the repo root.
pub struct GitDetector<'a> {
    runner: &'a dyn Runner,
}

impl<'a> GitDetector<'a> {
    pub fn new(runner: &'a dyn Runner) -> Self {
        GitDetector { runner }
    }
}

impl super::Detector for GitDetector<'_> {
    fn detect(&self, abs_path: &Path) -> Option<Recipe> {
        git_location(self.runner, abs_path).map(|loc| Recipe {
            restore_method: loc.recipe(),
            source: Source::Verified,
            category: None, // git says nothing about *type*; the classifier decides
        })
    }
}

/// Where a file lives in a repo, if git tracks it.
pub struct GitLocation {
    /// The repository root (parent dir containing `.git`), absolute.
    pub repo_root: PathBuf,
    /// The file's path relative to the repo root.
    pub rel_path: String,
    /// A remote URL if `origin` is configured (informational; may be empty).
    pub remote: Option<String>,
}

impl GitLocation {
    /// The recipe to re-derive this file: checkout from HEAD in that repo.
    pub fn recipe(&self) -> String {
        format!(
            "git -C {:?} checkout HEAD -- '{}'",
            self.repo_root.display(),
            self.rel_path
        )
    }
}

/// Find whether `abs_path` is tracked by a git repo, walking from its directory
/// up towards the root until a `.git` is found.
///
/// Uses `git -C <dir> ls-files --error-unmatch <rel>` for the actual check
/// (what the spec requires) rather than trusting a directory named `.git`.
fn git_location(runner: &dyn Runner, abs_path: &Path) -> Option<GitLocation> {
    let file_name = abs_path.file_name()?.to_str()?.to_string();
    let mut dir = abs_path.parent()?;
    loop {
        // A candidate repo root must contain a .git entry (dir, file, or gitlink).
        if dir.join(".git").exists() {
            let rel = relpath(dir, &file_name)?;
            let ok = tracked(runner, dir, &rel)?;
            if ok {
                return Some(GitLocation {
                    repo_root: dir.to_path_buf(),
                    rel_path: rel,
                    remote: remote_of(runner, dir),
                });
            }
        }
        dir = dir.parent()?;
    }
}

/// Ask git whether `rel` is tracked under `repo`, using the given runner.
fn tracked(runner: &dyn Runner, repo: &Path, rel: &str) -> Option<bool> {
    let out = runner
        .run_argv(
            "git",
            &["-C", repo.to_str()?, "ls-files", "--error-unmatch", rel],
        )
        .ok()?;
    Some(out.success())
}

/// The `origin` URL if configured; empty when absent (best effort).
fn remote_of(runner: &dyn Runner, repo: &Path) -> Option<String> {
    let out = runner
        .run_argv(
            "git",
            &["-C", repo.to_str()?, "remote", "get-url", "origin"],
        )
        .ok()?;
    if out.success() && !out.stdout.trim().is_empty() {
        Some(out.stdout.trim().to_string())
    } else {
        None
    }
}

fn relpath(repo: &Path, file_name: &str) -> Option<String> {
    let f = std::fs::canonicalize(repo.join(file_name)).ok()?;
    let r = std::fs::canonicalize(repo).ok()?;
    f.strip_prefix(&r).ok()?.to_str().map(|s| s.to_string())
}

#[cfg(test)]
mod git_detector_tests {
    use super::*;
    use crate::provenance::Detector;
    use crate::runner::Mock;

    #[test]
    fn untracked_file_is_not_git_sourced() {
        // No .git anywhere (the /tmp root has none up to filesystem root), and
        // git isn't even registered. Walking hits the root and stops.
        let mock = Mock::default();
        let d = GitDetector::new(&mock);
        assert!(d.detect(Path::new("/tmp/no/repo/here/file.txt")).is_none());
    }

    #[test]
    fn tracked_file_under_repo_yields_checkout_recipe() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        let f = dir.path().join("init.el");
        std::fs::write(&f, "").unwrap();

        let mut mock = Mock::default();
        mock.on_argv(|program, args| {
            if program == "git" && args.contains(&"ls-files") {
                Some(Mock::ok(""))
            } else if program == "git" && args.contains(&"remote") {
                Some(crate::runner::Output {
                    stdout: "https://example.com/dotfiles.git\n".into(),
                    stderr: String::new(),
                    code: Some(0),
                })
            } else {
                None
            }
        });

        let d = GitDetector::new(&mock);
        // canonicalization: detect() receives the canonical path.
        let canonical = std::fs::canonicalize(&f).unwrap();
        let r = d.detect(&canonical).expect("tracked file is git-sourced");
        assert!(r.restore_method.contains("checkout HEAD"));
        assert!(r.restore_method.contains("init.el"));
        assert_eq!(r.source, Source::Verified);
    }

    #[test]
    fn git_check_that_fails_yields_no_recipe() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        let canonical = std::fs::canonicalize(dir.path().join("a.txt")).unwrap();

        let mut mock = Mock::default();
        mock.on_argv(|program, args| {
            if program == "git" && args.contains(&"ls-files") {
                // error-unmatch fails for untracked files.
                Some(crate::runner::Output {
                    stdout: String::new(),
                    stderr: "not tracked".into(),
                    code: Some(1),
                })
            } else {
                None
            }
        });
        let d = GitDetector::new(&mock);
        assert!(d.detect(&canonical).is_none());
    }
}
