#!/usr/bin/env bash
# Run the whole chive harness: hermetic host tests + the real-container matrix.
# Mirrors Shall's audit/tools/run-harness.sh.
set -u
cd "$(dirname "$0")/.." || exit 1

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
cargo test --test main -- --ignored 2>/dev/null | rg '^test .* \.\.\. (ok|ignored)' \
    && echo "(each 'ok' here is a bug that needs filing/fixing)"

echo ""
echo "############################################################"
echo "# 2. CONTAINER harness (real package managers per distro)   "
echo "############################################################"
DISTROS="${DISTROS:-ubuntu arch fedora alpine}" ./docker/integration/run.sh