#!/usr/bin/env bash
# Smoke-tests a Linux release tarball from build-tarball.sh, as a user
# would get it: unpack, check the contents, run the binary.
#
#   bash packaging/linux/smoke-test.sh dist/orre-<version>-linux-<arch>.tar.gz
#
# Needs xvfb-run and xwininfo (Debian/Ubuntu: xvfb, xauth, x11-utils) and, for the window launch, a
# Vulkan driver (mesa-vulkan-drivers gives a software one). Fails on a
# missing file, an unstripped binary, an unresolved or undocumented shared
# library, a `--showcase` flag (dev-samples) or a launch that dies.
set -euo pipefail

archive=${1:?usage: smoke-test.sh <orre-…-linux-….tar.gz>}
archive=$(realpath "$archive")
name=$(basename "$archive" .tar.gz)
version=${name#orre-}
version=${version%%-linux-*}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
fail() { echo "smoke test FAILED: $*" >&2; exit 1; }

echo "== contents"
tar -tzf "$archive" | sort > "$work/listing"
cat "$work/listing"
expected=$(printf '%s\n' "$name/" "$name/LICENSE-APACHE" "$name/LICENSE-MIT" \
  "$name/OFL-GeistMono.txt" "$name/OFL-Inter.txt" "$name/README" \
  "$name/orre" "$name/orre.desktop" "$name/orre.png" | sort)
diff -u <(echo "$expected") "$work/listing" || fail "unexpected tarball contents"

tar -xzf "$archive" -C "$work"
cd "$work/$name"
[[ -x orre ]] || fail "orre is not executable"

echo "== binary"
if readelf -S --wide orre | grep -qE '\.(symtab|debug_info)\b'; then
  fail "orre has symbols or debug info (not a stripped release build)"
fi
# Every library the binary links must resolve, and must be one the README
# lists (plus glibc and the toolchain's own). A new one has to be
# documented before it ships.
documented='^(libwayland-client|libxkbcommon|libxkbcommon-x11|libxcb|libX11|libX11-xcb|libvulkan|libfontconfig|libfreetype|libgcc_s|libc|libm|libdl|libpthread|librt|ld-linux-x86-64|ld-linux-aarch64)\.so'
readelf -d orre | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | tee "$work/needed"
while read -r lib; do
  [[ $lib =~ $documented ]] || fail "links $lib, which packaging/linux/README.in doesn't list"
done < "$work/needed"
if ldd orre | grep -q 'not found'; then
  ldd orre
  fail "a shared library is missing"
fi

echo "== orre --help (under Xvfb)"
xvfb-run -a ./orre --help > "$work/help" || fail "orre --help exited non-zero"
cat "$work/help"
if grep -q -- '--showcase' "$work/help"; then
  fail "--showcase is in the help: dev-samples is in the release build"
fi
if ./orre --showcase > /dev/null 2>&1; then
  fail "orre --showcase was accepted: dev-samples is in the release build"
fi

echo "== orre --version"
got=$(./orre --version)
[[ $got == "orre $version" ]] || fail "orre --version said '$got', expected 'orre $version'"
echo "$got"

echo "== headless commands on a scratch data dir"
# Creates the database (every migration runs), then reads it back.
# `accounts add` acts as the user, so it wants a terminal: `script` gives
# it one, as the person running a downloaded Orre would have.
export ORRE_DATA_DIR="$work/data"
script -qec './orre accounts add --name "Smoke test" --kind current --currency GBP' /dev/null \
  || fail "orre accounts add failed on an empty data dir"
./orre accounts | tee "$work/accounts"
grep -q "Smoke test" "$work/accounts" || fail "orre accounts didn't list the new account"

echo "== window launch (under Xvfb, ${ORRE_SMOKE_SECONDS:=10}s)"
# Inside one Xvfb: start the app, wait for its "Orre" window to be mapped
# (xwininfo, from x11-utils), and check it's still running after the
# timeout. An early exit is a crash or a missing runtime library.
# shellcheck disable=SC2016 # expanded by the inner bash
xvfb-run -a bash -c '
  ./orre > "$2" 2>&1 &
  pid=$!
  for _ in $(seq "$1"); do
    sleep 1
    kill -0 "$pid" 2>/dev/null || { echo "exited" > "$3"; exit; }
    xwininfo -root -tree | grep -q "\"Orre\": " && found=1
  done
  kill -0 "$pid" 2>/dev/null || { echo "exited" > "$3"; exit; }
  kill "$pid"; wait "$pid" 2>/dev/null
  if [[ -n ${found:-} ]]; then echo "ok" > "$3"; else echo "no-window" > "$3"; fi
' _ "$ORRE_SMOKE_SECONDS" "$work/gui.log" "$work/gui.status" || true
status=$(cat "$work/gui.status" 2>/dev/null || echo "no status")
if [[ $status != ok ]]; then
  cat "$work/gui.log"
  fail "window launch: $status"
fi
echo "the Orre window opened and was still running after ${ORRE_SMOKE_SECONDS}s: ok"

echo "smoke test passed: $name"
