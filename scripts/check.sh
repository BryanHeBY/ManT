#!/usr/bin/env bash
# Run the current annotated-path verification boundary for ManT.
# Historical Flow/roff audits remain available via check-legacy-roff.sh.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
export LIBMANDOC_RS_DENY_WARNINGS=1

profile=debug
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

annotated_target_dir=${CARGO_TARGET_DIR:-$ROOT/target}
if [[ $annotated_target_dir != /* ]]; then
  annotated_target_dir="$ROOT/$annotated_target_dir"
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
run "test locked vendor source replay" \
  python3 crates/libmandoc-rs/scripts/test_sync_vendor.py
run "test CVS snapshot freezing" \
  python3 crates/libmandoc-rs/scripts/test_freeze_cvs_snapshot.py
run "test registered mandoc oracle identity" \
  python3 scripts/test_mandoc_oracle.py
run "test annotated Rust workspace" \
  cargo test --locked --workspace --features mant/annotated-preview \
  --exclude mant-loader --exclude mant-engine --exclude mant-ui
run "self-check annotated Fixed query gold" \
  python3 scripts/annotated_fixed_query_gold.py --self-check
run "build annotated Fixed query gold CLI" \
  cargo build --locked --package mant --features annotated-preview
run "gate annotated Fixed query gold" \
  python3 scripts/annotated_fixed_query_gold.py \
  --cli "$annotated_target_dir/debug/mant"
run "test Markdown-only codec" cargo test --locked --package mant-codec --no-default-features
run "check independent Markdown codec consumer" bash scripts/check-codec-consumer.sh
run "test Markdown-only loader" cargo test --locked --package mant-loader --no-default-features
run "check independent Markdown loader consumer" bash scripts/check-loader-consumer.sh
run "test independent query package" cargo test --locked --package mant-query --no-default-features
run "check independent IR query consumer" bash scripts/check-query-consumer.sh
run "test independent render package" cargo test --locked --package mant-render --no-default-features
run "check independent DTO render consumer" bash scripts/check-render-consumer.sh
run "check independent embedded reader consumer" bash scripts/check-ui-consumer.sh
run "test shared reader library" cargo test --locked --package mant-ui --lib
run "check isolated CLI capability combinations" python3 scripts/check-cli-features.py
run "test real terminal-cell geometry probe" \
  cargo test --locked --package mant-ui --example geometry_audit
run "test isolated annotated native boundary" \
  cargo test --locked --package libmandoc-rs --no-default-features --features annotated
run "check libmandoc native symbol namespace" \
  bash scripts/check-libmandoc-symbols.sh
run "test published crate source sets" \
  env CARGO_NET_OFFLINE=true bash scripts/check-packaged-crates.sh
run "check read-only engine feature boundary" \
  cargo check --locked --package mant-engine --no-default-features
run "build docs.rs documentation" \
  env RUSTDOCFLAGS=-Dwarnings cargo doc --locked --workspace \
  --features mant/annotated-preview --no-deps
run "lint Rust workspace" \
  env CARGO_INCREMENTAL=0 cargo clippy --locked --workspace --all-targets \
  --features mant/annotated-preview -- -D warnings
run "compile fuzz targets" \
  env CARGO_TARGET_DIR="$annotated_target_dir" \
  cargo check --locked --manifest-path fuzz/Cargo.toml --bins

bash scripts/build-and-smoke.sh "$profile" --annotated

printf '\nlocal verification succeeded\n'
