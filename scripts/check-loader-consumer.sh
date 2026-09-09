#!/usr/bin/env bash
# Verify read-only Markdown loading with an independent feature resolver.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
MANIFEST="$ROOT/tests/consumers/loader-markdown/Cargo.toml"
export CARGO_TARGET_DIR="$ROOT/target"

# A lockfile may list optional packages. Only enabled normal/build edges prove
# that Markdown-only loading avoids native parsing and acquisition/update code.
DEPENDENCIES=$(cargo tree --locked --manifest-path "$MANIFEST" \
  --edges normal,build --target all --prefix none --format '{p}')
FORBIDDEN=$(printf '%s\n' "$DEPENDENCIES" | awk '
  $1 ~ /^(libmandoc-rs|cc|zstd|zstd-safe|zstd-sys|flate2|mant-engine|mant-query|mant-render|mant-ui|ureq|tar|zip)$/ { print }
')
if [[ -n $FORBIDDEN ]]; then
  printf 'standalone Markdown loader has forbidden dependencies:\n%s\n' "$FORBIDDEN" >&2
  exit 1
fi

cargo check --locked --manifest-path "$MANIFEST" --all-targets
cargo test --locked --manifest-path "$MANIFEST"
printf 'standalone Markdown loader verification succeeded\n'
