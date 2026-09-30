#!/usr/bin/env bash
# Regenerate the definition row-machine matrix snapshots from the pinned
# CVS reference binary. Expectations must come from this run, never by hand
# (see tests/roff_lowering/definition_matrix.rs for the row-grouping
# rule). The normalization mirrors that file's normalize (reference side,
# footed page) in python3: page furniture drops by position window —
# row 0 (plus an optional wrapped center line) and the trailing footer
# block — never by content, so all-caps section heads like OPTIONS stay;
# blank rows carry no row-machine signal and all drop.
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
for source in crates/mant-engine/tests/roff_lowering/definition_matrix/cases/*.1; do
  name=$(basename "$source" .1)
  "$REF" -Tascii "$source" | python3 -c '
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
    # Overstrike collapse, then inline-whitespace squeeze.
    projected = []
    for ch in line:
        if ch == "\x08":
            if projected:
                projected.pop()
        else:
            projected.append(ch)
    rows.append(" ".join("".join(projected).split()))

# Furniture drops by position window, never by content: row 0 (plus an
# optional wrapped center line) is the header, and the trailing non-blank
# block is the footer. Blank rows carry no row-machine signal and all go;
# body rows — all-caps section heads like OPTIONS included — stay.
if rows:
    rows.pop(0)
if rows and rows[0] != "" and not any(section_token(t) for t in rows[0].split(" ")):
    rows.pop(0)
while rows and rows[-1] != "":
    rows.pop()
rows = [row for row in rows if row != ""]
sys.stdout.write("\n".join(rows) + ("\n" if rows else ""))
' > "crates/mant-engine/tests/roff_lowering/definition_matrix/cases/$name.expected"
done
echo "snapshots regenerated under crates/mant-engine/tests/roff_lowering/definition_matrix/cases/"
