#!/usr/bin/env bash
# Run the complete local verification boundary for the native ManT workspace.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

profile=release
if (( $# > 0 )); then
  if [[ ${1:-} != --build-profile || $# != 2 ]]; then
    echo "usage: check.sh [--build-profile debug|release]" >&2
    exit 2
  fi
  profile=$2
fi
if [[ "$profile" != debug && "$profile" != release ]]; then
  echo "usage: check.sh [--build-profile debug|release]" >&2
  exit 2
fi

run() {
  local label=$1
  shift
  printf '\n==> %s\n' "$label"
  printf '$'
  printf ' %q' "$@"
  printf '\n'
  "$@"
}

run "check Rust formatting" cargo fmt --all --check
run "check Unix installer syntax" sh -n scripts/install.sh
run "check manual packaging script syntax" bash -n scripts/package-manuals.sh
run "check protocol snapshot script syntax" bash -n scripts/update-protocol-schema-snapshot.sh
run "check screenshot script syntax" bash -n scripts/update-reader-screenshot.sh
run "check product build script syntax" bash -n scripts/build-and-smoke.sh
run "check CI verification script syntax" bash -n scripts/find-successful-ci.sh
run "check CI native dependency script syntax" \
  bash -n scripts/install-ci-native-dependencies.sh
run "check roff fidelity audit" python3 scripts/audit-roff-fidelity.py --self-check
run "test Rust workspace" cargo test --locked --workspace
run "check read-only engine feature boundary" \
  cargo check --locked --package mant-engine --no-default-features
run "build docs.rs documentation" \
  env RUSTDOCFLAGS=-Dwarnings cargo doc --locked --workspace --all-features --no-deps
run "lint Rust workspace" \
  cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
run "compile fuzz targets" \
  cargo check --locked --manifest-path fuzz/Cargo.toml --bins

bash scripts/build-and-smoke.sh "$profile"

printf '\nlocal verification succeeded\n'
