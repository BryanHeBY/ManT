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
run "lint Rust workspace" \
  env CARGO_INCREMENTAL=0 cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
run "check Unix installer syntax" sh -n scripts/install.sh
run "check manual packaging script syntax" bash -n scripts/release/package-manuals.sh
run "check protocol snapshot script syntax" bash -n scripts/dev/update-protocol-schema-snapshot.sh
run "check screenshot script syntax" bash -n scripts/dev/update-reader-screenshot.sh
run "check product build script syntax" bash -n scripts/build/build-and-smoke.sh
run "check CI verification script syntax" bash -n scripts/ci/find-successful-ci.sh
run "check CI native dependency script syntax" \
  bash -n scripts/ci/install-ci-native-dependencies.sh
run "test locked vendor source replay" \
  python3 crates/libmandoc-rs/scripts/test_sync_vendor.py
run "test CVS snapshot freezing" \
  python3 crates/libmandoc-rs/scripts/test_freeze_cvs_snapshot.py
run "test registered mandoc oracle identity" \
  python3 -m scripts.roff.tests.test_mandoc_oracle
run "test repository tool entrypoints" \
  python3 -m unittest scripts.roff.tests.test_tool_entrypoints
run "test tagged release tool paths" \
  python3 -m unittest scripts.release.tests.test_source_tool_paths
run "test paired measurement scopes and statistics" \
  python3 -m unittest scripts.dev.performance.tests
run "check roff fidelity audit" python3 -m scripts.roff.audit.audit_roff_fidelity --self-check
run "test bidirectional roff content comparison" python3 -m scripts.roff.lib.roff_content_compare
run "test source-bound presentation explanations" python3 -m scripts.roff.tests.test_roff_content_explanations
run "test source-bound roff layout geometry" python3 -m scripts.roff.tests.test_roff_layout_geometry
run "test roff acceptance axes and structural edges" \
  python3 -m unittest scripts.roff.tests.test_acceptance_replay
run "test finite rule registry and mutation boundaries" \
  python3 -m unittest scripts.roff.tests.test_rule_boundary_replay
run "test escape grammar fixture bindings" \
  python3 -m unittest scripts.roff.tests.test_escape_rule_fixtures
run "test field rule source and owner bindings" \
  python3 -m unittest scripts.roff.tests.test_field_rule_fixtures
run "test Markdown hard-row fixture bindings" \
  python3 -m unittest scripts.roff.tests.test_markdown_rule_fixtures
run "test Markdown reader structural observation" \
  python3 -m unittest scripts.roff.tests.test_markdown_reader_observer
run "test controlled omission fixture bindings" \
  python3 -m unittest scripts.roff.tests.test_integrity_rule_fixtures
run "test source recovery card scopes" \
  python3 -m unittest scripts.roff.tests.test_source_recovery_cards
run "test generated device cell card scopes" \
  python3 -m unittest scripts.roff.tests.test_rule_projection_cards
run "test immutable roff consumer fixture bindings" \
  python3 -m unittest scripts.roff.tests.test_consumer_fixture_contract
run "test rendering matrix mutation sensitivity" python3 -m scripts.roff.audit.check_roff_behavior_matrix --self-test
run "test bounded rendering census" python3 -m scripts.roff.audit.audit_roff_rendering --self-test
run "test roff audit parallelism planning" python3 -m scripts.roff.tests.test_roff_audit_common
run "test full-corpus audit orchestration" python3 -m scripts.roff.tests.test_audit_roff_all
run "check roff structure audit" python3 -m scripts.roff.audit.audit_roff_structure --self-check
run "check roff CommonMark projection audit" \
  python3 -m scripts.roff.audit.audit_roff_projection --self-check
run "check roff renderer-layout audit" python3 -m scripts.roff.audit.audit_roff_layout --self-check
run "check roff target-conservation audit" python3 -m scripts.roff.audit.audit_roff_targets --self-check
run "check roff semantic-entry audit" python3 -m scripts.roff.audit.audit_roff_semantics --self-check
run "test roff audit coverage admission" python3 -m scripts.roff.audit.check_roff_audit_coverage --self-check
run "check roff audit coverage contract" python3 -m scripts.roff.audit.check_roff_audit_coverage
run "test Rust workspace" cargo test --locked --workspace
run "test Markdown-only codec" cargo test --locked --package mant-codec --no-default-features
run "test roff codec" cargo test --locked --package mant-codec --features roff
run "check independent Markdown codec consumer" bash scripts/checks/check-codec-consumer.sh
run "test Markdown-only loader" cargo test --locked --package mant-loader --no-default-features
run "test native manual loader" cargo test --locked --package mant-loader --features roff
run "check independent Markdown loader consumer" bash scripts/checks/check-loader-consumer.sh
run "test independent query package" cargo test --locked --package mant-query --no-default-features
run "check independent IR query consumer" bash scripts/checks/check-query-consumer.sh
run "test independent render package" cargo test --locked --package mant-render --no-default-features
run "check independent DTO render consumer" bash scripts/checks/check-render-consumer.sh
run "check independent embedded reader consumer" bash scripts/checks/check-ui-consumer.sh
run "check isolated CLI capability combinations" python3 -m scripts.checks.check_cli_features
run "test roff audit profilers" \
  cargo test --locked --package mant-engine --examples
run "test real terminal-cell geometry probe" \
  cargo test --locked --package mant-ui --example geometry_audit
run "test optional libmandoc features" \
  cargo test --locked --package libmandoc-rs --all-features
run "check libmandoc native symbol namespace" \
  bash scripts/checks/check-libmandoc-symbols.sh
run "test published crate source sets" bash scripts/checks/check-packaged-crates.sh
run "build roff CommonMark projection profiler" \
  cargo build --locked --package mant-engine --example roff_projection_profile
run "build roff target-conservation profiler" \
  cargo build --locked --package mant-engine --example roff_target_profile
run "build roff semantic-entry profiler" \
  cargo build --locked --package mant-engine --example roff_semantic_profile
run "gate roff fixtures through the CommonMark projection" \
  python3 -m scripts.roff.audit.audit_roff_projection --fixtures --recheck-recorded \
  --verify --findings-only
run "gate roff fixtures through target conservation" \
  python3 -m scripts.roff.audit.audit_roff_targets --fixtures --recheck-recorded \
  --verify --findings-only
run "gate roff fixtures through semantic-entry precision" \
  python3 -m scripts.roff.audit.audit_roff_semantics --fixtures --recheck-recorded \
  --verify --findings-only
run "gate source-bound fixture explanation queries" \
  python3 -m scripts.roff.audit.audit_roff_semantics --fixtures \
  --query-gold tests/fixtures/roff/ENTRY_QUERY_GOLD.json
run "check read-only engine feature boundary" \
  cargo check --locked --package mant-engine --no-default-features
run "build docs.rs documentation" \
  env RUSTDOCFLAGS=-Dwarnings cargo doc --locked --workspace --all-features --no-deps
run "compile fuzz targets" \
  cargo check --locked --manifest-path fuzz/Cargo.toml --bins

bash scripts/build/build-and-smoke.sh "$profile"

printf '\nlocal verification succeeded\n'
