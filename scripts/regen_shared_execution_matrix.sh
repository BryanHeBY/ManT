#!/usr/bin/env bash
# Regenerate the shared-execution matrix B (generated glyphs vs pending \z)
# and list-gap snapshots from the pinned CVS reference binary.
# Expectations come only from this run, never by hand (see
# tests/roff_lowering/shared_execution_matrix.rs for the normalization rule).
set -euo pipefail
REF="${MANT_REFERENCE:-target/mandoc-migration/reference/mandoc}"
[ -x "$REF" ] || { echo "reference binary not found: $REF" >&2; exit 1; }
cd "$(dirname "$0")/.."
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
import subprocess, sys
from pathlib import Path

REF = sys.argv[1]
OUT = Path("crates/mant-engine/tests/roff_lowering/shared_execution_matrix/cases")

HEAD = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n"
TAIL = "\n.Sh NEXT\n.No END\n"


def project(line):
    out = ""
    for ch in line:
        if ch == "\x08":
            out = out[:-1]
        elif ch == "\u00a0":
            out += " "
        else:
            out += ch
    return out


def normalize(text):
    # Furniture leaves by position window only: row 0 is the header, the
    # trailing non-empty block is the footer. Interior blank rows are
    # paragraph structure and stay pinned; intra-row spacing is preserved
    # by stripping only the page margin (leading/trailing whitespace).
    lines = [project(l).strip() for l in text.split("\n")]
    if lines:
        lines = lines[1:]
    while lines and lines[0] == "":
        lines.pop(0)
    while lines and lines[-1] == "":
        lines.pop()
    i = len(lines) - 1
    while i >= 0 and lines[i] != "":
        i -= 1
    lines = lines[: i + 1]
    while lines and lines[-1] == "":
        lines.pop()
    return "\n".join(lines) + "\n"


def case(name, body):
    source = HEAD + body + TAIL
    path = OUT / f"{name}.1"
    path.write_text(source)
    rendered = subprocess.run(
        [REF, "-Tutf8", "-Owidth=78", str(path)],
        capture_output=True, text=True, check=True).stdout
    (OUT / f"{name}.expected").write_text(normalize(rendered))


# Matrix B: generated glyphs against pending \z state (review section 3).
case("b01", '.No "\\zA " Ns Bq B\n.No End')
case("b02", '.No "\\zA  " Ns Bq B\n.No End')
case("b03", '.No "\\zA   " Ns Bq B\n.No End')
case("b04", '.No "\\zA    " Ns Bq B\n.No End')
case("b05", '.No "\\z" Ns Bq B\n.No End')
case("b06", '.No "\\z " Ns Bq B\n.No End')
case("b07", '.No "\\zA\\&" Ns Bq B\n.No End')
case("b08", '.No "\\zA " Ns No B\n.No End')
case("b09", '.No "\\zA " Ns Sy B\n.No End')
case("b10", '.No "\\zA " Ns Fl x\n.No End')
case("b11", '.No "\\zA " Ns In x.h\n.No End')
case("b12", '.No "\\zA " Ns Bx\n.No End')
case("b13", '.No "\\zA " Bq B\n.No End')
case("b14", '.Sm off\n.No "\\zA" Bq B\n.Sm on\n.No End')
case("b15", '.No "\\zA " Pq B\n.No End')
case("b16", '.No "\\zA " Op B\n.No End')
case("b17", '.No "\\zA " Dq B\n.No End')
case("b18", '.Bf -emphasis\n.No "\\zA " Mt test@example.com\n.Ef\n.No End')
case("b19", '.No "\\zA " Bx NetBSD 7\n.No End')

# Generated list gaps: inset/diag/tag/hang/ohang x pending \z HEAD shapes
# (review section 3, generated list-gap table). Width geometry is covered
# separately; these rows pin glyph survival and hard breaks.
for kind, width in [("inset", ""), ("diag", ""), ("tag", " -width 4n"),
                    ("hang", " -width 4n"), ("ohang", "")]:
    for tag, head in [("zA", r"\zA"), ("zAs", r"\zA "),
                      ("z", r"\z"), ("zs", r"\z ")]:
        case(f"g_{kind}_{tag}",
             f'.Bl -{kind}{width}\n.It "{head}"\n.No BodyWord\n.El')
print("regenerated", len(list(OUT.glob("*.1"))), "cases")
REGEN
