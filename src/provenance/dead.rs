//! Provable-dead detection: the only automatic source of a `disposable` verdict.
//!
//! D19 forbids any automatic answer from calling a file disposable, because the
//! old model recorded "chive could not explain this" as "safe to delete". So the
//! bar here is *provable*: chive may return `disposable` only when it can show
//! the file is already non-functional, never when it merely looks like junk.
//!
//! One source qualifies: a symlink whose target does not resolve. On a
//! `/nix/store`-backed home this is not a corner case — a collected generation
//! leaves dozens behind — and each one is a file that cannot do its job.
//!
//! Name heuristics (`*.tmp`, a trailing `~`) are *not* here. They were the
//! `temporary` guess, and guessing from a filename is the failure D19 exists to
//! remove. The same policy is available, but the owner states it as a rule in
//! their own config (see [`crate::rules`]).
//!
//! Removed-package residue was considered and left out: proving it would mean
//! remembering that a package once owned the file, and a remembered ownership
//! guess is the same class of inference D19 removes. It is available as an owner
//! rule instead, where the owner is the one making the claim.

use std::path::Path;

use crate::model::{Origin, Verdict};

/// Why chive believes a path is already dead. Recorded so `status` can explain a
/// disposal instead of merely asserting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dead {
    /// A symlink whose target does not resolve.
    DanglingLink,
}

impl Dead {
    pub fn as_str(self) -> &'static str {
        match self {
            Dead::DanglingLink => "dangling symlink",
        }
    }
}

/// Whether a symlink's target resolves. This is an lstat-vs-stat question: the
/// link itself is a real occupant of the path (issue #21), but it only *works*
/// if its target is there.
pub fn link_resolves(link: &Path) -> bool {
    std::fs::metadata(link).is_ok()
}

/// The `link_resolves` fact a rule sees. `true` for anything that is not a
/// symlink, because a regular file does not resolve through a link target.
pub fn link_resolves_for_rule(abs: &Path) -> bool {
    match std::fs::symlink_metadata(abs) {
        Ok(meta) if meta.file_type().is_symlink() => link_resolves(abs),
        _ => true,
    }
}

/// Decide whether `abs` is provably dead.
pub fn detect(abs: &Path) -> Option<(Verdict, Origin, Dead)> {
    let meta = std::fs::symlink_metadata(abs).ok()?;
    if meta.file_type().is_symlink() && !link_resolves(abs) {
        // A dangling link is not `restorable`: `ln -s <target> <dest>` would
        // faithfully recreate the same broken link, and a recipe that cannot
        // produce a working file is not a recipe.
        return Some((Verdict::Disposable, Origin::Chive, Dead::DanglingLink));
    }
    None
}

#[cfg(test)]
mod dead_tests {
    use super::*;

    #[test]
    fn an_ordinary_file_is_not_provably_dead() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("plain.txt");
        std::fs::write(&f, "x").unwrap();
        assert_eq!(detect(&f), None);
    }

    #[test]
    #[cfg(unix)]
    fn a_dangling_symlink_is_provably_dead() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("dead");
        std::os::unix::fs::symlink(dir.path().join("gone"), &link).unwrap();
        assert!(!link.exists(), "fixture must be dangling");
        let got = detect(&link).expect("a dead link is detectable");
        assert_eq!(got.0, Verdict::Disposable);
        assert_eq!(got.1, Origin::Chive);
        assert_eq!(got.2, Dead::DanglingLink);
    }

    #[test]
    #[cfg(unix)]
    fn a_live_symlink_is_restorable_and_not_dead() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, "x").unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(detect(&link), None);
    }

    #[test]
    fn dead_verdicts_are_chive_owned_so_they_recompute() {
        // D22: a recomputing scan is what lets a repointed symlink be promoted
        // back to restorable rather than staying disposable forever.
        assert!(!Origin::Chive.is_sticky());
    }

    #[test]
    fn the_rule_fact_only_differs_for_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let plain = dir.path().join("p");
        std::fs::write(&plain, "x").unwrap();
        assert!(link_resolves_for_rule(&plain));
        #[cfg(unix)]
        {
            let dead = dir.path().join("d");
            std::os::unix::fs::symlink(dir.path().join("gone"), &dead).unwrap();
            assert!(!link_resolves_for_rule(&dead));
        }
    }
}
