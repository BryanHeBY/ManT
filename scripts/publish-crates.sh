#!/usr/bin/env bash
# Publish the lockstep crates.io graph in dependency order.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

PACKAGES=(mant-ir mant-protocol libmandoc-rs mant-sources mant-engine mant-ui mant)

fail() {
  printf 'crates.io publication failed: %s\n' "$1" >&2
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

package_version() {
  local package=$1
  local id
  id=$(cargo pkgid -p "$package")
  printf '%s\n' "${id##*[#@]}"
}

registry_has_version() {
  local package=$1
  local version=$2
  cargo info --quiet --registry crates-io "$package@$version" >/dev/null 2>&1
}

wait_for_registry() {
  local package=$1
  local version=$2

  for attempt in {1..60}; do
    if registry_has_version "$package" "$version"; then
      printf 'crates.io index contains %s %s\n' "$package" "$version"
      return
    fi
    if ((attempt == 60)); then
      fail "timed out waiting for $package $version in the crates.io index"
    fi
    sleep 5
  done
}

version=$(workspace_version)
[[ -n $version ]] || fail 'Cargo.toml has no workspace package version'

tag=${MANT_RELEASE_TAG:-${GITHUB_REF_NAME:-}}
[[ -n $tag ]] || fail 'MANT_RELEASE_TAG is required'
[[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] \
  || fail "release tag '$tag' must use the form vMAJOR.MINOR.PATCH"
[[ ${tag#v} == "$version" ]] \
  || fail "release tag $tag does not match workspace version $version"

[[ -n ${CARGO_REGISTRY_TOKEN:-} ]] \
  || fail 'CARGO_REGISTRY_TOKEN is required (use crates.io Trusted Publishing in CI)'

for package in "${PACKAGES[@]}"; do
  actual=$(package_version "$package")
  [[ $actual == "$version" ]] \
    || fail "$package has version $actual, expected lockstep version $version"
done

git diff --quiet && git diff --cached --quiet \
  || fail 'the Git worktree must be clean'

# Exact internal dependencies make publication inherently sequential: a
# dependent crate cannot even be packaged against crates.io until its newly
# published predecessor is visible in the index. Validate each package at the
# last reversible boundary, then publish and wait before advancing the graph.
for package in "${PACKAGES[@]}"; do
  if registry_has_version "$package" "$version"; then
    printf 'skipping existing crates.io release %s %s\n' "$package" "$version"
  else
    cargo package --locked --no-verify -p "$package"
    cargo publish --locked -p "$package"
  fi
  wait_for_registry "$package" "$version"
done

printf 'published every ManT crate at %s\n' "$version"
