#!/usr/bin/env bash
# Regenerate the escape/wave-1 matrix snapshots from the pinned CVS
# reference binary. Unlike scripts/regen_definition_matrix.sh this matrix is
# recorded with -Tutf8: ManT is a single-device UTF-8 renderer, and the
# device forks this matrix pins (`\:` NBRZW vs ASCII_BREAK, generated-cell
# overstrike order, `\p` pass rejections) are only observable on the UTF-8
# column (chars.c:47-53 with term.c:620-634). Expectations must come from
# this run, never by hand. The normalization is implemented in python3 so
# the NBSP/dash byte rules behave identically on every platform's sed. It
# mirrors normalize_mant in crates/mant-engine/tests/roff_lowering/
# escape_matrix.rs: every body row and every blank row between body rows is
# preserved, and only mandoc's page furniture (the name(1) title/header
# rows, the footer OS and date rows, and the page-edge blanks framing
# them) is dropped.
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
import re
import sys

FURNITURE_OS_WORDS = {
    "Linux", "macOS", "Darwin", "Apple", "Ubuntu", "Debian",
    "NetBSD", "FreeBSD", "OpenBSD", "AT&T", "GNU",
}
FURNITURE_MONTHS = {
    "January", "February", "March", "April", "May", "June", "July",
    "August", "September", "October", "November", "December",
}


def section_token(token):
    if not token.endswith(")"):
        return False
    open_ = token.rfind("(")
    if open_ == -1:
        return False
    inner = token[open_ + 1:-1]
    return bool(inner) and inner.isascii() and inner[0].isdigit() and inner.isalnum()


def title_row(row):
    tokens = row.split(" ")
    if not section_token(tokens[0]):
        return False
    return len(tokens) == 1 or section_token(tokens[-1])


def os_row(row):
    return row.split(" ")[0] in FURNITURE_OS_WORDS


def date_row(row):
    tokens = row.split(" ")
    if len(tokens) < 2 or not section_token(tokens[-1]):
        return False
    body = tokens[:-1]
    if any(re.fullmatch(r"[0-9]{4}-[0-9]{2}-[0-9]{2}", token) for token in body):
        return True
    return any(token in FURNITURE_MONTHS for token in body) and any(
        re.fullmatch(r"[0-9]{4}", token) for token in body
    )


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
    # Drop only page furniture; body and blank rows are the pinned content.
    if title_row(row) or os_row(row) or date_row(row):
        continue
    rows.append(row)
# Page-edge blank rows frame the furniture (the reference emits one blank
# after the header and one before the footer via term_vspace); blank rows
# between body rows are paragraph structure and stay.
while rows and rows[0] == "":
    rows.pop(0)
while rows and rows[-1] == "":
    rows.pop()
sys.stdout.write("\n".join(rows) + ("\n" if rows else ""))
' > "crates/mant-engine/tests/roff_lowering/escape_matrix/cases/$name.expected"
done
echo "snapshots regenerated under crates/mant-engine/tests/roff_lowering/escape_matrix/cases/"
