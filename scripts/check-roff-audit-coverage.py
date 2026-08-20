#!/usr/bin/env python3
"""Verify the shared source-identity baseline across roff audit ledgers.

The content-fidelity ledger is the historical breadth index. Structure and
CommonMark-projection audits must cover every recorded source identity. The
renderer-layout audit needs only identities whose fidelity comparison reached
a comparable ``clean`` or ``review`` result; it may also contain independent
layout sweeps. Checked-in fixtures form a second, reproducible baseline shared
by the structure and projection ledgers.
"""

from __future__ import annotations

import argparse
import csv
import re
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Sequence

from roff_audit_common import discover_pages, relative_label, source_digest


ROOT = Path(__file__).resolve().parents[1]
ROFF_ROOT = ROOT / "tests/fixtures/roff"
FIXTURE_ROOT = ROFF_ROOT / "real"
DEFAULT_FIDELITY_DB = ROFF_ROOT / "FIDELITY_AUDIT.csv"
DEFAULT_STRUCTURE_DB = ROFF_ROOT / "STRUCTURE_AUDIT.csv"
DEFAULT_PROJECTION_DB = ROFF_ROOT / "PROJECTION_AUDIT.csv"
DEFAULT_LAYOUT_DB = ROFF_ROOT / "LAYOUT_AUDIT.csv"

IDENTITY_FIELDS = ["corpus", "path", "section", "source_sha256"]
FIDELITY_FIELDS = IDENTITY_FIELDS + ["scan_status", "review_status", "note"]
STRUCTURE_FIELDS = IDENTITY_FIELDS + [
    "profile_schema",
    "scan_status",
    "review_status",
    "note",
]
LAYOUT_FIELDS = IDENTITY_FIELDS + [
    "layout_schema",
    "scan_status",
    "review_status",
    "note",
]

CURRENT_STRUCTURE_SCHEMA = "mant.roff-structure-profile/v4"
CURRENT_PROJECTION_SCHEMA = "mant.roff-projection-profile/v3"
CURRENT_LAYOUT_SCHEMA = "mant.roff-layout-audit/v3"
SOURCE_DIGEST = re.compile(r"[0-9a-f]{64}")
PROFILE_SCHEMA = re.compile(r"mant\.roff-(?:structure|projection)-profile/v[1-9][0-9]*")
LAYOUT_SCHEMA = re.compile(r"mant\.roff-layout-audit/v[1-9][0-9]*")
REVIEW_STATUSES = {
    "not-required",
    "pending",
    "false-positive",
    "confirmed-open",
    "confirmed-fixed",
}


@dataclass(frozen=True, order=True)
class Identity:
    corpus: str
    path: str
    digest: str


@dataclass(frozen=True)
class Coverage:
    fidelity: frozenset[Identity]
    comparable: frozenset[Identity]
    structure: frozenset[Identity]
    projection: frozenset[Identity]
    layout: frozenset[Identity]
    fixture_inventory: frozenset[Identity]
    summaries: tuple["LedgerSummary", ...]


@dataclass(frozen=True)
class LedgerSummary:
    name: str
    current_rows: int
    baseline_rows: int
    scan_statuses: Counter[str]
    pending: int
    baseline_pending: int


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="check that roff audit routes cover their shared source baselines"
    )
    parser.add_argument("--fidelity-db", type=Path, default=DEFAULT_FIDELITY_DB)
    parser.add_argument("--structure-db", type=Path, default=DEFAULT_STRUCTURE_DB)
    parser.add_argument("--projection-db", type=Path, default=DEFAULT_PROJECTION_DB)
    parser.add_argument("--layout-db", type=Path, default=DEFAULT_LAYOUT_DB)
    parser.add_argument("--self-check", action="store_true", help=argparse.SUPPRESS)
    return parser.parse_args(argv)


def read_rows(
    path: Path,
    fields: list[str],
    scan_statuses: set[str],
    schema_field: str | None = None,
) -> list[dict[str, str]]:
    if not path.is_file():
        raise ValueError(f"audit ledger does not exist: {path}")
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != fields:
            raise ValueError(
                f"invalid audit ledger header in {path}; expected {','.join(fields)}"
            )
        rows = list(reader)
    seen: dict[Identity, int] = {}
    schema_pattern = LAYOUT_SCHEMA if schema_field == "layout_schema" else PROFILE_SCHEMA
    for number, row in enumerate(rows, 2):
        identity = Identity(row["corpus"], row["path"], row["source_sha256"])
        if not identity.corpus or not identity.path or not row["section"]:
            raise ValueError(f"blank source identity field at {path}:{number}")
        if SOURCE_DIGEST.fullmatch(identity.digest) is None:
            raise ValueError(f"invalid source digest at {path}:{number}")
        if identity in seen:
            raise ValueError(
                f"duplicate source identity at {path}:{number}; first seen at line "
                f"{seen[identity]}"
            )
        seen[identity] = number
        if row["scan_status"] not in scan_statuses:
            raise ValueError(f"invalid scan status at {path}:{number}")
        if row["review_status"] not in REVIEW_STATUSES:
            raise ValueError(f"invalid review status at {path}:{number}")
        if schema_field is not None and schema_pattern.fullmatch(row[schema_field]) is None:
            raise ValueError(f"invalid audit schema at {path}:{number}")
    return rows


def identities(
    rows: Iterable[dict[str, str]], schema_field: str | None = None, schema: str | None = None
) -> frozenset[Identity]:
    return frozenset(
        Identity(row["corpus"], row["path"], row["source_sha256"])
        for row in rows
        if schema_field is None or row[schema_field] == schema
    )


def fixture_identities() -> frozenset[Identity]:
    found = set()
    for page in discover_pages([FIXTURE_ROOT]):
        digest = source_digest(page)
        if digest is None:
            raise ValueError(f"cannot decompress checked-in fixture: {page}")
        found.add(Identity("fixtures", relative_label(page, [FIXTURE_ROOT]), digest))
    return frozenset(found)


def load_coverage(arguments: argparse.Namespace) -> Coverage:
    fidelity_rows = read_rows(
        arguments.fidelity_db,
        FIDELITY_FIELDS,
        {"clean", "review", "hard-failure", "skipped"},
    )
    structure_rows = read_rows(
        arguments.structure_db,
        STRUCTURE_FIELDS,
        {"clean", "review", "hard-failure"},
        "profile_schema",
    )
    projection_rows = read_rows(
        arguments.projection_db,
        STRUCTURE_FIELDS,
        {"clean", "review", "hard-failure"},
        "profile_schema",
    )
    layout_rows = read_rows(
        arguments.layout_db,
        LAYOUT_FIELDS,
        {"clean", "review", "hard-failure"},
        "layout_schema",
    )
    fidelity = identities(fidelity_rows)
    comparable = identities(
        row for row in fidelity_rows if row["scan_status"] in {"clean", "review"}
    )
    current_structure = [
        row for row in structure_rows if row["profile_schema"] == CURRENT_STRUCTURE_SCHEMA
    ]
    current_projection = [
        row for row in projection_rows if row["profile_schema"] == CURRENT_PROJECTION_SCHEMA
    ]
    current_layout = [
        row for row in layout_rows if row["layout_schema"] == CURRENT_LAYOUT_SCHEMA
    ]
    summaries = tuple(
        LedgerSummary(
            name,
            len(rows),
            sum(
                Identity(row["corpus"], row["path"], row["source_sha256"])
                in baseline
                for row in rows
            ),
            Counter(row["scan_status"] for row in rows),
            sum(row["review_status"] == "pending" for row in rows),
            sum(
                row["review_status"] == "pending"
                and Identity(row["corpus"], row["path"], row["source_sha256"])
                in baseline
                for row in rows
            ),
        )
        for name, rows, baseline in (
            ("fidelity", fidelity_rows, fidelity),
            ("structure", current_structure, fidelity),
            ("projection", current_projection, fidelity),
            ("layout", current_layout, comparable),
        )
    )
    return Coverage(
        fidelity=fidelity,
        comparable=comparable,
        structure=identities(
            current_structure
        ),
        projection=identities(
            current_projection
        ),
        layout=identities(current_layout),
        fixture_inventory=fixture_identities(),
        summaries=summaries,
    )


def missing_sets(coverage: Coverage) -> dict[str, frozenset[Identity]]:
    return {
        "structure/fidelity": coverage.fidelity - coverage.structure,
        "projection/fidelity": coverage.fidelity - coverage.projection,
        "layout/comparable-fidelity": coverage.comparable - coverage.layout,
        "structure/fixtures": coverage.fixture_inventory - coverage.structure,
        "projection/fixtures": coverage.fixture_inventory - coverage.projection,
    }


def summarize(label: str, missing: frozenset[Identity]) -> None:
    by_corpus = Counter(item.corpus for item in missing)
    detail = ", ".join(f"{corpus}={count}" for corpus, count in sorted(by_corpus.items()))
    print(f"  {label}: {len(missing)} missing" + (f" ({detail})" if detail else ""))


def self_check() -> None:
    a = Identity("alpha", "man/man1/a.1", "a" * 64)
    b = Identity("alpha", "man/man1/b.1", "b" * 64)
    fixture = Identity("fixtures", "real/a.1", "c" * 64)
    aligned = Coverage(
        fidelity=frozenset({a, b}),
        comparable=frozenset({a}),
        structure=frozenset({a, b, fixture}),
        projection=frozenset({a, b, fixture}),
        layout=frozenset({a}),
        fixture_inventory=frozenset({fixture}),
        summaries=(),
    )
    assert all(not missing for missing in missing_sets(aligned).values())
    incomplete = Coverage(
        fidelity=aligned.fidelity,
        comparable=aligned.comparable,
        structure=frozenset({a}),
        projection=frozenset({b}),
        layout=frozenset(),
        fixture_inventory=aligned.fixture_inventory,
        summaries=(),
    )
    missing = missing_sets(incomplete)
    assert missing["structure/fidelity"] == frozenset({b})
    assert missing["projection/fidelity"] == frozenset({a})
    assert missing["layout/comparable-fidelity"] == frozenset({a})
    assert missing["structure/fixtures"] == frozenset({fixture})
    assert missing["projection/fixtures"] == frozenset({fixture})


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff audit coverage self-check succeeded")
        return 0
    try:
        coverage = load_coverage(arguments)
    except ValueError as error:
        print(f"check-roff-audit-coverage: {error}", file=sys.stderr)
        return 2
    missing = missing_sets(coverage)
    print("ManT roff audit coverage")
    print(f"  fidelity baseline:            {len(coverage.fidelity)}")
    print(f"  comparable fidelity baseline: {len(coverage.comparable)}")
    print(f"  checked-in fixture baseline:  {len(coverage.fixture_inventory)}")
    print("  current ledger rows:")
    for summary in coverage.summaries:
        statuses = ", ".join(
            f"{status}={count}" for status, count in sorted(summary.scan_statuses.items())
        )
        print(
            f"    {summary.name}: {summary.current_rows} "
            f"(baseline={summary.baseline_rows}; {statuses}; "
            f"pending-review={summary.baseline_pending} baseline/"
            f"{summary.pending} total)"
        )
    for label, items in missing.items():
        summarize(label, items)
    return 1 if any(missing.values()) else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
