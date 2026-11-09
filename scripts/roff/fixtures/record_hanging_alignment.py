"""Record native alignment groups and their enclosing hanging paragraph rows."""

import argparse
from concurrent.futures import ThreadPoolExecutor
import gzip
from itertools import product
import json
from pathlib import Path
import re

from scripts.roff.fixtures.reference_recipes import PROFILES, arguments, digest, environment
from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
REFERENCE = ROOT / "target/mandoc-migration/reference/mandoc"
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/grouped_line_flush/alignment.json.gz"
WIDTH = 160
MAN = '.TH CHECK 1 "2026-11-10"\n.SH NAME\ncheck \\- audit\n.SH DESCRIPTION\n'
MDOC = (".Dd November 10, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n"
        ".Nd audit\n.Sh DESCRIPTION\n")
PAYLOADS = {"plain": "ROWONE", "empty": "", "zero": r"\z", "break": r"ROWONE\p"}
TAILS = {"plain": "", "br": ".br\n", "pp": ".PP\n"}
MAN_CONTEXTS = {
    "plain": ("", ""),
    **{f"hp{n}": (f".HP {n}\n", "") for n in (0, 4, 16)},
    **{f"rs-hp{n}": (f".RS 3\n.HP {n}\n", ".RE\n") for n in (0, 4, 16)},
}
MDOC_CONTEXTS = {
    "plain": ("", ""),
    "tag": (".Bl -tag -width 6n\n.It LABEL\n", ".El\n"),
    "hang": (".Bl -hang -width 6n\n.It LABEL\n", ".El\n"),
    "column": (".Bl -column 12n 12n\n.It FIRST Ta SECOND\n", ".El\n"),
    "display": (".Bd -ragged -offset 4n\n", ".Ed\n"),
    "bracket": (".Bo\n", ".Bc\n"),
}


def sources():
    for context, filled, request, count, payload, tail in product(
            MAN_CONTEXTS, (True, False), ("ce", "rj"), ("", "0", "1", "2"), PAYLOADS, TAILS):
        begin, end = MAN_CONTEXTS[context]
        request_line = "." + request + (" " + count if count else "")
        group = PAYLOADS[payload] + "\n" + ("ROWTWO\n" if count == "2" else "")
        mode = "fill" if filled else "no-fill"
        source = (MAN + ("" if filled else ".nf\n") + begin + "BEFOREMARK\n"
                  + request_line + "\n" + group + TAILS[tail] + "AFTERMARK\n"
                  + end + ("" if filled else ".fi\n") + ".SH END\nENDMARK\n")
        yield f"man/{context}/{mode}/{request}/{count or 'default'}/{payload}/{tail}", source
    for context, filled, request, control in product(
            MAN_CONTEXTS, (True, False), ("ce", "rj"), (".nf", ".fi", ".ti +3n", ".br")):
        begin, end = MAN_CONTEXTS[context]
        mode = "fill" if filled else "no-fill"
        source = (MAN + ("" if filled else ".nf\n") + begin + f"BEFOREMARK\n.{request} 2\n"
                  f"ROWONE\n{control}\nROWTWO\nAFTERMARK\n" + end + ".SH END\nENDMARK\n")
        yield f"man/{context}/{mode}/{request}/2/embedded{control}/plain", source
    for context, position, request, linked in product(
            MDOC_CONTEXTS, ("start", "middle"), ("ce", "rj"), (False, True)):
        begin, end = MDOC_CONTEXTS[context]
        link = ".Lk https://example.test LABELMARK\n" if linked else ""
        source = (MDOC + begin + ("BEFOREMARK\n" if position == "middle" else "")
                  + f".{request} 2\nROWONE\nROWTWO\n" + link + "AFTERMARK\n"
                  + end + ".Sh END\nENDMARK\n")
        yield f"mdoc/{context}/{position}/{request}/2/{'link' if linked else 'plain'}/plain", source


def native_rows(output):
    text = output.decode()
    while "\b" in text:
        projected = re.sub(r".\x08", "", text)
        if projected == text:
            raise ValueError("unpaired native backspace")
        text = projected
    section = text.split("\nDESCRIPTION\n", 1)[1].split("\nEND\n", 1)[0]
    # Remove exactly the section separator. Retain all executed empty rows
    # and native columns, including the first line's device page margin.
    return section.removesuffix("\n").split("\n")


def group_tree(output):
    lines = output.decode().splitlines()
    start = next(i for i, line in enumerate(lines) if re.search(r"\b(?:ce|rj) \(elem\)", line))
    indent = len(lines[start]) - len(lines[start].lstrip())
    stop = next((i for i in range(start + 1, len(lines))
                 if len(lines[i]) - len(lines[i].lstrip()) <= indent + 1), len(lines))
    return lines[start:stop]


def record(item):
    case_id, source = item
    encoded = source.encode()
    case = dict(id=case_id, source=source, sourceSha256=digest(encoded), profiles={})
    ascii_rows = None
    for profile in PROFILES:
        result = run_reference(REFERENCE, arguments(profile, width=WIDTH),
                               input_bytes=encoded, env=environment())
        if result.stderr or result.returncode not in ((0, 1, 2) if profile == "lint" else (0,)):
            raise ValueError(f"{case_id}/{profile}: {result.returncode} {result.stderr!r}")
        case["profiles"][profile] = [result.returncode, digest(result.stdout), digest(result.stderr)]
        if profile == "ascii":
            ascii_rows = native_rows(result.stdout)
        elif profile == "utf8":
            case["rows"] = native_rows(result.stdout)
            if case["rows"] != ascii_rows:
                raise ValueError(f"{case_id}: ASCII and UTF-8 physical rows differ")
        elif profile == "tree":
            case["groupTree"] = group_tree(result.stdout)
        elif profile == "lint" and result.stdout:
            case["lint"] = result.stdout.decode()
    return case


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    options = parser.parse_args()
    registry = verified_reference(ROOT, REFERENCE)
    with ThreadPoolExecutor(max_workers=8) as pool:
        cases = list(pool.map(record, sources()))
    if len({case["sourceSha256"] for case in cases}) != len(cases):
        raise ValueError("duplicate alignment source")
    data = dict(oracleIdentity=registry["identity"], oracleSha256=digest(REFERENCE.read_bytes()),
                width=WIDTH, cases=cases)
    serialized = (json.dumps(data, ensure_ascii=False, separators=(",", ":")) + "\n").encode()
    if options.check:
        if gzip.decompress(FIXTURE.read_bytes()) != serialized:
            raise SystemExit("hanging alignment pristine receipts changed")
    else:
        FIXTURE.write_bytes(gzip.compress(serialized, compresslevel=9, mtime=0))
    print(f"verified {len(cases)} sources / {len(cases) * len(PROFILES)} pristine profiles")


if __name__ == "__main__":
    main()
