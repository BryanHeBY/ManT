#!/usr/bin/env bash
# Verify the exact source sets shipped in all independently versioned crates.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PACKAGES=(mant-ir mant-protocol mantdoc mant-sources mant-engine mant-ui mant)
PACKAGE_CHECK_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/mant-package-check.XXXXXX")
trap 'rm -rf "$PACKAGE_CHECK_ROOT"' EXIT

mkdir -p "$PACKAGE_CHECK_ROOT/crates"
cp "$ROOT/Cargo.toml" "$PACKAGE_CHECK_ROOT/Cargo.toml"
cp "$ROOT/Cargo.lock" "$PACKAGE_CHECK_ROOT/"

for package in "${PACKAGES[@]}"; do
  package_id=$(cargo pkgid --manifest-path "$ROOT/Cargo.toml" -p "$package")
  version=${package_id##*[#@]}
  archive="$ROOT/target/package/$package-$version.crate"
  destination="$PACKAGE_CHECK_ROOT/crates/$package"
  dependencies=()
  case "$package" in
    mant-protocol) dependencies=(mant-ir) ;;
    mant-engine) dependencies=(mantdoc mant-ir mant-protocol mant-sources) ;;
    mant-ui) dependencies=(mant-engine mant-ir mant-protocol) ;;
    mant) dependencies=(mant-engine mant-ir mant-protocol mant-sources mant-ui) ;;
  esac
  package_patches=()
  for dependency in "${dependencies[@]}"; do
    package_patches+=(
      --config "patch.crates-io.$dependency.path=\"$ROOT/crates/$dependency\""
    )
  done

  cargo package --quiet --manifest-path "$ROOT/Cargo.toml" --locked --no-verify \
    --allow-dirty -p "$package" "${package_patches[@]}"
  mkdir -p "$destination"
  tar -xzf "$archive" --strip-components=1 -C "$destination"
  if [[ -f $destination/Cargo.toml.orig ]]; then
    mv "$destination/Cargo.toml.orig" "$destination/Cargo.toml"
  fi
done

# Each run must compile the just-extracted sources. Reusing one target tree
# across dirty same-version package checks can retain an artifact built from a
# previous source set and hide, or invent, an API compatibility failure.
export CARGO_TARGET_DIR="$PACKAGE_CHECK_ROOT/target"
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked --workspace
printf 'packaged crate verification succeeded\n'
