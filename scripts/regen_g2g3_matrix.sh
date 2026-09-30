#!/usr/bin/env bash
# Regenerate the G2/G3 flush-retirement matrix snapshots from the pinned
# CVS reference binary. Expectations must come from this run, never by hand
# (see tests/roff_lowering/g2g3_matrix.rs for the row-grouping rule).
#
# Case families: `.mc`/control flush retirement across list kinds (G2),
# TAG tail-width thresholds (G3: emptyno/embedded/nbrsp/nbrsp_tilde/mixed/
# tab/tabq boundary pairs), and `.Xc` flags-timing probes (t_*). The
# `.mc <arg>` margin-note cases are deliberately NOT in this directory:
# the margin note is a registered deviation (endline's per-line trailing
# character, term.c:450-465), not a row-machine behavior.
set -euo pipefail
REF="${MANT_REFERENCE:-target/mandoc-migration/reference/mandoc}"
[ -x "$REF" ] || { echo "reference binary not found: $REF" >&2; exit 1; }
cd "$(dirname "$0")/.."
# Gate: expectations may only be regenerated from the registered pristine
# oracle (same preflight contract as regen_definition_matrix.sh).
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
python3 - "$REF" <<'REGEN'
import subprocess, sys, glob, os, re

REF = sys.argv[1]
OUT = "crates/mant-engine/tests/roff_lowering/g2g3_matrix/cases"

def project(ln):
    p = ""
    for ch in ln:
        if ch == "\x08":
            p = p[:-1]
        elif ch == "\u00a0":
            p += " "
        elif ch in "\u2013\u2014":
            p += "-"
        else:
            p += ch
    return p

def normalize(text):
    # Position-window furniture rule (verified against the full corpus
    # structure): row 0 is the header, the trailing non-empty block is the
    # footer, and page-edge blanks frame them. Content word lists are never
    # matched: body rows like `Linux command` or `printf(3)` survive.
    rows = [" ".join(project(l).split()) for l in text.split("\n")]
    if rows and rows[-1] == "":
        rows.pop()
    if rows:
        rows.pop(0)
    if rows and rows[0] != "" and len(rows) > 1 and rows[1] == "" \
            and not re.search(r"\(\d+\)|\(\)", rows[0]):
        rows.pop(0)  # folded second header line (long names)
    while rows and rows[0] == "":
        rows.pop(0)
    while rows and rows[-1] == "":
        rows.pop()
    while rows and rows[-1] != "":
        rows.pop()
    while rows and rows[-1] == "":
        rows.pop()
    return rows

count = 0
for source in sorted(glob.glob(OUT + "/*.1")):
    name = os.path.basename(source)[:-2]
    out = subprocess.run([REF, "-Tascii", source], capture_output=True, text=True).stdout
    with open(f"{OUT}/{name}.expected", "w") as f:
        f.write("\n".join(normalize(out)) + "\n")
    count += 1
print(f"regenerated {count} snapshots under {OUT}/", file=sys.stderr)
REGEN
