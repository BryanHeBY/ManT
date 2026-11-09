"""Replay frozen shared TEXT-visitor cases against the registered pristine mandoc."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import gzip
import hashlib
import json
from pathlib import Path
import re

from scripts.roff.fixtures.roff_fixture_reference import run_reference, verified_reference

ROOT = Path(__file__).resolve().parents[3]
FIXTURE = ROOT / "crates/mant-engine/tests/roff_lowering/shared_source_lines/cases.json.gz"
REFERENCE = ROOT / "target/mandoc-migration/reference/mandoc"
PROFILES = ("ascii", "utf8", "html", "tree", "lint")
MARKERS = re.compile(
    r"BEFOREMARK|MIDDLEMARK|AFTERMARK|FIRSTMARK|NEXTMARK|HEADMARK|BODYMARK|TAILMARK|ENDMARK|TAGMARK"
)


def digest(data):
    return hashlib.sha256(data).hexdigest()


def visible_text(terminal):
    """Apply the native terminal's overstrike to the accepted glyph sequence."""
    visible = []
    for char in terminal:
        if char == "\b":
            if visible and visible[-1] != "\n":
                visible.pop()
        else:
            visible.append(char)
    return "".join(visible)


def marker_rows(text):
    hits = [(match[0], index) for index, row in enumerate(text.split("\n"))
            for match in MARKERS.finditer(row)]
    return [[word, row - hits[0][1]] for word, row in hits]


def glyph_rows(text):
    lines = text.split("\n")
    occupied = [index for index, line in enumerate(lines) if MARKERS.search(line)]
    return ["".join(char for char in line if not char.isspace())
            for line in lines[occupied[0]:occupied[-1] + 1]]


def line_difference(native, product):
    if [word for word, _ in native] != [word for word, _ in product]:
        return "marker-sequence"
    if native == product:
        return "aligned"
    differences = [actual - expected for (_, expected), (_, actual) in zip(native, product, strict=True)]
    if differences == sorted(differences):
        return "extra-line"
    if differences == sorted(differences, reverse=True):
        return "missing-line"
    return "mixed-geometry"


def load():
    return json.loads(gzip.decompress(FIXTURE.read_bytes()))


def replay(case):
    source = case["source"].encode()
    outputs = {}
    for profile in PROFILES:
        run = run_reference(REFERENCE, [f"-T{profile}", "-Owidth=160"], input_bytes=source)
        receipt = [run.returncode, digest(run.stdout), digest(run.stderr)]
        assert receipt == case["profiles"][profile], (case["group"], case["id"], profile, receipt)
        outputs[profile] = dict(status=run.returncode, stdout=run.stdout.decode(), stderr=run.stderr.decode())
    visible = visible_text(outputs["utf8"]["stdout"])
    observed = marker_rows(visible)
    assert observed == case["native_rows"], (case["group"], case["id"], observed)
    if "glyph_rows" in case:
        assert glyph_rows(visible) == case["glyph_rows"], (case["group"], case["id"])
    return dict(group=case["group"], id=case["id"], sha256=digest(source), source=case["source"], profiles=outputs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    fixture = load()
    registration = verified_reference(ROOT, REFERENCE)
    assert fixture["oracle_identity"] == registration["identity"]
    assert fixture["oracle_sha256"] == digest(REFERENCE.read_bytes())
    cases = fixture["cases"]
    args.evidence.parent.mkdir(parents=True, exist_ok=True)
    with args.evidence.open("w", encoding="utf-8") as output, ThreadPoolExecutor(max_workers=8) as pool:
        for row in pool.map(replay, cases):
            print(json.dumps(row, ensure_ascii=False), file=output)
    print(dict(Counter(case["group"] for case in cases)))


if __name__ == "__main__":
    main()
