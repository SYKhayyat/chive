//! Fake, real-on-`PATH` package managers.
//!
//! chive's scan shells out to `dpkg`, `pacman`, ... (through the `Runner` seam,
//! `Runner::exists` + `run_argv`). The hermetic harness tests the real binary
//! end-to-end, so the package managers must be *real processes on `PATH`* that
//! answer the exact probe commands and output formats the built-in adapters
//! (`src/provenance/backends.toml`) expect — so the `name_match` regexes are
//! exercised for real. They are shell scripts written into a scratch `bin/` dir
//! that the test sets `PATH` to, each reading an ownership fixture and writing a
//! call log.
//!
//! This mirrors Shall's `tests/mock_providers`: the machine underneath the
//! subject is real but entirely owned by the test. `nix-store` is faked to
//! always *miss* (its `--deriver` restore is disputed, issue #24), so it can
//! never claim a file in the harness and flake an assertion.

use std::path::Path;

/// The built-in `os = ["linux"]` managers from `backends.toml` (nix removed:
/// its --deriver probe named a derivation no verb can restore from). This is
/// the set the scanner can probe per file.
pub const MANAGERS: &[&str] = &["dpkg", "pacman", "rpm", "dnf", "apk", "xbps"];

/// An ownership fixture: which installed package owns which absolute path, per
/// fake manager. Written as one `<pkg> <path>` line per file into `owners/<m>`.
#[derive(Debug, Default)]
pub struct Ownership {
    /// (manager, pkg, abs_path)
    rows: Vec<(String, String, String)>,
}

impl Ownership {
    pub fn add(&mut self, manager: &str, pkg: &str, abs_path: &str) {
        self.rows
            .push((manager.to_string(), pkg.to_string(), abs_path.to_string()));
    }

    /// Write `<owners_dir>/<manager>` for every manager that has a row.
    pub fn write_to(&self, owners_dir: &Path) {
        std::fs::create_dir_all(owners_dir).unwrap();
        std::fs::write(owners_dir.join("_has_any"), "").unwrap();
        for manager in MANAGERS {
            let rows: Vec<_> = self.rows.iter().filter(|(m, _, _)| m == manager).collect();
            if rows.is_empty() {
                continue;
            }
            let body: String = rows
                .iter()
                .map(|(_, pkg, path)| format!("{pkg} {path}\n"))
                .collect();
            std::fs::write(owners_dir.join(manager), body).unwrap();
        }
    }
}

/// The shell body for one fake manager. `$CHIVE_OWNERSHIP_FILE/own` is the
/// ownership dir; the script prints the real manager's output format for the
/// probed path, or exits 1 (no ownership) when the path is not in the fixture.
/// Every invocation is appended to `$CHIVE_CALL_LOG`.
///
/// The probe call shapes come from `backends.toml`:
///   dpkg  -S <path>       pacman -Qo <path>
///   rpm   -qf --queryformat %{NAME} <path>   (path at "$4")
///   dnf   repoquery --queryformat %{NAME} -f <path>   (path at "$5")
///   apk   info -W <path>   xbps-query -o <path>
fn script(manager: &str) -> String {
    // The shell positional that holds the probed path, per the adapter's
    // detect_args (chive passes program then the args array), and the output
    // line the real manager prints. rpm/dnf honor --queryformat %{NAME} by
    // printing the bare package name, exactly what the real tools do.
    let (pvar, emit) = match manager {
        "dpkg" => ("\"${2}\"", "{pkg}: $P"),
        "pacman" => ("\"${2}\"", "$P is owned by {pkg} 1.7.1-1"),
        "rpm" => ("\"${4}\"", "{pkg}"),
        "dnf" => ("\"${5}\"", "{pkg}"),
        // Real `apk info -W <path>` is path-first: "<path> is owned by
        // <pkg>-<ver>".
        "apk" => ("\"${3}\"", "$P is owned by {pkg}-1.7.1-r0"),
        // Real `xbps-query -o <path>`: "<pkg>-<ver>_<rel>: <path> (<type>)".
        "xbps" => ("\"${2}\"", "{pkg}-1.0_1: $P (regular file)"),
        _ => unreachable!("unknown manager {manager}"),
    };
    let caught = if pvar.is_empty() { ": none" } else { "owns" };
    let template = r#"#!/bin/sh
# fake @MANAGER@ for the chive harness. Read-only: never installs, only answers.
echo "$0|$*" >> "$CHIVE_CALL_LOG"
[ "$CHIVE_OWNERSHIP_FILE" ] || exit 1
P=@PVAR@
owns() {
    grep -q " $P$" "$CHIVE_OWNERSHIP_FILE/@MANAGER@" 2>/dev/null
}
emit() {
    grep " $P$" "$CHIVE_OWNERSHIP_FILE/@MANAGER@" 2>/dev/null | \
        awk -v p="$P" '{ print $1 }' | head -n1
}
if [ "$1" = "--version" ]; then
    exit 0
fi
if @CAUGHT@; then
    pkg="$(emit)"
    [ -n "$pkg" ] || exit 1
    printf '%s\n' "@EMIT@" | sed 's/{pkg}/'"$pkg"'/'
    exit 0
fi
exit 1
"#;
    template
        .replace("@MANAGER@", manager)
        .replace("@PVAR@", pvar)
        .replace("@EMIT@", emit)
        .replace("@CAUGHT@", caught)
}

/// The `program` field from `backends.toml` for each manager — the binary name
/// chive actually probes. The fake manager binary must be named this to be found
/// on `PATH`. The manager *name* (ownership-file key, recipe) may differ: xbps
/// probes `xbps-query`, not `xbps`.
fn program_of(manager: &str) -> &str {
    if manager == "xbps" {
        "xbps-query"
    } else {
        manager
    }
}

/// Write an executable fake manager script per known manager into `bin`,
/// named by its `program` (so `xbps-query`, not `xbps`). Returns the binary
/// paths created.
pub fn write_managers(bin: &Path) -> Vec<std::path::PathBuf> {
    std::fs::create_dir_all(bin).unwrap();
    MANAGERS
        .iter()
        .map(|m| {
            let path = bin.join(program_of(m));
            std::fs::write(&path, script(m)).expect("write fake manager");
            set_executable(&path);
            path
        })
        .collect()
}

#[cfg(unix)]
fn set_executable(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path).unwrap().permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm).unwrap();
}

#[cfg(not(unix))]
fn set_executable(_path: &Path) {
    // On Windows the fake managers are not on PATH via shebang; the host
    // harness gates on Unix. Docker distro harnesses (real managers) still work.
}

#[cfg(test)]
mod mock_providers_tests {
    use super::*;

    #[test]
    fn fake_dpkg_answers_version_as_present_and_reports_ownership() {
        let dir = tempfile::tempdir().unwrap();
        write_managers(&dir.path().join("bin"));
        let bin = dir.path().join("bin");
        let owners = dir.path().join("owners");
        let mut o = Ownership::default();
        o.add("dpkg", "coreutils", "/v/bin/ls");
        o.write_to(&owners);

        let log = dir.path().join("calls.log");
        let child = std::process::Command::new(bin.join("dpkg"))
            .env("CHIVE_OWNERSHIP_FILE", &owners)
            .env("CHIVE_CALL_LOG", &log)
            .arg("--version")
            .output()
            .unwrap();
        assert!(child.status.success(), "exists probe must say present");

        let child = std::process::Command::new(bin.join("dpkg"))
            .env("CHIVE_OWNERSHIP_FILE", &owners)
            .env("CHIVE_CALL_LOG", &log)
            .args(["-S", "/v/bin/ls"])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&child.stdout);
        assert_eq!(out.trim(), "coreutils: /v/bin/ls");
    }

    #[test]
    fn fake_manager_does_not_claim_unowned_paths() {
        let dir = tempfile::tempdir().unwrap();
        write_managers(&dir.path().join("bin"));
        let owners = dir.path().join("owners");
        Ownership::default().write_to(&owners);
        let log = dir.path().join("calls.log");
        let child = std::process::Command::new(dir.path().join("bin/pacman"))
            .env("CHIVE_OWNERSHIP_FILE", &owners)
            .env("CHIVE_CALL_LOG", &log)
            .args(["-Qo", "/v/nothing"])
            .output()
            .unwrap();
        assert_eq!(child.status.code(), Some(1));
        assert_eq!(String::from_utf8_lossy(&child.stdout).trim(), "");
    }
}
