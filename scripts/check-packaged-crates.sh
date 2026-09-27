#!/usr/bin/env bash
# Verify the exact source sets shipped in all independently versioned crates.

set -euo pipefail

legacy=false
if (( $# > 0 )); then
  if (( $# != 1 )) || [[ $1 != --legacy ]]; then
    echo "usage: check-packaged-crates.sh [--legacy]" >&2
    exit 2
  fi
  legacy=true
fi

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
PACKAGES=(mant-ir mant-protocol libmandoc-rs mant-sources mant-codec mant-loader mant-query mant-render mant-engine mant-ui mant)
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
    mant-codec) dependencies=(libmandoc-rs mant-ir mant-protocol mant-query mant-ui) ;;
    mant-loader) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec) ;;
    # Query/render use codec without native features, so no libmandoc patch applies.
    mant-query) dependencies=(mant-ir mant-protocol mant-codec) ;;
    mant-render) dependencies=(mant-ir mant-protocol mant-codec) ;;
    mant-engine) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec mant-loader mant-query mant-render) ;;
    mant-ui) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec mant-loader mant-query mant-render mant-engine) ;;
    mant) dependencies=(libmandoc-rs mant-ir mant-protocol mant-sources mant-codec mant-loader mant-query mant-render mant-engine mant-ui) ;;
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
  # Cargo archives normalize source mtimes far into the past. The shared repo
  # target may otherwise treat an older same-version packaged rlib as Fresh.
  # Refresh only this disposable extraction, so every source set is compiled.
  find "$destination" -type f -exec touch {} +
  if [[ $package == mant ]]; then
    # Private unit tests exercise the production terminal boundary. Their shared
    # PTY harness and the embedded pager's complete licenses must ship too.
    test -f "$destination/tests/support/display_pty.py"
    test -f "$destination/src/delivery/pager/vendor/LICENSE-APACHE"
    test -f "$destination/src/delivery/pager/vendor/LICENSE-MIT"
  fi
done

# Packaged source tests cover the repository's full parser fixture corpus and
# four representative annotated pages. Keep fixtures outside crate archives,
# but make the same exact inputs available to this disposable test workspace.
mkdir -p "$PACKAGE_CHECK_ROOT/tests/fixtures/roff"
cp -R "$ROOT/tests/fixtures/roff/." \
  "$PACKAGE_CHECK_ROOT/tests/fixtures/roff/"
# Private packaged protocol tests include the versioned repository contracts
# at compile time; make those fixtures available beside the extracted crates.
cp -R "$ROOT/tests/contracts" "$PACKAGE_CHECK_ROOT/tests/"

# Refreshed extracted source mtimes invalidate stale same-version fingerprints.
# Keep build products in the repository target tree; unrelated third-party
# dependency artifacts can still be reused.
export CARGO_TARGET_DIR="$ROOT/target"
if $legacy; then
  # Archived full package matrix. Use only while changing the old Flow route.
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked --workspace
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-codec --no-default-features
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-codec --no-default-features --features roff
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-codec --no-default-features --features native-annotated
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-loader --no-default-features
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-loader --no-default-features --features roff
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-query --no-default-features
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-render --no-default-features
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant --no-default-features --lib
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant --features annotated-preview --lib
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package libmandoc-rs --all-features
else
  # Compile every published source set, including its test and example
  # targets, once. Run the real native -> Fixed -> CLI path from extracted
  # sources without relinking and executing the retired Flow test matrix.
  cargo check --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --workspace --all-targets --features mant/annotated-preview
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package libmandoc-rs --no-default-features --features annotated annotated::
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant-codec --features native-annotated annotated_fixed::
  cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
    --package mant --features annotated-preview --lib annotated_preview_
fi

printf 'packaged crate verification succeeded\n'
