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
mirror the visible-fidelity sample. Historical package-mandoc conclusions and
new pinned-CVS fixture supplements are separate renderer cohorts: their source
identities must not overlap, and each content/layout pair is checked alone.
"""

from __future__ import annotations

import argparse
import csv
import re
import sys
from collections import Counter
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Callable, Iterable, Sequence

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
DEFAULT_MANDOC_CVS_FIDELITY_DB = ROFF_ROOT / "MANDOC_CVS_FIDELITY_AUDIT.csv"
DEFAULT_MANDOC_CVS_LAYOUT_DB = ROFF_ROOT / "MANDOC_CVS_LAYOUT_AUDIT.csv"
DEFAULT_DEVIATION_DB = ROFF_ROOT / "REFERENCE_RENDERER_DEVIATIONS.csv"
PACKAGE_MANDOC_REFERENCE_ID = "mandoc-1.14.6-1"
CVS_MANDOC_REFERENCE_ID = "cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1"

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
CURRENT_TARGET_SCHEMA = "mant.roff-target-profile/v4"
CURRENT_SEMANTIC_SCHEMA = "mant.roff-semantic-profile/v5"
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
    mandoc_cvs_fidelity: frozenset[Identity]
    mandoc_cvs_comparable: frozenset[Identity]
    mandoc_cvs_layout: frozenset[Identity]
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
    parser.add_argument(
        "--mandoc-cvs-fidelity-db", type=Path, default=DEFAULT_MANDOC_CVS_FIDELITY_DB
    )
    parser.add_argument(
        "--mandoc-cvs-layout-db", type=Path, default=DEFAULT_MANDOC_CVS_LAYOUT_DB
    )
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
    path: Path,
    rows: Iterable[dict[str, str]],
    expected_id: str,
    *,
    allow_empty: bool = False,
) -> tuple[str, str]:
    renderers = {(row["reference_kind"], row["reference_id"]) for row in rows}
    expected = ("mandoc", expected_id)
    # The CVS supplement starts header-only. Its fixed, registered reference
    # identity is a contract for future rows, not evidence that any page ran.
    if not renderers and allow_empty:
        return expected
    if renderers != {expected}:
        raise ValueError(
            f"{path} must contain only mandoc renderer identity {expected_id}"
        )
    return expected


def validate_mandoc_cohorts(
    package_rows: Iterable[dict[str, str]], cvs_rows: Iterable[dict[str, str]]
) -> None:
    overlap = identities(package_rows) & identities(cvs_rows)
    if overlap:
        raise ValueError(
            "package and pinned-CVS mandoc fidelity cohorts overlap in "
            f"{len(overlap)} source identities"
        )


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
    if (reference_kind, reference_id) != ("mandoc", PACKAGE_MANDOC_REFERENCE_ID):
        raise ValueError("historical mandoc deviations require the package cohort")
    expected_renderer = f"{reference_id} -T utf8 -O width=200"
    evidence = {
        Identity(row["corpus"], row["path"], row["source_sha256"]): row
        for row in fidelity_rows
    }
    current = 0
    for number, row in enumerate(deviations, 2):
        renderer_id = row["reference_renderer"].split(" ", 1)[0]
        if renderer_id == CVS_MANDOC_REFERENCE_ID:
            raise ValueError(
                f"pinned-CVS deviation needs a separate reviewed route at "
                f"{path}:{number}"
            )
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
    mandoc_cvs_fidelity_rows = read_rows(
        arguments.mandoc_cvs_fidelity_db,
        MANDOC_FIDELITY_FIELDS,
        {"clean", "review", "hard-failure", "skipped"},
    )
    mandoc_cvs_layout_rows = read_rows(
        arguments.mandoc_cvs_layout_db,
        MANDOC_LAYOUT_FIELDS,
        {"clean", "review", "hard-failure"},
        "layout_schema",
    )
    deviation_rows = read_deviation_rows(arguments.deviation_db)
    mandoc_fidelity_renderer = mandoc_renderer_identity(
        arguments.mandoc_fidelity_db,
        mandoc_fidelity_rows,
        PACKAGE_MANDOC_REFERENCE_ID,
    )
    mandoc_layout_renderer = mandoc_renderer_identity(
        arguments.mandoc_layout_db,
        mandoc_layout_rows,
        PACKAGE_MANDOC_REFERENCE_ID,
    )
    if mandoc_fidelity_renderer != mandoc_layout_renderer:
        raise ValueError(
            "package mandoc fidelity and layout ledgers use different renderer identities"
        )
    mandoc_cvs_fidelity_renderer = mandoc_renderer_identity(
        arguments.mandoc_cvs_fidelity_db,
        mandoc_cvs_fidelity_rows,
        CVS_MANDOC_REFERENCE_ID,
        allow_empty=True,
    )
    mandoc_cvs_layout_renderer = mandoc_renderer_identity(
        arguments.mandoc_cvs_layout_db,
        mandoc_cvs_layout_rows,
        CVS_MANDOC_REFERENCE_ID,
        allow_empty=True,
    )
    if mandoc_cvs_fidelity_renderer != mandoc_cvs_layout_renderer:
        raise ValueError(
            "pinned-CVS mandoc fidelity and layout ledgers use different renderer identities"
        )
    validate_mandoc_cohorts(mandoc_fidelity_rows, mandoc_cvs_fidelity_rows)
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
    current_mandoc_cvs_layout = [
        row
        for row in mandoc_cvs_layout_rows
        if row["layout_schema"] == CURRENT_LAYOUT_SCHEMA
    ]
    mandoc_fidelity = identities(mandoc_fidelity_rows)
    mandoc_comparable = identities(
        row
        for row in mandoc_fidelity_rows
        if row["scan_status"] in {"clean", "review"}
    )
    mandoc_cvs_fidelity = identities(mandoc_cvs_fidelity_rows)
    mandoc_cvs_comparable = identities(
        row
        for row in mandoc_cvs_fidelity_rows
        if row["scan_status"] in {"clean", "review"}
    )
    fixtures = fixture_identities()
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
            ("target", current_target, fixtures),
            ("semantic", current_semantic, fixtures),
            ("mandoc-package-fidelity", mandoc_fidelity_rows, mandoc_fidelity),
            ("mandoc-package-layout", current_mandoc_layout, mandoc_comparable),
            ("mandoc-cvs-fidelity", mandoc_cvs_fidelity_rows, fixtures),
            ("mandoc-cvs-layout", current_mandoc_cvs_layout, fixtures),
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
            ("mandoc-package-fidelity", mandoc_fidelity_rows),
            ("mandoc-package-layout", current_mandoc_layout),
            ("mandoc-cvs-fidelity", mandoc_cvs_fidelity_rows),
            ("mandoc-cvs-layout", current_mandoc_cvs_layout),
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
        mandoc_cvs_fidelity=mandoc_cvs_fidelity,
        mandoc_cvs_comparable=mandoc_cvs_comparable,
        mandoc_cvs_layout=identities(current_mandoc_cvs_layout),
        current_mandoc_deviations=current_mandoc_deviations,
        fixture_inventory=fixtures,
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
        - coverage.mandoc_fidelity
        - coverage.mandoc_cvs_fidelity,
        "mandoc-layout/comparable-cvs-fidelity": coverage.mandoc_cvs_comparable
        - coverage.mandoc_cvs_layout,
        "mandoc-layout/fixtures": coverage.fixture_inventory
        - coverage.mandoc_layout
        - coverage.mandoc_cvs_layout,
        "mandoc-fidelity/unexpected": coverage.mandoc_fidelity - coverage.fidelity,
        "mandoc-cvs-fidelity/unexpected": coverage.mandoc_cvs_fidelity
        - coverage.fixture_inventory,
        "mandoc-layout/unexpected": coverage.mandoc_layout
        - coverage.mandoc_comparable,
        "mandoc-cvs-layout/unexpected": coverage.mandoc_cvs_layout
        - coverage.mandoc_cvs_comparable,
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


def validate_current_profile_schemas() -> None:
    # These independent profilers deliberately own their schemas. A version
    # change must update coverage too, rather than treating fresh rows as absent.
    for route, script, expected in (
        ("target", "targets", CURRENT_TARGET_SCHEMA),
        ("semantic", "semantics", CURRENT_SEMANTIC_SCHEMA),
    ):
        for path in (
            ROOT / f"scripts/audit-roff-{script}.py",
            ROOT / f"crates/mant-engine/examples/roff_{route}_profile.rs",
        ):
            declared = re.search(
                r'^(?:const )?PROFILE_SCHEMA(?:\s*:\s*&str)?\s*=\s*"([^"]+)"',
                path.read_text(encoding="utf-8"),
                re.MULTILINE,
            )
            if not declared or declared[1] != expected:
                raise ValueError(f"coverage schema {expected} disagrees with {path}")


def self_check() -> None:
    validate_current_profile_schemas()
    a = Identity("alpha", "man/man1/a.1", "a" * 64)
    b = Identity("alpha", "man/man1/b.1", "b" * 64)
    fixture = Identity("fixtures", "real/a.1", "c" * 64)
    aligned = Coverage(
        fidelity=frozenset({a, b, fixture}),
        comparable=frozenset({a, fixture}),
        structure=frozenset({a, b, fixture}),
        projection=frozenset({a, b, fixture}),
        layout=frozenset({a, fixture}),
        target=frozenset({fixture}),
        semantic=frozenset({fixture}),
        mandoc_fidelity=frozenset({a, b, fixture}),
        mandoc_comparable=frozenset({a, fixture}),
        mandoc_layout=frozenset({a, fixture}),
        mandoc_cvs_fidelity=frozenset(),
        mandoc_cvs_comparable=frozenset(),
        mandoc_cvs_layout=frozenset(),
        current_mandoc_deviations=0,
        fixture_inventory=frozenset({fixture}),
        pending=(("mandoc-package-fidelity", frozenset()),),
        summaries=(),
    )
    assert all(not missing for missing in missing_sets(aligned).values())
    cvs_fixture = Identity("fixtures", "real/new.1", "d" * 64)
    supplemented = replace(
        aligned,
        structure=aligned.structure | {cvs_fixture},
        projection=aligned.projection | {cvs_fixture},
        target=aligned.target | {cvs_fixture},
        semantic=aligned.semantic | {cvs_fixture},
        mandoc_cvs_fidelity=frozenset({cvs_fixture}),
        mandoc_cvs_comparable=frozenset({cvs_fixture}),
        mandoc_cvs_layout=frozenset({cvs_fixture}),
        fixture_inventory=aligned.fixture_inventory | {cvs_fixture},
    )
    assert all(not missing for missing in missing_sets(supplemented).values())
    assert missing_sets(
        replace(supplemented, mandoc_cvs_layout=frozenset())
    )["mandoc-layout/comparable-cvs-fidelity"] == frozenset({cvs_fixture})
    cross_cohort_layout = missing_sets(
        replace(
            supplemented,
            mandoc_layout=supplemented.mandoc_layout | {cvs_fixture},
            mandoc_cvs_layout=frozenset(),
        )
    )
    assert cross_cohort_layout["mandoc-layout/comparable-cvs-fidelity"] == frozenset(
        {cvs_fixture}
    )
    assert cross_cohort_layout["mandoc-layout/unexpected"] == frozenset({cvs_fixture})
    assert missing_sets(
        replace(supplemented, mandoc_cvs_fidelity=frozenset())
    )["mandoc-fidelity/fixtures"] == frozenset({cvs_fixture})
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
        mandoc_cvs_fidelity=frozenset(),
        mandoc_cvs_comparable=frozenset(),
        mandoc_cvs_layout=frozenset(),
        current_mandoc_deviations=0,
        fixture_inventory=aligned.fixture_inventory,
        pending=(("mandoc-package-fidelity", frozenset({a})),),
        summaries=(),
    )
    missing = missing_sets(incomplete)
    assert missing["structure/fidelity"] == frozenset({b, fixture})
    assert missing["projection/fidelity"] == frozenset({a, fixture})
    assert missing["layout/comparable-fidelity"] == frozenset({a, fixture})
    assert missing["structure/fixtures"] == frozenset({fixture})
    assert missing["projection/fixtures"] == frozenset({fixture})
    assert missing["target/fixtures"] == frozenset({fixture})
    assert missing["semantic/fixtures"] == frozenset({fixture})
    assert missing["mandoc-fidelity/historical-fidelity"] == frozenset({b, fixture})
    assert missing["mandoc-layout/comparable-mandoc-fidelity"] == frozenset({a})
    assert missing["mandoc-fidelity/fixtures"] == frozenset({fixture})
    assert missing["mandoc-layout/fixtures"] == frozenset({fixture})
    assert missing["pending/mandoc-package-fidelity"] == frozenset({a})

    def rejected(action: Callable[[], object], reason: str) -> None:
        try:
            action()
        except ValueError:
            return
        raise AssertionError(f"{reason} was accepted")

    cvs_row = {
        "corpus": cvs_fixture.corpus,
        "path": cvs_fixture.path,
        "source_sha256": cvs_fixture.digest,
        "reference_kind": "mandoc",
        "reference_id": CVS_MANDOC_REFERENCE_ID,
    }
    package_row = {
        **cvs_row,
        "path": fixture.path,
        "source_sha256": fixture.digest,
        "reference_id": PACKAGE_MANDOC_REFERENCE_ID,
    }
    assert mandoc_renderer_identity(
        Path("cvs.csv"), [], CVS_MANDOC_REFERENCE_ID, allow_empty=True
    ) == ("mandoc", CVS_MANDOC_REFERENCE_ID)
    assert mandoc_renderer_identity(
        Path("cvs.csv"), [cvs_row], CVS_MANDOC_REFERENCE_ID, allow_empty=True
    ) == ("mandoc", CVS_MANDOC_REFERENCE_ID)
    rejected(
        lambda: mandoc_renderer_identity(
            Path("cvs.csv"),
            [cvs_row, package_row],
            CVS_MANDOC_REFERENCE_ID,
            allow_empty=True,
        ),
        "mixed renderer IDs",
    )
    rejected(
        lambda: mandoc_renderer_identity(
            Path("package.csv"), [cvs_row], PACKAGE_MANDOC_REFERENCE_ID
        ),
        "CVS evidence under the historical package ID",
    )
    rejected(
        lambda: validate_mandoc_cohorts([package_row], [package_row]),
        "overlapping renderer cohorts",
    )
    validate_mandoc_cohorts([package_row], [cvs_row])

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
        "reference_renderer": f"{PACKAGE_MANDOC_REFERENCE_ID} -T utf8 -O width=200",
        "review_state": "reproduced",
    }
    assert (
        validate_current_mandoc_deviations(
            Path("deviations.csv"),
            [mandoc_deviation],
            [mandoc_fidelity],
            ("mandoc", PACKAGE_MANDOC_REFERENCE_ID),
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
            ("mandoc", PACKAGE_MANDOC_REFERENCE_ID),
        )
        == 1
    )
    invalid_deviation = {**mandoc_deviation, "section": "2"}
    try:
        validate_current_mandoc_deviations(
            Path("deviations.csv"),
            [invalid_deviation],
            [mandoc_fidelity],
            ("mandoc", PACKAGE_MANDOC_REFERENCE_ID),
        )
    except ValueError:
        pass
    else:
        raise AssertionError("a mismatched mandoc deviation section was accepted")
    rejected(
        lambda: validate_current_mandoc_deviations(
            Path("deviations.csv"),
            [mandoc_deviation],
            [mandoc_fidelity],
            ("mandoc", CVS_MANDOC_REFERENCE_ID),
        ),
        "historical deviation reassigned to CVS",
    )
    rejected(
        lambda: validate_current_mandoc_deviations(
            Path("deviations.csv"),
            [
                {
                    **mandoc_deviation,
                    "reference_renderer": (
                        f"{CVS_MANDOC_REFERENCE_ID} -T utf8 -O width=200"
                    ),
                }
            ],
            [mandoc_fidelity],
            ("mandoc", PACKAGE_MANDOC_REFERENCE_ID),
        ),
        "CVS deviation silently accepted in historical index",
    )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff audit coverage self-check succeeded")
        return 0
    try:
        validate_current_profile_schemas()
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
        ) or "no rows"
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
