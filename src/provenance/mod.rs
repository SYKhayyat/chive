//! How chive infers a recipe for a file.
//!
//! Chive checks provenance sources in a fixed order; the first match wins. This
//! *order is load-bearing* (see `docs/spec/why.md`): a file owned by a package
//! that also happens to live in a git repo must use the package recipe because
//! it is more portable across machines.
//!
//! The chain runs package → git → symlink, and each source implements the
//! [`Detector`] trait. The scan does not hard-code *which* commands exclude a
//! package manager — that is declared in the package-adapter data files so new
//! managers (and user ones) arrive without recompiling.
//!
//! [`dead`] is not a recipe source but a verdict source: it is the only
//! automatic answer that may call a file `disposable`, and only where the file
//! is provably already non-functional (D19).

pub mod config;
pub mod dead;
pub mod git;
pub mod package;
pub mod symlink;

use crate::model::{Category, Source};

/// A successfully inferred recipe.
///
/// `category` is carried along because provenance detection happens before
/// classification in the scan, and git/package recipes know things the
/// extension classifier does not (e.g. a package that owns a program).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    /// The shell command that re-derives the file, ready for `{dest}`-only
    /// substitution where applicable.
    pub restore_method: String,
    /// Whether chive inferred it or the owner taught it.
    pub source: Source,
    /// Best-effort category, if the source can improve on extension sniffing.
    pub category: Option<Category>,
    /// The bare owning package name, when a package claimed the file. Carried
    /// rather than re-derived from `restore_method`: the adapters ask their
    /// manager for the exact name (`--queryformat %{NAME}`), so there is no
    /// reason to scrape it back out of a rendered command.
    pub package: Option<String>,
}

/// A source chive can ask "which recipe re-derives this file?".
pub trait Detector {
    /// Probe `abs_path` and return a recipe if this source explains the file.
    fn detect(&self, abs_path: &std::path::Path) -> Option<Recipe>;
}

#[cfg(test)]
mod provenance_tests {
    use super::*;

    #[test]
    fn recipe_carries_every_field() {
        let r = Recipe {
            restore_method: "ln -s /target {dest}".into(),
            source: Source::Verified,
            category: Some(Category::Code),
            package: None,
        };
        assert_eq!(r.restore_method, "ln -s /target {dest}");
        assert_eq!(r.source, Source::Verified);
        assert_eq!(r.category, Some(Category::Code));
        assert_eq!(r.package, None);
    }
}
