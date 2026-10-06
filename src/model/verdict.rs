use std::fmt;

use serde::{Deserialize, Serialize};

/// The one of three states a catalog entry is in.
///
/// A verdict makes exactly one claim, and the enum is small because the claims
/// are disjoint: can chive rebuild this, or is it safe to remove. The old model
/// folded "chive cannot explain this" and "safe to delete" into one value, so a
/// failed provenance probe became permission to delete — see D19.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// The catalog holds a recipe and chive can re-derive the file.
    Restorable,
    /// It matters and chive cannot rebuild it. A hole, and never cleanable.
    Unknown,
    /// The owner judged it a known gap. The only cleanable state.
    Disposable,
}

impl Verdict {
    /// Whether `clean` may remove a file in this state.
    ///
    /// Only `Disposable`. This is the whole safety guarantee under D19, and it
    /// is structural: no automatic verdict is cleanable except provable-dead,
    /// so the set `clean` may remove is exactly the set someone named.
    pub fn is_cleanable(self) -> bool {
        matches!(self, Verdict::Disposable)
    }

    /// The serialized name (also the CLI lowercase spelling).
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Restorable => "restorable",
            Verdict::Unknown => "unknown",
            Verdict::Disposable => "disposable",
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who decided a verdict. Drives stickiness (D22): an owner's word survives a
/// rescan, while chive's own inference is re-derived so it can be retracted when
/// the evidence changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// An owner act: `teach`, `dispose`, `withdraw`, or a hand-edited catalog.
    Owner,
    /// An owner-authored `[[rules]]` match. Owner policy, so not second-guessed.
    Rule,
    /// chive's own inference: provenance, or provable-dead.
    Chive,
}

impl Origin {
    /// Whether a scan may re-derive this verdict from scratch.
    ///
    /// Sticky by default for anything the owner authored, recompute for chive's
    /// own inference. Sticky *inferred* is configurable (D22) but is a footgun:
    /// a verdict nothing may revise is a verdict nothing may correct.
    pub fn is_sticky(self) -> bool {
        match self {
            Origin::Owner | Origin::Rule => true,
            Origin::Chive => false,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Origin::Owner => "owner",
            Origin::Rule => "rule",
            Origin::Chive => "chive",
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod verdict_tests {
    use super::*;

    #[test]
    fn only_disposable_is_cleanable() {
        assert!(Verdict::Disposable.is_cleanable());
        assert!(!Verdict::Restorable.is_cleanable());
        assert!(
            !Verdict::Unknown.is_cleanable(),
            "a hole is never cleanable: chive's blindness is not consent"
        );
    }

    #[test]
    fn serialized_names_match_display() {
        for v in [Verdict::Restorable, Verdict::Unknown, Verdict::Disposable] {
            assert_eq!(v.as_str(), v.to_string());
        }
    }

    #[test]
    fn deserializes_every_verdict_spelling() {
        // String parsing belongs to serde, not to a hand-written `FromStr`: five
        // of those existed only to feed the SQLite reader, which had none.
        for v in [Verdict::Restorable, Verdict::Unknown, Verdict::Disposable] {
            let parsed: Verdict = toml::Value::String(v.as_str().into()).try_into().unwrap();
            assert_eq!(parsed, v);
        }
    }

    #[test]
    fn deserializes_from_toml_value() {
        let v: Verdict = toml::Value::String("unknown".into()).try_into().unwrap();
        assert_eq!(v, Verdict::Unknown);
    }

    #[test]
    fn the_retired_status_spellings_do_not_deserialize() {
        // No legacy reader: a catalog carrying the old model must be refused
        // outright rather than quietly reinterpreted.
        for gone in ["not-restorable", "temporary", "orphaned"] {
            let parsed: Result<Verdict, _> = toml::Value::String(gone.into()).try_into();
            assert!(parsed.is_err(), "{gone} must not parse");
        }
    }

    #[test]
    fn owner_origins_are_sticky_and_chives_own_are_not() {
        assert!(Origin::Owner.is_sticky());
        assert!(Origin::Rule.is_sticky());
        assert!(
            !Origin::Chive.is_sticky(),
            "a recomputing scan is what lets chive retract a stale inference"
        );
    }
}
