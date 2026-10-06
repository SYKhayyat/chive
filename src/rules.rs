//! Verdict rules: the owner's own policy, evaluated by a sandboxed script.
//!
//! `temporary` used to do one job — make `clean` useful on day one by
//! auto-classifying thousands of files chive would otherwise have to ask about.
//! Removing it with the rest of the four-status model would leave the owner no
//! way to say "`.apk` files are disposable" except writing thousands of catalog
//! entries by hand, so the capability moved here: the owner states the rule
//! instead of chive guessing it (D21).
//!
//! Rhai rather than Python because chive ships a musl-static binary that its
//! Alpine and Void integration targets depend on, and embedded Python needs
//! glibc's `libpython`. Rhai is also sandboxed — no filesystem, no process
//! spawning — which matters while issue #14 (chive fabricating shell from
//! untrusted data) is open. Rules are a decision surface, not an execution one.

use std::path::Path;

use rhai::{Dynamic, Engine, Scope};

/// A rule runs once per file, so the budget is per-file rather than per-invocation
/// and is far tighter than a hook's would be: 10,000 operations is ample for
/// `path.ends_with(".apk")` and far below what a wedged loop reaches. Rhai
/// counts operations, not seconds, so the bound is not wall-clock.
const MAX_OPERATIONS: u64 = 10_000;

/// Space bounds, which the operation cap does not cover.
///
/// A script can stay inside its operation budget and still exhaust memory: build
/// one string by repeated concatenation and you have allocated gigabytes inside
/// ten thousand operations. Shall sets all three for exactly this reason
/// (`src/core/rhai_stdlib.rs`) — *"without them an approved script can grow one
/// string or array into the gigabytes inside its operation budget and take the run
/// — or the machine — down with it."* A rule here is less trusted than Shall's
/// (no shell, no filesystem), but a runaway is still a runaway.
///
/// Sized for a per-file predicate. Nothing a rule legitimately computes over one
/// path approaches these.
const MAX_STRING_BYTES: usize = 1 << 20; // 1 MiB
const MAX_ARRAY_ITEMS: usize = 100_000;
const MAX_MAP_ITEMS: usize = 100_000;

use crate::error::{Error, Result};
use crate::model::Verdict;

/// One configured rule.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// A label for `status` output and error messages. Not evaluated.
    #[serde(default)]
    pub name: String,
    /// The script body. Returns a verdict string, or `()` for no opinion.
    pub script: String,
}

impl Rule {
    fn label(&self, i: usize) -> String {
        if self.name.is_empty() {
            format!("rule {i}")
        } else {
            self.name.clone()
        }
    }
}

/// What a rule script is shown about one file. The fields are the facts a
/// filename cannot express — whether a symlink resolves, which package owns the
/// file — which is why this replaced a name-based heuristic rather than merely
/// deleting it.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub path: String,
    pub size: i64,
    pub extension: String,
    pub is_symlink: bool,
    pub link_resolves: bool,
    pub package: String,
}

/// The compiled rule set. Scripts are compiled once per scan rather than once
/// per file: a 12,000-file scan evaluates every rule 12,000 times, and parsing
/// the same source each time is pure waste.
pub struct Rules {
    engine: Engine,
    /// The label is an owned `String` and handed back as `&str`; a rule's name
    /// outlives the scan because the compiled set does.
    scripts: Vec<(String, rhai::AST)>,
}

impl Rules {
    /// Compile every rule once. A script that does not compile is a config error
    /// reported at startup, not a per-file failure discovered halfway through a
    /// scan — a rule that silently stops matching would quietly change verdicts.
    pub fn compile(rules: &[Rule]) -> Result<Rules> {
        let mut engine = Engine::new();
        // Operations bound *time*; the three below bound *space*. Both are
        // needed, and having only the first is the gap Shall's stdlib names.
        engine.set_max_operations(MAX_OPERATIONS);
        engine.set_max_string_size(MAX_STRING_BYTES);
        engine.set_max_array_size(MAX_ARRAY_ITEMS);
        engine.set_max_map_size(MAX_MAP_ITEMS);
        // No filesystem, no process, no network: the sandbox is the point, so
        // nothing here widens it.
        engine.disable_symbol("eval");
        let mut scripts = Vec::with_capacity(rules.len());
        for (i, r) in rules.iter().enumerate() {
            let ast = engine
                .compile(&r.script)
                .map_err(|e| Error::Config(format!("{}: {e}", r.label(i))))?;
            scripts.push((r.label(i), ast));
        }
        Ok(Rules { engine, scripts })
    }

    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }

    /// The first rule with an opinion wins, so rule order in the config is the
    /// precedence the owner wrote.
    pub fn evaluate<'r>(&'r self, facts: &Facts) -> Result<Option<(Verdict, &'r str)>> {
        if self.scripts.is_empty() {
            return Ok(None);
        }
        let mut scope = Scope::new();
        scope.push("path", facts.path.clone());
        scope.push("size", facts.size);
        scope.push("extension", facts.extension.clone());
        scope.push("is_symlink", facts.is_symlink);
        scope.push("link_resolves", facts.link_resolves);
        scope.push("package", facts.package.clone());

        for (label, ast) in &self.scripts {
            let out: Dynamic = self
                .engine
                .eval_ast_with_scope(&mut scope, ast)
                .map_err(|e| Error::Config(format!("{label}: {e}")))?;
            if let Some(v) = as_verdict(&out)? {
                return Ok(Some((v, label)));
            }
        }
        Ok(None)
    }
}

/// Interpret a script's return value.
///
/// A unit (`()`) means "no opinion" and moves to the next rule. A verdict string
/// claims the path. Anything else is an error rather than a shrug: a rule that
/// returns the wrong thing would otherwise pass silently and quietly change
/// every verdict in the catalog.
fn as_verdict(value: &Dynamic) -> Result<Option<Verdict>> {
    if value.is_unit() {
        return Ok(None);
    }
    let rendered = value.clone().into_string().unwrap_or_default();
    match rendered.as_str() {
        "restorable" => Ok(Some(Verdict::Restorable)),
        "unknown" => Ok(Some(Verdict::Unknown)),
        "disposable" => Ok(Some(Verdict::Disposable)),
        other => Err(Error::Config(format!(
            "rule returned {other:?}; expected \"restorable\", \"unknown\", \
             \"disposable\", or () for no opinion"
        ))),
    }
}

/// The facts a rule sees, gathered from a path on disk.
pub fn facts_for(abs: &Path, rel: &str, size: i64, package: Option<&str>) -> Facts {
    let meta = std::fs::symlink_metadata(abs).ok();
    let is_symlink = meta.as_ref().is_some_and(|m| m.file_type().is_symlink());
    Facts {
        path: rel.to_string(),
        size,
        extension: rel
            .rsplit('.')
            .next()
            .filter(|e| *e != rel)
            .unwrap_or_default()
            .to_ascii_lowercase(),
        is_symlink,
        link_resolves: crate::provenance::dead::link_resolves_for_rule(abs),
        package: package.unwrap_or_default().to_string(),
    }
}

#[cfg(test)]
mod rules_tests {
    use super::*;

    fn rules(scripts: &[&str]) -> Rules {
        let owned: Vec<Rule> = scripts
            .iter()
            .enumerate()
            .map(|(i, s)| Rule {
                name: format!("r{i}"),
                script: (*s).to_string(),
            })
            .collect();
        Rules::compile(&owned).unwrap()
    }

    fn facts(path: &str) -> Facts {
        Facts {
            path: path.into(),
            size: 10,
            extension: path.rsplit('.').next().unwrap_or_default().to_string(),
            ..Facts::default()
        }
    }

    #[test]
    fn a_matching_rule_assigns_its_verdict() {
        let r = rules(&[r#"if path.ends_with(".apk") { "disposable" } else { () }"#]);
        assert_eq!(
            r.evaluate(&facts("pkg.apk")).unwrap().unwrap().0,
            Verdict::Disposable
        );
    }

    #[test]
    fn a_rule_that_has_no_opinion_returns_nothing() {
        let r = rules(&[r#"if path.ends_with(".apk") { "disposable" } else { () }"#]);
        assert_eq!(r.evaluate(&facts("notes.md")).unwrap(), None);
    }

    #[test]
    fn the_first_rule_with_an_opinion_wins() {
        let r = rules(&[
            r#"if path.ends_with(".apk") { "disposable" } else { () }"#,
            r#""unknown""#,
        ]);
        assert_eq!(
            r.evaluate(&facts("pkg.apk")).unwrap().unwrap().0,
            Verdict::Disposable,
            "an earlier match must not be overridden by a later catch-all"
        );
    }

    #[test]
    fn no_rules_means_no_opinion() {
        let r = rules(&[]);
        assert!(r.is_empty());
        assert_eq!(r.evaluate(&facts("anything")).unwrap(), None);
    }

    #[test]
    fn rules_can_return_any_of_the_three_verdicts() {
        // One rule each, so no earlier match masks a later one.
        for (script, want) in [
            (r#""restorable""#, Verdict::Restorable),
            (r#""unknown""#, Verdict::Unknown),
            (r#""disposable""#, Verdict::Disposable),
        ] {
            let r = rules(&[script]);
            assert_eq!(
                r.evaluate(&facts("x")).unwrap().unwrap().0,
                want,
                "a rule must be able to return {want}"
            );
        }
    }

    #[test]
    fn a_rule_sees_facts_a_filename_cannot_express() {
        let r = rules(&[r#"if is_symlink && !link_resolves { "disposable" } else { () }"#]);
        let mut f = facts("link");
        f.is_symlink = true;
        f.link_resolves = false;
        assert_eq!(
            r.evaluate(&f).unwrap().unwrap().0,
            Verdict::Disposable,
            "a rule may act on link resolution, which the old heuristic could not"
        );
    }

    #[test]
    fn a_script_that_does_not_compile_is_refused_at_load() {
        // Failing here rather than per file keeps a typo from silently changing
        // verdicts halfway through a scan.
        let bad = vec![Rule {
            name: "typo".into(),
            script: "if { ".into(),
        }];
        assert!(Rules::compile(&bad).is_err());
    }

    #[test]
    fn a_rule_returning_nonsense_is_an_error_not_a_shrug() {
        let r = rules(&[r#""probably fine""#]);
        assert!(
            r.evaluate(&facts("x")).is_err(),
            "a wrong return value must not pass silently"
        );
    }

    /// Assert a runaway was stopped and that the failure names a bound, without
    /// asserting *which* one fired.
    ///
    /// I checked which fires: for a loop, the operation cap wins, because each
    /// iteration is an operation. The space caps are for the other shape — a
    /// script that allocates a great deal in few operations — and a test that
    /// claimed to be exercising the string cap while the operation cap did the
    /// work would be asserting a mechanism it never reached.
    fn assert_runaway_stopped(script: &str) {
        let r = rules(&[script]);
        let err = r
            .evaluate(&facts("x"))
            .expect_err("a runaway script must be stopped");
        let msg = err.to_string().to_lowercase();
        assert!(
            [
                "operation",
                "size",
                "string",
                "array",
                "map",
                "memory",
                "limit"
            ]
            .iter()
            .any(|w| msg.contains(w)),
            "the failure must name the bound it hit, got {err}"
        );
    }

    #[test]
    fn a_runaway_loop_is_stopped_and_says_which_bound() {
        assert_runaway_stopped(
            r#"
            let s = "";
            for i in 0..100000 { s += "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; }
            s
        "#,
        );
    }

    #[test]
    fn a_runaway_array_is_stopped_and_says_which_bound() {
        assert_runaway_stopped(
            r#"
            let a = [];
            for i in 0..1000000 { a.push(i); }
            "disposable"
        "#,
        );
    }

    #[test]
    fn an_ordinary_rule_is_nowhere_near_any_bound() {
        // Guards the other direction: a bound set so tight that real rules trip it
        // is a bound nobody will trust.
        let r = rules(&[r#"if path.ends_with(".apk") && size > 0 { "disposable" } else { () }"#]);
        let mut f = facts("pkg.apk");
        f.size = 1024;
        assert_eq!(r.evaluate(&f).unwrap().unwrap().0, Verdict::Disposable);
    }

    #[test]
    fn a_rule_cannot_touch_the_filesystem() {
        let r = rules(&[r#"read_dir("/")"#]);
        // Compilation may succeed for an unknown symbol; evaluation must not,
        // and it must not read anything.
        assert!(
            r.evaluate(&facts("x")).is_err(),
            "the sandbox must refuse filesystem access"
        );
    }

    #[test]
    fn the_extension_fact_is_lowercased() {
        let dir = tempfile::tempdir().unwrap();
        let abs = dir.path().join("FILE.APK");
        std::fs::write(&abs, "x").unwrap();
        let f = facts_for(&abs, "FILE.APK", 1, None);
        assert_eq!(f.extension, "apk");
    }
}
