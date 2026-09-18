//! Provenance order is a documented, load-bearing contract: package → git →
//! symlink, first match wins (see `docs/spec/target-state.md`, "Provenance
//! detection"). A file owned by a package that also happens to be a symlink or
//! git-tracked must get the *package* recipe, because package recipes are the
//! most portable across machines. The order was previously reversed in code
//! (issue #12); these tests pin the documented order.

use crate::harness::Env;

#[test]
fn a_package_owned_symlink_gets_the_package_recipe_not_the_ln_recipe() {
    let env = Env::new("order_pkg_vs_symlink");
    // Real managers probe the link path and resolve it to the target, so the
    // ownership fixture names the path chive probes: the link itself.
    let link = env.symlink("bin/tool", &env.home.join("real/tool"));
    env.own_at("pacman", &link, "myapp");

    env.ok(&["scan", env.home.to_str().unwrap()]);

    let line = env.status_line("bin/tool");
    assert!(
        line.contains("restorable") && line.contains("verified"),
        "package-owned link must be restorable(verified):\n{line}"
    );
    let (full, _) = env.run(&["status"]);
    assert!(
        full.contains("sudo pacman -S myapp"),
        "package recipe must win over the ln -s recipe:\n{full}"
    );
    assert!(
        !full.contains("ln -s"),
        "symlink recipe must not be used when a package owns the file:\n{full}"
    );
}

#[test]
fn a_package_owned_file_inside_a_git_repo_gets_the_package_recipe() {
    let env = Env::new("order_pkg_vs_git");
    // A committed file that a package also owns (a vendored binary, say).
    env.own("pacman", "repo/vendored/tool", "myapp");
    env.git_repo("repo", &["vendored/tool"]);

    env.ok(&["scan", env.home.to_str().unwrap()]);

    let (full, _) = env.run(&["status"]);
    assert!(
        full.contains("sudo pacman -S myapp"),
        "package recipe must win over the git checkout recipe:\n{full}"
    );
    assert!(
        !full.contains("checkout HEAD"),
        "git recipe must not be used when a package owns the file:\n{full}"
    );
}

#[test]
fn a_git_tracked_symlink_gets_the_git_recipe_not_the_ln_recipe() {
    let env = Env::new("order_git_vs_symlink");
    // A symlink committed to a repo (dotfiles do this: link tracked, target not).
    env.put("dotfiles/real.conf", "x=1");
    env.symlink("dotfiles/live.conf", &env.home.join("dotfiles/real.conf"));
    env.git_repo("dotfiles", &["real.conf", "live.conf"]);

    env.ok(&["scan", env.home.to_str().unwrap()]);

    let line = env.status_line("dotfiles/live.conf");
    assert!(
        line.contains("restorable") && line.contains("verified"),
        "tracked link must be restorable(verified):\n{line}"
    );
    let (full, _) = env.run(&["status"]);
    assert!(
        full.contains("dotfiles/live.conf") && full.contains("checkout HEAD"),
        "git recipe must win over ln -s when the link is tracked:\n{full}"
    );
    assert!(
        !full.contains("ln -s"),
        "symlink recipe must not be used when git owns the file:\n{full}"
    );
}
