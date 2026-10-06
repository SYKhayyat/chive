#!/usr/bin/env bash
# Run the whole chive harness: hermetic host tests + the real-container matrix.
# Mirrors Shall's audit/tools/run-harness.sh.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1

# `rg` is load-bearing below and `set -e` would abort the run on a machine that
# lacks it. Without this check the ignored-regression report simply vanished --
# the pipeline failed, the `&& echo` short-circuited, and the script carried on to
# exit 0. A ratchet that disappears on the wrong machine is not a ratchet.
if ! command -v rg >/dev/null 2>&1; then
    echo "rg is required (see PLAN.md #53); install ripgrep or re-run the tests directly" >&2
    exit 1
fi

echo "############################################################"
echo "# 1. HOST harness (hermetic: real git/shell/symlink + fake  "
echo "#    package managers that emit REAL output formats)        "
echo "############################################################"
cargo test --test main --no-fail-fast || { echo "HOST HARNESS FAILED"; exit 1; }

echo ""
echo "############################################################"
echo "#    ignored bug-regressions (should FAIL — they are open   "
echo "#    issues, see bug_regressions_document_open_issues_tests) "
echo "############################################################"
# Under `--ignored`, a PASSING test prints `ok`, which means the bug it documents
# has been fixed and its `#[ignore]` should come off. The message used to say the
# opposite, which is how #32-#36 stayed listed after they were fixed.
ignored_report="$(cargo test --test main -- --ignored 2>/dev/null \
    | rg '^test .* \.\.\. (ok|ignored)' || true)"
if [ -n "$ignored_report" ]; then
    echo "$ignored_report"
    echo "(each 'ok' above is a bug whose #[ignore] should be removed -- it is fixed)"
    if echo "$ignored_report" | rg -q ' \.\.\. ok$'; then
        echo "NOTE: at least one documented bug now PASSES. Drop its #[ignore]."
    fi
fi

echo ""
echo "############################################################"
echo "# 2. CONTAINER harness (real package managers per distro)   "
echo "############################################################"
DISTROS="${DISTROS:-ubuntu arch fedora alpine}" ./docker/integration/run.sh