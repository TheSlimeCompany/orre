#!/usr/bin/env bash
# Builds the unsigned macOS disk image (M5.6), on macOS (Apple Silicon):
#   dist/orre-<version>-macos-<arch>.dmg   (volume "Orre": Orre.app + Applications)
#
#   bash packaging/macos/build-dmg.sh             # build, then package
#   ORRE_SKIP_BUILD=1 bash packaging/macos/build-dmg.sh   # package only
#
# v0.1 has no Apple Developer account, so the app is not Developer ID
# signed or notarised. It is always ad-hoc signed (`codesign --sign -`),
# once the bundle is fully assembled (binary, Info.plist, Orre.icns), and
# the build stops if the result is anything but an ad-hoc signature.
# Why always, rather than only when needed: Apple Silicon refuses to run
# code without a valid signature, and a bundle whose contents aren't
# sealed by a signature tends to get Gatekeeper's "is damaged" alert
# rather than the usual "can't verify the developer" prompt that
# docs/install.md walks through. That is our reading of how macOS
# behaves, not something this script has checked on a downloaded copy.
# An ad-hoc signature is not an identity: it doesn't clear Gatekeeper for
# a downloaded app. No Apple account, certificate or secret is used here.
#
# Builds with the workspace release profile and no features: `dev-samples`
# and `dev-driver` never reach a release artifact.
set -euo pipefail

[[ $(uname -s) == Darwin ]] || { echo "error: build-dmg.sh runs on macOS" >&2; exit 1; }

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"

# The oldest macOS the binary is built for, and so the plist's
# LSMinimumSystemVersion. 11.0 is the first macOS on Apple Silicon and
# Rust's default for aarch64-apple-darwin; set here so the two can't drift.
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}

# `path+file:///…#orre-app@0.1.0` → 0.1.0 (the workspace version).
version=$(cargo pkgid -p orre-app)
version=${version##*@}
arch=$(uname -m)
name="orre-$version-macos-$arch"
target_dir=${CARGO_TARGET_DIR:-$root/target}
out_dir=${ORRE_DIST_DIR:-$root/dist}

if [[ ${ORRE_SKIP_BUILD:-} != 1 ]]; then
  cargo build --release --locked -p orre-app
fi
binary="$target_dir/release/orre"
[[ -x $binary ]] || { echo "error: $binary not found; build it first" >&2; exit 1; }

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
app="$stage/dmg/Orre.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

install -m755 "$binary" "$app/Contents/MacOS/orre"
sed -e "s/@VERSION@/$version/g" -e "s/@MIN_MACOS@/$MACOSX_DEPLOYMENT_TARGET/g" \
  packaging/macos/Info.plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"

# Orre.icns from the 1024 px app icon, every size iconutil expects.
iconset="$stage/Orre.iconset"
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" assets/brand/orre-app-icon-1024.png \
    --out "$iconset/icon_${size}x${size}.png" > /dev/null
  double=$((size * 2))
  sips -z "$double" "$double" assets/brand/orre-app-icon-1024.png \
    --out "$iconset/icon_${size}x${size}@2x.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Orre.icns"

# Licences beside the app in the image, where a user can read them.
install -m644 LICENSE-APACHE LICENSE-MIT "$stage/dmg/"
install -m644 assets/fonts/inter/OFL.txt "$stage/dmg/OFL-Inter.txt"
install -m644 assets/fonts/geist-mono/OFL.txt "$stage/dmg/OFL-GeistMono.txt"

# Ad-hoc signature only (see the top of this file). `-` is the ad-hoc
# identity; there is deliberately no other identity, keychain or team.
codesign --force --deep --sign - --timestamp=none "$app"
codesign --verify --deep --strict "$app"
if codesign -dv "$app" 2>&1 | grep -q '^Authority='; then
  echo "error: Orre.app has a certificate identity; v0.1 is ad-hoc signed only" >&2
  exit 1
fi
codesign -dv "$app" 2>&1 | grep -q 'Signature=adhoc' \
  || { echo "error: Orre.app is not ad-hoc signed" >&2; exit 1; }

# Drag-to-install layout: the app and a link to /Applications.
ln -s /Applications "$stage/dmg/Applications"

mkdir -p "$out_dir"
dmg="$out_dir/$name.dmg"
rm -f "$dmg"
hdiutil create -volname Orre -srcfolder "$stage/dmg" -fs HFS+ \
  -format UDZO -ov "$dmg" > /dev/null
(cd "$out_dir" && shasum -a 256 "$name.dmg" > "$name.dmg.sha256")

echo "unsigned (ad-hoc signed) macOS $arch, minimum macOS $MACOSX_DEPLOYMENT_TARGET"
echo "$dmg"
