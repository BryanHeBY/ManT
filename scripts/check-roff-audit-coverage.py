#!/usr/bin/env python3
"""Verify the shared source-identity baseline across roff audit ledgers.

The content-fidelity ledger is the historical breadth index. Structure and
CommonMark-projection audits must cover every recorded source identity. The
renderer-layout audit needs only identities whose fidelity comparison reached
a comparable ``clean`` or ``review`` result; it may also contain independent
layout sweeps. Checked-in fixtures form a second, reproducible baseline shared
by the structure and projection ledgers. The mandoc reference route must replay
the complete historical fidelity baseline, cover every comparable result in
its own layout ledger, and include every checked-in fixture in both ledgers.
The independent zero-width target and semantic-entry precision routes must
cover every checked-in fixture, but their distribution sweeps do not have to
mirror the visible-fidelity sample.
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
DEFAULT_TARGET_DB = ROFF_ROOT / "TARGET_AUDIT.csv"
DEFAULT_SEMANTIC_DB = ROFF_ROOT / "SEMANTIC_AUDIT.csv"
DEFAULT_MANDOC_FIDELITY_DB = ROFF_ROOT / "MANDOC_FIDELITY_AUDIT.csv"
DEFAULT_MANDOC_LAYOUT_DB = ROFF_ROOT / "MANDOC_LAYOUT_AUDIT.csv"
DEFAULT_DEVIATION_DB = ROFF_ROOT / "REFERENCE_RENDERER_DEVIATIONS.csv"

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
MANDOC_FIDELITY_FIELDS = ["reference_kind", "reference_id"] + FIDELITY_FIELDS
MANDOC_LAYOUT_FIELDS = ["reference_kind", "reference_id"] + LAYOUT_FIELDS
DEVIATION_FIELDS = [
    "id",
    "category",
    "review_state",
    *IDENTITY_FIELDS,
    "reference_renderer",
    "mant_advantage",
    "reference_limitation",
    "scope",
    "note",
]

CURRENT_STRUCTURE_SCHEMA = "mant.roff-structure-profile/v4"
CURRENT_PROJECTION_SCHEMA = "mant.roff-projection-profile/v3"
CURRENT_LAYOUT_SCHEMA = "mant.roff-layout-audit/v3"
CURRENT_TARGET_SCHEMA = "mant.roff-target-profile/v3"
CURRENT_SEMANTIC_SCHEMA = "mant.roff-semantic-profile/v1"
SOURCE_DIGEST = re.compile(r"[0-9a-f]{64}")
PROFILE_SCHEMA = re.compile(
    r"mant\.roff-(?:structure|projection|target|semantic)-profile/v[1-9][0-9]*"
)
LAYOUT_SCHEMA = re.compile(r"mant\.roff-layout-audit/v[1-9][0-9]*")
REVIEW_STATUSES = {
    "not-required",
    "pending",
    "false-positive",
    "confirmed-open",
    "confirmed-fixed",
}
DEVIATION_REVIEW_STATES = {"historical-reviewed", "reproduced"}


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
    target: frozenset[Identity]
    semantic: frozenset[Identity]
    mandoc_fidelity: frozenset[Identity]
    mandoc_comparable: frozenset[Identity]
    mandoc_layout: frozenset[Identity]
    current_mandoc_deviations: int
    fixture_inventory: frozenset[Identity]
    pending: tuple[tuple[str, frozenset[Identity]], ...]
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
    parser.add_argument("--target-db", type=Path, default=DEFAULT_TARGET_DB)
    parser.add_argument("--semantic-db", type=Path, default=DEFAULT_SEMANTIC_DB)
    parser.add_argument(
        "--mandoc-fidelity-db", type=Path, default=DEFAULT_MANDOC_FIDELITY_DB
    )
    parser.add_argument("--mandoc-layout-db", type=Path, default=DEFAULT_MANDOC_LAYOUT_DB)
    parser.add_argument("--deviation-db", type=Path, default=DEFAULT_DEVIATION_DB)
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


def mandoc_renderer_identity(
    path: Path, rows: Iterable[dict[str, str]]
) -> tuple[str, str]:
    identities = {(row["reference_kind"], row["reference_id"]) for row in rows}
    if len(identities) != 1:
        raise ValueError(f"{path} must contain exactly one mandoc renderer identity")
    identity = next(iter(identities))
    if identity[0] != "mandoc" or not identity[1]:
        raise ValueError(f"invalid mandoc renderer identity in {path}")
    return identity


def read_deviation_rows(path: Path) -> list[dict[str, str]]:
    if not path.is_file():
        raise ValueError(f"renderer-deviation ledger does not exist: {path}")
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != DEVIATION_FIELDS:
            raise ValueError(
                f"invalid renderer-deviation header in {path}; "
                f"expected {','.join(DEVIATION_FIELDS)}"
            )
        rows = list(reader)
    seen: dict[str, int] = {}
    for number, row in enumerate(rows, 2):
        if any(not row[field] for field in DEVIATION_FIELDS):
            raise ValueError(f"blank renderer-deviation field at {path}:{number}")
        if row["id"] in seen:
            raise ValueError(
                f"duplicate renderer-deviation id at {path}:{number}; first seen "
                f"at line {seen[row['id']]}"
            )
        seen[row["id"]] = number
        if row["review_state"] not in DEVIATION_REVIEW_STATES:
            raise ValueError(f"invalid renderer-deviation review state at {path}:{number}")
        if SOURCE_DIGEST.fullmatch(row["source_sha256"]) is None:
            raise ValueError(f"invalid renderer-deviation source digest at {path}:{number}")
    return rows


def validate_current_mandoc_deviations(
    path: Path,
    deviations: Iterable[dict[str, str]],
    fidelity_rows: Iterable[dict[str, str]],
    renderer: tuple[str, str],
) -> int:
    reference_kind, reference_id = renderer
    assert reference_kind == "mandoc"
    expected_renderer = f"{reference_id} -T utf8 -O width=200"
    evidence = {
        Identity(row["corpus"], row["path"], row["source_sha256"]): row
        for row in fidelity_rows
    }
    current = 0
    for number, row in enumerate(deviations, 2):
        renderer_id = row["reference_renderer"].split(" ", 1)[0]
        if renderer_id != reference_id:
            continue
        current += 1
        if row["reference_renderer"] != expected_renderer:
            raise ValueError(
                f"current mandoc deviation has the wrong renderer command at "
                f"{path}:{number}"
            )
        if row["review_state"] != "reproduced":
            raise ValueError(
                f"current mandoc deviation is not reproduced at {path}:{number}"
            )
        identity = Identity(row["corpus"], row["path"], row["source_sha256"])
        source_row = evidence.get(identity)
        if source_row is None:
            raise ValueError(
                f"current mandoc deviation is absent from the fidelity ledger at "
                f"{path}:{number}"
            )
        if source_row["section"] != row["section"]:
            raise ValueError(
                f"current mandoc deviation has a mismatched section at {path}:{number}"
            )
        source_conclusion = (
            source_row["scan_status"] == "review"
            and source_row["review_status"] == "false-positive"
        ) or (
            source_row["scan_status"] in {"clean", "review"}
            and source_row["review_status"] == "confirmed-fixed"
        )
        if not source_conclusion:
            raise ValueError(
                f"current mandoc deviation lacks a reviewed source "
                f"conclusion at {path}:{number}"
            )
    return current


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
    target_rows = read_rows(
        arguments.target_db,
        STRUCTURE_FIELDS,
        {"clean", "review", "hard-failure"},
        "profile_schema",
    )
    semantic_rows = read_rows(
        arguments.semantic_db,
        STRUCTURE_FIELDS,
        {"clean", "review", "hard-failure"},
        "profile_schema",
    )
    mandoc_fidelity_rows = read_rows(
        arguments.mandoc_fidelity_db,
        MANDOC_FIDELITY_FIELDS,
        {"clean", "review", "hard-failure", "skipped"},
    )
    mandoc_layout_rows = read_rows(
        arguments.mandoc_layout_db,
        MANDOC_LAYOUT_FIELDS,
        {"clean", "review", "hard-failure"},
        "layout_schema",
    )
    deviation_rows = read_deviation_rows(arguments.deviation_db)
    mandoc_fidelity_renderer = mandoc_renderer_identity(
        arguments.mandoc_fidelity_db, mandoc_fidelity_rows
    )
    mandoc_layout_renderer = mandoc_renderer_identity(
        arguments.mandoc_layout_db, mandoc_layout_rows
    )
    if mandoc_fidelity_renderer != mandoc_layout_renderer:
        raise ValueError(
            "mandoc fidelity and layout ledgers use different renderer identities"
        )
    current_mandoc_deviations = validate_current_mandoc_deviations(
        arguments.deviation_db,
        deviation_rows,
        mandoc_fidelity_rows,
        mandoc_fidelity_renderer,
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
    current_target = [
        row for row in target_rows if row["profile_schema"] == CURRENT_TARGET_SCHEMA
    ]
    current_semantic = [
        row
        for row in semantic_rows
        if row["profile_schema"] == CURRENT_SEMANTIC_SCHEMA
    ]
    current_mandoc_layout = [
        row
        for row in mandoc_layout_rows
        if row["layout_schema"] == CURRENT_LAYOUT_SCHEMA
    ]
    mandoc_fidelity = identities(mandoc_fidelity_rows)
    mandoc_comparable = identities(
        row
        for row in mandoc_fidelity_rows
        if row["scan_status"] in {"clean", "review"}
    )
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
            ("target", current_target, fixture_identities()),
            ("semantic", current_semantic, fixture_identities()),
            ("mandoc-fidelity", mandoc_fidelity_rows, mandoc_fidelity),
            ("mandoc-layout", current_mandoc_layout, mandoc_comparable),
        )
    )
    pending = tuple(
        (
            name,
            identities(row for row in rows if row["review_status"] == "pending"),
        )
        for name, rows in (
            ("fidelity", fidelity_rows),
            ("structure", current_structure),
            ("projection", current_projection),
            ("layout", current_layout),
            ("target", current_target),
            ("semantic", current_semantic),
            ("mandoc-fidelity", mandoc_fidelity_rows),
            ("mandoc-layout", current_mandoc_layout),
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
        target=identities(current_target),
        semantic=identities(current_semantic),
        mandoc_fidelity=mandoc_fidelity,
        mandoc_comparable=mandoc_comparable,
        mandoc_layout=identities(current_mandoc_layout),
        current_mandoc_deviations=current_mandoc_deviations,
        fixture_inventory=fixture_identities(),
        pending=pending,
        summaries=summaries,
    )


def missing_sets(coverage: Coverage) -> dict[str, frozenset[Identity]]:
    return {
        "structure/fidelity": coverage.fidelity - coverage.structure,
        "projection/fidelity": coverage.fidelity - coverage.projection,
        "layout/comparable-fidelity": coverage.comparable - coverage.layout,
        "structure/fixtures": coverage.fixture_inventory - coverage.structure,
        "projection/fixtures": coverage.fixture_inventory - coverage.projection,
        "target/fixtures": coverage.fixture_inventory - coverage.target,
        "semantic/fixtures": coverage.fixture_inventory - coverage.semantic,
        "mandoc-fidelity/historical-fidelity": coverage.fidelity
        - coverage.mandoc_fidelity,
        "mandoc-layout/comparable-mandoc-fidelity": coverage.mandoc_comparable
        - coverage.mandoc_layout,
        "mandoc-fidelity/fixtures": coverage.fixture_inventory
        - coverage.mandoc_fidelity,
        "mandoc-layout/fixtures": coverage.fixture_inventory - coverage.mandoc_layout,
        "mandoc-fidelity/unexpected": coverage.mandoc_fidelity
        - coverage.fidelity
        - coverage.fixture_inventory,
        "mandoc-layout/unexpected": coverage.mandoc_layout
        - coverage.mandoc_comparable,
        **{f"pending/{name}": items for name, items in coverage.pending},
    }


def summarize(label: str, missing: frozenset[Identity]) -> None:
    by_corpus = Counter(item.corpus for item in missing)
    detail = ", ".join(f"{corpus}={count}" for corpus, count in sorted(by_corpus.items()))
    if label.startswith("pending/"):
        noun = "unresolved"
    elif label.endswith("/unexpected"):
        noun = "unexpected"
    else:
        noun = "missing"
    print(f"  {label}: {len(missing)} {noun}" + (f" ({detail})" if detail else ""))


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
        target=frozenset({fixture}),
        semantic=frozenset({fixture}),
        mandoc_fidelity=frozenset({a, b, fixture}),
        mandoc_comparable=frozenset({a, fixture}),
        mandoc_layout=frozenset({a, fixture}),
        current_mandoc_deviations=0,
        fixture_inventory=frozenset({fixture}),
        pending=(("mandoc-fidelity", frozenset()),),
        summaries=(),
    )
    assert all(not missing for missing in missing_sets(aligned).values())
    incomplete = Coverage(
        fidelity=aligned.fidelity,
        comparable=aligned.comparable,
        structure=frozenset({a}),
        projection=frozenset({b}),
        layout=frozenset(),
        target=frozenset(),
        semantic=frozenset(),
        mandoc_fidelity=frozenset({a}),
        mandoc_comparable=frozenset({a}),
        mandoc_layout=frozenset(),
        current_mandoc_deviations=0,
        fixture_inventory=aligned.fixture_inventory,
        pending=(("mandoc-fidelity", frozenset({a})),),
        summaries=(),
    )
    missing = missing_sets(incomplete)
    assert missing["structure/fidelity"] == frozenset({b})
    assert missing["projection/fidelity"] == frozenset({a})
    assert missing["layout/comparable-fidelity"] == frozenset({a})
    assert missing["structure/fixtures"] == frozenset({fixture})
    assert missing["projection/fixtures"] == frozenset({fixture})
    assert missing["target/fixtures"] == frozenset({fixture})
    assert missing["semantic/fixtures"] == frozenset({fixture})
    assert missing["mandoc-fidelity/historical-fidelity"] == frozenset({b})
    assert missing["mandoc-layout/comparable-mandoc-fidelity"] == frozenset({a})
    assert missing["mandoc-fidelity/fixtures"] == frozenset({fixture})
    assert missing["mandoc-layout/fixtures"] == frozenset({fixture})
    assert missing["pending/mandoc-fidelity"] == frozenset({a})

    mandoc_fidelity = {
        "corpus": a.corpus,
        "path": a.path,
        "section": "1",
        "source_sha256": a.digest,
        "scan_status": "review",
        "review_status": "false-positive",
    }
    mandoc_deviation = {
        "corpus": a.corpus,
        "path": a.path,
        "section": "1",
        "source_sha256": a.digest,
        "reference_renderer": "mandoc-test -T utf8 -O width=200",
        "review_state": "reproduced",
    }
    assert (
        validate_current_mandoc_deviations(
            Path("deviations.csv"),
            [mandoc_deviation],
            [mandoc_fidelity],
            ("mandoc", "mandoc-test"),
        )
        == 1
    )
    fixed_fidelity = {
        **mandoc_fidelity,
        "scan_status": "clean",
        "review_status": "confirmed-fixed",
    }
    assert (
        validate_current_mandoc_deviations(
            Path("deviations.csv"),
            [mandoc_deviation],
            [fixed_fidelity],
            ("mandoc", "mandoc-test"),
        )
        == 1
    )
    invalid_deviation = {**mandoc_deviation, "section": "2"}
    try:
        validate_current_mandoc_deviations(
            Path("deviations.csv"),
            [invalid_deviation],
            [mandoc_fidelity],
            ("mandoc", "mandoc-test"),
        )
    except ValueError:
        pass
    else:
        raise AssertionError("a mismatched mandoc deviation section was accepted")


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
    print(
        "  current mandoc deviations:     "
        f"{coverage.current_mandoc_deviations}"
    )
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
