#!/usr/bin/env python3
"""Find AST-to-IR topology gaps in local roff corpora.

Unlike ``audit-roff-fidelity.py``, this development-only audit never compares
terminal wrapping or invokes a host renderer. It compares structural
obligations observed in libmandoc's owned AST with the source-aware ManT IR:
no-fill source lines, paragraph/list/definition container shape, table rows,
cells and spans, relative indentation, and typed navigation links.

The output is deliberately a review queue. A count mismatch can expose true
lowering loss, but a human must still inspect the source and IR before adding a
focused regression. Ordinary CI runs only the script's dependency-free
``--self-check`` and Rust regressions derived from confirmed findings.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import sys
from collections import Counter
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Iterable, Sequence

from scripts.roff.lib.roff_ledger_paths import audit_ledger_path
from scripts.roff.lib.roff_audit_common import (
    SourceReview, discover_pages, is_nonnegative_integer, manual_section, ordinal_source_reviews,
    compile_source_patterns, manual_compression, manual_transport_error, observe_sources,
    relative_label, run_jsonl_profile_batch,
    source_digest, source_reviews, uncovered_sources, verify_source_digest,
)


ROOT = Path(__file__).resolve().parents[3]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_PROFILER = ROOT / "target/debug/examples/roff_structure_profile"
DEFAULT_AUDIT_DB = ROOT / "tests/fixtures/roff/STRUCTURE_AUDIT.csv"
DEFAULT_FIDELITY_DB = ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv"
PROFILE_SCHEMA = "mant.roff-structure-profile/v29"
PROFILE_SCHEMA_PATTERN = re.compile(r"mant\.roff-structure-profile/v[1-9][0-9]*$")
STRUCTURE_DATABASE_FIELDS = [
    "corpus",
    "path",
    "section",
    "source_sha256",
    "profile_schema",
    "scan_status",
    "review_status",
    "note",
]
FIDELITY_DATABASE_FIELDS = [
    "corpus",
    "path",
    "section",
    "source_sha256",
    "scan_status",
    "review_status",
    "note",
]


@dataclass(frozen=True)
class AuditRecord:
    corpus: str
    path: str
    section: str
    digest: str
    profile_schema: str
    scan_status: str
    review_status: str
    note: str


@dataclass(frozen=True)
class LiteralReviewCell:
    table_line: int
    table_column: int
    table_occurrence: int
    row: int
    column: int


@dataclass(frozen=True)
class LiteralDisplaySourceReview(SourceReview):
    occurrence: int
    cell: LiteralReviewCell | None


@dataclass
class Finding:
    path: str
    status: str
    violations: list[str]
    detail: str | None = None
    expected: dict[str, int] | None = None
    observed: dict[str, int] | None = None
    source_review: list[SourceReview] = field(default_factory=list)


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="compare libmandoc AST topology with ManT IR over local roff inputs"
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument(
        "--fixtures",
        action="store_true",
        help="scan the checked-in real roff fixture catalogue (default)",
    )
    source.add_argument(
        "--manpath",
        action="append",
        type=Path,
        metavar="DIR",
        help="scan one or more local manual roots instead of checked-in fixtures",
    )
    sampling = parser.add_mutually_exclusive_group()
    sampling.add_argument(
        "--max-pages",
        type=non_negative_integer,
        default=0,
        help="stable path-ordered sample size; zero scans every selected page",
    )
    sampling.add_argument(
        "--max-pages-per-section",
        type=positive_integer,
        default=0,
        help="stable path-ordered sample size for each exact manual suffix",
    )
    parser.add_argument(
        "--man-section",
        action="append",
        metavar="SECTION",
        help="scan only an exact manual section; may be repeated",
    )
    parser.add_argument(
        "--source-pattern",
        action="append",
        metavar="REGEX",
        help="scan only sources matching every multiline regular expression",
    )
    parser.add_argument(
        "--profiler",
        type=Path,
        default=DEFAULT_PROFILER,
        metavar="FILE",
        help=(
            "batch AST-to-IR profiler built from the mant-engine example "
            f"(default: {DEFAULT_PROFILER.relative_to(ROOT)})"
        ),
    )
    parser.add_argument(
        "--timeout",
        type=positive_integer,
        default=600,
        help="seconds allowed for the complete profiler batch (default: 600)",
    )
    parser.add_argument(
        "--audit-db",
        type=Path,
        default=None,
        metavar="FILE",
        help=(
            "incremental structure-ledger CSV "
            "(fixture-local; external scans use MANT_ROFF_CORPUS or target/roff-audits)"
        ),
    )
    parser.add_argument(
        "--fidelity-db",
        type=Path,
        default=None,
        metavar="FILE",
        help=(
            "content-audit CSV used by --replay-fidelity-records "
            "(fixture-local; external scans use MANT_ROFF_CORPUS or target/roff-audits)"
        ),
    )
    parser.add_argument(
        "--corpus",
        metavar="NAME",
        help="stable corpus name (default: fixtures or local-manpath)",
    )
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument(
        "--recorded-only",
        action="store_true",
        help="scan only unchanged rows already present in the structure ledger",
    )
    selection.add_argument(
        "--recheck-recorded",
        action="store_true",
        help="scan every selected page, including completed structure-ledger rows",
    )
    selection.add_argument(
        "--replay-fidelity-records",
        action="store_true",
        help=(
            "scan only unchanged inputs previously recorded for this corpus in "
            "the content-fidelity ledger"
        ),
    )
    parser.add_argument(
        "--findings-only",
        action="store_true",
        help="print only REVIEW and HARD pages while retaining the summary",
    )
    parser.add_argument("--json", type=Path, metavar="FILE", help="write the complete JSON report")
    parser.add_argument("--self-check", action="store_true", help=argparse.SUPPRESS)
    arguments = parser.parse_args(argv)
    arguments.audit_db = audit_ledger_path(
        arguments.audit_db, DEFAULT_AUDIT_DB, external=bool(arguments.manpath),
    )
    arguments.fidelity_db = audit_ledger_path(
        arguments.fidelity_db, DEFAULT_FIDELITY_DB, external=bool(arguments.manpath),
    )
    return arguments


def non_negative_integer(value: str) -> int:
    parsed = int(value)
    if parsed < 0:
        raise argparse.ArgumentTypeError("must be zero or greater")
    return parsed


def positive_integer(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be one or greater")
    return parsed


def manual_hierarchy_root(path: Path, roots: Sequence[Path]) -> Path | None:
    """Return the narrow hierarchy owning one exact leaf and its `.so` targets."""
    section = manual_section(path)
    if section is None:
        return None
    for root in roots:
        try:
            relative = path.relative_to(root)
        except ValueError:
            continue
        candidates = [
            index
            for index, part in enumerate(relative.parts[:-1])
            if part.startswith("man") and part[3:] and section.startswith(part[3:])
        ]
        return root.joinpath(*relative.parts[: candidates[-1]]) if candidates else root
    return None


def stable_sample(pages: Sequence[Path], maximum: int) -> list[Path]:
    return list(pages) if maximum == 0 else list(pages[:maximum])


def stable_sample_by_section(pages: Sequence[Path], maximum: int) -> list[Path]:
    if maximum == 0:
        return list(pages)
    selected = []
    seen: Counter[str] = Counter()
    for path in pages:
        section = manual_section(path) or ""
        if seen[section] < maximum:
            selected.append(path)
            seen[section] += 1
    return selected


def read_structure_database(path: Path) -> dict[tuple[str, str, str], AuditRecord]:
    if not path.exists():
        return {}
    entries: dict[tuple[str, str, str], AuditRecord] = {}
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != STRUCTURE_DATABASE_FIELDS:
            raise ValueError(
                f"invalid structure database header in {path}; expected "
                f"{','.join(STRUCTURE_DATABASE_FIELDS)}"
            )
        for number, row in enumerate(reader, 2):
            status = row["scan_status"]
            review_status = row["review_status"]
            digest = row["source_sha256"]
            if status not in {"clean", "review", "hard-failure"}:
                raise ValueError(f"invalid scan status at {path}:{number}: {status}")
            if review_status not in {
                "not-required",
                "pending",
                "false-positive",
                "confirmed-open",
                "confirmed-fixed",
            }:
                raise ValueError(
                    f"invalid review status at {path}:{number}: {review_status}"
                )
            if not PROFILE_SCHEMA_PATTERN.fullmatch(row["profile_schema"]):
                raise ValueError(
                    f"invalid profile schema at {path}:{number}: {row['profile_schema']}"
                )
            if not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise ValueError(f"invalid source digest at {path}:{number}")
            entry = AuditRecord(
                corpus=row["corpus"],
                path=row["path"],
                section=row["section"],
                digest=digest,
                profile_schema=row["profile_schema"],
                scan_status=status,
                review_status=review_status,
                note=row["note"],
            )
            entries[(entry.corpus, entry.path, entry.digest)] = entry
    return entries


def read_fidelity_identities(path: Path, corpus: str) -> set[tuple[str, str]]:
    if not path.exists():
        raise ValueError(f"fidelity database does not exist: {path}")
    identities = set()
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != FIDELITY_DATABASE_FIELDS:
            raise ValueError(
                f"invalid fidelity database header in {path}; expected "
                f"{','.join(FIDELITY_DATABASE_FIELDS)}"
            )
        for row in reader:
            if row["corpus"] == corpus:
                identities.add((row["path"], row["source_sha256"]))
    return identities


def write_structure_database(path: Path, entries: Iterable[AuditRecord]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    with temporary.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.DictWriter(
            destination, fieldnames=STRUCTURE_DATABASE_FIELDS, lineterminator="\n"
        )
        writer.writeheader()
        for entry in sorted(entries, key=lambda item: (item.corpus, item.path, item.digest)):
            writer.writerow(
                {
                    "corpus": entry.corpus,
                    "path": entry.path,
                    "section": entry.section,
                    "source_sha256": entry.digest,
                    "profile_schema": entry.profile_schema,
                    "scan_status": entry.scan_status,
                    "review_status": entry.review_status,
                    "note": entry.note,
                }
            )
    temporary.replace(path)


RetryIdentity = tuple[str, str, str | None, str]


def read_retry_order(path: Path) -> dict[RetryIdentity, None]:
    if not path.exists():
        return {}
    document = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(document, dict) or not isinstance(document.get("attempted"), list):
        raise ValueError(f"invalid retry state in {path}")
    if document.get("schema") != PROFILE_SCHEMA:
        return {}
    attempts = {}
    for item in document["attempted"]:
        if (not isinstance(item, list) or len(item) != 4
                or not isinstance(item[3], str)
                or not all(isinstance(value, str) and value for value in item[:2])
                or (item[2] is not None and (not isinstance(item[2], str)
                                            or not re.fullmatch(r"[a-f0-9]{64}", item[2])))):
            raise ValueError(f"invalid retry identity in {path}")
        attempts[tuple(item)] = None
    return attempts


def write_retry_order(path: Path, attempts: dict[RetryIdentity, None]) -> None:
    if not attempts:
        path.unlink(missing_ok=True)
        return
    temporary = path.with_name(f".{path.name}.tmp")
    try:
        temporary.write_text(json.dumps({"schema": PROFILE_SCHEMA, "attempted": list(attempts)}) + "\n",
                             encoding="utf-8")
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def merge_review_status(previous: AuditRecord | None, status: str) -> str:
    if previous is None:
        return "pending" if status in {"review", "hard-failure"} else "not-required"
    if previous.review_status == "not-required" and status in {"review", "hard-failure"}:
        return "pending"
    if previous.review_status == "pending" and status == "clean":
        return "not-required"
    return previous.review_status


def run_profile_batch(
    profiler: Path, requests: dict[str, dict[str, str]], timeout: int
) -> dict[str, dict[str, object]]:
    return run_jsonl_profile_batch(profiler, requests, timeout, "structure")


def profile_findings(
    pages: Sequence[Path], roots: Sequence[Path], timeout: int, profiler: Path,
    *, exact_roots=None, batch_runner=None,
) -> list[Finding]:
    requests = {}
    labels = {}
    for path in pages:
        hierarchy_root = exact_roots[path] if exact_roots is not None else manual_hierarchy_root(path, roots)
        label = relative_label(path, roots)
        try:
            with path.open("rb") as stream:
                transport_error = manual_transport_error(path, manual_compression(stream.read(6)))
        except OSError:
            # The profiler owns file errors, including its include root.
            transport_error = None
        if transport_error:
            yield Finding(label, "hard-failure", [], transport_error)
            continue
        if hierarchy_root is None:
            yield Finding(label, "hard-failure", [], "manual hierarchy is unknown")
            continue
        request_id = hashlib.sha256(str(path).encode("utf-8")).hexdigest()
        requests[request_id] = {"id": request_id, "path": str(path), "root": str(hierarchy_root)}
        labels[request_id] = label
    for offset in range(0, len(requests), 256):
        batch = dict(list(requests.items())[offset : offset + 256])
        for request_id, response in (batch_runner or run_profile_batch)(profiler, batch, timeout).items():
            label = labels[request_id]
            if not isinstance(response.get("schema"), str) or response.get("schema") != PROFILE_SCHEMA:
                yield Finding(label, "hard-failure", [], "profiler returned an unsupported schema")
            elif isinstance(response.get("error"), str):
                yield Finding(label, "hard-failure", [], str(response["error"]))
            else:
                violations = response.get("violations")
                expected = response.get("expected")
                observed = response.get("observed")
                topology = response.get("topology")
                valid_topology = valid_structure_topology(topology)
                source_review = structure_source_reviews(response)
                if (
                    not isinstance(violations, list)
                    or not all(isinstance(item, str) for item in violations)
                    or not valid_structure_counts(expected)
                    or not valid_structure_counts(observed)
                    or not valid_topology
                    or source_review is None
                ):
                    yield Finding(label, "hard-failure", [], "profiler returned invalid structure observations")
                else:
                    if response.get("alias") is not True:
                        source_review.extend(
                            SourceReview(eq["sourceLine"], 0, "equation has no proved table cell owner")
                            for eq in topology["expected"]["equations"]
                            if eq["unprovedCell"]
                        )
                        source_review.extend(
                            SourceReview(eq["sourceLine"], 0, "equation has no proved paragraph owner")
                            for eq in topology["expected"]["equations"]
                            if eq.get("unprovedParagraph")
                        )
                    yield Finding(
                        label,
                        "review" if violations or source_review else "clean",
                        violations,
                        expected=expected,
                        observed=observed,
                        source_review=source_review,
                    )


def structure_source_reviews(response: dict[str, object]) -> list[SourceReview] | None:
    if response.get("alias") is True:
        # The followed target has its own source identity and review row.
        return []
    origins = response.get("sourceLinkOrigins")
    if not isinstance(origins, dict):
        return None
    section_reviews = source_reviews(
        origins.get("sectionLabelsForReview"),
        "section label depends on recovery of an invalid roff escape",
    )
    ordinal_reviews = ordinal_source_reviews(response)
    display_reviews = literal_source_reviews(response.get("literalDisplaysForReview"))
    head_reviews = definition_head_reviews(response.get("definitionHeadsForReview"))
    if any(reviews is None for reviews in (section_reviews, ordinal_reviews, display_reviews, head_reviews)):
        return None
    return [*section_reviews, *ordinal_reviews, *display_reviews, *head_reviews]


def definition_head_reviews(records: object) -> list[SourceReview] | None:
    if not isinstance(records, list):
        return None
    descriptions = {
        "ambiguous-owner": "definition term source coordinates do not identify a unique owner",
        "partial-payload": "definition term text is only partially proved",
    }
    reviews = []
    for record in records:
        if (not isinstance(record, dict)
                or not all(is_nonnegative_integer(record.get(key)) for key in ("line", "column"))
                or not isinstance(record.get("reason"), str)
                or record["reason"] not in descriptions):
            return None
        reviews.append(SourceReview(record["line"], record["column"], descriptions[record["reason"]]))
    return reviews


def literal_source_reviews(records: object) -> list[LiteralDisplaySourceReview] | None:
    if not isinstance(records, list):
        return None
    reviews = []
    identities = set()
    for record in records:
        if (not isinstance(record, dict)
                or not all(is_nonnegative_integer(record.get(key))
                           for key in ("line", "column", "occurrence"))
                or record["occurrence"] == 0 or "cell" not in record):
            return None
        identity = (record["line"], record["column"], record["occurrence"])
        if identity in identities:
            return None
        identities.add(identity)
        cell = record["cell"]
        if cell is not None:
            keys = ("tableLine", "tableColumn", "tableOccurrence", "row", "column")
            if (not isinstance(cell, dict)
                    or not all(is_nonnegative_integer(cell.get(key)) for key in keys)
                    or cell["tableOccurrence"] == 0):
                return None
            cell = LiteralReviewCell(*(cell[key] for key in keys))
        reviews.append(LiteralDisplaySourceReview(
            record["line"], record["column"], "display has unproved native glyph placement",
            record["occurrence"], cell,
        ))
    return reviews


def source_review_label(review: SourceReview) -> str:
    location = f"source {review.line}:{review.column}"
    if isinstance(review, LiteralDisplaySourceReview):
        location += f" occurrence {review.occurrence}"
        if (cell := review.cell) is not None:
            location += (f" in table {cell.table_line}:{cell.table_column}"
                         f" occurrence {cell.table_occurrence}, row {cell.row}, column {cell.column}")
    return f"{location}: {review.reason}"


def valid_structure_counts(value: object) -> bool:
    return isinstance(value, dict) and all(
        isinstance(key, str) and is_nonnegative_integer(count)
        for key, count in value.items()
    )


def valid_structure_topology(value: object) -> bool:
    if not isinstance(value, dict):
        return False
    for side in ("expected", "observed"):
        topology = value.get(side)
        if not isinstance(topology, dict):
            return False
        for field, validate in (("lists", valid_list_topology),
                                ("tableRows", lambda row: valid_table_topology(row, side)),
                                ("equations", lambda equation: valid_equation_topology(equation, side))):
            records = topology.get(field)
            if not isinstance(records, list) or not all(validate(record) for record in records):
                return False
        address = ("tableSourceLine", "tableSourceColumn", "tableOccurrence", "rowIndex")
        rows = {
            tuple(row[key] for key in address): row
            for row in topology["tableRows"]
        }
        if len(rows) != len(topology["tableRows"]):
            return False
        for equation in topology["equations"]:
            if (cell := equation["cell"]) is not None:
                row = rows.get(tuple(cell[key] for key in address))
                index = cell["cellIndex"]
                if row is None or row["kind"] != "data" or index >= len(row["cells"]):
                    return False
                excluded = "empty" if side == "observed" else "verticalContinuation"
                if row["cells"][index][excluded]:
                    return False
    return True


def valid_list_topology(value: object) -> bool:
    return (
        isinstance(value, dict)
        and is_nonnegative_integer(value.get("sourceLine"))
        and is_nonnegative_integer(value.get("items"))
        and isinstance(value.get("kind"), str)
        and value["kind"] in {"generic", "definition"}
    )


def valid_equation_topology(value: object, side: str) -> bool:
    if not (
        isinstance(value, dict)
        and is_nonnegative_integer(value.get("sourceLine"))
        and isinstance(value.get("context"), str)
        and value["context"] in {"display", "inline", "table-cell"}
        and isinstance(value.get("value"), str)
        and "cell" in value
        and (value["cell"] is None or valid_equation_cell(value["cell"]))
    ):
        return False
    cell = value["cell"]
    if "unprovedParagraph" in value and (
        side != "expected" or not isinstance(value["unprovedParagraph"], bool)
        or value["context"] != "inline"
    ):
        return False
    if side == "expected" and not isinstance(value.get("unprovedCell"), bool):
        return False
    unknown = side == "expected" and value["unprovedCell"]
    if unknown:
        return cell is None and value["context"] in {"table-cell", "display"}
    if value["context"] == "display":
        return True
    return (value["context"] == "table-cell") == (cell is not None)


def valid_equation_cell(value: object) -> bool:
    return (
        isinstance(value, dict)
        and all(is_nonnegative_integer(value.get(field)) for field in (
            "tableSourceLine", "tableSourceColumn", "tableOccurrence", "rowIndex", "cellIndex",
        ))
        and value["tableOccurrence"] > 0
    )


def valid_table_topology(value: object, side: str) -> bool:
    if not isinstance(value, dict) or not all(
        is_nonnegative_integer(value.get(field))
        for field in ("tableSourceLine", "tableSourceColumn", "tableOccurrence", "rowIndex")
    ):
        return False
    if value["tableOccurrence"] == 0:
        return False
    kind = value.get("kind")
    if isinstance(kind, dict) and list(kind) == ["layout-rule"]:
        layout = kind["layout-rule"]
        valid_kind = (isinstance(layout, dict) and isinstance(layout.get("cells"), list)
                      and all(isinstance(cell, str) and cell in {"horizontal", "double-horizontal"}
                              for cell in layout["cells"]))
    else:
        valid_kind = isinstance(kind, str) and kind in {"data", "horizontal-rule", "double-horizontal-rule"}
    cells = value.get("cells")
    return (valid_kind and isinstance(cells, list)
            and all(valid_table_cell(cell, side) for cell in cells))


def valid_table_cell(value: object, side: str) -> bool:
    return (
        isinstance(value, dict)
        and all(is_nonnegative_integer(value.get(field)) and value[field] > 0
                for field in ("columnSpan", "rowSpan"))
        and isinstance(value.get("verticalContinuation" if side == "expected" else "empty"), bool)
    )


def self_check() -> None:
    assert manual_section(Path("git.1.gz")) == "1"
    assert manual_section(Path("SSL_read.3ssl.zst")) == "3ssl"
    assert manual_hierarchy_root(
        Path("/usr/share/man/fr/man3/printf.3.gz"), [Path("/usr/share/man")]
    ) == Path("/usr/share/man/fr")
    assert merge_review_status(None, "clean") == "not-required"
    assert merge_review_status(None, "review") == "pending"


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff structure audit self-check succeeded")
        return 0
    try:
        if not arguments.profiler.is_file():
            raise ValueError(
                f"structure profiler not found: {arguments.profiler}; run "
                "`cargo build -p mant-engine --example roff_structure_profile`"
            )
        roots = [path.resolve() for path in arguments.manpath] if arguments.manpath else [FIXTURE_ROOT]
        corpus = arguments.corpus or ("fixtures" if not arguments.manpath else "local-manpath")
        complete = not any((arguments.man_section, arguments.source_pattern, arguments.max_pages,
                            arguments.max_pages_per_section, arguments.replay_fidelity_records,
                            arguments.recorded_only))
        all_pages = discover_pages(roots, strict=complete)
        root_scope = json.dumps([str(root) for root in roots])
        database = read_structure_database(arguments.audit_db)
        retry_path = arguments.audit_db.with_suffix(arguments.audit_db.suffix + ".retries.json")
        retries = read_retry_order(retry_path)
        if complete:
            discovered = {relative_label(page, roots) for page in all_pages}
            retries = {key: None for key in retries
                       if key[0] != corpus or key[3] != root_scope or key[1] in discovered}
        if arguments.man_section:
            sections = set(arguments.man_section)
            all_pages = [page for page in all_pages if manual_section(page) in sections]
        patterns = compile_source_patterns(arguments.source_pattern)
        records, unreadable = observe_sources(all_pages, roots, patterns)
        recovered = {
            label for label, digest in records.values()
            if digest is not None
            and (previous := database.get((corpus, label, digest))) is not None
            and previous.profile_schema == PROFILE_SCHEMA and previous.scan_status != "hard-failure"
        }
        retries = {key: None for key in retries
                   if key[0] != corpus or key[3] != root_scope or key[1] not in recovered}
        pages = list(records)
        if arguments.replay_fidelity_records:
            identities = read_fidelity_identities(arguments.fidelity_db, corpus)
            pages = [
                page
                for page in pages
                if records[page][1] is not None and (records[page][0], records[page][1]) in identities
            ]
        elif arguments.recorded_only:
            pages = [
                page
                for page in pages
                if records[page][1] is not None
                and (corpus, records[page][0], records[page][1]) in database
            ]
        elif not arguments.recheck_recorded:
            pages = [
                page
                for page in pages
                if (previous := database.get((corpus, *records[page]))) is None
                or previous.profile_schema != PROFILE_SCHEMA
                or previous.scan_status == "hard-failure"
            ]
            if arguments.max_pages or arguments.max_pages_per_section:
                # New identities first, then old failures in attempt order.
                # An unreadable file participates in scheduling without ever
                # being stamped with a decoded source identity in the ledger.
                order = {key: number for number, key in enumerate(retries, start=2)}

                def priority(page: Path) -> int:
                    key = (corpus, *records[page])
                    previous = database.get(key)
                    legacy_retry = (previous is not None and previous.profile_schema == PROFILE_SCHEMA
                                    and previous.scan_status == "hard-failure")
                    return order.get((*key, root_scope), int(legacy_retry))

                pages.sort(key=priority)
        pages = (
            stable_sample_by_section(pages, arguments.max_pages_per_section)
            if arguments.max_pages_per_section
            else stable_sample(pages, arguments.max_pages)
        )
    except ValueError as error:
        print(f"audit-roff-structure: {error}", file=sys.stderr)
        return 2

    for path, reason in (unreadable.items() if patterns else ()):
        print(
            f"audit-roff-structure: source pattern could not inspect unreadable path: {path}: {reason}",
            file=sys.stderr,
        )
    if not pages:
        write_retry_order(retry_path, retries)
        print("audit-roff-structure: no selected manual pages")
        write_report(arguments.json, corpus, roots, [], unreadable)
        return 0

    print("ManT roff structure audit")
    print(f"  profiler:  {arguments.profiler}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    print(f"  corpus:    {corpus}")
    print("  contract:  AST-to-IR topology; terminal wrapping is intentionally ignored")
    print()

    try:
        readable = [page for page in pages if page not in unreadable]
        findings = list(profile_findings(readable, roots, arguments.timeout, arguments.profiler))
        findings.extend(
            Finding(records[page][0], "hard-failure", [], f"could not decode source: {unreadable[page]}")
            for page in pages if page in unreadable
        )
    except ValueError as error:
        print(f"audit-roff-structure: {error}", file=sys.stderr)
        return 2
    changed = {records[path][0] for path in readable if not verify_source_digest(path, records[path][1])}
    if changed:
        findings = [
            Finding(finding.path, "hard-failure", [], "source changed during audit")
            if finding.path in changed else finding for finding in findings
        ]
    summary = Counter(finding.status for finding in findings)
    source_review_count = sum(len(finding.source_review) for finding in findings)
    by_label = {finding.path: finding for finding in findings}
    selected_labels = {(corpus, records[path][0]) for path in pages}
    retries = {key: None for key in retries if key[3] != root_scope or key[:2] not in selected_labels}
    for path in pages:
        label, digest = records[path]
        finding = by_label[label]
        if finding.status == "hard-failure" or digest is None:
            retries[(corpus, label, digest, root_scope)] = None
        if not arguments.findings_only or finding.status != "clean":
            detail = f" — {finding.detail}" if finding.detail else ""
            observations = "; ".join([
                *finding.violations,
                *(source_review_label(review) for review in finding.source_review),
            ])
            print(f"{finding.status.upper():12} {label}{detail}{': ' + observations if observations else ''}")
        if digest is not None and label not in changed:
            key = (corpus, label, digest)
            previous = database.get(key)
            database[key] = AuditRecord(
                corpus=corpus,
                path=label,
                section=manual_section(path) or "",
                digest=digest,
                profile_schema=PROFILE_SCHEMA,
                scan_status=finding.status,
                review_status=merge_review_status(previous, finding.status),
                note=previous.note if previous is not None else "",
            )
    print()
    print(
        "summary: "
        f"examined={len(findings)}, clean={summary['clean']}, review={summary['review']}, "
        f"hard={summary['hard-failure']}, source_review={source_review_count}"
    )
    print("REVIEW is an AST-to-IR candidate, not a regression until source and IR are inspected.")
    write_structure_database(arguments.audit_db, database.values())
    write_retry_order(retry_path, retries)
    print(f"structure database: {arguments.audit_db}")
    write_report(arguments.json, corpus, roots, findings, unreadable)
    return 1 if summary["hard-failure"] else 0


def write_report(path: Path | None, corpus, roots, findings, unreadable) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    report = {
        "schema": PROFILE_SCHEMA,
        "corpus": corpus,
        "roots": [str(root) for root in roots],
        "summary": dict(Counter(finding.status for finding in findings)),
        "source_review_occurrences": sum(len(finding.source_review) for finding in findings),
        "uncovered_sources": uncovered_sources(unreadable, roots),
        "findings": [asdict(finding) for finding in findings],
    }
    path.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"report: {path}")


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
