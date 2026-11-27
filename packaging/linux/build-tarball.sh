#!/usr/bin/env bash
# Builds the Linux release tarball (M5.6):
#   dist/orre-<version>-linux-<arch>.tar.gz
# holding the stripped release `orre`, its .desktop file and icon, the
# licences and a short README with the runtime libraries and glibc floor.
#
#   bash packaging/linux/build-tarball.sh            # build, then package
#   ORRE_SKIP_BUILD=1 bash packaging/linux/build-tarball.sh   # package only
#   ORRE_MAX_GLIBC=2.35 bash packaging/linux/build-tarball.sh # assert the floor
#
# ORRE_MAX_GLIBC, when set, is the highest glibc floor allowed: the script
# fails if the binary needs a newer glibc (compared with `sort -V`). Unset,
# there is no limit, so local builds on newer distros still work.
#
# Builds with the workspace release profile and no features: `dev-samples`
# and `dev-driver` never reach a release artifact. The release workflow
# (.github/workflows/release.yml) runs this, then smoke-test.sh.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$root"

# `path+file:///…#orre-app@0.1.0` → 0.1.0 (the workspace version).
version=$(cargo pkgid -p orre-app)
version=${version##*@}
arch=$(uname -m)
name="orre-$version-linux-$arch"
target_dir=${CARGO_TARGET_DIR:-$root/target}
out_dir=${ORRE_DIST_DIR:-$root/dist}

if [[ ${ORRE_SKIP_BUILD:-} != 1 ]]; then
  cargo build --release --locked -p orre-app
fi
binary="$target_dir/release/orre"
[[ -x $binary ]] || { echo "error: $binary not found; build it first" >&2; exit 1; }

# A release build is stripped (profile.release strip = true): no symbol
# table and no DWARF. A debug or unstripped binary fails here.
if readelf -S --wide "$binary" | grep -qE '\.(symtab|debug_info)\b'; then
  echo "error: $binary has symbols or debug info; is it a release build?" >&2
  exit 1
fi

# The newest glibc symbol version the binary needs is its glibc floor.
glibc=$(objdump -T "$binary" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n1)
[[ -n $glibc ]] || { echo "error: no GLIBC_ symbols in $binary" >&2; exit 1; }
max_glibc=${ORRE_MAX_GLIBC:-}
if [[ -n $max_glibc ]] &&
  [[ $(printf '%s\n' "$glibc" "$max_glibc" | sort -V | tail -n1) != "$max_glibc" ]]; then
  echo "error: $binary needs glibc $glibc, above ORRE_MAX_GLIBC=$max_glibc" >&2
  exit 1
fi
build_host=$(sed -n 's/^PRETTY_NAME="\{0,1\}\([^"]*\)"\{0,1\}$/\1/p' /etc/os-release 2>/dev/null || true)
build_host=${build_host:-unknown}

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
dir="$stage/$name"
mkdir -p "$dir"

install -m755 "$binary" "$dir/orre"
install -m644 LICENSE-APACHE LICENSE-MIT "$dir/"
install -m644 assets/fonts/inter/OFL.txt "$dir/OFL-Inter.txt"
install -m644 assets/fonts/geist-mono/OFL.txt "$dir/OFL-GeistMono.txt"
install -m644 packaging/linux/orre.desktop "$dir/orre.desktop"
install -m644 assets/brand/orre-app-icon-1024.png "$dir/orre.png"
sed -e "s/@VERSION@/$version/g" -e "s/@ARCH@/$arch/g" \
  -e "s/@GLIBC@/$glibc/g" -e "s|@BUILD_HOST@|$build_host|g" \
  packaging/linux/README.in > "$dir/README"
chmod 644 "$dir/README"

mkdir -p "$out_dir"
archive="$out_dir/$name.tar.gz"
# Stable order and owner, so the archive's listing doesn't depend on the host.
tar --sort=name --owner=0 --group=0 --numeric-owner \
  -C "$stage" -czf "$archive" "$name"
(cd "$out_dir" && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")

echo "glibc floor: $glibc (built on $build_host)"
echo "$archive"
