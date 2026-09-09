#!/usr/bin/env bash
# Verify an embedded reader without acquisition, native parsing or process delivery.
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/tests/consumers/ui-ir/Cargo.toml"
export CARGO_TARGET_DIR="$ROOT/target"

DEPENDENCIES=$(cargo tree --locked --manifest-path "$MANIFEST" \
  --edges normal,build --target all --prefix none --format '{p}')
FORBIDDEN=$(printf '%s\n' "$DEPENDENCIES" | awk '
  $1 ~ /^(libmandoc-rs|cc|zstd|zstd-safe|zstd-sys|flate2|mant-engine|mant-loader|mant-query|mant-sources|ureq|tar|zip|crossbeam-channel|textwrap)$/ { print }
')
if [[ -n $FORBIDDEN ]]; then
  printf 'standalone IR reader has forbidden dependencies:\n%s\n' "$FORBIDDEN" >&2
  exit 1
fi

# Crossterm still uses parking_lot and signal-hook transitively for its event
# API. Their presence is not pager or process-lifecycle ownership by the UI.

cargo check --locked --manifest-path "$MANIFEST" --all-targets
cargo test --locked --manifest-path "$MANIFEST"
printf 'standalone IR reader verification succeeded\n'
