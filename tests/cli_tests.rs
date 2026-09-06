//! End-to-end CLI tests: run the real binary against an isolated tree and store.
//!
//! Each test builds its own scratch tree and config dir, so no test depends on
//! the host machine's actual packages, home contents, or prior runs.

use std::path::PathBuf;

use assert_cmd::Command;
use tempfile::TempDir;

/// A scratch environment: a config store plus a tree to scan.
struct Env {
    _store: TempDir,
    tree: TempDir,
    conf: PathBuf,
}

impl Env {
    fn new() -> Env {
        let store = tempfile::tempdir().unwrap();
        let tree_d = tempfile::tempdir().unwrap();
        let conf = store.path().to_path_buf();
        Env {
            _store: store,
            tree: tree_d,
            conf,
        }
    }

    fn chive(&self) -> Command {
        let mut c = Command::cargo_bin("chive").expect("binary built");
        c.env("CHIVE_CONFIG_DIR", &self.conf);
        c
    }

    fn put(&self, rel: &str, contents: &str) {
        let p = self.tree.path().join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, contents).unwrap();
    }
}

#[test]
fn scan_then_status_and_stats() {
    let env = Env::new();
    env.put("notes.md", "hello");
    env.put("scratch~", "tmp");

    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();

    let status = env.chive().arg("status").assert().success();
    let out = status.get_output().stdout.clone();
    let s = String::from_utf8_lossy(&out);
    assert!(s.contains("notes.md"), "status lists notes.md:\n{s}");
    assert!(
        s.contains("orphaned"),
        "notes.md is orphaned without provenance"
    );
    assert!(s.contains("temporary"), "scratch~ is temporary");

    let stats = env.chive().arg("stats").assert().success();
    let so = String::from_utf8_lossy(&stats.get_output().stdout);
    assert!(
        so.contains("Catalog: 2 files"),
        "stats counts the catalog:\n{so}"
    );
}

#[test]
fn scan_respects_ignore_and_teach_overrules() {
    let env = Env::new();
    env.put("node_modules/junk.js", "x");
    env.put("conf.ini", "port=1");

    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();

    let status = env.chive().arg("status").assert().success();
    let s = String::from_utf8_lossy(&status.get_output().stdout);
    assert!(!s.contains("junk.js"), "node_modules is ignored:\n{s}");
    assert!(s.contains("conf.ini"));

    // Teach an inferred->orphaned file; it becomes restorable (user_supplied).
    env.chive()
        .args(["teach", "conf.ini"])
        .args(["--method", "cp ~/seed/conf.ini '{dest}'"])
        .assert()
        .success();
    let status = env.chive().arg("status").assert().success();
    let s = String::from_utf8_lossy(&status.get_output().stdout);
    assert!(
        s.contains("user_supplied"),
        "taught recipe is user_supplied:\n{s}"
    );
}

#[test]
fn mark_changes_status_and_survives_reload() {
    let env = Env::new();
    env.put("photo.nef", "bytes");
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();

    env.chive()
        .args(["mark", "photo.nef", "--status", "not-restorable"])
        .assert()
        .success();

    let status = env.chive().arg("status").assert().success();
    let s = String::from_utf8_lossy(&status.get_output().stdout);
    assert!(s.contains("not-restorable"), "mark persisted:\n{s}");
}

#[test]
fn clean_dry_run_and_force() {
    let env = Env::new();
    // Restorable file must survive clean; a temporary file must not.
    env.put("keep.md", "keep");
    env.put("trash~", "remove me");
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();
    env.chive()
        .args(["teach", "keep.md"])
        .args(["--method", "echo X > '{dest}'"])
        .assert()
        .success();

    let dry = env
        .chive()
        .args(["clean", "--scope", "both", "--dry-run"])
        .assert()
        .success();
    let s = String::from_utf8_lossy(&dry.get_output().stdout);
    assert!(s.contains("trash~"), "dry-run lists the temp file:\n{s}");
    assert!(
        !s.contains("keep.md"),
        "dry-run must not list the restorable file"
    );

    env.chive()
        .args(["clean", "--scope", "both", "--force"])
        .assert()
        .success();
    assert!(
        !env.tree.path().join("trash~").exists(),
        "temp file was removed"
    );
    assert!(
        env.tree.path().join("keep.md").exists(),
        "restorable survived clean"
    );
}

#[test]
fn export_import_round_trip_across_stores() {
    let env_a = Env::new();
    env_a.put("a.md", "x");
    env_a
        .chive()
        .arg("scan")
        .arg(env_a.tree.path())
        .assert()
        .success();

    let exported = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
    env_a
        .chive()
        .arg("export")
        .arg("--to")
        .arg(&exported)
        .assert()
        .success();

    // Import into a fresh store.
    let env_b = Env::new();
    env_b
        .chive()
        .arg("import")
        .arg("--from")
        .arg(&exported)
        .assert()
        .success();
    let status = env_b.chive().arg("status").assert().success();
    let s = String::from_utf8_lossy(&status.get_output().stdout);
    assert!(s.contains("a.md"), "imported catalog has the file:\n{s}");
}

#[test]
fn plan_and_restore_preview_and_rebuild() {
    let env = Env::new();
    env.put("notes.md", "x");
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();

    // Give it a recipe so it is restorable.
    env.chive()
        .args(["teach", "notes.md"])
        .args(["--method", "echo 'X' > '{dest}'"])
        .assert()
        .success();

    let plan = env
        .chive()
        .args(["plan", "restore", "--root"])
        .arg(env.tree.path())
        .assert()
        .success();
    let s = String::from_utf8_lossy(&plan.get_output().stdout);
    assert!(
        s.contains("Plan: 1 file(s)"),
        "plan previews one file:\n{s}"
    );
    assert!(s.contains("notes.md"), "plan mentions the file:\n{s}");

    // Restore refuses because the dest already exists (no clobber).
    let restore = env
        .chive()
        .args(["restore", "--root"])
        .arg(env.tree.path())
        .assert()
        .success();
    let s = String::from_utf8_lossy(&restore.get_output().stdout);
    assert!(
        s.contains("skipped (exists)"),
        "existing dest is not clobbered:\n{s}"
    );
}

#[test]
fn missing_catalog_errors_explicitly() {
    let env = Env::new();
    env.chive().arg("status").assert().code(4);
}

#[test]
fn help_and_version_exit_zero() {
    let env = Env::new();
    env.chive().arg("--help").assert().success();
    env.chive().arg("--version").assert().success();
}
