#!/usr/bin/env bash
# Verify pure IR queries without the application's loader or native features.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/tests/consumers/query-ir/Cargo.toml"
export CARGO_TARGET_DIR="$ROOT/target"

# The Rust-only canonical Markdown artifact is allowed; its optional roff
# dependency and every host/loading/update capability must remain disabled.
DEPENDENCIES=$(cargo tree --locked --manifest-path "$MANIFEST" \
  --edges normal,build --target all --prefix none --format '{p}')
FORBIDDEN=$(printf '%s\n' "$DEPENDENCIES" | awk '
  $1 ~ /^(libmandoc-rs|cc|zstd|zstd-safe|zstd-sys|flate2|mant-engine|mant-loader|mant-render|mant-ui|mant-sources|ureq|tar|zip)$/ { print }
')
if [[ -n $FORBIDDEN ]]; then
  printf 'standalone IR query has forbidden dependencies:\n%s\n' "$FORBIDDEN" >&2
  exit 1
fi

cargo check --locked --manifest-path "$MANIFEST" --all-targets
cargo test --locked --manifest-path "$MANIFEST"
printf 'standalone IR query verification succeeded\n'
