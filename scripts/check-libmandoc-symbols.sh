#!/usr/bin/env bash
# Reject unnamespaced C definitions in libmandoc-rs's downstream static link.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

cargo build --locked --package libmandoc-rs --all-features
TARGET_DIR=$(cargo metadata --format-version=1 --no-deps \
  | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')
ARCHIVE=$(find "$TARGET_DIR/debug/build" -type f -name libmant_mandoc.a \
  -exec ls -t {} + | sed -n '1p')
[[ -n $ARCHIVE ]] || {
  printf 'libmandoc symbol audit failed: native archive not found\n' >&2
  exit 1
}

LEAKED=$(nm --defined-only -g "$ARCHIVE" \
  | awk '$2 == "T" || $2 == "D" || $2 == "B" { print $3 }' \
  | sort -u \
  | grep -Ev '^mant_' || true)
if [[ -n $LEAKED ]]; then
  printf 'libmandoc symbol audit failed: unnamespaced definitions:\n%s\n' \
    "$LEAKED" >&2
  exit 1
fi

printf 'libmandoc symbol namespace verification succeeded\n'
