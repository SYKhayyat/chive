use std::fmt;

use serde::{Deserialize, Serialize};

/// The one of four states a catalog entry is in.
///
/// Status carries a single claim about a file: can chive re-derive it, and is
/// it safe to remove. It deliberately never mixes those two questions with
/// the question *who* supplied the recipe — that lives in [`Source`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The catalog holds a recipe and the file can be re-derived.
    Restorable,
    /// The owner says it matters but no recipe exists yet; never cleaned.
    NotRestorable,
    /// A transient file; safe to clean, never a restore target.
    Temporary,
    /// Present on disk with no provenance and no recipe; cleanable unless protected.
    Orphaned,
}

impl Status {
    /// Whether `clean` may remove a file in this state.
    pub fn is_cleanable(self) -> bool {
        matches!(self, Status::Temporary | Status::Orphaned)
    }

    /// The serialized name (also the CLI lowercase spelling).
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Restorable => "restorable",
            Status::NotRestorable => "not-restorable",
            Status::Temporary => "temporary",
            Status::Orphaned => "orphaned",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for Status {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        match s {
            "restorable" => Ok(Status::Restorable),
            "not-restorable" => Ok(Status::NotRestorable),
            "temporary" => Ok(Status::Temporary),
            "orphaned" => Ok(Status::Orphaned),
            _ => Err(()),
        }
    }
}

#[cfg(test)]
mod status_tests {
    use super::*;

    #[test]
    fn clean_can_only_remove_temporary_and_orphaned() {
        assert!(Status::Temporary.is_cleanable());
        assert!(Status::Orphaned.is_cleanable());
        assert!(!Status::Restorable.is_cleanable());
        assert!(!Status::NotRestorable.is_cleanable());
    }

    #[test]
    fn serialized_names_match_display() {
        for s in [
            Status::Restorable,
            Status::NotRestorable,
            Status::Temporary,
            Status::Orphaned,
        ] {
            assert_eq!(s.as_str(), s.to_string());
        }
        assert_eq!(Status::NotRestorable.as_str(), "not-restorable");
    }

    #[test]
    fn deserializes_every_status_spelling() {
        use std::str::FromStr;
        for s in [
            Status::Restorable,
            Status::NotRestorable,
            Status::Temporary,
            Status::Orphaned,
        ] {
            assert_eq!(Status::from_str(s.as_str()), Ok(s));
        }
    }

    #[test]
    fn deserializes_from_toml_value() {
        let v: Status = toml::Value::String("restorable".into()).try_into().unwrap();
        assert_eq!(v, Status::Restorable);
    }
}
