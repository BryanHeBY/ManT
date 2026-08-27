#!/usr/bin/env bash
# Publish independently versioned crates.io packages in dependency order.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

ALL_PACKAGES=(mant-ir mant-protocol mantdoc mant-sources mant-engine mant-ui mant)
CRATE_TAG_PACKAGES=(mant-ir mant-protocol mantdoc mant-sources mant-engine mant-ui)

fail() {
  printf 'crates.io publication failed: %s\n' "$1" >&2
  exit 1
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

tag=${MANT_RELEASE_TAG:-${GITHUB_REF_NAME:-}}
[[ -n $tag ]] || fail 'MANT_RELEASE_TAG is required'

publish_package=${MANT_PUBLISH_PACKAGE:-}
if [[ -n $publish_package ]]; then
  valid=false
  for package in "${CRATE_TAG_PACKAGES[@]}"; do
    if [[ $publish_package == "$package" ]]; then
      valid=true
      break
    fi
  done
  [[ $valid == true ]] || fail "unknown package '$publish_package'"
  PACKAGES=("$publish_package")
  version=$(package_version "$publish_package")
  [[ $tag == "$publish_package-v$version" ]] \
    || fail "crate tag $tag does not match $publish_package version $version"
else
  PACKAGES=("${ALL_PACKAGES[@]}")
  version=$(package_version mant)
  [[ $tag =~ ^v[0-9]+\.[0-9]+\.[0-9]+([.-][0-9A-Za-z.-]+)?$ ]] \
    || fail "product release tag '$tag' must use the form vMAJOR.MINOR.PATCH"
  [[ ${tag#v} == "$version" ]] \
    || fail "product release tag $tag does not match mant version $version"
fi

[[ -n ${CARGO_REGISTRY_TOKEN:-} ]] \
  || fail 'CARGO_REGISTRY_TOKEN is required (use crates.io Trusted Publishing in CI)'

git diff --quiet && git diff --cached --quiet \
  || fail 'the Git worktree must be clean'

# A dependent crate can only be published after every version allowed by its
# manifest is visible in crates.io. Validate each selected package at the last
# reversible boundary, then wait for its own version before advancing the graph.
for package in "${PACKAGES[@]}"; do
  version=$(package_version "$package")
  if registry_has_version "$package" "$version"; then
    printf 'skipping existing crates.io release %s %s\n' "$package" "$version"
  else
    cargo package --locked --no-verify -p "$package"
    cargo publish --locked -p "$package"
  fi
  wait_for_registry "$package" "$version"
done

if [[ -n $publish_package ]]; then
  printf 'published %s %s\n' "$publish_package" "$version"
else
  printf 'published every unpublished ManT crate selected by %s\n' "$tag"
fi
