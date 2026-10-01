#!/usr/bin/env python3
"""Record the acceptance-axes oracle gold with the registered pristine CVS oracle.

The case files below
``crates/mant-engine/tests/roff_lowering/acceptance_axes/cases`` are the sole
input definitions; recording never rewrites them. Every case must be legal
(lint status 0) and render cleanly in the UTF-8, ASCII and tree profiles.

The committed ``.expected`` snapshot per case is the structural
``DESCRIPTION``..``NEXT`` window of the ``-Tutf8 -Owidth=78`` projection:
backspace cells pop the previous cell, nonbreaking blanks keep their device
spelling, interior and trailing blank rows stay pinned and the common page
margin is retained verbatim for the indent policies of the Rust harness.

Complete raw profiles (UTF-8/ASCII/tree/lint stdout and stderr, hashes and
source identity) are written below ``target`` as recording evidence. Product
output never enters any expectation.
"""

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

from scripts.roff.fixtures import roff_fixture_reference

ROOT = Path(__file__).resolve().parents[3]
DIRECTORY = "crates/mant-engine/tests/roff_lowering/acceptance_axes"
CASE_COUNT = 11
# The guide's exact oracle invocations: UTF-8 is the primary reading, ASCII
# is the separate device fork, tree and lint are structure and admission.
PROFILES = (
    ("utf8", ("-Tutf8", "-Owidth=78")),
    ("ascii", ("-Tascii", "-Owidth=78")),
    ("tree", ("-Ttree",)),
    ("lint", ("-Tlint",)),
)


def project(text):
    """Apply the device cell projection: backspace pops, NBSP stays."""
    cells = []
    for character in text:
        if character == "\x08":
            if cells:
                cells.pop()
        else:
            cells.append(character)
    return "".join(cells)


def description_window(rows):
    """Rows strictly between the DESCRIPTION and NEXT heading rows."""
    starts = [index for index, row in enumerate(rows) if row == "DESCRIPTION"]
    ends = [index for index, row in enumerate(rows) if row == "NEXT"]
    if len(starts) != 1 or len(ends) != 1:
        raise ValueError(
            f"expected one DESCRIPTION and one NEXT row, found {starts} and {ends}")
    if starts[0] >= ends[0]:
        raise ValueError(f"DESCRIPTION row {starts[0]} not before NEXT row {ends[0]}")
    return rows[starts[0] + 1:ends[0]]


def record_profiles(binary, source):
    profiles = {}
    for name, arguments in PROFILES:
        result = roff_fixture_reference.run_reference(
            binary, [*arguments, str(source)], timeout=30, check=False)
        profiles[name] = {
            "arguments": list(arguments),
            "status": result.returncode,
            "stdout": result.stdout.decode("utf-8"),
            "stderr": result.stderr.decode("utf-8"),
            "stdout_sha256": hashlib.sha256(result.stdout).hexdigest(),
            "stderr_sha256": hashlib.sha256(result.stderr).hexdigest(),
        }
        # Legal gold admission: every case renders in every profile and the
        # pristine linter accepts it. Diagnostics on stderr stay evidence.
        result.check_returncode()
    return profiles


def record(check, evidence=None):
    cases = ROOT / DIRECTORY / "cases"
    binary = Path(os.environ.get(
        "MANT_REFERENCE", "target/mandoc-migration/reference/mandoc"))
    if not binary.is_absolute():
        binary = ROOT / binary
    registration = roff_fixture_reference.verified_reference(ROOT, binary)
    print(f"oracle preflight passed: {registration['identity']}", file=sys.stderr)
    sources = sorted(cases.glob("*.1"))
    if len(sources) != CASE_COUNT:
        raise ValueError(f"expected {CASE_COUNT} cases, found {len(sources)}")
    evidence = Path(evidence or ROOT / "target/mandoc-migration/snapshot-recording"
                    / "acceptance-axes").resolve()
    if not evidence.is_relative_to(ROOT / "target"):
        raise ValueError("complete reference evidence must stay below target")
    evidence.mkdir(parents=True, exist_ok=True)
    snapshots = []
    with (evidence / "pristine-profiles.jsonl").open("w", encoding="utf-8") as output:
        for source in sources:
            profiles = record_profiles(binary, source)
            source_bytes = source.read_bytes()
            output.write(json.dumps({
                "name": source.stem,
                "source": source_bytes.decode("utf-8"),
                "source_sha256": hashlib.sha256(source_bytes).hexdigest(),
                "native_tree_witness": profiles["tree"]["stdout_sha256"],
                "profiles": profiles,
            }, ensure_ascii=False) + "\n")
            rows = project(profiles["utf8"]["stdout"]).split("\n")
            window = description_window(rows)
            if not window:
                raise ValueError(f"{source.stem}: empty DESCRIPTION window")
            snapshots.append((source.with_suffix(".expected"),
                              "\n".join(window) + "\n"))
    # Render every input before changing any snapshot. A failed oracle run
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
        "identity": registration["identity"],
        "reference_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
        "matrix": "acceptance-axes",
        "source_count": len(sources),
        "profiles": [name for name, _ in PROFILES],
        "expectations_from_product": False,
        "check_only": check,
    }, indent=2) + "\n", encoding="utf-8")
    print(f"{'checked' if check else 'recorded'} {len(snapshots)} acceptance-axes snapshots")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true",
                        help="verify snapshots without changing files")
    parser.add_argument("--evidence", type=Path,
                        help="complete profile records below target (default used otherwise)")
    args = parser.parse_args()
    try:
        record(args.check, args.evidence)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        parser.exit(1, f"acceptance-axes recording failed: {error}\n")


if __name__ == "__main__":
    main()
