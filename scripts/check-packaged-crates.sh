#!/usr/bin/env bash
# Verify the exact source sets shipped in all independently versioned crates.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PACKAGES=(mant-ir mant-protocol libmandoc-rs mant-sources mant-codec mant-loader mant-engine mant-ui mant)
mkdir -p "$ROOT/target"
PACKAGE_CHECK_ROOT=$(mktemp -d "$ROOT/target/mant-package-check.XXXXXX")
trap 'rm -rf "$PACKAGE_CHECK_ROOT"' EXIT

mkdir -p "$PACKAGE_CHECK_ROOT/crates"
cp "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$PACKAGE_CHECK_ROOT/"

for package in "${PACKAGES[@]}"; do
  package_id=$(cargo pkgid --manifest-path "$ROOT/Cargo.toml" -p "$package")
  version=${package_id##*[#@]}
  archive="$ROOT/target/package/$package-$version.crate"
  destination="$PACKAGE_CHECK_ROOT/crates/$package"
  dependencies=()
  case "$package" in
    mant-protocol) dependencies=(mant-ir) ;;
    mant-codec) dependencies=(libmandoc-rs mant-ir) ;;
    mant-loader) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec) ;;
    mant-engine) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec mant-loader) ;;
    mant-ui) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec mant-loader mant-engine) ;;
    mant) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec mant-loader mant-engine mant-ui) ;;
  esac
  package_patches=()
  for dependency in "${dependencies[@]}"; do
    package_patches+=(
      --config "patch.crates-io.$dependency.path=\"$ROOT/crates/$dependency\""
    )
  done

  cargo package --manifest-path "$ROOT/Cargo.toml" --locked --no-verify \
    --allow-dirty -p "$package" "${package_patches[@]}"
  mkdir -p "$destination"
  tar -xzf "$archive" --strip-components=1 -C "$destination"
  if [[ -f $destination/Cargo.toml.orig ]]; then
    mv "$destination/Cargo.toml.orig" "$destination/Cargo.toml"
  fi
done

# The unique extracted source path invalidates workspace fingerprints even for
# dirty same-version checks. Keep build products in the repository target tree
# (not a temporary filesystem); third-party dependency artifacts can be reused.
export CARGO_TARGET_DIR="$ROOT/target"
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked --workspace
# Exercise both packaged codec surfaces separately. This checks packaged tests,
# not native dependency exclusion: that requires an isolated minimal consumer.
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package mant-codec --no-default-features
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package mant-codec --no-default-features --features roff
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package mant-loader --no-default-features
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package mant-loader --no-default-features --features roff
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package libmandoc-rs --all-features

printf 'packaged crate verification succeeded\n'
