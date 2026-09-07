//! The two owner verbs — `mark` and `teach` — and that their rulings survive a
//! reload (the catalog is rebuilt from TOML each command, so persistence means
//! the TOML/SQLite pair round-trips).
//!
//!   mark  -> sets an explicit status (not-restorable / temporary / orphaned)
//!   teach -> promotes ANY status to restorable (user_supplied), and wins over
//!            a scan-time inference (D16).

use crate::harness::Env;

#[test]
fn mark_sets_status_and_it_survives_reload() {
    let env = Env::new("mark_reload");
    env.put("photo.nef", "bytes");
    env.ok(&["scan", env.home.to_str().unwrap()]);

    // an orphaned scan-unknown is marked not-restorable -> protected from clean
    env.ok(&["mark", "photo.nef", "--status", "not-restorable"]);

    let s = env.ok(&["status"]);
    assert!(
        s.contains("not-restorable") && s.contains("photo.nef"),
        "mark persisted:\n{s}"
    );

    // and clean must refuse to touch it
    let clean = env.ok(&["clean", "--scope", "both", "--dry-run"]);
    assert!(
        !clean.contains("photo.nef"),
        "clean must never list a not-restorable file:\n{clean}"
    );
}

#[test]
fn teach_ends_orphanhood_and_overrules_inference_after_reload() {
    let env = Env::new("teach_reload");
    env.put("conf/init.el", "(init)");
    env.ok(&["scan", env.home.to_str().unwrap()]);

    // teach promotes the orphaned file to restorable(user_supplied)
    env.ok(&[
        "teach",
        "conf/init.el",
        "--method",
        "cp ~/seed/init.el '{dest}'",
    ]);
    let s = env.ok(&["status"]);
    assert!(
        s.contains("user_supplied") && s.contains("conf/init.el"),
        "taught file is user_supplied restorable:\n{s}"
    );
}

#[test]
fn mark_clears_a_recipe_so_a_taught_file_can_be_unrestorable_again() {
    // D16 + the unified mark: marking clears the recipe; teaching then mark
    // round-trips a file out of restorable.
    let env = Env::new("mark_clears_recipe");
    env.put("conf/x.toml", "x=1");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&[
        "teach",
        "conf/x.toml",
        "--method",
        "cp ~/seed/x.toml '{dest}'",
    ]);
    let s = env.ok(&["status"]);
    assert!(s.contains("conf/x.toml"), "{s}");

    env.ok(&["mark", "conf/x.toml", "--status", "orphaned"]);
    let s = env.ok(&["status"]);
    // after mark, the file is no longer restorable
    let (out, _) = env.run(&["status", "--restorable"]);
    assert!(
        !out.contains("conf/x.toml"),
        "marked file must not appear as restorable:\n{out}"
    );
    assert!(
        s.contains("orphaned") && s.contains("conf/x.toml"),
        "marked file is a cleanable orphan:\n{s}"
    );
}
