//! Symlink provenance: a symbolic link is re-derived by re-creating the link.

use std::path::Path;

use crate::model::Source;
use crate::provenance::Recipe;

/// A file is a symbolic link → recipe: `ln -s <target> <dest>`.
pub struct SymlinkDetector;

impl super::Detector for SymlinkDetector {
    fn detect(&self, abs_path: &Path) -> Option<Recipe> {
        let target = std::fs::read_link(abs_path).ok()?;
        Some(Recipe {
            // {dest} is substituted at restore time; target is captured now.
            restore_method: format!("ln -s '{}' '{{dest}}'", target.display()),
            source: Source::Verified,
            category: None,
        })
    }
}

#[cfg(test)]
mod symlink_detector_tests {
    use super::*;
    use crate::provenance::Detector;

    #[test]
    fn regular_file_is_not_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("plain.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(SymlinkDetector.detect(&f).is_none());
    }

    #[test]
    #[cfg(unix)]
    fn symlink_yields_ln_recipe_using_dest() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, "x").unwrap();
        let link = dir.path().join("link.txt");
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let r = SymlinkDetector.detect(&link).expect("symlink is detected");
        assert!(r.restore_method.contains("ln -s '"));
        assert!(r.restore_method.ends_with("'{dest}'"));
        assert!(r.restore_method.contains("real.txt"));
    }
}
