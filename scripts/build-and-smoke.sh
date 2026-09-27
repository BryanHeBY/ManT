#!/usr/bin/env bash
# Build one Unix product profile and smoke-test the resulting executable.

set -euo pipefail

ROOT=${MANT_WORKSPACE:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}
cd "$ROOT"
export LIBMANDOC_RS_DENY_WARNINGS=1

profile=${1:-release}
mode=${2:-}
if (( $# > 2 )) || [[ -n $mode && $mode != --annotated ]]; then
  echo "usage: build-and-smoke.sh [debug|release] [--annotated]" >&2
  exit 2
fi
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
    echo "usage: build-and-smoke.sh [debug|release] [--annotated]" >&2
    exit 2
    ;;
esac
if [[ $mode == --annotated ]]; then
  cargo_args+=(--features annotated-preview)
fi

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
grep -Fq 'mant --input <PATH|-> [--input-format <FORMAT>] [OPTIONS]' <<<"$help"
grep -Fxq 'TLDR:' <<<"$help"
grep -Fxq 'ManT manual:' <<<"$help"
grep -Fq -- '--display' <<<"$help"

query=$("$mant" --input README.md --format json --compact)
grep -Fq '"schema":"mant.query/v0.12"' <<<"$query"
grep -Fq '"schema":"mant.document/v0.12"' <<<"$query"

if [[ $mode == --annotated ]]; then
  # Pinned CVS man_term.c::pre_TP prints the tag HEAD and then its BODY.
  # This exact fixture was rendered with the pinned CVS reference before
  # asserting its body witness in the annotated product process.
  fixture="$ROOT/tests/fixtures/roff/annotated-fixed-mentions.1"
  body=$("$mant" --annotated-preview --input "$fixture" --input-format roff \
    --format text --display direct --color never)
  grep -Fq 'own --foo body' <<<"$body"
fi

printf '\nproduct build succeeded\n'
printf '  executable: %s\n' "$mant"
