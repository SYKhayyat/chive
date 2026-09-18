//! Integration test harness.
//!
//! With `autotests = false` in Cargo.toml, every test file must be declared
//! here as a `mod`, or Cargo silently never compiles or runs it. The
//! [`every_test_file_is_listed`] test enforces that rule: add a file to
//! `tests/` and the suite fails until you declare it, so a new test can never
//! silently drop out of the run.

use std::path::{Path, PathBuf};

// Declare every integration test file here.
mod a_machine_migration_rebuilds_files_tests;
mod a_real_home_dir_scan_assigns_statuses_tests;
mod bug_regressions_document_open_issues_tests;
mod cli_tests;
mod harness;
mod mark_and_teach_round_trip_across_reload_tests;
mod mock_providers;
mod package_provenance_through_the_real_path_tests;
mod provenance_order_is_package_then_git_then_symlink_tests;
mod restore_git_and_symlink_end_to_end_tests;

/// The files that *are* wired in — the single source of truth for the suite.
const DECLARED: &[&str] = &[
    "a_machine_migration_rebuilds_files_tests",
    "a_real_home_dir_scan_assigns_statuses_tests",
    "bug_regressions_document_open_issues_tests",
    "cli_tests",
    "mark_and_teach_round_trip_across_reload_tests",
    "mock_providers",
    "package_provenance_through_the_real_path_tests",
    "provenance_order_is_package_then_git_then_symlink_tests",
    "restore_git_and_symlink_end_to_end_tests",
];

/// Walk `tests/` and fail on any `*_tests.rs` not declared above. This is the
/// guard that keeps `autotests = false` honest.
#[test]
fn every_test_file_is_listed() {
    let links: Vec<PathBuf> =
        walkdir_test_files(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")).collect();
    for file in &links {
        let name = file
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("test file stem must be UTF-8");
        if !DECLARED.contains(&name) {
            panic!(
                "test file {name} is not declared in tests/main.rs; add `mod {name};` to DECLARED"
            );
        }
    }
}

/// Yield every `*_tests.rs` under `root` (recursive).
fn walkdir_test_files(root: PathBuf) -> impl Iterator<Item = PathBuf> {
    let mut out = Vec::new();
    if !root.exists() {
        return out.into_iter();
    }
    collect(&root, &mut out);
    out.into_iter()
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(read) = std::fs::read_dir(dir) {
        for e in read.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect(&p, out);
            } else if p
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|n| n.ends_with("_tests.rs"))
            {
                out.push(p);
            }
        }
    }
}
