#!/usr/bin/env bash
# Regenerate the review §25 matrix families (LK links / AQ quote enclosures /
# MP macro-post / CW declared columns) from the pinned CVS reference binary.
# Expectations come only from this run, never by hand (see
# tests/roff_lowering/review25_matrix for the normalization rule and the
# per-family KNOWN_RED tracking lists).
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
OUT = Path("crates/mant-engine/tests/roff_lowering/review25_matrix/cases")
OUT.mkdir(parents=True, exist_ok=True)

MDOC_HEAD = (
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n"
    ".Sh DESCRIPTION\n"
)
SYNOPSIS_HEAD = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh SYNOPSIS\n"
TAIL = "\n.Sh NEXT\n.No END\n"
MAN_HEAD = ".TH TEST 1\n.SH DESCRIPTION\n"
MAN_TAIL = "\n.SH NEXT\nEND\n"


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
    # Same furniture rule as the shared-execution matrix: row 0 is the
    # header, the trailing non-empty block is the footer; interior blank
    # rows are paragraph structure and stay pinned; only the page margin
    # is trimmed so intra-row spacing survives.
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


def case(name, body, head=MDOC_HEAD, tail=TAIL):
    source = head + body + tail
    path = OUT / f"{name}.1"
    path.write_text(source, encoding="utf-8")
    rendered = subprocess.run(
        [REF, "-Tutf8", "-Owidth=78", str(path)],
        capture_output=True, text=True, check=True).stdout
    (OUT / f"{name}.expected").write_text(normalize(rendered), encoding="utf-8")


# ---------------------------------------------------------------- LK (§25.3)
# Links, targets and positions (NF03/NF04). lk01 (Markdown input) has no
# roff oracle and is pinned inside link.rs as a ManT-side contract test.
case("lk02_ur", ".UR https://example.com/x\nlabel\n.UE\n", MAN_HEAD, MAN_TAIL)
case("lk03", ".Lk https://example.com/label label\n")
case("lk04", ".Lk https://example.com/x https://example.com/x\n")
case("lk05", ".Lk https://example.com/x\n")
case("lk06_punct", ".Lk https://e.example/x label ,\n")
case("lk06_multi", ".Lk https://e.example/x word1 word2\n")
# An in-line macro operand closes the Lk scope: Sy executes as a trailing
# sibling, not as an Lk description (mdoc in-line closing rule).
case("lk06_style", ".Bf -emphasis\n.Lk https://e.example/x Sy BOLD\n.Ef\n")
case("lk07", ".Rs\n.%R RFC 1149\n.%U https://u.example/x\n.Re\n")
case("lk08", ".Rs\n.%U https://example.com/a\\&b\n.Re\n")
case("lk09_font", ".Rs\n.%U https://e.example/\\fBa\\fPb\n.Re\n")
case("lk09_z", ".Rs\n.%U https://e.example/a\\zb\n.Re\n")
case("lk09_uni", ".Rs\n.%U https://e.example/café\n.Re\n")
case("lk10_nonrfc", ".Rs\n.%R RFC abc\n.%U https://u.example/y\n.Re\n")
case("lk10_lower", ".Rs\n.%R rfc 1149\n.%U https://u.example/y\n.Re\n")
case("lk10_nospace", ".Rs\n.%R RFC1149\n.%U https://u.example/y\n.Re\n")
case("lk11_mt", ".Mt user@example.com\n")
case("lk11_suffix", ".Mt https://example.com/user@example.com\n")
case("lk12_tailz", ".Lk https://e.example/x \"label\\z\"\n")
case("lk12_empty", ".Lk https://e.example/x \"\"\n")
case("lk12_ctl", ".Lk https://e.example/x \\&\n")
case("lk12_nf", ".nf\n.Lk https://e.example/x label\n.fi\n")
case("lk13_def",
     ".Bl -tag -width 4n\n.It Xo\n.Lk https://e.example/x label\n.Xc\n.No BODY\n.El\n")
case("lk13_cell", ".Bl -column xx yy\n.It Lk https://e.example/x label Ta CELL\n.El\n")
case("lk15_bad", ".Lk \"::not a uri::\" label\n")
case("lk15_empty", ".Lk \"\" label\n")
case("lk16_long",
     ".Lk https://e.example/" + "x" * 120 + " " + "L" * 120 + "\n")
case("lk16_many",
     ".Lk https://e.example/1 one\n.Lk https://e.example/2 two\n"
     ".Lk https://e.example/3 three\n")
case("lk16_nested", ".Lk https://e.example/x Sy BOLD Em ital\n")

# --------------------------------------------------------------- AQ (§25.4)
# New quote-enclosure shapes beyond the eight-form contract already landed
# in mant-codec basic_inline::quote_enclosure (AQ01-AQ04 deduplicated
# against it; see quote.rs header).
case("aq05", ".Aq Mt user@host No \\&\n")
case("aq06_pre", ".Aq No pre Mt user@host\n")
case("aq06_empty", ".Aq\n")
case("aq06_empty_ao", ".Ao\n.Ac\n")
case("aq07", ".Aq Mt a@b c@d\n")
case("aq08_nested", ".Ao pre Aq Mt a@b Ac post\n.Ac\n")
case("aq08_cross", ".Ao\n.Aq Mt user@host\n.Ac\n.Ac\n")
case("aq08_z", ".Aq \\z Mt user@host\n")
case("aq08_p", ".Aq \\p Mt user@host\n")
case("aq08_c", ".Aq \\c Mt user@host\n")

# --------------------------------------------------------------- MP (§25.4)
# Fd macro-post family (NF05; NF-POST pending).
case("mp01", ".Fd XSHARED\n.No B\n")
case("mp02", ".nf\n.Fd \"A\\c\"\n.No B\n.fi\n")
case("mp03", ".Bd -literal\n.Fd \"A\\c\"\n.No B\n.Ed\n")
case("mp04", ".nf\n.Fd A\n.No B\n.fi\n")
case("mp05", ".nf\n.Cd \"A\\c\"\n.No B\n.fi\n")
case("mp06",
     ".Bl -tag -width 4n\n.It Xo\n.Fd \"A\\c\"\n.No B\n.Xc\n.No BODY\n.El\n")
case("mp07_hang",
     ".Bl -hang -width 4n\n.It Xo\n.Fd \"A\\c\"\n.No B\n.Xc\n.No BODY\n.El\n")
case("mp07_inset",
     ".Bl -inset\n.It Xo\n.Fd \"A\\c\"\n.No B\n.Xc\n.No BODY\n.El\n")
case("mp07_diag",
     ".Bl -diag\n.It Xo\n.Fd \"A\\c\"\n.No B\n.Xc\n.No BODY\n.El\n")
case("mp07_cell", ".Bl -column xx yy\n.It\n.Fd \"A\\c\"\n.No B\n.Ta CELL\n.El\n")
case("mp07_body", ".Bl -tag -width 4n\n.It HEAD\n.Fd \"A\\c\"\n.No B\n.El\n")
case("mp08_syn", ".Ft int\n.Fd ENABLED\n.Fn f \"void\"\n", SYNOPSIS_HEAD)
case("mp08_in",
     ".In sys/types.h\n.Fd \"A\\c\"\n.No B\n.Ft int\n.Fo f\n.Fc\n", SYNOPSIS_HEAD)
case("mp09_empty", ".nf\n.Fd\n.No B\n.fi\n")
case("mp09_amp", ".nf\n.Fd \"\\&\"\n.No B\n.fi\n")
case("mp09_z", ".nf\n.Fd \"\\z\"\n.No B\n.fi\n")
case("mp09_zx", ".nf\n.Fd \"\\zX\"\n.No B\n.fi\n")
case("mp09_p", ".nf\n.Fd \"\\p\"\n.No B\n.fi\n")
case("mp09_pp", ".nf\n.Fd \"A\\p\"\n.No B\n.fi\n")
case("mp09_font", ".nf\n.Fd \"\\fBA\\c\\fP\"\n.No B\n.fi\n")
case("mp10_link", ".nf\n.Fd \"A\\c\"\n.Lk https://e.example/x label\n.fi\n")
case("mp10_xc", ".Bl -tag -width 4n\n.It Xo\n.Fd \"A\\c\"\n.Xc\n.El\n")

# --------------------------------------------------------------- CW (§25.2)
# Declared column geometry/boundaries (NF02/NF07; CW01-02 panic seeds are
# NFSafe's). cw05_comb uses decomposed e + U+0301.
case("cw03", ".Bl -column \"a\" \"b\"\n.It ABCDEFGHIJKLM Ta SECOND\n.El\n")
case("cw04", ".Bl -column \"12345678\" \"b\"\n.It Sy A Ta SECOND\n.El\n")
case("cw05_cjk", ".Bl -column \"12345678\" \"b\"\n.It 中 Ta SECOND\n.El\n")
case("cw05_comb", ".Bl -column \"12345678\" \"b\"\n.It é Ta SECOND\n.El\n")
case("cw05_emoji", ".Bl -column \"12345678\" \"b\"\n.It 😀 Ta SECOND\n.El\n")
case("cw06_em", ".Bl -column \"\\(em\" \"b\"\n.It A Ta SECOND\n.El\n")
case("cw06_ha", ".Bl -column \"\\(ha\" \"b\"\n.It A Ta SECOND\n.El\n")
case("cw06_font", ".Bl -column \"\\fB12\\fP\" \"b\"\n.It A Ta SECOND\n.El\n")
case("cw06_zw", ".Bl -column \"\\&1234\" \"b\"\n.It A Ta SECOND\n.El\n")
case("cw07", ".Bl -column \"xxxxxxxx\" \"yyyyyyyy\"\n.It A Ta B Ta C\n.El\n")
case("cw08_4", ".Bl -column 1n 2n 3n 4n\n.It a Ta b Ta c Ta d\n.El\n")
case("cw08_5", ".Bl -column 1n 1n 1n 1n 1n\n.It a Ta b Ta c Ta d Ta e\n.El\n")
case("cw08_6",
     ".Bl -column 1n 1n 1n 1n 1n 1n\n.It a Ta b Ta c Ta d Ta e Ta f\n.El\n")
case("cw09_space", ".Bl -column \"1234\" \"b\"\n.It \"A \" Ta B\n.El\n")
case("cw09_empty", ".Bl -column \"1234\" \"b\"\n.It Ta B\n.El\n")
case("cw09_amp", ".Bl -column \"1234\" \"b\"\n.It \\& Ta B\n.El\n")
case("cw10_br", ".Bl -column \"a\" \"b\"\n.It A\n.br\n.No B\n.Ta C\n.El\n")
case("cw10_sp", ".Bl -column \"a\" \"b\"\n.It A\n.sp\n.No B\n.Ta C\n.El\n")
case("cw10_nf", ".Bl -column \"a\" \"b\"\n.It A\n.nf\n.No B\n.fi\n.Ta C\n.El\n")
case("cw10_link",
     ".Bl -column \"a\" \"b\"\n.It A\n.Lk https://e.example/x label\n.Ta C\n.El\n")
case("cw10_para", ".Bl -column \"a\" \"b\"\n.It A\n.Pp\n.No B\n.Ta C\n.El\n")
case("cw10_list",
     ".Bl -column \"a\" \"b\"\n.It A\n.Bl -tag -compact -width 2n\n.It x\n.No B\n"
     ".El\n.Ta C\n.El\n")
case("cw11_nested",
     ".Bl -offset 12n\n.Bl -column \"aa\" \"b\"\n.It X Ta Y\n.El\n.Ed\n")
case("cw11_outdent",
     ".RS -4n\n.Bl -column \"aa\" \"b\"\n.It X Ta Y\n.El\n.RE\n")
case("cw11_bigindent",
     ".Bl -offset 60n\n.Bl -column \"aa\" \"b\"\n.It X Ta Y\n.El\n.Ed\n")
case("cw12_box", ".TS\nbox;\nc c.\nA B\n.TE\n")
case("cw12_span", ".TS\nc s.\nA\n_\nB C\n.TE\n")

count = len(list(OUT.glob("*.1")))
print(f"regenerated {count} review25 cases")
REGEN
