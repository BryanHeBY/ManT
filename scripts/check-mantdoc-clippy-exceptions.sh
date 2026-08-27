#!/usr/bin/env bash
# Keep mantdoc's temporary structural Clippy debt monotonic and explicit.

set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$ROOT"

unexpected=$( { find crates/mantdoc -name '*.rs' -type f \
  -exec grep -hoE 'clippy::[a-z_]+' {} + || true; } | sort -u | grep -Ev \
  '^clippy::(struct_excessive_bools|too_many_arguments|too_many_lines)$' || true)
if [[ -n $unexpected ]]; then
  printf 'unexpected mantdoc Clippy exception(s):\n%s\n' "$unexpected" >&2
  exit 1
fi

while IFS=: read -r lint maximum; do
  actual=$( { find crates/mantdoc -name '*.rs' -type f \
    -exec grep -ho "clippy::$lint" {} + || true; } | wc -l)
  if (( actual > maximum )); then
    printf 'mantdoc Clippy exception budget grew: %s = %s (maximum %s)\n' \
      "$lint" "$actual" "$maximum" >&2
    exit 1
  fi
done <<'EOF'
struct_excessive_bools:4
too_many_arguments:47
too_many_lines:37
EOF

printf 'mantdoc Clippy exception budget valid\n'
