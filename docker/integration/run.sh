#!/usr/bin/env bash
# chive integration harness — real package managers, in disposable containers.
#
# Each distro image carries a real native package manager, installs a known
# package into it (warming the ownership DB), copies in the *released chive
# binary* and a fixture tree, and lets chive scan that tree as a human would.
# This is where the adapters in `src/provenance/backends.toml` are tested: the
# real `dpkg -S` / `pacman -Qo` / `rpm -qf` / `apk info -W` / `xbps-query -f`
# output is what chive actually sees, so the probe commands and `name_match`
# regexes are validated against reality (not a mock or a fixture).
#
#   ./docker/integration/run.sh                    # every distro
#   DISTROS="ubuntu arch" ./docker/integration/run.sh   # subset
#   BUILD_ONLY=1 ./docker/integration/run.sh       # build images, run nothing
#
# Docker client: `DOCKER` (default = a docker binary on PATH), `DOCKER_HOST`
# (default = the engine's default). On this dev box dockerd is rootless under a
# nix-shell; see the companion `run_rootless_docker.sh` note in the README.
set -u
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "$0")/../.." && pwd))"
cd "$REPO_ROOT" || exit 1

DOCKER="${DOCKER:-docker}"
CTX="$REPO_ROOT/docker/integration/context"
DISTROS="${DISTROS:-ubuntu arch fedora alpine}"

[ -x "$CTX/chive" ] || { echo "FATAL: $CTX/chive is missing. Run: cargo build --release && cp target/release/chive $CTX/chive"; exit 1; }
# Stage the canonical scenario script into the (gitignored) build context.
cp "$REPO_ROOT/docker/integration/run-in-container.sh" "$CTX/run-in-container.sh"
chmod +x "$CTX/chive" "$CTX/run-in-container.sh"

summary=""; overall=0
for d in $DISTROS; do
    echo "############### BUILD $d ###############"
    # `--network host` lets buildkit's apt/dnf/xbps reach a real DNS (rootless
    # Docker's sandboxed build network cannot always resolve the host LAN DNS,
    # and these images must install `jq` + `git` during build).
    if ! "$DOCKER" build -q --network host -f "docker/integration/Dockerfile.$d" -t "chive-it-$d" "$CTX" ; then
        summary="${summary}\n  $d: BUILD-FAIL"; overall=1; continue
    fi
    if [ -n "${BUILD_ONLY:-}" ]; then
        summary="${summary}\n  $d: built"; continue
    fi
    echo "############### RUN $d ###############"
    if "$DOCKER" run --rm "chive-it-$d"; then
        summary="${summary}\n  $d: PASS"
    else
        summary="${summary}\n  $d: FAIL"; overall=1
    fi
done

echo ""; echo "===================== SUMMARY ====================="
printf "%b\n" "$summary"
echo "======================================================="
exit $overall