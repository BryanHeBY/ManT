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
# Full-recording template (matrix A/C): keeps the NAME section so the
# `.Nd` dash row is part of the pin, and a `.br` + After + NEXT tail
# (review section 2). Tails never start with a blank source line — a
# blank line is itself a paragraph break in roff.
MDOC_HEAD = (
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n"
    ".Sh DESCRIPTION\n"
)
A_TAIL = ".br\n.No After\n.Sh NEXT\n.No END\n"


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


def case(name, body, tail=TAIL):
    source = HEAD + body + tail


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


def mdoc_case(name, body, tail=A_TAIL):
    source = MDOC_HEAD + body + tail
    path = OUT / f"{name}.1"
    path.write_text(source)
    rendered = subprocess.run(
        [REF, "-Tutf8", "-Owidth=78", str(path)],
        capture_output=True, text=True, check=True).stdout
    (OUT / f"{name}.expected").write_text(normalize(rendered))


# Matrix A: 16 accept/reject/cross-word sequences x 7 contexts (review
# section 2). `_text` skips the empty-operand sequences (a13/a14).
A = {
    "a01": ["X", r"\p", "Tail"],
    "a02": [r"\zX\p", r"\p Y", "Tail"],
    "a03": [r"\p", r"\zX\p", "Tail"],
    "a04": [r"\zX", r"\p", "Tail"],
    "a05": [r"X\p Y", "Tail"],
    "a06": [r"X \p Y", "Tail"],
    "a07": [r"\p X", "Tail"],
    "a08": [r"\zX \p Y", "Tail"],
    "a09": [r"\p\& Y", "Tail"],
    "a10": [r"\p\:Y", "Tail"],
    "a11": ["X", r"\p", r"\&", "Tail"],
    "a12": [r"\p", r"\zX", "Tail"],
    "a13": [r"X\p", "", "Tail"],
    "a14": [r"\p", "", "Tail"],
    "a15": [r"\zX\p", r"\p Y", r"\p Z", "Tail"],
    "a16": ["X", r"\p\~Y", "Tail"],
}
for aid, seq in A.items():
    nos = "\n".join(f'.No "{a}"' for a in seq) + "\n"
    mdoc_case(f"{aid}_no", nos)                      # independent .No
    if aid not in ("a13", "a14"):                    # raw TEXT
        mdoc_case(f"{aid}_text", "\n".join(seq) + "\n")
    mdoc_case(f"{aid}_same",                         # one source line
              "." + " ".join(f'No "{a}"' for a in seq) + "\n")
    mdoc_case(f"{aid}_nf", ".nf\n" + nos)            # no-fill
    for ctx, flags in (("tag", "-tag -width 4n"), ("hang", "-hang -width 4n"),
                       ("inset", "-inset")):         # extended HEAD
        mdoc_case(f"{aid}_{ctx}",
                  f".Bl {flags}\n.It Xo\n{nos}.Xc\n.No BodyWord\n.El\n")

# man raw TEXT forms (review section 2): own headers, no mdoc tail.
for name, body in (
    ("man1", "\\zX\\p\n\\p Y\nTail\n"),
    ("man2", "X\n\\p\nTail\n.br\nAfter\n"),
):
    source = ".TH TEST 1\n.SH DESCRIPTION\n" + body + "\n.SH NEXT\nEND\n"
    (OUT / f"{name}.1").write_text(source)
    rendered = subprocess.run(
        [REF, "-Tutf8", "-Owidth=78", str(OUT / f"{name}.1")],
        capture_output=True, text=True, check=True).stdout
    (OUT / f"{name}.expected").write_text(normalize(rendered))

# Matrix C: control boundaries x three initial buffer states (review
# section 5): no cells, bare armed \z, cells already written.
CONTROLS = [None, ".ft B", ".ta 4n 8n", ".Tg marker", ".br", ".sp 0",
            ".sp 1", ".sp -1", ".mc", ".Pp", ".nf", ".ti 2n"]
CTL_IDS = ["none", "ft", "ta", "tg", "br", "sp0", "sp1", "spm1",
           "mc", "pp", "nf", "ti"]
STATES = [("s0", []), ("s1", ['.No "\\z"']),
          ("s2", ['.No X', '.No "\\p"'])]
for st, prelude in STATES:
    for cid, line in zip(CTL_IDS, CONTROLS):
        parts = list(prelude)
        if line:
            parts.append(line)
        parts += [".No Tail", ".br", ".No After"]
        mdoc_case(f"c_{st}_{cid}", "\n".join(parts) + "\n")

# RF01-07 minimal repros (external review section 3-9): sources are the
# checked-in reviewed cases; expectations are always re-recorded from the
# oracle, never edited to match ManT.
for path in sorted(OUT.glob("rf*.1")):
    rendered = subprocess.run(
        [REF, "-Tutf8", "-Owidth=78", str(path)],
        capture_output=True, text=True, check=True).stdout
    (OUT / f"{path.stem}.expected").write_text(normalize(rendered))
print("regenerated", len(list(OUT.glob("*.1"))), "cases")
REGEN
