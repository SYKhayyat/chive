//! Owner decisions are durable: a verdict survives a rescan, and the newest act
//! wins.
//!
//! This file replaces the old `mark`-and-teach suite, which pinned a capability
//! one rescan silently revoked. That test was the reason #43 survived: it scoped
//! itself to *reload* and never scanned between the verb and the assertion, so
//! the data-loss chain it sat next to — mark protected → rescan → cleanable →
//! clean deletes it — was never exercised end to end.
//!
//! The verbs here are `teach`, `dispose`, and `withdraw`, recorded as an ordered
//! act log the scanner reads rather than replaces (D20).

use crate::harness::Env;

fn scanned_home(name: &str) -> Env {
    let env = Env::new(name);
    env.put("photo.nef", "bytes");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env
}

#[test]
fn a_disposal_survives_a_rescan_and_clean_still_honours_it() {
    // Issue #43's chain, closed. The half that matters is the end: after a
    // routine rescan, clean must still see the owner's decision.
    let env = scanned_home("dispose_survives_rescan");
    env.ok(&["dispose", "photo.nef"]);

    // a routine rescan, exactly what a user runs without thinking
    env.ok(&["scan", env.home.to_str().unwrap()]);

    let s = env.ok(&["status"]);
    assert!(
        s.contains("disposable") && s.contains("photo.nef"),
        "a rescan must not erase the owner's decision:\n{s}"
    );

    let preview = env.ok(&["clean", "--dry-run"]);
    assert!(
        preview.contains("photo.nef"),
        "the decision is still live after a rescan:\n{preview}"
    );
    env.ok(&["clean", "--force"]);
    assert!(
        !env.home.join("photo.nef").exists(),
        "the file the owner called disposable should be gone"
    );
}

#[test]
fn a_teach_survives_a_rescan() {
    let env = scanned_home("teach_survives_rescan");
    env.ok(&["teach", "photo.nef", "--method", "echo bytes > '{dest}'"]);
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let s = env.ok(&["status"]);
    assert!(
        s.contains("restorable") && s.contains("photo.nef"),
        "a taught recipe must survive a rescan:\n{s}"
    );
    assert!(
        s.contains("echo bytes"),
        "the recipe itself must come back, not just the verdict:\n{s}"
    );
}

#[test]
fn the_newest_act_wins_either_way_round() {
    let env = scanned_home("last_act_wins");
    env.ok(&["teach", "photo.nef", "--method", "echo bytes > '{dest}'"]);
    env.ok(&["dispose", "photo.nef"]);

    let s = env.ok(&["status"]);
    assert!(
        s.contains("disposable"),
        "dispose after teach: the later act governs:\n{s}"
    );
    let restorable = env.ok(&["status", "--restorable"]);
    assert!(
        !restorable.contains("photo.nef"),
        "the hidden recipe must not linger in the restorable set:\n{restorable}"
    );

    // and the reverse order
    env.ok(&["teach", "photo.nef", "--method", "echo bytes > '{dest}'"]);
    let s = env.ok(&["status"]);
    assert!(
        s.contains("restorable"),
        "teach after dispose: the later act governs:\n{s}"
    );
    let preview = env.ok(&["clean", "--dry-run"]);
    assert!(
        !preview.contains("photo.nef"),
        "a path taught after disposal is no longer cleanable:\n{preview}"
    );
}

#[test]
fn a_withdraw_takes_the_judgement_back() {
    let env = scanned_home("withdraw");
    env.ok(&["dispose", "photo.nef"]);
    env.ok(&["withdraw", "photo.nef"]);

    let s = env.ok(&["status"]);
    assert!(
        s.contains("unknown"),
        "withdrawn: nothing here is restorable, so it is a hole again:\n{s}"
    );
    let preview = env.ok(&["clean", "--dry-run"]);
    assert!(
        !preview.contains("photo.nef"),
        "a withdrawn judgement must not leave the path cleanable:\n{preview}"
    );
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let after = env.ok(&["status"]);
    assert!(
        after.contains("unknown"),
        "and the withdrawal must itself survive a rescan:\n{after}"
    );
}

#[test]
fn an_explained_file_stays_restorable_after_a_withdrawal() {
    // The withdrawal needs no rule of its own: recipe present => restorable,
    // absent => unknown. Here the recipe is chive's own, so it remains.
    let env = Env::new("withdraw_keeps_verified_recipe");
    env.put("dotfiles/init.el", "(init)");
    env.git_repo("dotfiles", &["init.el"]);
    env.ok(&["scan", env.home.to_str().unwrap()]);

    let before = env.status_line("dotfiles/init.el");
    assert!(before.contains("restorable"), "{before}");

    env.ok(&["dispose", "dotfiles/init.el"]);
    assert!(env.status_line("dotfiles/init.el").contains("disposable"));

    env.ok(&["withdraw", "dotfiles/init.el"]);
    let after = env.status_line("dotfiles/init.el");
    assert!(
        after.contains("restorable"),
        "withdrawing the judgement leaves chive's own recipe standing:\n{after}"
    );
}

#[test]
fn a_recipe_taught_for_an_absent_path_reaches_a_new_machine() {
    // Issue #45: planning a new machine means teaching for a file that is not
    // here yet. The recipe must survive the archive, not sit in a sidecar file.
    let env = scanned_home("absent_recipe_travels");
    env.ok(&[
        "teach",
        "not/here/yet.conf",
        "--method",
        "echo built > '{dest}'",
    ]);

    let archive = env.root.join("catalog.toml");
    env.ok(&[
        "scan",
        env.home.to_str().unwrap(),
        "--to",
        archive.to_str().unwrap(),
    ]);
    let text = std::fs::read_to_string(&archive).expect("the archive was written");
    assert!(
        text.contains("echo built"),
        "the taught recipe must be in the catalog, not a sidecar:\n{text}"
    );
    assert!(
        text.contains("not/here/yet.conf"),
        "and it must name the absent path:\n{text}"
    );

    // machine B: import the archive into a clean store and find the recipe
    let other = crate::harness::Env::new("absent_recipe_machine_b");
    let (out, code) = other.run(&["import", "--from", archive.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    let s = other.ok(&["status"]);
    assert!(
        s.contains("not/here/yet.conf") && s.contains("echo built"),
        "a new machine must see the recipe for a file it does not have:\n{s}"
    );
    assert!(
        s.contains("not on this machine"),
        "and it must be honest that the file is absent:\n{s}"
    );
}

#[test]
fn a_hole_survives_a_rescan_as_a_hole() {
    // D19's whole point: an unexplained file stays unexplained rather than
    // quietly becoming cleanable on a later scan.
    let env = scanned_home("hole_stays_hole");
    for _ in 0..2 {
        env.ok(&["scan", env.home.to_str().unwrap()]);
    }
    let line = env.status_line("photo.nef");
    assert!(line.contains("unknown"), "{line}");
    let preview = env.ok(&["clean", "--dry-run"]);
    assert!(
        !preview.contains("photo.nef"),
        "repeated scans must never make a hole cleanable:\n{preview}"
    );
}
