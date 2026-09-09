#!/usr/bin/env bash
# Verify report rendering without querying, loading, or native/application features.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/tests/consumers/render-ir/Cargo.toml"
export CARGO_TARGET_DIR="$ROOT/target"

# Canonical Markdown encoding is allowed; native roff parsing and every
# host/query/update capability must remain disabled for this independent client.
DEPENDENCIES=$(cargo tree --locked --manifest-path "$MANIFEST" \
  --edges normal,build --target all --prefix none --format '{p}')
FORBIDDEN=$(printf '%s\n' "$DEPENDENCIES" | awk '
  $1 ~ /^(libmandoc-rs|cc|zstd|zstd-safe|zstd-sys|flate2|mant-engine|mant-loader|mant-query|mant-ui|mant-sources|ureq|tar|zip)$/ { print }
')
if [[ -n $FORBIDDEN ]]; then
  printf 'standalone IR renderer has forbidden dependencies:\n%s\n' "$FORBIDDEN" >&2
  exit 1
fi

cargo check --locked --manifest-path "$MANIFEST" --all-targets
cargo test --locked --manifest-path "$MANIFEST"
printf 'standalone IR renderer verification succeeded\n'
