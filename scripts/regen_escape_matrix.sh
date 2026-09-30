#!/usr/bin/env bash
# Regenerate the escape/wave-1 matrix snapshots from the pinned CVS
# reference binary. Unlike scripts/regen_definition_matrix.sh this matrix is
# recorded with -Tutf8: ManT is a single-device UTF-8 renderer, and the
# device forks this matrix pins (`\:` NBRZW vs ASCII_BREAK, generated-cell
# overstrike order, `\p` pass rejections) are only observable on the UTF-8
# column (chars.c:47-53 with term.c:620-634). Expectations must come from
# this run, never by hand. The normalization is implemented in python3 so
# the NBSP/dash byte rules behave identically on every platform's sed. It
# mirrors normalize in crates/mant-engine/tests/roff_lowering/
# escape_matrix.rs (reference side, footed page): page furniture drops by
# position window, never by content — row 0 (plus an optional wrapped
# center line) is the header, the trailing non-blank block is the footer,
# and the page-edge blanks framing them go too. Every body row and every
# blank row between body rows is preserved.
set -euo pipefail
REF="${MANT_REFERENCE:-target/mandoc-migration/reference/mandoc}"
[ -x "$REF" ] || { echo "reference binary not found: $REF" >&2; exit 1; }
cd "$(dirname "$0")/.."
# Gate: expectations may only be regenerated from the registered pristine
# oracle. MANT_REFERENCE still overrides the binary location, but the binary
# it names must pass the full preflight (active registry attestation, binary
# hash, ascii/utf8/html profiles) — there is no bypass path.
python3 - "$REF" <<'PREFLIGHT'
import sys
from pathlib import Path

sys.path.insert(0, str(Path("scripts").resolve()))
import mandoc_oracle
import rebuild_reference_mandoc

root = Path.cwd().resolve()
binary = Path(sys.argv[1]).resolve()
try:
    attestation, value = rebuild_reference_mandoc.active_attestation(root)
    archive = mandoc_oracle.repository_path(
        root, value["source"]["archive"]["path"], "source archive")
    rebuild_reference_mandoc.verify_all(root, binary, archive, attestation, value["identity"])
except (OSError, ValueError, KeyError) as error:
    print(f"oracle preflight rejected {binary}: {error}", file=sys.stderr)
    raise SystemExit(1)
print(f"oracle preflight passed: {value['identity']}", file=sys.stderr)
PREFLIGHT
for source in crates/mant-engine/tests/roff_lowering/escape_matrix/cases/*.1; do
  name=$(basename "$source" .1)
  "$REF" -Tutf8 "$source" | python3 -c '
import sys


def section_token(token):
    if not token.endswith(")"):
        return False
    open_ = token.rfind("(")
    if open_ == -1:
        return False
    inner = token[open_ + 1:-1]
    return bool(inner) and inner.isascii() and inner[0].isdigit() and inner.isalnum()


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
    rows.append(" ".join("".join(projected).split()))

# Furniture drops by position window, never by content. Row 0 is always
# the header; a non-blank row 1 without a section token is the wrapped
# center line of a two-line header. The reference always foots the page,
# so the trailing non-blank block is the footer (1-3 rows) framed by
# page-edge blanks; body rows and paragraph blanks are pinned content.
if rows:
    rows.pop(0)
if rows and rows[0] != "" and not any(section_token(t) for t in rows[0].split(" ")):
    rows.pop(0)
while rows and rows[0] == "":
    rows.pop(0)
while rows and rows[-1] != "":
    rows.pop()
while rows and rows[-1] == "":
    rows.pop()
sys.stdout.write("\n".join(rows) + ("\n" if rows else ""))
' > "crates/mant-engine/tests/roff_lowering/escape_matrix/cases/$name.expected"
done
echo "snapshots regenerated under crates/mant-engine/tests/roff_lowering/escape_matrix/cases/"
