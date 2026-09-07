//! A real, human-shaped home directory scanned end to end: real git, real
//! symlinks, real temp files, real package-owned files (via fake-but-executable
//! package managers on PATH), and a real ignored `node_modules`.

use crate::harness::Env;

#[test]
fn a_real_home_dir_scan_assigns_every_status() {
    let env = Env::new("real_home");
    let home = &env.home;

    // a dotfiles git repo: one file at the root, one nested (nested is bug #19)
    env.put("dotfiles/init.el", "(setq foo 1)");
    env.put("dotfiles/emacs/init.el", "'()");
    env.git_repo("dotfiles", &["init.el", "emacs/init.el"]);

    // a symlink out to a tool
    env.put("tools/fmt.sh", "#!/bin/sh\n");
    env.symlink("bin/fmt", &home.join("tools/fmt.sh"));

    // editor temp + cache artifacts
    env.put("scratch~", "x");
    env.put("#auto#", "x");
    env.put("build_cache.tmp", "x");

    // a package that owns files in the home env
    env.own("pacman", "opt/myapp/bin/app", "myapp");

    // a real ignored dir
    env.put("node_modules/junk.js", "x");

    // a plain orphan
    env.put("Pictures/photo.nef", "bytes");

    let (status, _) = env.run(&["scan", env.home.to_str().unwrap()]);
    println!("{status}");

    let s = env.ok(&["status"]);

    // package-owned -> restorable(verified), via the pacman fake
    let line = env.status_line("opt/myapp/bin/app");
    assert!(
        line.contains("restorable"),
        "package-owned file must be restorable:\n{line}"
    );
    assert!(
        line.contains("verified"),
        "package-owned file is verified provenance:\n{line}"
    );

    // git-tracked, root-level -> restorable(verified)
    let dl = env.status_line("dotfiles/init.el");
    assert!(dl.contains("restorable") && dl.contains("verified"), "{dl}");

    // symlink -> restorable(verified) with an ln -s recipe
    let sym = env.status_line("bin/fmt");
    assert!(sym.contains("restorable"), "{sym}");
    let (full, _) = env.run(&["status"]);
    assert!(full.contains("ln -s"), "symlink recipe is ln -s:\n{full}");

    // temp files -> temporary
    for rel in ["scratch~", "#auto#", "build_cache.tmp"] {
        assert!(
            env.status_line(rel).contains("temporary"),
            "{rel} should be temporary"
        );
    }

    // orphan -> orphaned
    assert!(env.status_line("Pictures/photo.nef").contains("orphaned"));

    // ignored dir never catalogued
    assert!(
        !s.contains("node_modules"),
        "node_modules must be absent from the catalog:\n{s}"
    );
}
