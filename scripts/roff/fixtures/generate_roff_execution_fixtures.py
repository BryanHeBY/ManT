#!/usr/bin/env python3
"""Record canonical formatter fixtures exclusively from the registered oracle.

The product is never invoked by this generator. Complete five-profile evidence
stays below target; checked-in JSONL contains source, oracle rows and assertion
scope, so ordinary regression tests do not require a local mandoc executable.
"""
import argparse
import collections
import concurrent.futures
import hashlib
from html.parser import HTMLParser
import json
from pathlib import Path
import re
from scripts.roff.fixtures import roff_fixture_reference
from scripts.roff.fixtures import roff_execution_cases
ROOT = Path(__file__).resolve().parents[3]
REFERENCE = ROOT / "target/mandoc-migration/reference/mandoc"
IDENTITIES = {}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def unstyle(text):
    # Both bold/underline overstrike and the actual BACKBEFORE replacement
    # leave their last scalar at the same device cell.
    while "\b" in text:
        text = re.sub(r".\x08", "", text)
    return text


def section_rows(text, terminal):
    rows = unstyle(text).splitlines()
    if "DESCRIPTION" not in rows or terminal not in rows:
        return None
    start = rows.index("DESCRIPTION") + 1
    end = rows.index(terminal)
    # One fixed section separator belongs to the next heading. Every other
    # empty row, including an empty first/last source row, remains in the gold.
    body = rows[start:end]
    if not body or body[-1] != "":
        raise ValueError("oracle section separator is missing")
    return body[:-1]


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.targets = []

    def handle_starttag(self, tag, attributes):
        values = dict(attributes)
        if tag != "a" or "Lk" not in values.get("class", "").split():
            return
        uri = values.get("href", "")
        # These exact matrices use only HTTP(S) authored URI identities.
        # Empty target text has no valid clickable identity.
        if uri.startswith(("https://", "http://")):
            self.targets.append(uri)


def assertion_scope(case, profiles, matrix):
    lint = profiles["lint"]
    diagnostic = lint["stdout"] + lint["stderr"]
    header_only = (
        matrix == "links"
        and lint["code"] == 2
        and diagnostic.count("\n") == 1
        and 'first section is not "NAME": Sh DESCRIPTION' in diagnostic
    )
    if lint["code"] != 0 and not header_only:
        return "recovery-only", "nonzero pristine lint; original input retained"
    if case.get("boundary") == ".mc |\n":
        return "recovery-only", "margin-character side output requires a separate artifact projection"
    if case.get("context") in {
        "tag", "hang", "tag-body", "hang-body", "column", "TP", "tbl"
    } or case["family"] in {"column-bodies", "head-requests"} or (
        case["family"] == "output-owners" and case.get("inner") in {"tag", "hang", "column", "RS"}
    ):
        return "accepted-content", "responsive field and HEAD/BODY geometry is not terminal hard-row gold"
    return "hard-rows", "source hard rows are exact; only the common manual margin is omitted"


def authored_destinations(source):
    # These canonical inputs contain only the fixed Lk/UR target forms below;
    # this extracts their authored operands, not a second roff interpreter.
    # The isolated oracle attribute path decodes each operand independently
    # of preceding text's HTML_SKIPCHAR register (html.c:473-583).
    pattern = r'(?<!\S)(?:\.?Lk|\.UR)\s+("[^"]*"|\S+)'
    return [word[1:-1] if word.startswith('"') else word
            for word in re.findall(pattern, source)]


def record_identity(operand):
    source = (roff_execution_cases.HEAD["mdoc"] +
              '.Lk "' + operand + '" label\n.Sh NEXT\n.No END\n')
    completed = roff_fixture_reference.run_reference(
        REFERENCE, ["-Thtml"], input_bytes=source.encode(), timeout=15, check=False)
    if completed.returncode > 2:
        raise ValueError(f"oracle identity probe failed for {operand!r}")
    html = Links()
    html.feed(completed.stdout.decode("utf-8"))
    return dict(source=source, targets=html.targets,
                stdout=completed.stdout.decode("utf-8"),
                stderr=completed.stderr.decode("utf-8"), code=completed.returncode,
                stdout_sha256=digest(completed.stdout))


def portable_identity_scope(case):
    # CommonMark fenced displays/tables are literal export by contract.
    # A generated <br> can start an HTML block, and man UR's visible angle
    # target can form another autolink (manual mant-roff.md export contract).
    # None of those exports promises a lossless typed-occurrence inventory.
    rich = (case["context"] in {"filled", "plain"}
            and case["family"] not in {"column-bodies", "head-requests"}
            and r"\p" not in case["source"]
            and ".UR " not in case["source"])
    return "rich-inline" if rich else "display-and-safety"


def reading_rows(case, utf8_rows, end_heading):
    if utf8_rows is None:
        return None, "source recovery has no addressable section", None
    rows = [row.removeprefix(" " * 5) for row in utf8_rows]
    if case.get("boundary") == ".ti 2n\n":
        # The frozen reading contract omits temporary device indentation,
        # while keeping this request's actual term_newln() and blank rows.
        # An oracle pair changes only that offset, never source word/line
        # events; preserve both original and paired evidence independently.
        source = case["source"].replace(".ti 2n\n", ".ti 0n\n")
        completed = roff_fixture_reference.run_reference(
            REFERENCE, ["-Tutf8", "-Owidth=78"], input_bytes=source.encode(),
            timeout=15, check=False)
        if completed.returncode > 2:
            raise ValueError("temporary-origin paired oracle failed")
        paired = section_rows(completed.stdout.decode("utf-8"), end_heading)
        paired = [row.removeprefix(" " * 5) for row in paired]
        if len(rows) != len(paired) or [row.lstrip(" ") for row in rows] != [
                row.lstrip(" ") for row in paired]:
            raise ValueError("temporary origin changed non-padding oracle output")
        return paired, "frozen temporary device origin omission; all hard rows retained", {
            "source": source, "stdout": completed.stdout.decode("utf-8"),
            "stderr": completed.stderr.decode("utf-8"), "code": completed.returncode,
            "stdout_sha256": digest(completed.stdout)}
    # term_word()573-589 buffers automatic blanks even after graphless
    # operands. These cells belong to the accepted native buffer rather
    # than device origin, and remain after omitting the common page margin.
    return rows, "only the five-column common manual margin is omitted", None


def record(item):
    matrix, index, case = item
    source = case["source"]
    profiles = {}
    for profile in ("ascii", "utf8", "html", "tree", "lint"):
        command = ["-T" + profile]
        if profile in ("ascii", "utf8"):
            command.append("-Owidth=78")
        completed = roff_fixture_reference.run_reference(
            REFERENCE, command, input_bytes=source.encode(), timeout=15,
            env={"PATH": "/usr/bin:/bin", "LC_ALL": "C.UTF-8", "TZ": "UTC"},
            check=False,
        )
        profiles[profile] = {
            "code": completed.returncode,
            "stdout": completed.stdout.decode("utf-8"),
            "stderr": completed.stderr.decode("utf-8"),
            "stdout_sha256": digest(completed.stdout),
            "stderr_sha256": digest(completed.stderr),
        }
        if profile != "lint" and completed.returncode > 2:
            raise ValueError(f"oracle failed for {matrix}/{index}/{profile}")
    scope, explanation = assertion_scope(case, profiles, matrix)
    end_heading = "ENDTEST" if matrix == "links" else "NEXT"
    html = Links()
    html.feed(profiles["html"]["stdout"])
    utf8_rows = section_rows(profiles["utf8"]["stdout"], end_heading)
    ascii_rows = section_rows(profiles["ascii"]["stdout"], end_heading)
    if utf8_rows is None or ascii_rows is None:
        scope = "recovery-only"
        explanation = "oracle section headings include side output or source recovery; complete profiles retained"
    reading, reading_rule, reading_evidence = reading_rows(case, utf8_rows, end_heading)
    fixture = {
        "id": f"{matrix}-{index:04}",
        **case,
        "source_sha256": digest(source.encode()),
        "scope": scope,
        "scope_reason": explanation,
        "terminal_heading": end_heading,
        "utf8_rows": utf8_rows,
        "ascii_rows": ascii_rows,
        "reading_utf8_rows": reading,
        "reading_rule": reading_rule,
        "targets": html.targets,
        "authored_targets": [target for operand in authored_destinations(source)
                             for target in IDENTITIES[operand]["targets"]],
        "portable_identity_scope": portable_identity_scope(case),
        "lint": profiles["lint"],
        "profile_sha256": {name: value["stdout_sha256"] for name, value in profiles.items()},
    }
    return matrix, fixture, {**fixture, "profiles": profiles,
                            "reading_origin_pristine": reading_evidence}


def check_sources(fixtures):
    for matrix, cases in roff_execution_cases.matrices():
        canonical = list(cases)
        recorded = [json.loads(line) for line in
                    (fixtures / f"native_{matrix}_matrix.jsonl").read_text().splitlines()]
        if len(canonical) != len(recorded):
            raise ValueError(f"{matrix}: source count differs")
        for index, (source, fixture) in enumerate(zip(canonical, recorded)):
            if any(fixture.get(key) != value for key, value in source.items()):
                raise ValueError(f"{matrix}/{index}: canonical source or axes differ")
            if fixture["source_sha256"] != digest(source["source"].encode()):
                raise ValueError(f"{matrix}/{index}: recorded source hash differs")
        print(f"{matrix}: {len(canonical)} canonical sources and hashes match")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--fixtures", type=Path,
                        default=ROOT / "crates/mant-engine/tests/roff_lowering/native_execution/fixtures")
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--check", action="store_true",
                        help="validate all oracle records against fixtures without changing them")
    parser.add_argument("--check-sources", action="store_true",
                        help="check canonical sources and hashes without an oracle or writes")
    args = parser.parse_args()
    if args.check_sources:
        check_sources(args.fixtures)
        return
    if args.evidence is None:
        parser.error("--evidence is required for complete oracle recording")
    evidence = args.evidence.resolve()
    if not evidence.is_relative_to(ROOT / "target"):
        parser.error("complete stage evidence must stay below target")
    registration = roff_fixture_reference.verified_reference(ROOT, REFERENCE)
    evidence.mkdir(parents=True, exist_ok=True)
    args.fixtures.mkdir(parents=True, exist_ok=True)
    tasks = [(matrix, index, case) for matrix, cases in roff_execution_cases.matrices()
             for index, case in enumerate(cases)]
    # Native identity is immutable authored data. A preceding display \z
    # can corrupt the stateful HTML href output, so also record the selected
    # attribute decoder in an isolated oracle owner, before any assertion.
    for operand in sorted({operand for _, _, case in tasks
                           for operand in authored_destinations(case["source"])}):
        IDENTITIES[operand] = record_identity(operand)
    (evidence / "identity-pristine.json").write_text(
        json.dumps(IDENTITIES, ensure_ascii=False, indent=2) + "\n")
    expected = {"formatter": 2520, "links": 868}
    assert collections.Counter(item[0] for item in tasks) == expected
    paths = {matrix: args.fixtures / f"native_{matrix}_matrix.jsonl" for matrix in expected}
    candidates = evidence / "fixture-candidates"
    candidates.mkdir(exist_ok=True)
    files = {matrix: (candidates / path.name).open("w") for matrix, path in paths.items()}
    evidence_files = {matrix: (evidence / f"{matrix}-pristine.jsonl").open("w")
                      for matrix in expected}
    scopes = collections.defaultdict(collections.Counter)
    try:
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
            for processed, (matrix, fixture, full) in enumerate(pool.map(record, tasks), 1):
                files[matrix].write(json.dumps(fixture, ensure_ascii=False, separators=(",", ":")) + "\n")
                evidence_files[matrix].write(json.dumps(full, ensure_ascii=False, separators=(",", ":")) + "\n")
                scopes[matrix][fixture["scope"]] += 1
                if processed % 400 == 0:
                    print(f"pristine inputs recorded: {processed}/{len(tasks)}", flush=True)
    finally:
        for file in [*files.values(), *evidence_files.values()]:
            file.close()
    # Leave previously recorded gold intact on any failed source/profile.
    # Only a complete oracle-only run may replace the checked-in fixtures.
    if args.check:
        mismatches = [matrix for matrix, file in files.items()
                      if Path(file.name).read_bytes() != paths[matrix].read_bytes()]
        if mismatches:
            raise ValueError(f"pristine fixtures differ: {mismatches}; checked-in fixtures were not changed")
    else:
        for matrix, file in files.items():
            Path(file.name).replace(paths[matrix])
    manifest = {
        "identity": registration["identity"],
        "reference_sha256": digest(REFERENCE.read_bytes()),
        "source_definitions_sha256": digest(Path(roff_execution_cases.__file__).read_bytes()),
        "counts": expected,
        "scopes": scopes,
        "fixtures": {matrix: digest(path.read_bytes()) for matrix, path in paths.items()},
        "expectations_from_product": False,
        "check_only": args.check,
    }
    (evidence / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()
