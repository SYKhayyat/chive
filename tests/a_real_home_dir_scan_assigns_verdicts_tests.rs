//! A real, human-shaped home directory scanned end to end: real git, real
//! symlinks, a real dangling symlink, real package-owned files (via
//! fake-but-executable package managers on PATH), and a real ignored
//! `node_modules`.
//!
//! The point of this file is D19: nothing here is judged `disposable` except the
//! one thing chive can *prove* is dead, and the files the retired heuristic used
//! to guess about are now holes.

use std::path::Path;

use crate::harness::Env;

#[test]
fn a_real_home_dir_scan_assigns_every_verdict() {
    let env = Env::new("real_home");
    let home = &env.home;

    // a dotfiles git repo: one file at the root, one nested (nested is bug #19)
    env.put("dotfiles/init.el", "(setq foo 1)");
    env.put("dotfiles/emacs/init.el", "'()");
    env.git_repo("dotfiles", &["init.el", "emacs/init.el"]);

    // a symlink out to a tool
    env.put("tools/fmt.sh", "#!/bin/sh\n");
    env.symlink("bin/fmt", &home.join("tools/fmt.sh"));

    // files the retired `temporary` heuristic used to judge by name
    env.put("scratch~", "x");
    env.put("#auto#", "x");
    env.put("build_cache.tmp", "x");

    // a symlink whose target does not resolve: provably dead, and therefore the
    // only automatic source of a cleanable verdict
    env.symlink(
        "bin/collected-link",
        Path::new("/definitely/gone/collected"),
    );

    // a package that owns files in the home env
    env.own("pacman", "usr/bin/myapp", "myapp");

    // a real ignored dir
    env.put("node_modules/junk.js", "x");

    // a plain unexplained file: a hole
    env.put("Pictures/photo.nef", "bytes");

    let (status, _) = env.run(&["scan", env.home.to_str().unwrap()]);
    println!("{status}");

    let s = env.ok(&["status"]);

    // package-owned -> restorable, chive-decided, via the pacman fake
    let line = env.status_line("usr/bin/myapp");
    assert!(
        line.contains("restorable") && line.contains("chive"),
        "package-owned file must be restorable:\n{line}"
    );

    // git-tracked, root-level -> restorable
    let dl = env.status_line("dotfiles/init.el");
    assert!(dl.contains("restorable") && dl.contains("chive"), "{dl}");

    // a resolving symlink -> restorable with an ln -s recipe
    let sym = env.status_line("bin/fmt");
    assert!(sym.contains("restorable"), "{sym}");
    let (full, _) = env.run(&["status"]);
    assert!(full.contains("ln -s"), "symlink recipe is ln -s:\n{full}");

    // a dangling symlink -> disposable, and provably so
    let dead = env.status_line("bin/collected-link");
    assert!(
        dead.contains("disposable"),
        "a link whose target is gone cannot work:\n{dead}"
    );

    // D19: the retired name heuristic guesses nothing. These are holes.
    for rel in ["scratch~", "#auto#", "build_cache.tmp"] {
        let l = env.status_line(rel);
        assert!(
            l.contains("unknown"),
            "{rel} must be a hole, never guessed cleanable:\n{l}"
        );
    }

    // an unexplained file -> unknown, never cleanable
    let hole = env.status_line("Pictures/photo.nef");
    assert!(
        hole.contains("unknown") && !hole.contains("disposable"),
        "{hole}"
    );

    // and clean, which only ever touches disposable, must leave every hole
    let (_, code) = env.run(&["clean", "--dry-run"]);
    assert_eq!(code, 0);
    let (preview, _) = env.run(&["clean", "--dry-run"]);
    assert!(
        !preview.contains("scratch~") && !preview.contains("photo.nef"),
        "clean must never propose deleting a hole:\n{preview}"
    );

    // ignored dir never catalogued
    assert!(
        !s.contains("node_modules"),
        "node_modules must be absent from the catalog:\n{s}"
    );
}
