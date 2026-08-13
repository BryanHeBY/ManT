#!/usr/bin/env bash
# Package the platform-independent ManT manuals reproducibly.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

fail() {
  printf 'manual packaging failed: %s\n' "$1" >&2
  exit 1
}

workspace_version() {
  awk '
    /^\[workspace\.package\]$/ { workspace = 1; next }
    workspace && /^\[/ { exit }
    workspace && /^version[[:space:]]*=/ {
      gsub(/^[^"]*"|".*$/, "")
      print
      exit
    }
  ' Cargo.toml
}

version=$(workspace_version)
[[ -n $version ]] || fail "Cargo.toml has no workspace package version"

if [[ -n ${MANT_RELEASE_TAG:-} ]]; then
  [[ $MANT_RELEASE_TAG =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] \
    || fail "release tag '$MANT_RELEASE_TAG' must use the form vMAJOR.MINOR.PATCH"
  [[ ${MANT_RELEASE_TAG#v} == "$version" ]] \
    || fail "release tag $MANT_RELEASE_TAG does not match workspace version $version"
fi

archive_root="mant-$version-manuals"
dist="$ROOT/dist"
staging="$dist/.manual-staging"
package="$staging/$archive_root"
archive="$dist/$archive_root.tar.gz"

cleanup() {
  rm -rf "$staging"
}
trap cleanup EXIT

rm -rf "$staging"
mkdir -p "$package/LICENSES" "$package/manuals"
install -m 0644 docs/manuals/manifest.txt "$package/manuals/manifest.txt"
while IFS= read -r manual; do
  [[ -n $manual ]] || continue
  [[ $manual == *.md && $manual != */* ]] \
    || fail "invalid bundled manual name '$manual'"
  [[ -f docs/manuals/$manual ]] || fail "missing bundled manual '$manual'"
  install -m 0644 "docs/manuals/$manual" "$package/manuals/$manual"
done < docs/manuals/manifest.txt
install -m 0644 LICENSE "$package/LICENSE"
install -m 0644 THIRD_PARTY_NOTICES.md "$package/THIRD_PARTY_NOTICES.md"
install -m 0644 LICENSES/CC-BY-4.0.txt "$package/LICENSES/CC-BY-4.0.txt"

# GNU tar pins path order, timestamps, and ownership. `gzip -n` removes the
# final timestamp/name fields so identical inputs produce identical archives.
mkdir -p "$dist"
tar \
  --sort=name \
  --mtime=@0 \
  --owner=0 \
  --group=0 \
  --numeric-owner \
  -cf - \
  -C "$staging" \
  "$archive_root" \
  | gzip -n > "$archive"

(
  cd "$dist"
  sha256sum "$(basename "$archive")" > "$(basename "$archive").sha256"
)

printf 'packaged %s\n' "$archive"
cat "$archive.sha256"
