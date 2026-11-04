"""Persist source-bound review records and reuse completed source identities."""
from __future__ import annotations

import csv
import re
from pathlib import Path
from typing import Iterable, Sequence

from .model import AuditRecord
from .sources import source_audit_identity, source_is_context_independent


AUDIT_DATABASE_FIELDS = [
    "corpus",
    "path",
    "section",
    "source_sha256",
    "scan_status",
    "review_status",
    "note",
]
MANDOC_AUDIT_DATABASE_FIELDS = [
    "reference_kind",
    "reference_id",
    *AUDIT_DATABASE_FIELDS,
]


def read_source_identities(path: Path, corpus: str) -> set[tuple[str, str]]:
    """Read exact path/hash targets from the historical groff breadth ledger."""
    if not path.is_file():
        raise ValueError(f"source ledger does not exist: {path}")
    identities = set()
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != AUDIT_DATABASE_FIELDS:
            raise ValueError(
                f"invalid source ledger header in {path}; expected "
                f"{','.join(AUDIT_DATABASE_FIELDS)}"
            )
        for number, row in enumerate(reader, 2):
            digest = row["source_sha256"]
            if not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise ValueError(f"invalid source digest at {path}:{number}")
            if row["corpus"] == corpus:
                identities.add((row["path"], digest))
    if not identities:
        raise ValueError(f"source ledger {path} has no rows for corpus {corpus!r}")
    return identities


def reusable_cross_corpus_sources(
    pages: Sequence[Path],
    page_records: dict[Path, tuple[str, str | None]],
    database: dict[tuple[str, str, str], AuditRecord],
    corpus: str,
) -> dict[Path, AuditRecord]:
    reusable: dict[tuple[str, str, str], AuditRecord] = {}
    for record in database.values():
        if (
            record.corpus == corpus
            or record.scan_status not in {"clean", "review"}
            or record.review_status == "pending"
        ):
            continue
        identity = source_audit_identity(record.path, record.digest)
        if identity is not None:
            reusable.setdefault(identity, record)

    duplicates = {}
    for page in pages:
        label, digest = page_records[page]
        if digest is None:
            continue
        identity = source_audit_identity(label, digest)
        origin = reusable.get(identity) if identity is not None else None
        if origin is not None and source_is_context_independent(page):
            duplicates[page] = origin
    return duplicates


def read_audit_database(
    path: Path,
    reference_kind: str = "man",
    reference_id: str = "",
) -> dict[tuple[str, str, str], AuditRecord]:
    if not path.exists():
        return {}
    entries: dict[tuple[str, str, str], AuditRecord] = {}
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        fields = (
            MANDOC_AUDIT_DATABASE_FIELDS
            if reference_kind == "mandoc"
            else AUDIT_DATABASE_FIELDS
        )
        if reader.fieldnames != fields:
            raise ValueError(
                f"invalid audit database header in {path}; expected "
                f"{','.join(fields)}"
            )
        for number, row in enumerate(reader, 2):
            if reference_kind == "mandoc" and (
                row["reference_kind"] != reference_kind
                or row["reference_id"] != reference_id
            ):
                raise ValueError(
                    f"mandoc reference identity mismatch at {path}:{number}; "
                    f"expected {reference_kind}/{reference_id}"
                )
            status = row["scan_status"]
            review_status = row["review_status"]
            digest = row["source_sha256"]
            if status not in {"clean", "review", "hard-failure", "skipped"}:
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
            if not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise ValueError(f"invalid source digest at {path}:{number}")
            entry = AuditRecord(
                corpus=row["corpus"],
                path=row["path"],
                section=row["section"],
                digest=digest,
                scan_status=status,
                review_status=review_status,
                note=row["note"],
                reference_kind=reference_kind,
                reference_id=reference_id,
            )
            entries[(entry.corpus, entry.path, entry.digest)] = entry
    return entries


def write_audit_database(
    path: Path,
    entries: Iterable[AuditRecord],
    reference_kind: str = "man",
    reference_id: str = "",
) -> None:
    rows = sorted(entries, key=lambda entry: (entry.corpus, entry.path, entry.digest))
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    with temporary.open("w", encoding="utf-8", newline="") as destination:
        fields = (
            MANDOC_AUDIT_DATABASE_FIELDS
            if reference_kind == "mandoc"
            else AUDIT_DATABASE_FIELDS
        )
        writer = csv.DictWriter(
            destination,
            fieldnames=fields,
            lineterminator="\n",
        )
        writer.writeheader()
        for entry in rows:
            row = {
                "corpus": entry.corpus,
                "path": entry.path,
                "section": entry.section,
                "source_sha256": entry.digest,
                "scan_status": entry.scan_status,
                "review_status": entry.review_status,
                "note": entry.note,
            }
            if reference_kind == "mandoc":
                row = {
                    "reference_kind": reference_kind,
                    "reference_id": reference_id,
                    **row,
                }
            writer.writerow(row)
    temporary.replace(path)


def merged_review_status(
    previous: AuditRecord | None,
    scan_status: str,
) -> str:
    if previous is None:
        return (
            "pending"
            if scan_status in {"review", "hard-failure"}
            else "not-required"
        )
    if (
        previous.review_status == "not-required"
        and scan_status in {"review", "hard-failure"}
    ):
        return "pending"
    if previous.review_status == "pending" and scan_status == "clean":
        return "not-required"
    return previous.review_status
