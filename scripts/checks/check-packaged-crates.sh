#!/usr/bin/env bash
# Verify the exact source sets shipped in all independently versioned crates.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
source "$ROOT/scripts/checks/build-environment.sh"
export CARGO_TARGET_DIR="$ROOT/target"
PACKAGES=(mant-ir mant-protocol libmandoc-rs mant-sources mant-codec mant-loader mant-query mant-render mant-engine mant-ui mant)
mkdir -p "$ROOT/target"
PACKAGE_CHECK_ROOT=$(mktemp -d "$ROOT/target/mant-package-check.XXXXXX")
clean_workspace=false
cleanup() {
  local status=$?
  local scratch_status=0
  trap - EXIT
  if $clean_workspace; then
    # Cargo owns artifact layout. Scope cleanup to our packages and debug
    # profile, retaining third-party caches, release outputs and the oracle.
    local clean_args=(clean --offline --locked --manifest-path "$ROOT/Cargo.toml" --profile dev)
    for package in "${PACKAGES[@]}"; do
      clean_args+=(--package "$package")
    done
    if cargo "${clean_args[@]}"; then
      :
    else
      local clean_status=$?
      printf 'packaged workspace cleanup failed; third-party cache was retained\n' >&2
      if (( status == 0 )); then status=$clean_status; fi
    fi
  fi
  rm -rf -- "$PACKAGE_CHECK_ROOT" || scratch_status=$?
  if (( status == 0 )); then status=$scratch_status; fi
  if (( status == 0 )); then
    printf 'packaged crate verification succeeded\n'
  fi
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p "$PACKAGE_CHECK_ROOT/crates"
cp "$ROOT/Cargo.toml" "$ROOT/Cargo.lock" "$PACKAGE_CHECK_ROOT/"

for package in "${PACKAGES[@]}"; do
  package_id=$(cargo pkgid --manifest-path "$ROOT/Cargo.toml" -p "$package")
  version=${package_id##*[#@]}
  archive="$PACKAGE_CHECK_ROOT/artifacts/package/$package-$version.crate"
  destination="$PACKAGE_CHECK_ROOT/crates/$package"
  dependencies=()
  case "$package" in
    mant-protocol) dependencies=(mant-ir) ;;
    mant-codec) dependencies=(libmandoc-rs mant-ir) ;;
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
    --target-dir "$PACKAGE_CHECK_ROOT/artifacts" \
    --allow-dirty -p "$package" "${package_patches[@]}"
  mkdir -p "$destination"
  tar -xzf "$archive" --strip-components=1 -C "$destination"
  if [[ -f $destination/Cargo.toml.orig ]]; then
    mv "$destination/Cargo.toml.orig" "$destination/Cargo.toml"
  fi
  if [[ $package == mant ]]; then
    # Private unit tests exercise the production terminal boundary. Their shared
    # PTY harness and the embedded pager's complete licenses must ship too.
    test -f "$destination/tests/support/display_pty.py"
    test -f "$destination/src/delivery/pager/vendor/LICENSE-APACHE"
    test -f "$destination/src/delivery/pager/vendor/LICENSE-MIT"
  fi
done

# Fresh source identities ensure dirty same-version archives are actually
# rebuilt. Share third-party artifacts with checkout builds, but remove our
# debug artifacts on exit so deleted snapshots cannot accumulate orphan caches.
clean_workspace=true
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
  --package mant-query --no-default-features
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package mant-render --no-default-features
# Keep the executable's minimal unit surface independent of the workspace's
# default full-product feature unification, using the same packaged sources.
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package mant --no-default-features --lib
cargo test --manifest-path "$PACKAGE_CHECK_ROOT/Cargo.toml" --locked \
  --package libmandoc-rs --all-features
# The EXIT handler reports success only after scoped cleanup has completed.
