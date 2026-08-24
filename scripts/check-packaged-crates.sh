#!/usr/bin/env bash
# Verify the exact source sets shipped in all independently versioned crates.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PACKAGES=(mant-ir mant-protocol libmandoc-rs mant-sources mant-engine mant-ui mant)
PACKAGE_CHECK_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/mant-package-check.XXXXXX")
trap 'rm -rf "$PACKAGE_CHECK_ROOT"' EXIT

mkdir -p "$PACKAGE_CHECK_ROOT/crates"
cp "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$PACKAGE_CHECK_ROOT/"

for package in "${PACKAGES[@]}"; do
  package_id=$(cargo pkgid --manifest-path "$ROOT/Cargo.toml" -p "$package")
  version=${package_id##*[#@]}
  archive="$ROOT/target/package/$package-$version.crate"
  destination="$PACKAGE_CHECK_ROOT/crates/$package"

  cargo package --manifest-path "$ROOT/Cargo.toml" --locked --no-verify \
    --allow-dirty -p "$package"
  mkdir -p "$destination"
  tar -xzf "$archive" --strip-components=1 -C "$destination"
  if [[ -f $destination/Cargo.toml.orig ]]; then
    mv "$destination/Cargo.toml.orig" "$destination/Cargo.toml"
  fi
done

export CARGO_TARGET_DIR="$ROOT/target/package-check"
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked --workspace
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package libmandoc-rs --all-features

printf 'packaged crate verification succeeded\n'
