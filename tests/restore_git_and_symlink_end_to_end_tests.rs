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
fn a_git_restore_cache_obscurity_is_documented() {
    // Open design gap: a git recipe embeds the *source machine's absolute repo
    // path (`git -C /abs/repo checkout ...`), so a restore on a machine whose
    // repo lives elsewhere silently fails. Detection is the subject here; the
    // cross-machine portability of that path is not yet resolved (the recipe has
    // no `{dest}` and no clobber check). This test only asserts detection holds,
    // so it stays green while the portability gap is an open issue.
    let env = Env::new("git_portability");
    let home = &env.home;
    let repo = home.join("dotfiles");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("rc"), "x").unwrap();
    env.git_repo("dotfiles", &["rc"]);
    env.ok(&["scan", home.to_str().unwrap()]);
    assert!(env.status_line("dotfiles/rc").contains("restorable"));
}
