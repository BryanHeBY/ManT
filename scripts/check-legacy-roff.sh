#!/usr/bin/env bash
# Explicit historical Flow/roff audit. Not part of the annotated default gate.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"
export LIBMANDOC_RS_DENY_WARNINGS=1

run() {
  local label=$1
  shift
  printf '\n==> %s\n' "$label"
  printf '$'
  printf ' %q' "$@"
  printf '\n'
  "$@"
}

run "check historical roff fidelity audit" python3 scripts/audit-roff-fidelity.py --self-check
run "test historical bidirectional roff content comparison" python3 scripts/roff_content_compare.py
run "test historical source-bound presentation explanations" python3 scripts/test_roff_content_explanations.py
run "test historical source-bound roff layout geometry" python3 scripts/test-roff-layout-geometry.py
run "test historical rendering matrix mutation sensitivity" python3 scripts/check-roff-behavior-matrix.py --self-test
run "test historical bounded rendering census" python3 scripts/audit-roff-rendering.py --self-test
run "test historical roff audit parallelism planning" python3 scripts/test_roff_audit_common.py
run "test historical full-corpus audit orchestration" python3 scripts/test_audit_roff_all.py
run "check historical roff structure audit" python3 scripts/audit-roff-structure.py --self-check
run "check historical roff CommonMark projection audit" \
  python3 scripts/audit-roff-projection.py --self-check
run "check historical roff renderer-layout audit" python3 scripts/audit-roff-layout.py --self-check
run "check historical roff target-conservation audit" python3 scripts/audit-roff-targets.py --self-check
run "check historical roff semantic-entry audit" python3 scripts/audit-roff-semantics.py --self-check
run "check historical roff audit coverage contract" python3 scripts/check-roff-audit-coverage.py
run "test historical Rust workspace" cargo test --locked --workspace
run "test historical roff codec" cargo test --locked --package mant-codec --features roff
run "test historical native manual loader" cargo test --locked --package mant-loader --features roff
run "test historical roff audit profilers" cargo test --locked --package mant-engine --examples
run "test historical optional libmandoc features" \
  cargo test --locked --package libmandoc-rs --all-features
run "test historical packaged crate feature matrix" bash scripts/check-packaged-crates.sh --legacy
run "build historical roff CommonMark projection profiler" \
  cargo build --locked --package mant-engine --example roff_projection_profile
run "build historical roff target-conservation profiler" \
  cargo build --locked --package mant-engine --example roff_target_profile
run "build historical roff semantic-entry profiler" \
  cargo build --locked --package mant-engine --example roff_semantic_profile
run "gate historical roff fixtures through CommonMark projection" \
  python3 scripts/audit-roff-projection.py --fixtures --recheck-recorded \
  --verify --findings-only
run "gate historical roff fixtures through target conservation" \
  python3 scripts/audit-roff-targets.py --fixtures --recheck-recorded \
  --verify --findings-only
run "gate historical roff fixtures through semantic-entry precision" \
  python3 scripts/audit-roff-semantics.py --fixtures --recheck-recorded \
  --verify --findings-only
run "gate historical source-bound fixture explanation queries" \
  python3 scripts/audit-roff-semantics.py --fixtures \
  --query-gold tests/fixtures/roff/ENTRY_QUERY_GOLD.json

printf '\nhistorical Flow/roff audit succeeded\n'
