#!/usr/bin/env bash
# Smoke test for W2-11: `orre` exits non-zero when its window can't open,
# and still opens (and stays up) when it can.
#
# Usage: scripts/smoke-window-fail.sh <path-to-orre-binary>
#
# Needs xvfb-run; xwininfo is optional. Runs against a scratch ORRE_DATA_DIR.
# Can fold into W2-1's packaging/linux/smoke-test.sh later.
set -euo pipefail

if [[ $# -ne 1 ]]; then
    echo "usage: $0 <path-to-orre-binary>" >&2
    exit 2
fi
orre=$(realpath "$1")
if [[ ! -x "$orre" ]]; then
    echo "not an executable: $orre" >&2
    exit 2
fi
command -v xvfb-run >/dev/null || { echo "xvfb-run is required" >&2; exit 2; }

scratch=$(mktemp -d)
trap 'rm -rf "$scratch"' EXIT
export ORRE_DATA_DIR="$scratch/data"
mkdir -p "$ORRE_DATA_DIR"

failures=0

# 1. Failure path: no usable GPU driver, so the surface can't be created.
status=0
VK_ICD_FILENAMES=/nonexistent.json \
    __EGL_VENDOR_LIBRARY_FILENAMES=/nonexistent.json \
    LIBGL_DRIVERS_PATH=/nonexistent \
    timeout 30 xvfb-run -a "$orre" 2>"$scratch/fail.stderr" >/dev/null || status=$?
if [[ $status -ne 0 && $status -ne 124 ]] \
    && grep -q "couldn't open the window" "$scratch/fail.stderr"; then
    echo "PASS failure path: exit status $status, error logged"
else
    echo "FAIL failure path: exit status $status (want non-zero, not 124);" \
        "stderr tail:"
    tail -n 5 "$scratch/fail.stderr" | sed 's/^/    /'
    failures=$((failures + 1))
fi

# 2. Normal path: the app stays up with a window.
status=0
# shellcheck disable=SC2016 # expanded by the inner shell
timeout 60 xvfb-run -a bash -c '
    "$1" >/dev/null 2>"$2/normal.stderr" &
    pid=$!
    for _ in $(seq 1 16); do
        sleep 0.5
        kill -0 "$pid" 2>/dev/null || { wait "$pid"; echo "exited early: $?"; exit 1; }
    done
    if command -v xwininfo >/dev/null; then
        if xwininfo -root -tree | grep -q "\"Orre\""; then
            echo "window \"Orre\" found"
        else
            echo "no window named \"Orre\""; kill "$pid"; exit 1
        fi
    else
        echo "note: xwininfo not installed; window check skipped"
    fi
    kill "$pid"
    wait "$pid" 2>/dev/null || true
' _ "$orre" "$scratch" >"$scratch/normal.out" || status=$?
if [[ $status -eq 0 ]]; then
    echo "PASS normal path: still running after 8 s ($(tail -n 1 "$scratch/normal.out"))"
else
    echo "FAIL normal path: status $status: $(cat "$scratch/normal.out")"
    tail -n 5 "$scratch/normal.stderr" 2>/dev/null | sed 's/^/    /' || true
    failures=$((failures + 1))
fi

exit $((failures > 0))
