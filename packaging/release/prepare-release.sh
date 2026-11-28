#!/usr/bin/env bash
# Checks a release tag and writes its release notes (SLI-35):
#
#   bash packaging/release/prepare-release.sh <tag> <notes-file>
#   ORRE_CHANGELOG=/tmp/CHANGELOG.md bash packaging/release/prepare-release.sh v0.1.0 notes.md
#
# The tag must be exactly `v<version>`, where <version> is the `orre-app`
# crate's version (the workspace version in Cargo.toml). The notes are that
# version's section of CHANGELOG.md (or ORRE_CHANGELOG): from its
# `## [<version>]` heading (optionally followed by ` - <date>`) up to the next
# `## ` heading or the link-reference block at the end (`[0.1.0]: https://…`),
# with leading and trailing blank lines stripped and the rest kept verbatim.
#
# On success it writes <notes-file>, prints the version as its last line, and
# appends `version=<version>` to $GITHUB_OUTPUT when that is set. On any
# failure it exits non-zero and leaves no <notes-file>. The release workflow
# (.github/workflows/release.yml) runs this in its publish job before it
# creates anything.
set -euo pipefail

die() { echo "error: $*" >&2; exit 1; }

[[ $# -eq 2 ]] || die "usage: prepare-release.sh <tag> <notes-file>"
tag=$1
notes=$2
# A stale notes file from an earlier run must not survive a failure.
rm -f -- "$notes"

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
changelog=${ORRE_CHANGELOG:-$root/CHANGELOG.md}
[[ -f $changelog ]] || die "$changelog not found"

version=$(cargo metadata --no-deps --format-version 1 --manifest-path "$root/Cargo.toml" |
  jq -r '.packages[] | select(.name == "orre-app") | .version')
[[ -n $version ]] || die "no orre-app package in $root/Cargo.toml"

[[ $tag == v?* ]] || die "tag '$tag' is not a version tag (expected v$version)"
[[ $tag == "v$version" ]] ||
  die "tag '$tag' does not match the orre-app version $version (expected v$version)"

tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

# Print the section's body, then drop blank lines at either end.
awk -v v="$version" '
  BEGIN { head = "## [" v "]" }
  !in_section {
    if ($0 == head || index($0, head " - ") == 1) { in_section = 1; found = 1 }
    next
  }
  /^## / || /^\[[^]]+\]: / { exit }
  { print }
  END { if (!found) exit 2 }
' "$changelog" |
  awk '
    NF { started = 1; printf "%s", held; held = ""; print; next }
    started { held = held $0 "\n" }
  ' >"$tmp" ||
  die "CHANGELOG has no '## [$version]' section ($changelog)"

[[ -s $tmp ]] || die "the '## [$version]' section of $changelog is empty"

mv -- "$tmp" "$notes"
trap - EXIT
if [[ -n ${GITHUB_OUTPUT:-} ]]; then
  echo "version=$version" >>"$GITHUB_OUTPUT"
fi
echo "release notes for $tag: $notes ($(wc -l <"$notes") lines)" >&2
echo "$version"
