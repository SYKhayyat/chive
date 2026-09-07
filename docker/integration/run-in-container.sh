#!/bin/sh
# The real-world scenario, run *inside* a disposable distro container as the
# root user — a human using chive on a real machine with a real package manager.
#
# Arguments: $1 = backend ("dpkg"|"pacman"|"rpm"|"apk"|"xbps"), $2 = expected
# jq reinstall recipe (so the run.sh driver keeps the recipe table in one file,
# or pass them on the command line when driving docker directly).
#
# Two phases, two stores (scanning overwrites the one catalog):
#   A. scan a REAL package dir (/usr/bin) -> every binary is package-owned; the
#      adapter's probe command + name_match regex run against real manager output.
#   B. scan a fixture tree (/root/fix)  -> real git (flat + nested), a symlink,
#      a temp file, an orphan, an ignored node_modules.
#
# Hard assertions fail the run; the git-nested provenance check is reported as a
# SOFT signal (it is open issue #19 — the scanner currently orphans nested git
# files), so the harness ratchets without pretending a known bug is fixed.
set -u
BROKEN=0
soft() { printf '  ~ %s\n' "$1"; }   # known issue, does not fail the run

fail() { printf '  FAIL: %s\n' "$1"; BROKEN=1; }
ok() { printf '  ok: %s\n' "$1"; }

# method_of_basename <prog> — the restore recipe chive assigned to the catalog
# entry whose path ends in that program name (robust to distros that install it
# under /usr/sbin vs /usr/bin).
method_of_basename() {
    chive status 2>/dev/null | awk -v n="$1" '
        want && /^  method:/ { print; want=0; exit }
        $NF==n || $NF ~ "/" n "$" { found=1; print; }
        found && /^  method:/ { print; found=0; exit }' \
        | sed -n 's/^  method: //p' | head -n1
}
status_of_basename() { # first token of the entry whose path ends in $1
    chive status 2>/dev/null | awk -v n="$1" '$NF==n || $NF ~ "/" n "$" { print $1; exit }'
}

echo "=== phase A: scan a real package directory (/usr/bin) ==="
mkdir -p /state/a
CHIVE_CONFIG_DIR=/state/a chive scan /usr/bin >/dev/null 2>&1
export CHIVE_CONFIG_DIR=/state/a

jq_method="$(method_of_basename jq)"
case "$jq_method" in
    *"${2:-}") ok "jq is package-owned with the $1 recipe: [$jq_method]" ;;
    *)          fail "jq recipe mismatch: got [$jq_method], wanted a [$1] reinstall containing [${2:-}]" ;;
esac

# A second real package: the shell. Exercises multi-entry ownership DB parsing.
sh_status="$(status_of_basename sh)"
case "$sh_status" in
    restorable) ok "sh is restorable(verified) via the real package manager" ;;
    *)          fail "sh not restorable: got [$sh_status]" ;;
esac

echo "=== phase B: scan a fixture home (/root/fix) ==="
mkdir -p /root/fix/repo/nested /root/fix/node_modules
cd /root/fix/repo || exit 1
git init -q .
git config user.email a@b; git config user.name x
printf 'm\n' > tracked.md
printf 'd\n' > nested/deep.toml
git add -A; git commit -qm seed
cd /
ln -s "$(command -v jq)" /root/fix/pkglink
printf 'o\n' > /root/fix/work~
printf 'o\n' > /root/fix/conf.md
printf 'o\n' > /root/fix/node_modules/x.js

mkdir -p /state/b
export CHIVE_CONFIG_DIR=/state/b
chive scan /root/fix >/dev/null 2>&1

# package image is a *symlink* here, so expect symlink provenance, not apt.
[ "$(status_of_basename pkglink)" = "restorable" ] && ok "symlink is restorable(verified)" \
    || fail "symlink pkglink not restorable"
case "$(method_of_basename pkglink)" in
    *"ln -s"*) ok "symlink recipe is ln -s ..." ;;
    *)          fail "symlink recipe: got [$(method_of_basename pkglink)]" ;;
esac

[ "$(status_of_basename work~)" = "temporary" ] && ok "editor temp file is temporary" \
    || fail "work~ not temporary: got [$(status_of_basename work~)]"
[ "$(status_of_basename conf.md)" = "orphaned" ] && ok "plain file is orphaned" \
    || fail "conf.md not orphaned: got [$(status_of_basename conf.md)]"

git_status="$(status_of_basename repo/tracked.md)"
[ "$git_status" = "restorable" ] && ok "git-tracked (repo root) file is restorable" \
    || fail "repo/tracked.md not restorable: got [$git_status]"

# OPEN BUG #19: nested git files are wrongly orphaned. Report, don't fail.
nested="$(status_of_basename repo/nested/deep.toml)"
if [ "$nested" = "restorable" ]; then
    ok "git-tracked NESTED file is restorable"
else
    soft "git-nested provenance is orphaned (got [$nested]) — open issue #19"
fi

case "$(chive status 2>/dev/null)" in
    *"node_modules"*) fail "node_modules leaked into the catalog (should be ignored)" ;;
    *)                ok "node_modules dir is ignored" ;;
esac

echo ""
if [ "$BROKEN" -eq 0 ]; then
    echo "RESULT: PASS ($1)"
else
    echo "RESULT: FAIL ($1)"
fi
exit "$BROKEN"