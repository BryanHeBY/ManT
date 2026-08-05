#!/usr/bin/env bash
# Run the complete local verification boundary for the native ManT workspace.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

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
run "test Rust workspace" cargo test --locked --workspace
run "lint Rust workspace" \
  cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
run "build release executable" cargo build --locked --release --package mant

MANT="$ROOT/target/release/mant"
if [[ ! -x "$MANT" ]]; then
  printf 'error: Cargo did not produce %s\n' "$MANT" >&2
  exit 1
fi

printf '\n==> smoke-test release executable\n'
help=$("$MANT" --help)
grep -Fq 'mant <TOPIC|MARKDOWN|-> [OPTIONS]' <<<"$help"
grep -Fq 'mant README.md' <<<"$help"
grep -Fq -- '--ui' <<<"$help"

query=$("$MANT" README.md --format json --compact)
grep -Fq '"schema":"mant.query/v3"' <<<"$query"
grep -Fq '"schema":"mant.document/v3"' <<<"$query"

printf '\nlocal verification succeeded\n'
printf '  executable: %s\n' "$MANT"
