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
run "check Unix installer syntax" sh -n scripts/install.sh
run "check screenshot script syntax" bash -n scripts/update-reader-screenshot.sh
run "test Rust workspace" cargo test --locked --workspace
run "lint Rust workspace" \
  cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
run "compile fuzz targets" \
  cargo check --locked --manifest-path fuzz/Cargo.toml --bins
run "build release executable" cargo build --locked --release --package mant

MANT="$ROOT/target/release/mant"
if [[ ! -x "$MANT" ]]; then
  printf 'error: Cargo did not produce %s\n' "$MANT" >&2
  exit 1
fi

printf '\n==> smoke-test release executable\n'
help=$("$MANT" --help)
grep -Fq 'mant <NAME|MARKDOWN|-> [OPTIONS]' <<<"$help"
grep -Fq 'mant README.md' <<<"$help"
grep -Fq -- '--ui' <<<"$help"

query=$("$MANT" README.md --format json --compact)
grep -Fq '"schema":"mant.query/v5"' <<<"$query"
grep -Fq '"schema":"mant.document/v5"' <<<"$query"

printf '\nlocal verification succeeded\n'
printf '  executable: %s\n' "$MANT"
