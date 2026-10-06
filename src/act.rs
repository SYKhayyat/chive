//! The owner act log: the durable, ordered record of what the owner decided.
//!
//! Every owner act — `teach`, `dispose`, `withdraw` — is an entry here with a
//! monotonic sequence number, and it lives in the catalog rather than in a
//! sidecar file or the config. Two properties follow, and both were data-loss
//! bugs before (issue #43):
//!
//! - The newest act for a path governs, so "the last thing I said wins" is the
//!   entire precedence rule. There is no table of which class of decision
//!   outranks which, and none is needed.
//! - A scan *reads* this log and re-applies it. The scanner is downstream of the
//!   record, so a rescan cannot erase or reorder an act.
//!
//! The log is also what makes a verdict travel: the catalog is the file a new
//! machine imports, so a judgement made here arrives there (D20). A verdict kept
//! in `config.toml` would stay behind with the old machine.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// What an act does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActKind {
    /// The owner supplied a recipe. Makes the path `restorable`.
    Teach,
    /// The owner judged the path a known gap. Makes it `disposable`.
    Dispose,
    /// Take back the owner's decision; the path re-derives from evidence.
    Withdraw,
}

impl std::str::FromStr for ActKind {
    type Err = ();

    fn from_str(s: &str) -> std::result::Result<Self, ()> {
        match s {
            "teach" => Ok(ActKind::Teach),
            "dispose" => Ok(ActKind::Dispose),
            "withdraw" => Ok(ActKind::Withdraw),
            _ => Err(()),
        }
    }
}

impl ActKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActKind::Teach => "teach",
            ActKind::Dispose => "dispose",
            ActKind::Withdraw => "withdraw",
        }
    }

    /// Whether this act pins a verdict, or merely releases the path to re-derive.
    pub fn is_binding(self) -> bool {
        !matches!(self, ActKind::Withdraw)
    }
}

/// One owner act.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Act {
    pub seq: u64,
    pub path: String,
    pub kind: ActKind,
    /// The recipe, for a `teach` act. Carried in the act so the log alone is
    /// enough to re-apply the decision — a taught recipe cannot be lost to a
    /// rescan, and it reaches a new machine through `export` with no join
    /// against some other file (issue #45).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
}

impl Act {
    pub fn teach(seq: u64, path: impl Into<String>, method: impl Into<String>) -> Self {
        Act {
            seq,
            path: path.into(),
            kind: ActKind::Teach,
            method: Some(method.into()),
        }
    }

    pub fn dispose(seq: u64, path: impl Into<String>) -> Self {
        Act {
            seq,
            path: path.into(),
            kind: ActKind::Dispose,
            method: None,
        }
    }

    pub fn withdraw(seq: u64, path: impl Into<String>) -> Self {
        Act {
            seq,
            path: path.into(),
            kind: ActKind::Withdraw,
            method: None,
        }
    }
}

/// The ordered log, plus the sequence counter that hands out the next number.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ActLog {
    /// Sorted by `seq`, so the newest act for a path is the last one appended.
    acts: Vec<Act>,
    next_seq: u64,
}

impl ActLog {
    pub fn new(acts: Vec<Act>, next_seq: u64) -> Self {
        let mut acts = acts;
        acts.sort_by_key(|a| a.seq);
        // A log claiming a next_seq at or below a used seq would hand out a
        // duplicate, which would make "newest wins" ambiguous. An empty log has
        // no used seq, so its counter is left exactly as given.
        let next_seq = match acts.iter().map(|a| a.seq).max() {
            Some(highest) => next_seq.max(highest + 1),
            None => next_seq,
        };
        ActLog { acts, next_seq }
    }

    pub fn acts(&self) -> &[Act] {
        &self.acts
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    pub fn is_empty(&self) -> bool {
        self.acts.is_empty()
    }

    pub fn len(&self) -> usize {
        self.acts.len()
    }

    /// The newest act for `path`, which is the one that governs it.
    pub fn latest(&self, path: &str) -> Option<&Act> {
        self.acts.iter().rev().find(|a| a.path == path)
    }

    /// Append an act, assigning it the next sequence number. Returns it.
    pub fn append(&mut self, mut act: Act) -> Act {
        act.seq = self.next_seq;
        self.next_seq += 1;
        let recorded = act.clone();
        self.acts.push(act);
        recorded
    }

    /// Every path the owner has acted on, mapped to its governing act.
    pub fn governing(&self) -> BTreeMap<&str, &Act> {
        let mut out: BTreeMap<&str, &Act> = BTreeMap::new();
        for act in &self.acts {
            out.insert(act.path.as_str(), act);
        }
        out
    }

    /// The teaching acts keyed by path, for the paths whose newest act is a
    /// `teach`. A `dispose` after a `teach` hides the recipe, so this is not
    /// simply "every teach in the log".
    pub fn active_recipes(&self) -> BTreeMap<&str, &str> {
        self.governing()
            .into_iter()
            .filter(|(_, a)| a.kind == ActKind::Teach)
            .filter_map(|(p, a)| a.method.as_deref().map(|m| (p, m)))
            .collect()
    }
}

/// Reject a catalog whose act log cannot be applied unambiguously. Two sequence
/// numbers sharing a value would make "newest wins" a coin flip, so this fails
/// at the boundary rather than silently picking one.
pub fn check_unique_seqs(acts: &[Act]) -> Result<()> {
    let mut seen = std::collections::HashSet::new();
    for a in acts {
        if !seen.insert(a.seq) {
            return Err(Error::Catalog(format!(
                "act log has two acts with seq {} ({} {}): newest-wins would be ambiguous",
                a.seq,
                a.kind.as_str(),
                a.path
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod act_log_tests {
    use super::*;

    #[test]
    fn append_hands_out_increasing_sequence_numbers() {
        let mut log = ActLog::default();
        log.append(Act::teach(0, "a", "echo a"));
        log.append(Act::dispose(0, "b"));
        let seqs: Vec<_> = log.acts().iter().map(|a| a.seq).collect();
        assert_eq!(seqs, vec![0, 1]);
    }

    #[test]
    fn the_newest_act_for_a_path_governs() {
        let mut log = ActLog::default();
        log.append(Act::teach(0, "x", "echo x"));
        log.append(Act::dispose(0, "x"));
        assert_eq!(log.latest("x").unwrap().kind, ActKind::Dispose);
    }

    #[test]
    fn a_withdraw_releases_the_path_to_re_derive() {
        let mut log = ActLog::default();
        log.append(Act::dispose(0, "x"));
        log.append(Act::withdraw(0, "x"));
        let latest = log.latest("x").unwrap();
        assert_eq!(latest.kind, ActKind::Withdraw);
        assert!(!latest.kind.is_binding(), "withdraw must not pin a verdict");
        assert!(
            log.active_recipes().is_empty(),
            "a withdrawn path has no active recipe"
        );
    }

    #[test]
    fn a_dispose_after_a_teach_hides_the_recipe() {
        let mut log = ActLog::default();
        log.append(Act::teach(0, "x", "echo x"));
        log.append(Act::dispose(0, "x"));
        assert!(
            log.active_recipes().is_empty(),
            "the later disposal governs, so the recipe is not active"
        );
    }

    #[test]
    fn construction_sorts_by_sequence() {
        let log = ActLog::new(vec![Act::dispose(9, "b"), Act::teach(2, "a", "echo a")], 10);
        let seqs: Vec<_> = log.acts().iter().map(|a| a.seq).collect();
        assert_eq!(seqs, vec![2, 9]);
    }

    #[test]
    fn an_empty_log_keeps_its_counter() {
        let log = ActLog::new(vec![], 0);
        assert_eq!(log.next_seq(), 0);
    }

    #[test]
    fn next_seq_is_never_reissued() {
        // A hand-edited catalog can carry a stale counter; the log must not hand
        // out a sequence number that already exists, or "newest wins" breaks.
        let log = ActLog::new(vec![Act::dispose(7, "b")], 1);
        assert_eq!(log.next_seq(), 8);
        assert!(check_unique_seqs(log.acts()).is_ok());
    }

    #[test]
    fn duplicate_sequence_numbers_are_refused() {
        let acts = vec![Act::dispose(3, "a"), Act::dispose(3, "b")];
        assert!(check_unique_seqs(&acts).is_err());
    }
}
