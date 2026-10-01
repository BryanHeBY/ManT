#!/usr/bin/env python3
"""Record checked-in roff cases with the registered pristine CVS oracle.

The case files are the sole input definitions. Recording never regenerates
their headers or bodies. The profiles mirror their Rust consumer harnesses:
UTF-8 rows retain interior spaces; ASCII row groups squeeze word spacing.
Both retain interior blank rows and remove page furniture by position.
"""

import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

import mandoc_oracle
import rebuild_reference_mandoc


ROOT = Path(__file__).resolve().parent.parent
MATRICES = {
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


def record(matrix, check):
    directory, profile, count = MATRICES[matrix]
    cases = ROOT / "crates/mant-engine/tests/roff_lowering" / directory / "cases"
    binary = Path(os.environ.get(
        "MANT_REFERENCE", "target/mandoc-migration/reference/mandoc"))
    if not binary.is_absolute():
        binary = ROOT / binary
    attestation, value = rebuild_reference_mandoc.active_attestation(ROOT)
    archive = mandoc_oracle.repository_path(
        ROOT, value["source"]["archive"]["path"], "source archive")
    rebuild_reference_mandoc.verify_all(
        ROOT, binary, archive, attestation, value["identity"])
    print(f"oracle preflight passed: {value['identity']}", file=sys.stderr)
    sources = sorted(cases.glob("*.1"))
    if len(sources) != count:
        raise ValueError(f"{matrix}: expected {count} cases, found {len(sources)}")
    snapshots = []
    for source in sources:
        result = subprocess.run(
            [str(binary), f"-T{profile}", "-Owidth=78", str(source)],
            capture_output=True, text=True, encoding="utf-8", check=True,
            timeout=30)
        snapshots.append((source.with_suffix(".expected"),
                          normalize(result.stdout, profile)))
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
    print(f"{'checked' if check else 'recorded'} {len(snapshots)} {matrix} snapshots")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--matrix", choices=MATRICES, required=True)
    parser.add_argument("--check", action="store_true",
                        help="verify snapshots without changing files")
    args = parser.parse_args()
    try:
        record(args.matrix, args.check)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        parser.exit(1, f"snapshot recording failed: {error}\n")


if __name__ == "__main__":
    main()
