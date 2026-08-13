#!/usr/bin/env bash
# Regenerate the checked-in structural snapshot for the current protocol version.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

snapshot=tests/contracts/protocol-schemas-v7.json
temporary=$(mktemp "${TMPDIR:-/tmp}/mant-protocol-schema.XXXXXX")
trap 'rm -f "$temporary"' EXIT

cargo run --quiet --locked -p mant -- --schema all > "$temporary"
mv "$temporary" "$snapshot"
trap - EXIT

printf 'updated %s\n' "$snapshot"
