#!/usr/bin/env python3
"""Verify the shared source-identity baseline across roff audit ledgers.

The content-fidelity ledger is the historical breadth index. Structure and
CommonMark-projection audits must cover every recorded source identity. The
renderer-layout audit needs only identities whose fidelity comparison reached
a comparable ``clean`` or ``review`` result; it may also contain independent
layout sweeps. Checked-in fixtures form a second, reproducible baseline shared
by the structure and projection ledgers. The mandoc reference route must replay
the complete historical fidelity baseline and cover every comparable result
in its own layout ledger. New fixtures can instead use a separate paired
ledger bound to the currently registered pristine CVS renderer; that fixture
supplement never replaces or relabels the historical distribution baseline.
The independent zero-width target and semantic-entry precision routes must
cover every checked-in fixture, but their distribution sweeps do not have to
mirror the visible-fidelity sample.
"""

from __future__ import annotations

import argparse
import csv
import json
import re
import shutil
import sys
import tempfile
from collections import Counter
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Iterable, Sequence

from scripts.roff.oracle import mandoc_oracle
from scripts.roff.lib.roff_audit_common import discover_pages, manual_section, relative_label, source_digest


ROOT = Path(__file__).resolve().parents[3]
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
DEFAULT_CVS_FIXTURE_FIDELITY_DB = ROFF_ROOT / "MANDOC_CVS_FIXTURE_FIDELITY_AUDIT.csv"
DEFAULT_CVS_FIXTURE_LAYOUT_DB = ROFF_ROOT / "MANDOC_CVS_FIXTURE_LAYOUT_AUDIT.csv"
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
    cvs_fixture_fidelity: frozenset[Identity]
    cvs_fixture_layout: frozenset[Identity]
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
        "--cvs-fixture-fidelity-db", type=Path,
        default=DEFAULT_CVS_FIXTURE_FIDELITY_DB,
        help="fixture-only content ledger for the active pristine CVS oracle",
    )
    parser.add_argument(
        "--cvs-fixture-layout-db", type=Path,
        default=DEFAULT_CVS_FIXTURE_LAYOUT_DB,
        help="matching fixture-only layout ledger for the active CVS oracle",
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
    path: Path, rows: Iterable[dict[str, str]]
) -> tuple[str, str]:
    identities = {(row["reference_kind"], row["reference_id"]) for row in rows}
    if len(identities) != 1:
        raise ValueError(f"{path} must contain exactly one mandoc renderer identity")
    identity = next(iter(identities))
    if identity[0] != "mandoc" or not identity[1]:
        raise ValueError(f"invalid mandoc renderer identity in {path}")
    return identity


def active_fixture_renderer(root: Path = ROOT) -> tuple[str, str]:
    """Verify tracked trust records without requiring a local oracle binary.

    Actual audit execution still performs binary/archive/build preflight.
    Coverage only binds recorded rows to an active registration and this
    checkout's locked source and recipe; it does not claim another render.
    """
    try:
        registry = json.loads((root / mandoc_oracle.REGISTRY).read_text())
        mandoc_oracle.exact_keys(registry, {"schema", "attestations"}, "oracle registry")
        if (registry["schema"] != "mant.mandoc-oracle-registry/v1"
                or not isinstance(registry["attestations"], dict)
                or any(not isinstance(record, dict) for record in registry["attestations"].values())):
            raise ValueError("unsupported oracle registry")
        active = [
            (identity, record)
            for identity, record in registry["attestations"].items()
            if record.get("status") == "active"
        ]
        if len(active) != 1:
            raise ValueError("fixture supplement requires exactly one active CVS oracle")
        identity, registration = active[0]
        mandoc_oracle.exact_keys(
            registration, {"path", "sha256", "status"}, "oracle registration"
        )
        path = mandoc_oracle.validate_hash_record(
            root, {key: registration[key] for key in ("path", "sha256")},
            "registered attestation",
        )
        attestation = json.loads(path.read_text())
        mandoc_oracle.exact_keys(attestation, {
            "schema", "identity", "source", "recipe", "toolchain", "platform",
            "buildEvidence", "artifact", "profiles",
        }, "fixture oracle attestation")
        mandoc_oracle.exact_keys(attestation["source"], {
            "lock", "manifest", "archive", "inventory", "pristine",
        }, "fixture oracle source")
        profiles = attestation["profiles"]
        if (attestation["schema"] != mandoc_oracle.SCHEMA
                or attestation["identity"] != identity
                or not isinstance(profiles, list)
                or any(not isinstance(profile, str) for profile in profiles)
                or len(set(profiles)) != len(profiles)
                or set(profiles) - mandoc_oracle.PROFILES
                or "utf8" not in profiles
                or attestation["source"]["pristine"] is not True):
            raise ValueError("fixture supplement requires a pristine UTF-8 CVS attestation")
        for key, expected in (
            ("lock", "crates/libmandoc-rs/upstream/SOURCE"),
            ("manifest", "crates/libmandoc-rs/upstream/FILES"),
            ("inventory", "crates/libmandoc-rs/upstream/CVS_INVENTORY.json"),
        ):
            record = attestation["source"][key]
            if record["path"] != expected:
                raise ValueError(f"fixture oracle {key} is not the current locked source")
            mandoc_oracle.validate_hash_record(root, record, f"oracle {key}")
        source_lock = dict(
            line.strip().split(" = ", 1)
            for line in (root / attestation["source"]["lock"]["path"]).read_text().splitlines()
            if " = " in line and not line.lstrip().startswith("#")
        )
        archive = attestation["source"]["archive"]
        mandoc_oracle.exact_keys(archive, {"path", "sha256"}, "oracle archive")
        mandoc_oracle.repository_path(root, archive["path"], "oracle archive")
        if (archive["sha256"] != source_lock["archive_sha256"]
                or Path(archive["path"]).name != source_lock["archive"]):
            raise ValueError("fixture oracle archive does not match the current source lock")
        recipe = attestation["recipe"]["file"]
        if recipe["path"] != "crates/libmandoc-rs/upstream/oracle/recipe.json":
            raise ValueError("fixture oracle recipe is not the current locked recipe")
        mandoc_oracle.validate_hash_record(root, recipe, "oracle recipe")
        return "mandoc", identity
    except (OSError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise ValueError(f"invalid fixture oracle trust records: {error}") from error


def validate_fixture_supplement(
    fidelity_path: Path,
    layout_path: Path,
    fidelity_rows: list[dict[str, str]],
    layout_rows: list[dict[str, str]],
    fixtures: frozenset[Identity],
    renderer: tuple[str, str],
) -> None:
    """A supplement proves only paired comparisons of exact checkout fixtures."""
    for path, rows in ((fidelity_path, fidelity_rows), (layout_path, layout_rows)):
        if mandoc_renderer_identity(path, rows) != renderer:
            raise ValueError(f"fixture supplement does not use the active CVS oracle: {path}")
        for number, row in enumerate(rows, 2):
            identity = Identity(row["corpus"], row["path"], row["source_sha256"])
            if identity not in fixtures:
                raise ValueError(f"supplement source is not an exact checked-in fixture at {path}:{number}")
            if row["section"] != manual_section(Path(identity.path)):
                raise ValueError(f"supplement has a mismatched fixture section at {path}:{number}")
            if row["scan_status"] not in {"clean", "review"}:
                raise ValueError(f"supplement comparison did not complete at {path}:{number}")
    if identities(fidelity_rows) != identities(layout_rows):
        raise ValueError("CVS fixture fidelity and layout supplements must cover the same sources")
    if any(row["layout_schema"] != CURRENT_LAYOUT_SCHEMA for row in layout_rows):
        raise ValueError("CVS fixture supplement requires the current layout schema")


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
    fixtures = fixture_identities()
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
    cvs_fixture_fidelity_rows = read_rows(
        arguments.cvs_fixture_fidelity_db,
        MANDOC_FIDELITY_FIELDS,
        {"clean", "review", "hard-failure", "skipped"},
    )
    cvs_fixture_layout_rows = read_rows(
        arguments.cvs_fixture_layout_db,
        MANDOC_LAYOUT_FIELDS,
        {"clean", "review", "hard-failure"},
        "layout_schema",
    )
    cvs_fixture_renderer = active_fixture_renderer()
    validate_fixture_supplement(
        arguments.cvs_fixture_fidelity_db,
        arguments.cvs_fixture_layout_db,
        cvs_fixture_fidelity_rows,
        cvs_fixture_layout_rows,
        fixtures,
        cvs_fixture_renderer,
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
    current_mandoc_deviations += validate_current_mandoc_deviations(
        arguments.deviation_db,
        deviation_rows,
        cvs_fixture_fidelity_rows,
        cvs_fixture_renderer,
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
            ("target", current_target, fixtures),
            ("semantic", current_semantic, fixtures),
            ("mandoc-fidelity", mandoc_fidelity_rows, mandoc_fidelity),
            ("mandoc-layout", current_mandoc_layout, mandoc_comparable),
            ("cvs-fixture-fidelity", cvs_fixture_fidelity_rows, fixtures),
            ("cvs-fixture-layout", cvs_fixture_layout_rows, fixtures),
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
            ("cvs-fixture-fidelity", cvs_fixture_fidelity_rows),
            ("cvs-fixture-layout", cvs_fixture_layout_rows),
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
        cvs_fixture_fidelity=identities(cvs_fixture_fidelity_rows),
        cvs_fixture_layout=identities(cvs_fixture_layout_rows),
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
        - coverage.mandoc_fidelity - coverage.cvs_fixture_fidelity,
        "mandoc-layout/fixtures": coverage.fixture_inventory
        - coverage.mandoc_layout - coverage.cvs_fixture_layout,
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


def validate_current_profile_schemas() -> None:
    # These independent profilers deliberately own their schemas. A version
    # change must update coverage too, rather than treating fresh rows as absent.
    for route, script, expected in (
        ("target", "targets", CURRENT_TARGET_SCHEMA),
        ("semantic", "semantics", CURRENT_SEMANTIC_SCHEMA),
    ):
        for path in (
            ROOT / f"scripts/roff/audit/audit_roff_{script}.py",
            ROOT / f"crates/mant-engine/examples/roff_{route}_profile.rs",
        ):
            declared = re.search(
                r'^(?:const )?PROFILE_SCHEMA(?:\s*:\s*&str)?\s*=\s*"([^"]+)"',
                path.read_text(encoding="utf-8"),
                re.MULTILINE,
            )
            if not declared or declared[1] != expected:
                raise ValueError(f"coverage schema {expected} disagrees with {path}")


def self_check_fixture_trust_records() -> None:
    """Mutate tracked trust inputs, never an actual oracle or vendor source."""
    target = ROOT / "target"
    target.mkdir(exist_ok=True)
    registry = json.loads((ROOT / mandoc_oracle.REGISTRY).read_text())
    identity, registration = next(
        (identity, record) for identity, record in registry["attestations"].items()
        if record["status"] == "active"
    )
    attestation = json.loads((ROOT / registration["path"]).read_text())
    tracked = [
        mandoc_oracle.REGISTRY, registration["path"],
        *[attestation["source"][key]["path"] for key in ("lock", "manifest", "inventory")],
        attestation["recipe"]["file"]["path"],
    ]
    with tempfile.TemporaryDirectory(prefix="roff-coverage-trust-", dir=target) as scratch:
        root = Path(scratch)
        for relative in tracked:
            destination = root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(ROOT / relative, destination)
        assert active_fixture_renderer(root) == ("mandoc", identity)
        for relative in tracked:
            path = root / relative
            original = path.read_bytes()
            path.write_bytes(original + b"\nmodified\n")
            try:
                active_fixture_renderer(root)
            except ValueError:
                pass
            else:
                raise AssertionError(f"modified oracle trust input accepted: {relative}")
            path.write_bytes(original)
        historical_registry = json.loads(json.dumps(registry))
        historical_registry["attestations"][identity]["status"] = "historical"
        (root / mandoc_oracle.REGISTRY).write_text(json.dumps(historical_registry))
        try:
            active_fixture_renderer(root)
        except ValueError:
            pass
        else:
            raise AssertionError("a historical oracle supplied current fixture coverage")
        (root / mandoc_oracle.REGISTRY).write_text(json.dumps(registry))
        # Rehashing a newly registered record cannot excuse non-pristine,
        # mismatched, or unsupported source metadata.
        for label, changed in (
            ("non-pristine", {**attestation, "source": {**attestation["source"], "pristine": False}}),
            ("different identity", {**attestation, "identity": "different"}),
            ("no UTF-8 authorization", {**attestation, "profiles": ["ascii"]}),
            ("unknown profile", {**attestation, "profiles": ["utf8", "unknown"]}),
            ("different archive", {**attestation, "source": {**attestation["source"], "archive": {**attestation["source"]["archive"], "sha256": "0" * 64}}}),
        ):
            path = root / registration["path"]
            path.write_text(json.dumps(changed))
            changed_registry = json.loads(json.dumps(registry))
            changed_registry["attestations"][identity]["sha256"] = mandoc_oracle.sha256(path)
            (root / mandoc_oracle.REGISTRY).write_text(json.dumps(changed_registry))
            try:
                active_fixture_renderer(root)
            except ValueError:
                pass
            else:
                raise AssertionError(f"fixture oracle accepted {label}")


def self_check() -> None:
    validate_current_profile_schemas()
    self_check_fixture_trust_records()
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
        cvs_fixture_fidelity=frozenset(),
        cvs_fixture_layout=frozenset(),
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
        cvs_fixture_fidelity=frozenset(),
        cvs_fixture_layout=frozenset(),
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

    supplemented = replace(
        incomplete,
        cvs_fixture_fidelity=frozenset({fixture}),
        cvs_fixture_layout=frozenset({fixture}),
    )
    supplemented_missing = missing_sets(supplemented)
    assert not supplemented_missing["mandoc-fidelity/fixtures"]
    assert not supplemented_missing["mandoc-layout/fixtures"]
    assert supplemented_missing["mandoc-fidelity/historical-fidelity"] == frozenset({b})

    renderer = active_fixture_renderer()
    fixture_row = {
        "reference_kind": renderer[0], "reference_id": renderer[1],
        "corpus": fixture.corpus, "path": fixture.path, "section": "1",
        "source_sha256": fixture.digest, "scan_status": "clean",
        "review_status": "not-required", "note": "",
    }
    fixture_layout_row = {**fixture_row, "layout_schema": CURRENT_LAYOUT_SCHEMA}
    validate_fixture_supplement(
        Path("fixture-content.csv"), Path("fixture-layout.csv"),
        [fixture_row], [fixture_layout_row], frozenset({fixture}), renderer,
    )
    for label, content, layout in (
        ("unregistered renderer", [{**fixture_row, "reference_id": "unregistered"}], [fixture_layout_row]),
        ("different layout renderer", [fixture_row], [{**fixture_layout_row, "reference_id": "historical"}]),
        ("missing layout source", [fixture_row], []),
        ("missing content source", [], [fixture_layout_row]),
        ("stale source digest", [{**fixture_row, "source_sha256": "d" * 64}], [fixture_layout_row]),
        ("distribution supplement", [{**fixture_row, "corpus": "distribution"}], [fixture_layout_row]),
        ("wrong source section", [{**fixture_row, "section": "2"}], [fixture_layout_row]),
        ("old layout schema", [fixture_row], [{**fixture_layout_row, "layout_schema": "mant.roff-layout-audit/v1"}]),
        ("skipped comparison", [{**fixture_row, "scan_status": "skipped"}], [fixture_layout_row]),
        ("failed comparison", [fixture_row], [{**fixture_layout_row, "scan_status": "hard-failure"}]),
    ):
        try:
            validate_fixture_supplement(
                Path("fixture-content.csv"), Path("fixture-layout.csv"),
                content, layout, frozenset({fixture}), renderer,
            )
        except ValueError:
            pass
        else:
            raise AssertionError(f"fixture supplement accepted {label}")
    unresolved = replace(supplemented, pending=(("cvs-fixture-fidelity", frozenset({fixture})),))
    assert missing_sets(unresolved)["pending/cvs-fixture-fidelity"] == frozenset({fixture})

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
