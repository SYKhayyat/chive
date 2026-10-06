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
        nested.contains("restorable") && nested.contains("chive"),
        "nested git file should be restorable, decided by chive, got:\n{nested}"
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
         [[files]]\npath = \"../../outside.txt\"\nverdict = \"restorable\"\n\
         verdict_source = \"owner\"\n\
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

/// Fixed issue #21 — a dangling symlink at `{dest}` must count as present (no
/// clobber), not as absent: exists() follows links, lstat does not.
#[test]
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

/// Fixed issue #20 (dup #30) — package `exists()` probes are hoisted out of
/// the per-file loop: managers are detected once per scan, then reused.
#[test]
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

/// Issue #50 — the *ownership* probe runs once per file per manager, and the
/// #20 regression test only counted `--version`, so the suite was structurally
/// blind to this whole class.
#[test]
fn bug_50_ownership_probes_do_not_scale_with_the_file_count() {
    // The property, not a constant: doubling the files must not double the
    // ownership probes. A file-count-based assertion would need to know how many
    // adapters exist, and a seventh manager would break it.
    let small = Env::new("bug50_small");
    for i in 0..6 {
        small.put(&format!("d/file_{i}.txt"), "x");
    }
    small.ok(&["scan", small.home.to_str().unwrap()]);
    let small_calls = small.ownership_probe_count();

    let big = Env::new("bug50_big");
    for i in 0..24 {
        big.put(&format!("d/file_{i}.txt"), "x");
    }
    big.ok(&["scan", big.home.to_str().unwrap()]);
    let big_calls = big.ownership_probe_count();

    assert_eq!(
        small_calls, big_calls,
        "4x the files must not mean 4x the ownership probes (small {small_calls}, big {big_calls})"
    );
}

/// Issue #51 — a manager that declares `owns_under` is never probed for a path
/// it cannot own, so a home-directory scan forks nothing.
#[test]
fn bug_51_owns_under_skips_the_probe_entirely() {
    let env = Env::new("bug51_prefix");
    // Everything under the home, which no shipped adapter declares.
    for i in 0..10 {
        env.put(&format!("Documents/file_{i}.txt"), "x");
    }
    env.ok(&["scan", env.home.to_str().unwrap()]);
    assert_eq!(
        env.ownership_probe_count(),
        0,
        "a path no adapter can own must cost no subprocess at all"
    );

    // and a path one *can* own is probed, and still resolves to the right recipe
    let owned = Env::new("bug51_owned");
    owned.own("dpkg", "usr/bin/jq", "jq");
    owned.put("Documents/notes.md", "x");
    owned.ok(&["scan", owned.home.to_str().unwrap()]);
    let line = owned.status_line("usr/bin/jq");
    assert!(line.contains("restorable"), "{line}");
}

// ---- fixed issue #24: broken package-manager adapters (evidenced by the
// container harness on alpine/fedora/void and by the realistic fakes here).
// Each pins the *correct* package name extracted from real manager output.

#[test]
fn bug_apk_adapter_extracts_the_package_name() {
    let env = Env::new("bug_apk");
    env.own("apk", "usr/bin/jq", "jq");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let line = env.status_line("bin/jq");
    assert!(
        line.contains("restorable") && line.contains("chive"),
        "apk-owned file should be restorable, got:\n{line}"
    );
    let (full, _) = env.run(&["status"]);
    assert!(
        full.contains("sudo apk add --upgrade jq"),
        "apk recipe must name the package (not the path):\n{full}"
    );
}

#[test]
fn bug_rpm_adapter_extracts_the_bare_package_name() {
    let env = Env::new("bug_rpm");
    env.own("rpm", "usr/bin/jq", "jq");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let (full, _) = env.run(&["status"]);
    // real rpm prints "jq-<ver>-<rel>.x86_64"; the recipe must reinstall `jq`.
    assert!(
        full.contains("sudo dnf reinstall jq") && !full.contains("reinstall jq-"),
        "rpm recipe must use the bare package name, got:\n{full}"
    );
}

#[test]
fn bug_xbps_adapter_extracts_the_package_name() {
    let env = Env::new("bug_xbps");
    env.own("xbps", "usr/bin/htop", "htop");
    env.ok(&["scan", env.home.to_str().unwrap()]);
    let line = env.status_line("bin/htop");
    assert!(
        line.contains("restorable") && line.contains("chive"),
        "xbps-owned file should be restorable, got:\n{line}"
    );
    let (full, _) = env.run(&["status"]);
    assert!(
        full.contains("sudo xbps-install -S htop"),
        "xbps recipe must reinstall the package:\n{full}"
    );
}

/// Fixed issue #22 (root #10, D9) — the derived SQLite index is never read as
/// truth: deleting the TOML catalog deletes the catalog, even though a stale
/// index file is still lying around.
#[test]
fn a_stale_derived_index_is_never_read_as_the_catalog() {
    let env = Env::new("bug_d9_truth");
    env.put("f.txt", "x");
    env.ok(&["scan", env.home.to_str().unwrap()]);

    // the TOML truth + the derived index both exist now
    let truth = env.store.join("catalog.toml");
    assert!(truth.exists(), "scan wrote the TOML truth");
    assert!(
        env.store.join("catalog.toml").exists(),
        "derived index exists"
    );

    // lose the truth: the derived index must not impersonate it
    std::fs::remove_file(&truth).unwrap();
    let (_, code) = env.run(&["status"]);
    assert_eq!(
        code, 4,
        "no TOML truth means no catalog; the derived index is not a fallback"
    );
}

// ---- open issues: each fails under `-- --ignored` against today's code ----

/// Issue #32 — scan must never silently destroy the existing catalog: a
/// nonexistent root must fail, and scanning a different root must leave the
/// prior catalog's entries in place.
#[test]
#[ignore = "issue #32 — scan silently clobbers the existing catalog"]
fn bug_32_scan_never_clobbers_an_existing_catalog() {
    let env = Env::new("bug32_clobber");
    env.put(".bashrc", "b");
    env.put("work/notes.txt", "n");
    env.ok(&["scan", env.home.to_str().unwrap()]);

    // a nonexistent root must be an error, not an empty catalog
    let (_, code) = env.run(&["scan", "/definitely/not/here"]);
    assert_ne!(code, 0, "scanning a nonexistent path must fail");
    assert!(
        env.status_line(".bashrc").contains("bashrc"),
        "a failed scan must leave the prior catalog untouched"
    );

    // scanning a subroot must not replace the whole-home catalog
    let (_, code) = env.run(&["scan", env.home.join("work").to_str().unwrap()]);
    assert_ne!(
        code, 0,
        "scanning a different root over an existing catalog must refuse"
    );
    assert!(
        env.status_line(".bashrc").contains("bashrc"),
        "the HOME catalog must survive a refused subroot scan"
    );
}

/// Issue #33 — the chive store dir is never cataloged, so clean can never eat
/// its own database.
#[test]
#[ignore = "issue #33 — chive catalogs (and clean deletes) its own store files"]
fn bug_33_scan_excludes_its_own_store() {
    let env = Env::new("bug33_store");
    env.put(".bashrc", "b");

    // default store location: ~/.config/chive inside the scanned home
    let run = |args: &[&str]| {
        let mut c = env.cmd();
        c.env_remove("CHIVE_CONFIG_DIR");
        let out = c.args(args).output().expect("chive should run");
        (
            format!(
                "{}{}",
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ),
            out.status.code().unwrap_or(-1),
        )
    };

    let home = env.home.to_str().unwrap().to_string();
    // the store is written after the first scan's walk, so the bug shows on a
    // rescan: the store now exists and the second walk catalogs it
    run(&["scan", &home]);
    run(&["scan", &home]);
    let (status, _) = run(&["status"]);
    assert!(
        !status.contains(".config/chive"),
        "chive must not catalog its own store:\n{status}"
    );

    let (_, code) = run(&["clean", "--force"]);
    assert_eq!(code, 0, "cleaning the disposable files must succeed");
    assert!(
        env.home.join(".config/chive/catalog.toml").exists(),
        "the store must survive clean"
    );
    let (_, code) = run(&["status"]);
    assert_eq!(code, 0, "the catalog must still be readable after clean");
}

/// Issue #34 — the scan root is canonicalized, so a relative `scan .` cannot
/// make every later command resolve against whatever cwd it is run from.
#[test]
#[ignore = "issue #34 — relative scan root resolved against the invocation cwd"]
fn bug_34_scan_root_is_canonicalized() {
    let env = Env::new("bug34_relroot");
    env.put("proj/important.conf", "keep");
    let proj = env.home.join("proj");

    let mut c = env.cmd();
    c.current_dir(&proj);
    c.args(["scan", "."]);
    let out = c.output().expect("chive should run");
    assert!(out.status.success(), "scan . must succeed");

    let toml = std::fs::read_to_string(env.store.join("catalog.toml")).unwrap();
    let root_line = toml
        .lines()
        .find(|l| l.starts_with("root"))
        .expect("catalog has a root");
    assert!(
        root_line.contains(proj.to_string_lossy().as_ref()),
        "the recorded root must be absolute, got {root_line}"
    );

    // from a different cwd, clean must target the real file (or refuse)
    env.ok(&["dispose", "proj/important.conf"]);
    let (_, code) = env.run(&["clean", "--force"]);
    assert_eq!(code, 0, "clean must succeed against the canonical root");
    assert!(
        !proj.join("important.conf").exists(),
        "clean must have removed the real file, not a cwd-relative phantom"
    );
}

/// Issue #35 — the README's migration flow must actually work: a foreign-root
/// catalog imported on a new machine restores under the new $HOME.
#[test]
#[ignore = "issue #35 — restore without --root targets the old machine's root"]
fn bug_35_migration_flow_restores_into_home() {
    let env = Env::new("bug35_migrate");
    let exported = env.root.join("catalog.toml");
    std::fs::write(
        &exported,
        "[meta]\nroot = \"/home/alice-old-machine\"\nscanned_at = \"2026-01-01T00:00:00+00:00\"\nhost = \"old\"\n\
             [[files]]\npath = \".gitconfig\"\nverdict = \"restorable\"\nverdict_source = \"owner\"\n\
             restore_method = \"printf '[user]\\n' > '{{dest}}'\"\nsource = \"user_supplied\"\nsize = 8\n",
    )
    .unwrap();

    // the README flow, verbatim (README.md lines 33-38)
    env.ok(&["import", "--from", exported.to_str().unwrap()]);
    env.ok(&["plan", "restore", "--root", env.home.to_str().unwrap()]);
    env.ok(&["restore", "--all"]);

    assert!(
        env.home.join(".gitconfig").exists(),
        "the documented migration flow must restore into $HOME"
    );
    assert!(
        !std::path::Path::new("/home/alice-old-machine").exists(),
        "restore must never create the old machine's tree"
    );
}

/// Issue #36 — the hostname is detected from the kernel, not from an env var
/// that non-interactive shells never export.
#[test]
#[ignore = "issue #36 — hostname() reads only $HOSTNAME"]
#[cfg(unix)]
fn bug_36_hostname_is_detected_without_env() {
    let env = Env::new("bug36_host");
    env.put(".bashrc", "b");

    let mut c = env.cmd();
    c.env_remove("HOSTNAME");
    c.args(["scan", env.home.to_str().unwrap()]);
    let out = c.output().expect("chive should run");
    assert!(out.status.success(), "scan must succeed");

    let toml = std::fs::read_to_string(env.store.join("catalog.toml")).unwrap();
    assert!(
        !toml.contains("host = \"unknown\""),
        "the kernel hostname must be used, got:\n{toml}"
    );
}

/// Issue #37 (narrowed) — the owner verbs agree on which paths are refusable.
///
/// The absent-path half is gone: teaching for a file this machine does not have
/// is legitimate (planning a new machine, issue #45), so `teach` accepting it is
/// correct rather than drift. What both verbs must still agree on is refusing a
/// malformed or root-escaping path.
#[test]
fn bug_37_owner_verbs_agree_on_refusable_paths() {
    let env = Env::new("bug37_teach");
    env.put(".bashrc", "b");
    env.ok(&["scan", env.home.to_str().unwrap()]);

    // an absent path is fine, and both verbs say so by succeeding
    env.ok(&["teach", "no/such/file.txt", "--method", "echo x > '{dest}'"]);
    env.ok(&["dispose", "no/such/other.txt"]);

    for evil in ["../escape.txt", "a/../../escape.txt", "/etc/passwd", "a//b"] {
        let (_, teach_code) = env.run(&["teach", evil, "--method", "echo x"]);
        let (_, dispose_code) = env.run(&["dispose", evil]);
        assert_ne!(
            teach_code, 0,
            "teach must refuse {evil:?}: it escapes the scan root"
        );
        assert_ne!(
            dispose_code, 0,
            "dispose must refuse {evil:?} too — sibling verbs must not disagree"
        );
    }
}
