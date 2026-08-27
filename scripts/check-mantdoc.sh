#!/usr/bin/env bash
# Fast, complete native-migration work-loop verification.
#
# This is intentionally narrower than check.sh: it validates the mantdoc
# package and every parser/AST/IR differential lane while a migration change is
# being developed.  The product, packaging, documentation, and platform
# release boundary remains scripts/check.sh and CI.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

usage() {
  cat <<'EOF' >&2
usage: scripts/check-mantdoc.sh [archive] [--renderer] [--shards N] [--jobs N]

archive defaults to $HOME/dev/tmp/mandoc-1.14.6.tar.gz. M9's exact
ASCII/UTF-8/HTML comparison is always part of the strict native gate;
--renderer remains accepted as a no-op compatibility spelling. The complete
native canonical snapshot remains a separate release gate.
EOF
}

archive="$HOME/dev/tmp/mandoc-1.14.6.tar.gz"
if (( $# > 0 )) && [[ $1 != --* ]]; then
  archive=$1
  shift
fi

max_shards=12
max_jobs=20
cpu_count=$(getconf _NPROCESSORS_ONLN 2>/dev/null || printf '1')
if [[ ! "$cpu_count" =~ ^[1-9][0-9]*$ ]]; then
  cpu_count=1
fi
shards=${MANTDOC_SHARDS:-$cpu_count}
jobs=${MANTDOC_JOBS:-$cpu_count}
(( shards > max_shards )) && shards=$max_shards
(( jobs > max_jobs )) && jobs=$max_jobs

while (( $# > 0 )); do
  case $1 in
    --renderer)
      # M9 is strict and enabled unconditionally. Keep the historical switch
      # so existing local invocations do not fail merely because they now run
      # a stronger gate.
      ;;
    --shards|--jobs)
      if (( $# < 2 )); then
        usage
        exit 2
      fi
      if [[ $1 == --shards ]]; then
        shards=$2
      else
        jobs=$2
      fi
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
  shift
done

if [[ ! "$shards" =~ ^[1-9][0-9]*$ || ! "$jobs" =~ ^[1-9][0-9]*$ ]]; then
  echo "--shards and --jobs must be positive integers" >&2
  exit 2
fi
(( shards > max_shards )) && shards=$max_shards
(( jobs > max_jobs )) && jobs=$max_jobs
if [[ ! -f "$archive" ]]; then
  echo "upstream archive does not exist: $archive" >&2
  exit 2
fi

run_preflight() {
  local label=$1
  shift
  printf '\n==> %s\n' "$label"
  printf '$'
  printf ' %q' "$@"
  printf '\n'
  "$@" &
  preflight_pids+=("$!")
  preflight_labels+=("$label")
}

wait_preflights() {
  local index
  local failed=0
  for index in "${!preflight_pids[@]}"; do
    if ! wait "${preflight_pids[$index]}"; then
      printf 'preflight failed: %s\n' "${preflight_labels[$index]}" >&2
      failed=1
    fi
  done
  return "$failed"
}

# Read-only work can overlap with the first incremental Rust build.  Cargo
# invocations deliberately share target/ and remain serial after this point:
# a second writer would block on Cargo's package lock and lose the cache reuse
# that makes this work-loop quick.
declare -a preflight_pids=()
declare -a preflight_labels=()
run_preflight "check Rust formatting" cargo fmt --all --check
run_preflight "check mantdoc Clippy exception budget" \
  bash scripts/check-mantdoc-clippy-exceptions.sh

printf '\n==> test mantdoc with all features\n'
cargo test --quiet --locked --package mantdoc --all-features
wait_preflights

printf '\n==> lint mantdoc with all targets and features\n'
cargo clippy --quiet --locked --package mantdoc --all-targets --all-features -- \
  -D warnings \
  -D clippy::branches_sharing_code \
  -D clippy::or_fun_call \
  -D clippy::redundant_clone \
  -D clippy::unnecessary_struct_initialization

lanes=canonical,lint,m3,m4,m5,m6,m9
printf '\n==> run deterministic native differential lanes (%s shards, %s workers)\n' \
  "$shards" "$jobs"
python3 scripts/run-mantdoc-differential-shards.py "$archive" \
  --lanes "$lanes" --shards "$shards" --jobs "$jobs"

printf '\nmantdoc migration verification succeeded\n'
