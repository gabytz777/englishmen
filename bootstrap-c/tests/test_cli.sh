#!/bin/sh
# Smoke test for emrun. Uses an image passed as $1, $EMRUN_TEST_IMAGE, a
# checked-in tiny image if one exists, or a generated minimal image consisting
# of a single OP_RETURN_NULL instruction. Skips cleanly if emrun is not built.

set -eu

BOOTSTRAP_DIR=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
RUNNER="$BOOTSTRAP_DIR/emrun"
image=''
tmpdir=''

cleanup() {
    if [ -n "$tmpdir" ] && [ -d "$tmpdir" ]; then
        rm -rf "$tmpdir"
    fi
}
trap cleanup 0 1 2 15

if [ "$#" -gt 0 ] && [ -n "${1:-}" ]; then
    image=$1
elif [ -n "${EMRUN_TEST_IMAGE:-}" ]; then
    image=$EMRUN_TEST_IMAGE
else
    for candidate in "$BOOTSTRAP_DIR/tests/tiny.emb" "$BOOTSTRAP_DIR/tests/smoke.emb"; do
        if [ -f "$candidate" ]; then
            image=$candidate
            break
        fi
    done
fi

if [ -z "$image" ]; then
    if ! command -v mktemp >/dev/null 2>&1; then
        echo "SKIP: no test image and mktemp is unavailable"
        exit 0
    fi
    tmpdir=$(mktemp -d "${TMPDIR:-/tmp}/emrun-test.XXXXXX")
    image="$tmpdir/tiny.emb"
    # EMBC, version 2, 0 constants, 0 functions, 1 instruction,
    # RETURN_NULL(a=0, b=0, line=1, col=1).
    printf 'EMBC\002\000\000\000\000\000\000\000\000\000\000\001\000\000\000\050\000\000\000\000\000\000\000\000\000\000\000\001\000\000\000\001\000\000\000' > "$image"
fi

if [ ! -f "$image" ]; then
    echo "FAIL: image not found: $image" >&2
    exit 1
fi

if [ ! -x "$RUNNER" ]; then
    echo "SKIP: $RUNNER is not built (run 'make' in $BOOTSTRAP_DIR)"
    exit 0
fi

echo "emrun smoke test: $image"
"$RUNNER" "$image"
echo "ok: emrun executed the image"
