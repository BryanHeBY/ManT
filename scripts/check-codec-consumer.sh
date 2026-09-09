#!/usr/bin/env bash
# Verify the default codec through a separate workspace and feature resolver.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/tests/consumers/codec-markdown/Cargo.toml"
# Keep all build products in the repository, including this independent fixture.
export CARGO_TARGET_DIR="$ROOT/target"

# Unlike a workspace build, the standalone root cannot inherit the product's
# enabled native parser feature. Check every target's normal/build graph; the
# optional dependency's presence in a lockfile alone is not an enabled edge.
DEPENDENCIES=$(cargo tree --locked --manifest-path "$MANIFEST" \
  --edges normal,build --target all --prefix none --format '{p}')
FORBIDDEN=$(printf '%s\n' "$DEPENDENCIES" | awk '
  $1 ~ /^(libmandoc-rs|cc|mant-engine|mant-protocol|mant-loader|mant-query|mant-render|mant-ui|mant-sources)$/ { print }
')
if [[ -n $FORBIDDEN ]]; then
  printf 'standalone Markdown codec has forbidden dependencies:\n%s\n' "$FORBIDDEN" >&2
  exit 1
fi

cargo check --locked --manifest-path "$MANIFEST" --all-targets
cargo test --locked --manifest-path "$MANIFEST"
printf 'standalone Markdown codec verification succeeded\n'
