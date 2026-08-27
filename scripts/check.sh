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

# These read-only preflight checks do not create Cargo artifacts and are
# independent. Run them together before serial Cargo work so a routine gate
# does not spend most of its wall time waiting on shell syntax and audit scans.
# Cargo commands below intentionally remain serialized: sharing one `target`
# directory makes concurrent Cargo processes contend on its package lock.
declare -a preflight_pids=()
declare -a preflight_labels=()

run_preflight() {
  local label=$1
  shift
  printf '\n==> %s\n' "$label"
  printf '$'
  printf ' %q' "$@"
  printf '\n'
  "$@" &
  preflight_pids+=("$!")
  preflight_labels+=("$label")
}

wait_preflight() {
  local index
  local failed=0
  for index in "${!preflight_pids[@]}"; do
    if ! wait "${preflight_pids[$index]}"; then
      printf 'preflight failed: %s\n' "${preflight_labels[$index]}" >&2
      failed=1
    fi
  done
  (( failed == 0 )) || exit 1
}

run_preflight "check Rust formatting" cargo fmt --all --check
run_preflight "check Unix installer syntax" sh -n scripts/install.sh
run_preflight "check manual packaging script syntax" bash -n scripts/package-manuals.sh
run_preflight "check protocol snapshot script syntax" bash -n scripts/update-protocol-schema-snapshot.sh
run_preflight "check screenshot script syntax" bash -n scripts/update-reader-screenshot.sh
run_preflight "check product build script syntax" bash -n scripts/build-and-smoke.sh
run_preflight "check CI verification script syntax" bash -n scripts/find-successful-ci.sh
run_preflight "check mantdoc migration verification script syntax" \
  bash -n scripts/check-mantdoc.sh
run_preflight "check mantdoc conformance manifests" \
  python3 scripts/check-mantdoc-conformance-manifests.py
run_preflight "check roff fidelity audit" python3 scripts/audit-roff-fidelity.py --self-check
run_preflight "check roff structure audit" python3 scripts/audit-roff-structure.py --self-check
run_preflight "check roff CommonMark projection audit" \
  python3 scripts/audit-roff-projection.py --self-check
run_preflight "check roff renderer-layout audit" \
  python3 scripts/audit-roff-layout.py --self-check
run_preflight "check roff audit coverage contract" \
  python3 scripts/check-roff-audit-coverage.py
wait_preflight

run "test Rust workspace with all features" \
  cargo test --locked --workspace --all-features
run "test published crate source sets" bash scripts/check-packaged-crates.sh
run "build roff CommonMark projection profiler" \
  cargo build --locked --package mant-engine --example roff_projection_profile
run "gate roff fixtures through the CommonMark projection" \
  python3 scripts/audit-roff-projection.py --fixtures --recheck-recorded \
  --verify --findings-only
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
