use std::fmt;

use serde::{Deserialize, Serialize};

/// Whether a recipe was inferred by chive or supplied by the owner.
///
/// This is a statement about the *recipe's* provenance, not the file's, and it is
/// separate from [`Origin`][crate::model::Origin], which says who may revise the
/// verdict. Both a verified and a user-supplied entry can be `restorable`; the tag
/// only tells the user how to weigh trust in the recipe.
///
/// The old doc claimed this was orthogonal to `Status` — a type deleted in the
/// 10-06 rewrite, so the claim described a model that no longer existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Inferred automatically by provenance detection.
    Verified,
    /// Written explicitly via `chive teach`.
    UserSupplied,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Verified => "verified",
            Source::UserSupplied => "user_supplied",
        }
    }
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn display_matches_serialized_spelling() {
        assert_eq!(Source::Verified.as_str(), "verified");
        assert_eq!(Source::UserSupplied.as_str(), "user_supplied");
    }

    #[test]
    fn deserializes_both_spellings_and_refuses_anything_else() {
        for (s, want) in [
            ("verified", Source::Verified),
            ("user_supplied", Source::UserSupplied),
        ] {
            let got: Source = toml::Value::String(s.into()).try_into().unwrap();
            assert_eq!(got, want);
        }
        let bad: Result<Source, _> = toml::Value::String("bogus".into()).try_into();
        assert!(bad.is_err());
    }
}
