use std::fmt;

use serde::{Deserialize, Serialize};

/// Whether a recipe was inferred by chive or supplied by the owner.
///
/// This is a statement about the *recipe's* provenance, not the file's, and it
/// stays orthogonal to [`Status`][crate::model::Status]. Both a verified and a
/// user-supplied entry can be `restorable`; the tag only tells the user how to
/// weigh trust in the recipe.
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

impl std::str::FromStr for Source {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        match s {
            "verified" => Ok(Source::Verified),
            "user_supplied" => Ok(Source::UserSupplied),
            _ => Err(()),
        }
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
    fn deserializes_both_spellings() {
        use std::str::FromStr;
        assert_eq!(Source::from_str("verified"), Ok(Source::Verified));
        assert_eq!(Source::from_str("user_supplied"), Ok(Source::UserSupplied));
        assert!(Source::from_str("bogus").is_err());
    }
}
