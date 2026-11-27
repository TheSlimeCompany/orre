#!/usr/bin/env bash
# Smoke-tests a macOS disk image from build-dmg.sh: mounts it read-only and
# checks the bundle, its plist, its ad-hoc signature and the binary.
#
#   bash packaging/macos/smoke-test.sh dist/orre-<version>-macos-<arch>.dmg
#
# A CI-built image isn't quarantined, so Gatekeeper doesn't run here: a
# pass doesn't prove a downloaded .dmg opens (docs/install.md covers that).
# Doesn't open the window; the release job's runner may have no display.
set -euo pipefail

dmg=${1:?usage: smoke-test.sh <orre-…-macos-….dmg>}
name=$(basename "$dmg" .dmg)
version=${name#orre-}
version=${version%%-macos-*}

fail() { echo "smoke test FAILED: $*" >&2; exit 1; }

work=$(mktemp -d)
mnt="$work/mnt"
attached=0
# Set before the attach, so a failed attach still removes $work. Detach
# only once the image is mounted, and never remove $work while it is: a
# recursive delete through a mounted read-only image would fail and could
# leave it half-cleaned.
cleanup() {
  if [[ $attached == 1 ]]; then
    hdiutil detach "$mnt" -quiet || hdiutil detach "$mnt" -force -quiet || {
      echo "warning: couldn't detach $mnt; leaving $work in place" >&2
      return
    }
  fi
  rm -rf "$work"
}
trap cleanup EXIT
mkdir -p "$mnt"
hdiutil attach -nobrowse -readonly -mountpoint "$mnt" "$dmg" > /dev/null \
  || fail "hdiutil couldn't attach $dmg"
attached=1

echo "== image contents"
ls -la "$mnt"
for file in Orre.app LICENSE-APACHE LICENSE-MIT OFL-Inter.txt OFL-GeistMono.txt; do
  [[ -e $mnt/$file ]] || fail "$file is missing from the image"
done
[[ -L $mnt/Applications ]] || fail "the Applications link is missing"

app="$mnt/Orre.app"
bin="$app/Contents/MacOS/orre"
[[ -x $bin ]] || fail "Orre.app/Contents/MacOS/orre is missing or not executable"
[[ -f $app/Contents/Resources/Orre.icns ]] || fail "Orre.icns is missing"

echo "== Info.plist"
plist="$app/Contents/Info.plist"
plutil -lint "$plist"
check_key() {
  local got
  got=$(plutil -extract "$1" raw "$plist")
  [[ $got == "$2" ]] || fail "Info.plist $1 is '$got', expected '$2'"
  echo "$1 = $got"
}
check_key CFBundleIdentifier app.orre
check_key CFBundleExecutable orre
check_key CFBundlePackageType APPL
check_key CFBundleShortVersionString "$version"
check_key LSApplicationCategoryType public.app-category.finance

echo "== signature (ad-hoc only)"
codesign --verify --deep --strict "$app" || fail "the bundle's signature doesn't verify"
codesign -dv "$app" 2>&1 | tee "$work/codesign"
grep -q 'Signature=adhoc' "$work/codesign" || fail "Orre.app is not ad-hoc signed"
if grep -q '^Authority=' "$work/codesign"; then
  fail "Orre.app has a certificate identity; v0.1 is ad-hoc signed only"
fi

echo "== binary"
file "$bin"
archs=$(lipo -archs "$bin")
echo "lipo -archs: $archs"
[[ $archs == arm64 ]] || fail "orre's architectures are '$archs', expected exactly 'arm64' (v0.1 is Apple Silicon only)"
"$bin" --help > "$work/help" || fail "orre --help exited non-zero"
cat "$work/help"
if grep -q -- '--showcase' "$work/help"; then
  fail "--showcase is in the help: dev-samples is in the release build"
fi
if "$bin" --showcase > /dev/null 2>&1; then
  fail "orre --showcase was accepted: dev-samples is in the release build"
fi
got=$("$bin" --version)
[[ $got == "orre $version" ]] || fail "orre --version said '$got', expected 'orre $version'"
echo "$got"

echo "== headless commands on a scratch data dir"
# `accounts add` acts as the user, so it wants a terminal: BSD `script`
# gives it one, as the person running Orre would have.
export ORRE_DATA_DIR="$work/data"
script -q /dev/null "$bin" accounts add --name "Smoke test" --kind current --currency GBP \
  || fail "orre accounts add failed on an empty data dir"
"$bin" accounts | tee "$work/accounts"
grep -q "Smoke test" "$work/accounts" || fail "orre accounts didn't list the new account"

echo "smoke test passed: $name"
