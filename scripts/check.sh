#!/usr/bin/env bash
# Run the complete local verification boundary for the native ManT workspace.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
export LIBMANDOC_RS_DENY_WARNINGS=1

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
run "check roff structure audit" python3 scripts/audit-roff-structure.py --self-check
run "check roff CommonMark projection audit" \
  python3 scripts/audit-roff-projection.py --self-check
run "check roff renderer-layout audit" python3 scripts/audit-roff-layout.py --self-check
run "check roff target-conservation audit" python3 scripts/audit-roff-targets.py --self-check
run "check roff semantic-entry audit" python3 scripts/audit-roff-semantics.py --self-check
run "check roff audit coverage contract" python3 scripts/check-roff-audit-coverage.py
run "test Rust workspace" cargo test --locked --workspace
run "test optional libmandoc features" \
  cargo test --locked --package libmandoc-rs --all-features
run "check libmandoc native symbol namespace" \
  bash scripts/check-libmandoc-symbols.sh
run "test published crate source sets" bash scripts/check-packaged-crates.sh
run "build roff CommonMark projection profiler" \
  cargo build --locked --package mant-engine --example roff_projection_profile
run "build roff target-conservation profiler" \
  cargo build --locked --package mant-engine --example roff_target_profile
run "build roff semantic-entry profiler" \
  cargo build --locked --package mant-engine --example roff_semantic_profile
run "gate roff fixtures through the CommonMark projection" \
  python3 scripts/audit-roff-projection.py --fixtures --recheck-recorded \
  --verify --findings-only
run "gate roff fixtures through target conservation" \
  python3 scripts/audit-roff-targets.py --fixtures --recheck-recorded \
  --verify --findings-only
run "gate roff fixtures through semantic-entry precision" \
  python3 scripts/audit-roff-semantics.py --fixtures --recheck-recorded \
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
