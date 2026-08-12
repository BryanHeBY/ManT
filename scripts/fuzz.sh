#!/usr/bin/env bash
# Run every maintained fuzz target serially with a bounded local time budget.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

seconds=${1:-30}
if [[ ! "$seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "usage: scripts/fuzz.sh [seconds-per-target] [target ...]" >&2
  exit 2
fi
shift $(( $# > 0 ? 1 : 0 ))

if (( $# > 0 )); then
  targets=("$@")
else
  targets=(
    markdown_parse
    markdown_pipeline
    tldr_page
    roff_pipeline
    catalog_query
  )
fi

for target in "${targets[@]}"; do
  printf '\n==> fuzz %s for %ss\n' "$target" "$seconds"
  cargo +nightly fuzz run "$target" -- \
    "-max_total_time=$seconds" \
    -timeout=10 \
    -rss_limit_mb=2048 \
    -max_len=65536 \
    -print_final_stats=1 \
    -verbosity=0
done
