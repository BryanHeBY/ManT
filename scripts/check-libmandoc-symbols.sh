#!/usr/bin/env bash
# Reject unnamespaced C definitions in libmandoc-rs's downstream static link.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

LIBMANDOC_PACKAGE_ID=$(cargo metadata --format-version=1 --no-deps \
  | python3 -c 'import json, sys; print(next(package["id"] for package in json.load(sys.stdin)["packages"] if package["name"] == "libmandoc-rs"))')
ARCHIVE=$(cargo build --locked --package libmandoc-rs --all-features \
  --message-format=json \
  | python3 -c '
import json
from pathlib import Path
import sys

package_id = sys.argv[1]
for line in sys.stdin:
    record = json.loads(line)
    if (record.get("reason") == "build-script-executed"
            and record.get("package_id") == package_id):
        print(Path(record["out_dir"]) / "libmant_mandoc.a")
' "$LIBMANDOC_PACKAGE_ID")
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
