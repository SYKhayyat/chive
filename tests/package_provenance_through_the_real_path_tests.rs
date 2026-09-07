//! Package provenance through real executables on `PATH`. The fake `dpkg` /
//! `pacman` / `rpm` / `apk` / `xbps` produce their real output formats, so the
//! scan's `name_match` extraction and recipe templates are exercised against
//! realistic manager output — the host-level analogue of the container harness.

use crate::harness::Env;

#[test]
fn each_package_manager_yields_its_documented_reinstall_recipe() {
    // (manager, pkg, rel_file, expected recipe fragment). Only the adapters whose
    // name_match regex actually survives real manager output are here:
    //   dpkg -S "<pkg>: <path>"      -> ^([^: ]+)   works
    //   pacman -Qo "<path> is owned by <pkg> <ver>" -> ([^ ]+)   works
    // apk / rpm / xbps are genuinely broken against real output (issue #24) and
    // so live in bug_regressions_document_open_issues_tests instead:
    //   apk  output is path-first -> recipe becomes "add --upgrade <path>"
    //   rpm  captures pkg-version (:w) -> "dnf reinstall <pkg>-<ver>"
    //   xbps regex "^([^ -]+) " cannot match "<pkg>-<ver>_<rel> <path>".
    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "dpkg",
            "coreutils",
            "bin/core",
            "sudo apt-get install --reinstall coreutils",
        ),
        ("pacman", "jq", "bin/jq", "sudo pacman -S jq"),
    ];

    for &(manager, pkg, rel, expected) in cases {
        let env = Env::new(&format!("pkg_{manager}"));
        env.own(manager, rel, pkg);

        let (out, code) = env.run(&["scan", env.home.to_str().unwrap()]);
        assert!(code == 0, "scan failed (manager {manager}):\n{out}");
        let line = env.status_line(rel);
        assert!(
            line.contains("restorable") && line.contains("verified"),
            "[{manager}] {rel} should be restorable(verified):\n{line}"
        );

        // The recipe is the documented reinstall for that package.
        let (full, _) = env.run(&["status"]);
        assert!(
            full.contains(expected),
            "[{manager}] recipe for {pkg} should contain `{expected}`:\n{full}"
        );
    }
}

#[test]
fn a_file_no_manager_owns_is_orphaned_even_with_managers_present() {
    let env = Env::new("orphan_with_managers");
    // write managers + a file that no fake manager owns
    env.put("misc/notes.txt", "body");
    let (out, _) = env.run(&["scan", env.home.to_str().unwrap()]);
    println!("{out}");
    assert!(
        env.status_line("misc/notes.txt").contains("orphaned"),
        "unowned file must be orphaned even when package managers exist"
    );
}
