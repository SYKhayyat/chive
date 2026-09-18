//! Beyond detection: actually **re-derive** a git-tracked file and a symlink by
//! running their recipes and checking the destination comes back. The earlier
//! harness tests prove the *status* is `restorable(verified)`; these prove the
//! restore command works for the two non-package provenance kinds.

use crate::harness::Env;

#[test]
fn restore_recreates_a_deleted_git_tracked_file() {
    let env = Env::new("restore_git");
    let home = &env.home;
    let repo = home.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("config.toml"), "key=value\n").unwrap();
    env.git_repo("repo", &["config.toml"]);
    env.ok(&["scan", home.to_str().unwrap()]);

    // the git-tracked file is restorable(verified) with a checkout recipe
    let line = env.status_line("repo/config.toml");
    assert!(
        line.contains("restorable") && line.contains("verified"),
        "git-tracked file restorable:\n{line}"
    );

    // lose the file (as on a broken machine) and re-derive it
    std::fs::remove_file(repo.join("config.toml")).unwrap();
    let restore = env.ok(&["restore", "--root", home.to_str().unwrap()]);

    assert!(
        repo.join("config.toml").exists(),
        "restore must re-create the git-tracked file:\n{restore}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("config.toml")).unwrap(),
        "key=value\n"
    );
}

#[test]
fn restore_recreates_a_deleted_symlink() {
    #[cfg(unix)]
    {
        let env = Env::new("restore_symlink");
        let home = &env.home;
        env.put("tools/app", "#!/bin/sh\n");
        let link = env.symlink("bin/app", &home.join("tools/app"));

        env.ok(&["scan", home.to_str().unwrap()]);
        let line = env.status_line("bin/app");
        assert!(line.contains("restorable"), "symlink restorable:\n{line}");

        // lose the link (its target survives) and re-derive it
        std::fs::remove_file(&link).unwrap();
        let restore = env.ok(&["restore", "--root", home.to_str().unwrap()]);

        assert!(
            link.exists(),
            "restore must recreate the symlink:\n{restore}"
        );
        let target = std::fs::read_link(&link).unwrap();
        assert_eq!(target, home.join("tools/app"));
    }
}

#[test]
fn a_git_recipe_is_portable_across_machines() {
    // Issue #28: a git recipe used to embed the source machine's absolute repo
    // path, which can never succeed (or worse, points at an unrelated
    // directory) on the machine a restore is for. The recipe now addresses the
    // repo relative to the scan root with a {root} token, so the same catalog
    // restores on any machine whose repo sits at the same relative location.
    let a = Env::new("git_port_a");
    let repo_a = a.home.join("dotfiles");
    std::fs::create_dir_all(&repo_a).unwrap();
    std::fs::write(repo_a.join("rc"), "committed").unwrap();
    a.git_repo("dotfiles", &["rc"]);
    a.ok(&["scan", a.home.to_str().unwrap()]);

    // the recipe must not name machine A's absolute path
    let (full, _) = a.run(&["status"]);
    let method_line = full
        .lines()
        .find(|l| l.contains("method:") && l.contains("checkout"))
        .expect("git recipe method line");
    assert!(
        !method_line.contains(a.home.to_str().unwrap()),
        "git recipe must not embed the absolute source path:\n{method_line}"
    );
    assert!(
        method_line.contains("{root}"),
        "git recipe must use the portable root token:\n{method_line}"
    );

    let catalog = a.root.join("catalog.toml");
    a.ok(&["export", "--to", catalog.to_str().unwrap()]);

    // machine B: the same repo, same relative place, different absolute home
    let b = Env::new("git_port_b");
    let repo_b = b.home.join("dotfiles");
    std::fs::create_dir_all(&repo_b).unwrap();
    std::fs::write(repo_b.join("rc"), "committed").unwrap();
    std::fs::write(repo_b.join("other"), "o").unwrap();
    b.git_repo("dotfiles", &["rc", "other"]);
    b.ok(&["import", "--from", catalog.to_str().unwrap()]);

    // lose the file and re-derive it from B's own repo
    std::fs::remove_file(repo_b.join("rc")).unwrap();
    let restore = b.ok(&["restore", "--root", b.home.to_str().unwrap()]);
    assert!(
        repo_b.join("rc").exists(),
        "restore must use machine B's repo path:\n{restore}"
    );
    assert_eq!(
        std::fs::read_to_string(repo_b.join("rc")).unwrap(),
        "committed",
        "the file came back from B's checkout"
    );
}
