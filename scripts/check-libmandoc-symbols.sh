#!/usr/bin/env bash
# Reject unnamespaced C definitions in libmandoc-rs's downstream static link.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

ARCHIVE=$(cargo build --locked --package libmandoc-rs --features annotated \
  --message-format=json \
  | python3 -c 'import json, pathlib, sys
paths = [pathlib.Path(item["out_dir"]) / "libmant_mandoc.a"
         for line in sys.stdin if (item := json.loads(line)).get("reason") == "build-script-executed"
         and pathlib.Path(item["out_dir"]).parent.name.startswith("libmandoc-rs-")]
if len(paths) != 1:
    raise SystemExit(f"expected one libmandoc-rs build archive, got {len(paths)}")
print(paths[0])')
[[ -f $ARCHIVE ]] || {
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
