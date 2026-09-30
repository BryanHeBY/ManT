#!/usr/bin/env bash
# Regenerate the escape/wave-1 matrix snapshots from the pinned CVS
# reference binary. Unlike scripts/regen_definition_matrix.sh this matrix is
# recorded with -Tutf8: ManT is a single-device UTF-8 renderer, and the
# device forks this matrix pins (`\:` NBRZW vs ASCII_BREAK, generated-cell
# overstrike order, `\p` pass rejections) are only observable on the UTF-8
# column (chars.c:47-53 with term.c:620-634). Expectations must come from
# this run, never by hand. The normalization is implemented in python3 so
# the NBSP/dash byte rules behave identically on every platform's sed.
set -euo pipefail
REF="${MANT_REFERENCE:-target/mandoc-migration/reference/mandoc}"
[ -x "$REF" ] || { echo "reference binary not found: $REF" >&2; exit 1; }
cd "$(dirname "$0")/.."
for source in crates/mant-engine/tests/roff_lowering/escape_matrix/cases/*.1; do
  name=$(basename "$source" .1)
  "$REF" -Tutf8 "$source" | python3 -c '
import sys

FURNITURE_FIRST_WORD = {
    "Linux", "macOS", "Darwin", "Apple", "Ubuntu", "Debian",
    "NetBSD", "FreeBSD", "OpenBSD", "AT&T", "GNU",
}

rows = []
for line in sys.stdin.read().splitlines():
    # Overstrike collapse, then fold NBSP and the en/em dashes.
    projected = []
    for ch in line:
        if ch == "\x08":
            if projected:
                projected.pop()
        elif ch == "\u00a0":
            projected.append(" ")
        elif ch in "\u2013\u2014":
            projected.append("-")
        else:
            projected.append(ch)
    row = " ".join("".join(projected).split())
    if not row or row.endswith("(1)"):
        continue
    # Section heads are single all-caps words; short or spaced all-caps
    # rows are content on this corpus (e.g. `X`, `A X`).
    if len(row) >= 3 and row.isascii() and row.isupper() and " " not in row:
        continue
    first = row.split(" ")[0]
    if first in FURNITURE_FIRST_WORD:
        continue
    head = first[:10]
    if len(head) == 10 and head[:4].isdigit() and head[4] == "-" and head[5:7].isdigit():
        continue
    rows.append(row)
sys.stdout.write("\n".join(rows) + ("\n" if rows else ""))
' > "crates/mant-engine/tests/roff_lowering/escape_matrix/cases/$name.expected"
done
echo "snapshots regenerated under crates/mant-engine/tests/roff_lowering/escape_matrix/cases/"
