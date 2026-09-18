//! Regression tests for open issues. Each asserts the *correct* (spec) behaviour
//! and is `#[ignore]`d with the issue number. The suite stays green; running
//! them with `cargo test -- --ignored bug_` shows each one FAIL against today's
//! code, proving the bug, and they turn green the commit the fix lands.
//!
//!   cargo test --test main -- --ignored bug_regressions

use crate::harness::Env;

/// Fixed issue #19 (dup #29) — git provenance must work for tracked files not
/// at the repo root (the detector asked git for the basename, not the path
/// relative to the repo).
#[test]
fn bug_git_nested_files_are_restorable() {
    let env = Env::new("bug_git_nested");
    let home = &env.home;
    std::fs::create_dir_all(home.join("repo/nested")).unwrap();
    std::fs::write(home.join("repo/root.md"), "r").unwrap();
    std::fs::write(home.join("repo/nested/deep.toml"), "d").unwrap();
    env.git_repo("repo", &["root.md", "nested/deep.toml"]);

    env.ok(&["scan", env.home.to_str().unwrap()]);

    let nested = env.status_line("repo/nested/deep.toml");
    assert!(
        nested.contains("restorable") && nested.contains("verified"),
        "nested git file should be restorable(verified), got:\n{nested}"
    );
}

/// Fixed issue #17 — catalog paths are contained: `import` refuses any entry
/// whose path could address a file outside the scan root, so restore/clean can
/// never be pointed outside the tree the catalog describes.
#[test]
fn import_rejects_paths_outside_the_root() {
    let env = Env::new("bug_traversal");
    std::fs::create_dir_all(env.home.join("tree")).unwrap();

    // a crafted catalog placing a file outside the scan root
    let evil = env.root.join("evil.toml");
    let body = format!(
        "[meta]\nroot = \"{tree}\"\nscanned_at = \"t\"\nhost = \"h\"\n\
         [[files]]\npath = \"../../outside.txt\"\nstatus = \"restorable\"\n\
         restore_method = \"echo pwn > {{dest}}\"\nsource = \"verified\"\nsize = 1\n",
        tree = env.home.join("tree").display()
    );
    std::fs::write(&evil, body).unwrap();

    let (out, code) = env.run(&["import", "--from", evil.to_str().unwrap()]);
    assert!(
        code != 0,
        "import must refuse a catalog whose paths escape the root:\n{out}"
    );
    assert!(
        out.contains("escapes the scan root"),
        "the refusal must say why:\n{out}"
    );
    assert!(
        !env.root.join("outside.txt").exists(),
        "a `..` path must never be written outside the root"
    );
    // nothing was imported: the store stays empty
    let (_, status_code) = env.run(&["status"]);
    assert_eq!(
        status_code, 4,
        "no catalog may exist after a refused import"
    );
}

/// Fixed issue #18 (dup #13) — restore must return non-zero when any recipe
/// fails, while still running (and reporting) the rest.
#[test]
fn bug_restore_failure_is_a_nonzero_exit() {
    let env = Env::new("bug_restore_exit");
    env.put("f.txt", "x");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&["teach", "f.txt", "--method", "false"]);
    std::fs::remove_file(env.home.join("f.txt")).unwrap();

    let (out, code) = env.run(&["restore", "--root", env.home.to_str().unwrap()]);
    assert!(
        out.contains("failed:"),
        "the failing recipe must be reported:\n{out}"
    );
    assert!(
        code != 0,
        "restore must exit non-zero when a recipe fails (got {code}):\n{out}"
    );
}

/// Open issue #21 — a dangling symlink at `{dest}` must count as present (no
/// clobber), not as absent.
#[test]
#[ignore = "issue #21: no-clobber treats a dangling symlink dest as absent"]
fn bug_dangling_symlink_is_present_for_noclobber() {
    let env = Env::new("bug_noclobber_dangling");
    env.put("conf/x.txt", "v");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    env.ok(&["teach", "conf/x.txt", "--method", "echo X > '{dest}'"]);

    // replace dest with a dangling symlink (target gone) -> still "present"
    std::fs::remove_file(env.home.join("conf/x.txt")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(env.home.join("gone"), env.home.join("conf/x.txt")).unwrap();

    let out = env.ok(&["restore", "--root", env.home.to_str().unwrap()]);
    assert!(
        out.contains("skipped (exists)"),
        "a dangling symlink at dest must be treated as present:\n{out}"
    );
}

/// Open issue #20 — package `exists()` probes must not run per file.
#[test]
#[ignore = "issue #20: package probe spawns ~7 `exists` subprocesses per file"]
fn bug_package_exists_probe_is_hoisted_out_of_the_per_file_loop() {
    let env = Env::new("bug_exists_hoist");
    // a tree with enough files that "per file" is distinguishable from "once"
    for i in 0..12 {
        env.put(&format!("d/file_{i}.txt"), "x");
    }
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let probes = env.version_probe_count();
    assert!(
        probes <= 8,
        "expect a bounded number of manager `--version` probes, got {probes}"
    );
}

// ---- issue #24: broken package-manager adapters (evidenced by the container
// harness on alpine/fedora/void and by the realistic fakes here). Each asserts
// the *correct* package name is extracted from real manager output.

#[test]
#[ignore = "issue #24: apk name_match captures the path, not the package (real `apk info -W` is path-first)"]
fn bug_apk_adapter_extracts_the_package_name() {
    let env = Env::new("bug_apk");
    env.own("apk", "bin/jq", "jq");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let line = env.status_line("bin/jq");
    assert!(
        line.contains("restorable") && line.contains("verified"),
        "apk-owned file should be restorable, got:\n{line}"
    );
    let (full, _) = env.run(&["status"]);
    assert!(
        full.contains("sudo apk add --upgrade jq"),
        "apk recipe must name the package (not the path):\n{full}"
    );
}

#[test]
#[ignore = "issue #24: rpm name_match captures pkg-version, so the recipe reinstalls `pkg-version`, not the bare name"]
fn bug_rpm_adapter_extracts_the_bare_package_name() {
    let env = Env::new("bug_rpm");
    env.own("rpm", "bin/jq", "jq");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let (full, _) = env.run(&["status"]);
    // real rpm prints "jq-<ver>-<rel>.x86_64"; the recipe must reinstall `jq`.
    assert!(
        full.contains("sudo dnf reinstall jq") && !full.contains("reinstall jq-"),
        "rpm recipe must use the bare package name, got:\n{full}"
    );
}

#[test]
#[ignore = "issue #24: xbps name_match `^([^ -]+) ` cannot match real `xbps-query -f` output (`pkg-ver_rel path`)"]
fn bug_xbps_adapter_extracts_the_package_name() {
    let env = Env::new("bug_xbps");
    env.own("xbps", "bin/htop", "htop");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let line = env.status_line("bin/htop");
    assert!(
        line.contains("restorable") && line.contains("verified"),
        "xbps-owned file should be restorable, got:\n{line}"
    );
    let (full, _) = env.run(&["status"]);
    assert!(
        full.contains("sudo xbps-install -S htop"),
        "xbps recipe must reinstall the package:\n{full}"
    );
}
