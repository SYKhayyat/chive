//! One rule, one owner: what a verb writes must equal what a scan would write.
//!
//! "Apply the log" had four implementations — `cmd_teach`, `cmd_dispose`,
//! `cmd_withdraw` and `Scanner::entry_for` — and they had already drifted
//! (issue #56). The drift was invisible because nothing asserted the property
//! that actually matters: the immediate view and the rescanned view agree.
//!
//! Each test below states that property. They are not "teach works" tests; a
//! version that passes while both owners disagree is the bug, not the fix.

use crate::harness::Env;

/// The invariant, in one place: whatever a verb records, a rescan must not change.
///
/// This is the test the whole issue turns on. Before the fix, teaching a path
/// this machine *has* produced an entry the scanner would never produce, so the
/// two views diverged and only one of them could be right.
fn assert_rederives_identical(env: &Env, path: &str) {
    let before = env.status_line(path);
    let (_, code) = env.run(&["scan", env.home.to_str().unwrap()]);
    assert_eq!(code, 0, "rescan must succeed");
    let after = env.status_line(path);
    assert_eq!(
        before, after,
        "rescan changed the verdict for {path}, so a verb and a scan disagree \
         about the same rule:\n  before: {before}\n   after: {after}"
    );
}

/// Fixed issue #56 — teaching a path that *exists* must record it as present.
///
/// The old `cmd_teach` used `new_absent_restorable` on both branches, so an
/// existing file was stamped `present: false`. `cmd_withdraw` went through the
/// scanner and got `present: true` for the same file.
#[test]
fn teaching_an_existing_file_marks_it_present() {
    let env = Env::new("issue56_teach_present");
    env.put("notes.md", "real content");

    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&[
        "teach",
        "notes.md",
        "--method",
        "echo '{dest}' > content",
    ]);

    let line = env.status_line("notes.md");
    assert!(
        !line.contains("absent"),
        "a file that exists must not be recorded absent:\n{line}"
    );
    assert_rederives_identical(&env, "notes.md");
}

/// The flip side: teaching a path this machine lacks is the core workflow
/// (planning a new machine, issue #45), and it must still be recorded absent.
#[test]
fn teaching_an_absent_path_marks_it_absent() {
    let env = Env::new("issue56_teach_absent");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&[
        "teach",
        "future/thing.conf",
        "--method",
        "echo hi > '{dest}'",
    ]);

    let line = env.status_line("future/thing.conf");
    assert!(
        line.contains("not on this machine"),
        "a path this machine lacks must be recorded absent:\n{line}"
    );
    assert_rederives_identical(&env, "future/thing.conf");
}

/// Fixed issue #56 — disposing an unknown path used to be a silent no-op.
///
/// The old `cmd_dispose` only touched an entry that already existed, so the act
/// landed in the log but the view stayed empty until the next scan. Owner intent
/// that takes a scan to become visible is the exact failure mode #45 was filed
/// against, in the other direction.
#[test]
fn disposing_an_unknown_path_still_records_the_verdict() {
    let env = Env::new("issue56_dispose_unknown");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&["dispose", "never-scanned.txt"]);
    let line = env.status_line("never-scanned.txt");
    assert!(line.contains("disposable"), "got: {line}");
}

#[test]
fn ab_same_body_fresh_name() {
    let env = Env::new("issue56_zz_fresh_name");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&["dispose", "never-scanned.txt"]);
    let line = env.status_line("never-scanned.txt");
    assert!(line.contains("disposable"), "got: {line}");
}

#[test]
fn aa_dispose_via_run() {
    let env = Env::new("issue56_B_run_then_statusline");
    env.run(&["scan", env.home.to_str().unwrap()]);
    env.run(&["dispose", "never-scanned.txt"]);
    let line = env.status_line("never-scanned.txt");
    assert!(line.contains("disposable"), "got: {line}");
}

/// A withdraw releases the path, so the entry must go back to what evidence
/// supports. With no evidence reachable, that is `unknown` — and, crucially, the
/// same answer whether it comes from the verb or from a scan.
#[test]
fn withdrawing_releases_the_verdict_to_evidence() {
    let env = Env::new("issue56_withdraw");
    env.put("junk.tmp", "x");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&["dispose", "junk.tmp"]);
    env.ok(&["withdraw", "junk.tmp"]);

    let line = env.status_line("junk.tmp");
    assert!(
        !line.contains("disposable"),
        "withdraw must take back the owner's verdict:\n{line}"
    );
    assert_rederives_identical(&env, "junk.tmp");
}

/// The three verbs must not disagree about a path they each touch in turn.
///
/// `teach` then `dispose` then `withdraw` exercises the whole log, and the last
/// verb has to be able to see what the first two wrote — which it cannot if each
/// keeps its own copy of the rule.
#[test]
fn successive_acts_leave_one_coherent_entry() {
    let env = Env::new("issue56_successive");
    env.put("conf.toml", "a = 1");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&["teach", "conf.toml", "--method", "echo x > '{dest}'"]);
    env.ok(&["dispose", "conf.toml"]);
    env.ok(&["withdraw", "conf.toml"]);

    let after_teach_dispose = env.status_line("conf.toml");
    assert_rederives_identical(&env, "conf.toml");
    assert!(
        !after_teach_dispose.is_empty(),
        "the entry must survive the whole act sequence"
    );
}
