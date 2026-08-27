#!/usr/bin/env bash
# Build one Unix product profile and smoke-test the resulting executable.

set -euo pipefail

ROOT=${MANT_WORKSPACE:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
cd "$ROOT"
profile=${1:-release}
case "$profile" in
  debug)
    cargo_args=(build --locked --package mant)
    output_dir=debug
    ;;
  release)
    cargo_args=(build --locked --release --package mant)
    output_dir=release
    ;;
  *)
    echo "usage: build-and-smoke.sh [debug|release]" >&2
    exit 2
    ;;
esac

printf '\n==> build %s executable\n' "$profile"
printf '$'
printf ' %q' cargo "${cargo_args[@]}"
printf '\n'
cargo "${cargo_args[@]}"

mant="$ROOT/target/$output_dir/mant"
if [[ ! -x "$mant" ]]; then
  printf 'error: Cargo did not produce %s\n' "$mant" >&2
  exit 1
fi

printf '\n==> smoke-test %s executable\n' "$profile"
help=$("$mant" --help)
grep -Fq 'mant <SELECTOR> [OPTIONS]' <<<"$help"
grep -Fq 'mant --input README.md' <<<"$help"
grep -Fq -- '--ui' <<<"$help"

query=$("$mant" --input README.md --format json --compact)
grep -Fq '"schema":"mant.query/v0.10"' <<<"$query"
grep -Fq '"schema":"mant.document/v0.10"' <<<"$query"

printf '\nproduct build succeeded\n'
printf '  executable: %s\n' "$mant"
