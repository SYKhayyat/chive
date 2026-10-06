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
        s.contains("unknown"),
        "an unexplained file is a hole (D19): notes.md has no provenance\n{s}"
    );
    assert!(
        s.contains("unknown"),
        "and a name the retired heuristic guessed about is a hole too: scratch~\n{s}"
    );

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

    // Teach an unexplained file; it becomes restorable and owner-decided.
    env.chive()
        .args(["teach", "conf.ini"])
        .args(["--method", "cp ~/seed/conf.ini '{dest}'"])
        .assert()
        .success();
    let status = env.chive().arg("status").assert().success();
    let s = String::from_utf8_lossy(&status.get_output().stdout);
    assert!(
        s.contains("restorable") && s.contains("owner"),
        "taught recipe is an owner-decided restorable:\n{s}"
    );
}

#[test]
fn holes_leads_with_what_cannot_be_rebuilt() {
    // D23: the loop the product exists to support had no verb.
    let env = Env::new();
    env.put("small.nef", "a");
    env.put("big.nef", &"x".repeat(4096));
    env.put("notes.md", &"y".repeat(2048));
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();
    env.chive()
        .args(["teach", "notes.md"])
        .args(["--method", "echo x > '{dest}'"])
        .assert()
        .success();

    let out = env.chive().args(["holes"]).assert().success();
    let s = String::from_utf8_lossy(&out.get_output().stdout);
    assert!(s.contains("2 holes"), "holes counted:\n{s}");
    assert!(s.contains("big.nef"), "the big hole is listed:\n{s}");
    assert!(
        !s.contains("notes.md"),
        "a restorable path is not a hole:\n{s}"
    );
    let big = s.find("big.nef").unwrap();
    let small = s.find("small.nef").unwrap();
    assert!(big < small, "largest hole first:\n{s}");
}

#[test]
fn dispose_makes_a_path_cleanable_and_it_survives_reload() {
    let env = Env::new();
    env.put("photo.nef", "bytes");
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();

    env.chive()
        .args(["dispose", "photo.nef"])
        .assert()
        .success();

    let status = env.chive().arg("status").assert().success();
    let s = String::from_utf8_lossy(&status.get_output().stdout);
    assert!(
        s.contains("disposable") && s.contains("owner"),
        "the disposal persisted and is owner-owned:\n{s}"
    );
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

    let dry = env.chive().args(["clean", "--dry-run"]).assert().success();
    let s = String::from_utf8_lossy(&dry.get_output().stdout);
    assert!(
        s.contains("Would remove 0"),
        "nothing is cleanable until something says so (D19):\n{s}"
    );

    // so the owner must say so
    env.chive().args(["dispose", "trash~"]).assert().success();
    let dry = env.chive().args(["clean", "--dry-run"]).assert().success();
    let s = String::from_utf8_lossy(&dry.get_output().stdout);
    assert!(
        s.contains("trash~"),
        "dry-run lists the disposed file:\n{s}"
    );
    assert!(
        !s.contains("keep.md"),
        "dry-run must not list the restorable file"
    );

    env.chive().args(["clean", "--force"]).assert().success();
    assert!(
        !env.tree.path().join("trash~").exists(),
        "the disposed file was removed"
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
fn status_filter_selects_by_verdict() {
    let env = Env::new();
    env.put("done.md", "x");
    env.put("temp~", "y");
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();
    env.chive()
        .args(["teach", "done.md"])
        .args(["--method", "echo X > '{dest}'"])
        .assert()
        .success();

    let restorable = env
        .chive()
        .arg("status")
        .arg("--restorable")
        .assert()
        .success();
    let s = String::from_utf8_lossy(&restorable.get_output().stdout);
    assert!(s.contains("done.md"));
    assert!(!s.contains("temp~"), "--restorable hides holes:\n{s}");

    let unknown = env
        .chive()
        .arg("status")
        .arg("--unknown")
        .assert()
        .success();
    let s = String::from_utf8_lossy(&unknown.get_output().stdout);
    assert!(s.contains("temp~"), "a hole shows under --unknown:\n{s}");
}

#[test]
fn config_show_reports_every_setting_and_its_default() {
    // The settings have to be discoverable: a policy nobody can find is a policy
    // nobody can change (the lamdan audit's region-1 finding).
    let env = Env::new();
    let out = env.chive().args(["config", "show"]).assert().success();
    let s = String::from_utf8_lossy(&out.get_output().stdout);
    for expected in [
        "policy.restore.overwrite",
        "Refuse",
        "backup",
        "policy.catalog.root_scope",
        "Warn",
        "home-only",
        "ignore:",
        "rules:",
    ] {
        assert!(
            s.contains(expected),
            "config show must mention {expected}:\n{s}"
        );
    }
    assert!(
        s.contains("not written yet"),
        "with no file, say so rather than implying one was read: {s}"
    );
}

#[test]
fn config_init_writes_a_template_that_changes_nothing() {
    let env = Env::new();
    env.chive().args(["config", "init"]).assert().success();

    let path = env.conf.join("config.toml");
    let text = std::fs::read_to_string(&path).unwrap();
    // Every judgement call stays commented out, so writing the file cannot
    // silently change behaviour -- the reason Shall's template does the same.
    assert!(text.contains("# overwrite = \"refuse\""));
    assert!(text.contains("# root_scope = \"warn\""));
    for setting in ["overwrite", "root_scope"] {
        let active = text
            .lines()
            .any(|l| !l.trim_start().starts_with('#') && l.contains(setting));
        assert!(!active, "{setting} must not be set by the template");
    }
    assert!(
        text.contains("[[rules]]"),
        "the template shows the rule form"
    );

    // and chive still reads its defaults out of it
    let out = env.chive().args(["config", "show"]).assert().success();
    let s = String::from_utf8_lossy(&out.get_output().stdout);
    assert!(s.contains("Refuse") && s.contains("Warn"), "{s}");
}

#[test]
fn config_init_refuses_to_clobber_an_existing_file() {
    let env = Env::new();
    env.chive().args(["config", "init"]).assert().success();
    let path = env.conf.join("config.toml");
    std::fs::write(&path, "# mine\n").unwrap();

    env.chive().args(["config", "init"]).assert().code(3);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# mine\n",
        "a refused init must not touch the file"
    );
    env.chive()
        .args(["config", "init", "--force"])
        .assert()
        .success();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("chive settings")
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

#[test]
fn restore_all_is_the_documented_spelling_of_the_default() {
    // Issue #27: the README documents `chive restore --all`, but the flag did
    // not exist. It must parse and behave exactly like a bare `restore`.
    let env = Env::new();
    env.put("seed/a.txt", "A");
    env.put("conf/a.txt", "old");
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();
    env.chive()
        .args(["teach", "conf/a.txt"])
        .args([
            "--method",
            "cp \"$(dirname '{dest}')/../seed/a.txt\" '{dest}'",
        ])
        .assert()
        .success();
    // remove the dest so restore has work to do
    std::fs::remove_file(env.tree.path().join("conf/a.txt")).unwrap();

    let restore = env
        .chive()
        .args(["restore", "--all", "--root"])
        .arg(env.tree.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let out = String::from_utf8_lossy(&restore);
    assert!(
        out.contains("restored: conf/a.txt"),
        "--all restored the taught file:\n{out}"
    );
    assert_eq!(
        std::fs::read_to_string(env.tree.path().join("conf/a.txt")).unwrap(),
        "A"
    );

    // --all is mutually exclusive with explicit paths
    env.chive()
        .args(["restore", "--all", "conf/a.txt", "--root"])
        .arg(env.tree.path())
        .assert()
        .code(2);
}

#[test]
fn plan_restore_all_also_parses() {
    let env = Env::new();
    env.put("notes.md", "x");
    env.chive()
        .arg("scan")
        .arg(env.tree.path())
        .assert()
        .success();
    env.chive()
        .args(["teach", "notes.md"])
        .args(["--method", "echo X > '{dest}'"])
        .assert()
        .success();
    env.chive()
        .args(["plan", "restore", "--all", "--root"])
        .arg(env.tree.path())
        .assert()
        .success();
}
