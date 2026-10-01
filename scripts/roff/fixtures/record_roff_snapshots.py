#!/usr/bin/env python3
"""Record checked-in roff cases with the registered pristine CVS oracle.

The case files are the sole input definitions. Recording never regenerates
their headers or bodies. The profiles mirror their Rust consumer harnesses:
UTF-8 rows retain interior spaces; ASCII row groups squeeze word spacing.
Both retain interior blank rows and remove page furniture by position.
"""

import argparse
import collections
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from scripts.roff.fixtures import roff_fixture_reference
ROOT = Path(__file__).resolve().parents[3]
MATRICES = {
    "definition": ("definition_matrix", "ascii", 54),
    "escape": ("escape_matrix", "utf8", 80),
    "macro-consumer": ("macro_consumer_matrix", "utf8", 87),
    "shared-execution": ("shared_execution_matrix", "utf8", 209),
    "field-retirement": ("field_retirement_matrix", "ascii", 161),
}


def project(line, profile):
    cells = []
    for character in line:
        if character == "\x08":
            if cells:
                cells.pop()
        elif character == "\u00a0":
            cells.append(" ")
        elif profile == "ascii" and character in "\u2013\u2014":
            cells.append("-")
        else:
            cells.append(character)
    return "".join(cells)


def normalize(text, profile):
    rows = [project(line, profile) for line in text.split("\n")]
    if profile == "ascii":
        rows = [" ".join(row.split()) for row in rows]
    else:
        rows = [row.strip() for row in rows]
    if rows:
        rows.pop(0)
    if (profile == "ascii" and len(rows) > 1 and rows[0]
            and not rows[1] and not re.search(r"\(\d+\)|\(\)", rows[0])):
        rows.pop(0)  # Wrapped second header line, as in the ASCII harness.
    while rows and not rows[0]:
        rows.pop(0)
    while rows and not rows[-1]:
        rows.pop()
    while rows and rows[-1]:
        rows.pop()  # The final nonempty block is the native page footer.
    while rows and not rows[-1]:
        rows.pop()
    return "\n".join(rows) + "\n"


def section_token(token):
    if not token.endswith(")"):
        return False
    open_ = token.rfind("(")
    if open_ == -1:
        return False
    inner = token[open_ + 1:-1]
    return bool(inner) and inner.isascii() and inner[0].isdigit() and inner.isalnum()


def normalize_definition(text):
    # This is the original definition recorder's row-group projection.
    # Empty rows have no signal in that historical corpus; newer physical-row
    # fixtures retain them and must not use this projection.
    rows = []
    for line in text.splitlines():
        projected = []
        for ch in line:
            if ch == "\x08":
                if projected:
                    projected.pop()
            else:
                projected.append(ch)
        rows.append(" ".join("".join(projected).split()))
    if rows:
        rows.pop(0)
    if rows and rows[0] != "" and not any(section_token(t) for t in rows[0].split(" ")):
        rows.pop(0)
    while rows and rows[-1] != "":
        rows.pop()
    rows = [row for row in rows if row != ""]
    return "\n".join(rows) + ("\n" if rows else "")


def normalize_escape(text):
    # Keep the original UTF-8 device forks and interior empty rows. Unlike
    # definition's historical row grouping, these are physical-row evidence.
    rows = []
    for line in text.splitlines():
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
    return "\n".join(rows) + ("\n" if rows else "")


def record_profiles(binary, source, matrix):
    profiles = {}
    for profile in ("ascii", "utf8", "html", "tree", "lint"):
        arguments = [f"-T{profile}"]
        # The two older shell recorders used the default width. The other
        # three recorders explicitly selected 78 columns; retain both APIs.
        if matrix not in {"definition", "escape"}:
            arguments.append("-Owidth=78")
        arguments.append(str(source))
        result = roff_fixture_reference.run_reference(
            binary, arguments, timeout=30, check=False)
        profiles[profile] = {
            "status": result.returncode,
            "stdout": result.stdout.decode("utf-8"),
            "stderr": result.stderr.decode("utf-8"),
            "stdout_sha256": hashlib.sha256(result.stdout).hexdigest(),
            "stderr_sha256": hashlib.sha256(result.stderr).hexdigest(),
        }
        # Full diagnostics, including lint UNSUPP status=4, are evidence.
        # A failed rendering or AST profile cannot supply new expectations.
        if profile != "lint":
            result.check_returncode()
        elif result.returncode < 0:
            result.check_returncode()
    return profiles


def record(matrix, check, evidence=None):
    directory, profile, count = MATRICES[matrix]
    cases = ROOT / "crates/mant-engine/tests/roff_lowering" / directory / "cases"
    binary = Path(os.environ.get(
        "MANT_REFERENCE", "target/mandoc-migration/reference/mandoc"))
    if not binary.is_absolute():
        binary = ROOT / binary
    value = roff_fixture_reference.verified_reference(ROOT, binary)
    print(f"oracle preflight passed: {value['identity']}", file=sys.stderr)
    sources = sorted(cases.glob("*.1"))
    if len(sources) != count:
        raise ValueError(f"{matrix}: expected {count} cases, found {len(sources)}")
    evidence = Path(evidence or ROOT / "target/mandoc-migration/snapshot-recording" / matrix).resolve()
    if not evidence.is_relative_to(ROOT / "target"):
        raise ValueError("complete reference evidence must stay below target")
    evidence.mkdir(parents=True, exist_ok=True)
    snapshots = []
    lint_statuses = collections.Counter()
    with (evidence / "pristine-profiles.jsonl").open("w", encoding="utf-8") as output:
        for source in sources:
            profiles = record_profiles(binary, source, matrix)
            source_bytes = source.read_bytes()
            output.write(json.dumps({
                "name": source.stem,
                "source": source_bytes.decode("utf-8"),
                "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
                "profiles": profiles,
            }, ensure_ascii=False) + "\n")
            lint_statuses[str(profiles["lint"]["status"])] += 1
            text = profiles[profile]["stdout"]
            if matrix == "definition":
                text = normalize_definition(text)
            elif matrix == "escape":
                text = normalize_escape(text)
            else:
                text = normalize(text, profile)
            snapshots.append((source.with_suffix(".expected"), text))
    # Render every input before changing any snapshot. A failed native run
    # must not leave a partially re-recorded expectation set.
    if check:
        changed = [str(path.relative_to(ROOT)) for path, text in snapshots
                   if path.read_text(encoding="utf-8") != text]
        if changed:
            raise ValueError("oracle snapshot differences:\n" + "\n".join(changed))
    else:
        for path, text in snapshots:
            path.write_text(text, encoding="utf-8")
    (evidence / "manifest.json").write_text(json.dumps({
        "identity": value["identity"],
        "reference_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "matrix": matrix,
        "source_count": len(sources),
        "selected_profile": profile,
        "profiles": ["ascii", "utf8", "html", "tree", "lint"],
        "lint_status_counts": lint_statuses,
        "expectations_from_product": False,
        "check_only": check,
    }, indent=2) + "\n", encoding="utf-8")
    print(f"{'checked' if check else 'recorded'} {len(snapshots)} {matrix} snapshots")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", choices=MATRICES, required=True)
    parser.add_argument("--check", action="store_true",
                        help="verify snapshots without changing files")
    parser.add_argument("--evidence", type=Path,
                        help="complete profile records below target (a matrix-specific default is used)")
    args = parser.parse_args()
    try:
        record(args.matrix, args.check, args.evidence)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"snapshot recording failed: {error}\n")


if __name__ == "__main__":
    main()
