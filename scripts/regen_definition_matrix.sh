#!/usr/bin/env bash
# Regenerate the definition row-machine matrix snapshots from the pinned
# CVS reference binary. Expectations must come from this run, never by hand
# (see tests/roff_lowering/definition_matrix.rs for the row-grouping rule).
set -euo pipefail
REF="${MANT_REFERENCE:-target/mandoc-migration/reference/mandoc}"
[ -x "$REF" ] || { echo "reference binary not found: $REF" >&2; exit 1; }
cd "$(dirname "$0")/.."
for source in crates/mant-engine/tests/roff_lowering/definition_matrix/cases/*.1; do
  name=$(basename "$source" .1)
  "$REF" -Tascii "$source" | sed -e ':a' -e 's/\(.\)\x08\1/\1/g' -e 's/_\x08\(.\)/\1/g' -e 'ta' \
    -e 's/^[[:space:]]*//' -e 's/[[:space:]]\+/ /g' -e 's/[[:space:]]*$//' \
    | grep -v '^$' | grep -v '(1)$' | grep -Ev '^[A-Z][A-Z ]*$' \
    | grep -Ev '^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
    | grep -Ev '^(Linux|macOS|Darwin|Apple|Ubuntu|Debian|NetBSD|FreeBSD|OpenBSD|AT&T|GNU) ' \
    > "crates/mant-engine/tests/roff_lowering/definition_matrix/cases/$name.expected"
done
echo "snapshots regenerated under crates/mant-engine/tests/roff_lowering/definition_matrix/cases/"
