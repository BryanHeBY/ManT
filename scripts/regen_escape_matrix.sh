#!/usr/bin/env bash
# Regenerate the escape/wave-1 matrix snapshots from the pinned CVS
# reference binary. Unlike scripts/regen_definition_matrix.sh this matrix is
# recorded with -Tutf8: ManT is a single-device UTF-8 renderer, and the
# device forks this matrix pins (`\:` NBRZW vs ASCII_BREAK, generated-cell
# overstrike order, `\p` pass rejections) are only observable on the UTF-8
# column (chars.c:47-53 with term.c:620-634). Expectations must come from
# this run, never by hand.
set -euo pipefail
REF="${MANT_REFERENCE:-target/mandoc-migration/reference/mandoc}"
[ -x "$REF" ] || { echo "reference binary not found: $REF" >&2; exit 1; }
cd "$(dirname "$0")/.."
for source in crates/mant-engine/tests/roff_lowering/escape_matrix/cases/*.1; do
  name=$(basename "$source" .1)
  "$REF" -Tutf8 "$source" | sed -e 's/.\x08//g' -e 's/\xc2\xa0/ /g' \
    -e 's/\xe2\x80\x93/-/g' -e 's/\xe2\x80\x94/-/g' \
    -e 's/^[[:space:]]*//' -e 's/[[:space:]]\+/ /g' -e 's/[[:space:]]*$//' \
    | grep -v '^$' | grep -v '(1)$' | grep -Ev '^[A-Z]{3,}$' \
    | grep -Ev '^[0-9]{4}-[0-9]{2}-[0-9]{2}' \
    | grep -Ev '^(Linux|macOS|Darwin|Apple|Ubuntu|Debian|NetBSD|FreeBSD|OpenBSD|AT&T|GNU) ' \
    > "crates/mant-engine/tests/roff_lowering/escape_matrix/cases/$name.expected"
done
echo "snapshots regenerated under crates/mant-engine/tests/roff_lowering/escape_matrix/cases/"
