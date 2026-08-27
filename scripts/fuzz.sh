#!/usr/bin/env bash
# Run every maintained fuzz target with a bounded local time budget.  Each
# target has its own Cargo target directory, so independent libFuzzer builds do
# not contend on Cargo's workspace lock.  Corpus and artifact directories stay
# target-scoped under `fuzz/`, as cargo-fuzz expects.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

seconds=${1:-30}
if [[ ! "$seconds" =~ ^[1-9][0-9]*$ ]]; then
  echo "usage: scripts/fuzz.sh [seconds-per-target] [target ...]" >&2
  exit 2
fi
shift $(( $# > 0 ? 1 : 0 ))

if [[ -n ${FUZZ_JOBS:-} ]]; then
  jobs=$FUZZ_JOBS
else
  jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)
  if [[ ! "$jobs" =~ ^[1-9][0-9]*$ ]]; then
    jobs=$(sysctl -n hw.ncpu 2>/dev/null || printf '1')
  fi
  # Keep the normal developer/CI machine responsive while still exercising
  # independent targets concurrently.  Larger fuzz farms can opt in via
  # FUZZ_JOBS.
  (( jobs > 4 )) && jobs=4
fi

if [[ ! "$jobs" =~ ^[1-9][0-9]*$ ]]; then
  echo "FUZZ_JOBS must be a positive integer" >&2
  exit 2
fi

if (( $# > 0 )); then
  targets=("$@")
else
  targets=(
    markdown_parse
    markdown_pipeline
    tldr_page
    roff_pipeline
    mantdoc_scanner
    catalog_query
  )
fi

run_target() {
  local target=$1

  printf '\n==> fuzz %s for %ss\n' "$target" "$seconds"
  CARGO_TARGET_DIR="$ROOT/target/fuzz/$target" cargo +nightly fuzz run "$target" -- \
    "-max_total_time=$seconds" \
    -timeout=10 \
    -rss_limit_mb=2048 \
    -max_len=65536 \
    -print_final_stats=1 \
    -verbosity=0
}

printf '==> fuzz %s target(s), %s worker(s), %ss each\n' "${#targets[@]}" "$jobs" "$seconds"

declare -a running_pids=()
declare -a running_targets=()
declare -a failed_targets=()

reap_one() {
  local pid=${running_pids[0]}
  local target=${running_targets[0]}
  if ! wait "$pid"; then
    failed_targets+=("$target")
  fi
  running_pids=("${running_pids[@]:1}")
  running_targets=("${running_targets[@]:1}")
}

for target in "${targets[@]}"; do
  while (( ${#running_pids[@]} >= jobs )); do
    reap_one
  done
  run_target "$target" &
  running_pids+=("$!")
  running_targets+=("$target")
done

while (( ${#running_pids[@]} > 0 )); do
  reap_one
done

if (( ${#failed_targets[@]} > 0 )); then
  printf 'fuzz target(s) failed: %s\n' "${failed_targets[*]}" >&2
  exit 1
fi
