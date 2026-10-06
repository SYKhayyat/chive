#!/usr/bin/env bash
# Build the statically-linked `chive` the container harness needs.
#
# The container images (alpine, void, fedora, arch, ubuntu) have no `/nix/store`,
# so a glibc-dynamic binary dies on startup with an interpreter it cannot find.
# This is not a close call and it is why the whole rule engine is Rhai rather than
# embedded Python (D21).
#
# The fact that this script had to exist is the finding: D21's ruling and
# `why.md`'s adapter guidance both cite the musl-static build as load-bearing, and
# the repository could not produce one. `docker/integration/README.md` pointed at
# `nix-build /tmp/build-static.nix`, a file that was never committed, and the
# alternative it also pointed at was a *dynamic* build -- the exact thing that
# cannot work. An architectural constraint the repo cannot reproduce is a
# constraint held in the author's memory.
set -euo pipefail
cd "$(dirname "$0")/../.." || exit 1

TARGET=x86_64-unknown-linux-musl
OUT="docker/integration/context/chive"

if ! command -v rustup >/dev/null 2>&1; then
    echo "rustup is required to add the $TARGET target" >&2
    exit 1
fi
rustup target add "$TARGET"

echo "building $TARGET (this takes a few minutes)..."
cargo build --release --target "$TARGET"

BIN="target/$TARGET/release/chive"
[ -x "$BIN" ] || { echo "FATAL: $BIN was not produced" >&2; exit 1; }

# Assert the property rather than trusting the build to have it. The claim is
# load-bearing enough that a silently-dynamic binary should fail here, not three
# containers later.
if command -v file >/dev/null 2>&1 && file "$BIN" | grep -q 'dynamically linked'; then
    echo "FATAL: $BIN is dynamically linked; the container images cannot run it" >&2
    exit 1
fi
if command -v ldd >/dev/null 2>&1; then
    if ldd "$BIN" 2>&1 | grep -qv 'not a dynamic executable\|statically linked'; then
        echo "FATAL: $BIN links against something:" >&2
        ldd "$BIN" >&2 || true
        exit 1
    fi
fi

mkdir -p "$(dirname "$OUT")"
cp "$BIN" "$OUT"
echo "wrote $OUT ($(du -h "$OUT" | cut -f1))"
