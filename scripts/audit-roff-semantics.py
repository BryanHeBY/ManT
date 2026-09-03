#!/usr/bin/env python3
"""Audit semantic-entry precision after native roff lowering."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import subprocess
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Iterable, Sequence

from roff_audit_common import (
    compile_source_patterns,
    discover_pages,
    filter_pages_by_source,
    manual_hierarchy_root,
    manual_section,
    non_negative_integer,
    relative_label,
    run_jsonl_profile_batch,
    source_digest,
    stable_sample,
)


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_PROFILER = ROOT / "target/debug/examples/roff_semantic_profile"
DEFAULT_AUDIT_DB = ROOT / "tests/fixtures/roff/SEMANTIC_AUDIT.csv"
PROFILE_SCHEMA = "mant.roff-semantic-profile/v1"
DATABASE_FIELDS = [
    "corpus",
    "path",
    "section",
    "source_sha256",
    "profile_schema",
    "scan_status",
    "review_status",
    "note",
]
SOURCE_DIGEST = re.compile(r"[0-9a-f]{64}")
REVIEW_STATUSES = {
    "not-required",
    "pending",
    "false-positive",
    "confirmed-open",
    "confirmed-fixed",
}
ENTRY_KINDS = {
    "command",
    "option",
    "marker",
    "operand",
    "configuration-key",
    "environment-variable",
    "variable",
    "value",
    "term",
}


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


@dataclass
class Finding:
    path: str
    status: str
    violations: list[str]
    detail: str | None = None
    entry_count: int = 0
    entry_counts: dict[str, int] | None = None
    ordinal_entries: list[dict[str, object]] | None = None
    ordinal_definitions: list[dict[str, object]] | None = None
    empty_entries: list[dict[str, object]] | None = None
    aliasless_generic_term_count: int = 0
    note_like_entry_count: int = 0
    value_domain_violations: list[str] | None = None


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="audit semantic-entry precision after roff lowering"
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument(
        "--fixtures", action="store_true", help="scan checked-in real fixtures (default)"
    )
    source.add_argument(
        "--manpath", action="append", type=Path, metavar="DIR", help="scan local manual roots"
    )
    parser.add_argument("--source-pattern", action="append", metavar="REGEX")
    parser.add_argument("--max-pages", type=non_negative_integer, default=0)
    parser.add_argument("--corpus")
    parser.add_argument("--profiler", type=Path, default=DEFAULT_PROFILER)
    parser.add_argument("--timeout", type=int, default=600)
    parser.add_argument("--audit-db", type=Path, default=DEFAULT_AUDIT_DB)
    parser.add_argument("--recheck-recorded", action="store_true")
    parser.add_argument("--recorded-only", action="store_true")
    parser.add_argument("--findings-only", action="store_true")
    parser.add_argument("--verify", action="store_true")
    parser.add_argument("--json", type=Path, metavar="FILE")
    parser.add_argument("--self-check", action="store_true", help=argparse.SUPPRESS)
    return parser.parse_args(argv)


def read_database(path: Path) -> dict[tuple[str, str, str], AuditRecord]:
    if not path.exists():
        return {}
    records = {}
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != DATABASE_FIELDS:
            raise ValueError(f"invalid semantic audit header in {path}")
        for number, row in enumerate(reader, 2):
            if SOURCE_DIGEST.fullmatch(row["source_sha256"]) is None:
                raise ValueError(f"invalid source digest at {path}:{number}")
            if row["profile_schema"] != PROFILE_SCHEMA:
                raise ValueError(f"unsupported semantic profile schema at {path}:{number}")
            if row["scan_status"] not in {"clean", "review", "hard-failure"}:
                raise ValueError(f"invalid scan status at {path}:{number}")
            if row["review_status"] not in REVIEW_STATUSES:
                raise ValueError(f"invalid review status at {path}:{number}")
            key = (row["corpus"], row["path"], row["source_sha256"])
            if key in records:
                raise ValueError(f"duplicate semantic audit identity at {path}:{number}")
            records[key] = AuditRecord(
                corpus=row["corpus"],
                path=row["path"],
                section=row["section"],
                digest=row["source_sha256"],
                profile_schema=row["profile_schema"],
                scan_status=row["scan_status"],
                review_status=row["review_status"],
                note=row["note"],
            )
    return records


def write_database(path: Path, records: Iterable[AuditRecord]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8", newline="") as output:
        writer = csv.DictWriter(output, fieldnames=DATABASE_FIELDS, lineterminator="\n")
        writer.writeheader()
        for record in sorted(records, key=lambda item: (item.corpus, item.path, item.digest)):
            writer.writerow(
                {
                    "corpus": record.corpus,
                    "path": record.path,
                    "section": record.section,
                    "source_sha256": record.digest,
                    "profile_schema": record.profile_schema,
                    "scan_status": record.scan_status,
                    "review_status": record.review_status,
                    "note": record.note,
                }
            )


def merge_review_status(previous: AuditRecord | None, status: str) -> str:
    if previous is None:
        return "not-required" if status == "clean" else "pending"
    if previous.review_status == "not-required" and status != "clean":
        return "pending"
    if previous.review_status == "pending" and status == "clean":
        return "not-required"
    return previous.review_status


def valid_entry(value: object) -> bool:
    if not isinstance(value, dict):
        return False
    return (
        isinstance(value.get("id"), str)
        and value.get("kind") in ENTRY_KINDS
        and all(
            isinstance(value.get(field), list)
            and all(isinstance(item, str) for item in value[field])
            for field in ("aliases", "forms", "targets")
        )
        and (value.get("containingSection") is None or isinstance(value.get("containingSection"), str))
        and (
            value.get("containingSectionTitle") is None
            or isinstance(value.get("containingSectionTitle"), str)
        )
        and isinstance(value.get("containingSectionSourceLine"), int)
        and value["containingSectionSourceLine"] >= 0
        and isinstance(value.get("nestedDepth"), int)
        and value["nestedDepth"] >= 0
        and (
            value.get("valueDomainOrigin") is None
            or value.get("valueDomainOrigin")
            in {"child-choices", "external-entry-set", "union"}
        )
    )


def valid_definition_candidate(value: object) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("form"), str)
        and (value.get("identity") is None or isinstance(value.get("identity"), str))
        and (value.get("role") is None or value.get("role") in ENTRY_KINDS)
        and (
            value.get("containingSection") is None
            or isinstance(value.get("containingSection"), str)
        )
        and (
            value.get("containingSectionTitle") is None
            or isinstance(value.get("containingSectionTitle"), str)
        )
        and isinstance(value.get("containingSectionSourceLine"), int)
        and isinstance(value.get("irPath"), str)
    )


def profile_findings(
    pages: Sequence[Path], roots: Sequence[Path], profiler: Path, timeout: int
) -> Iterable[Finding]:
    requests = {}
    labels = {}
    for path in pages:
        label = relative_label(path, roots)
        hierarchy_root = manual_hierarchy_root(path, roots)
        if hierarchy_root is None:
            yield Finding(label, "hard-failure", [], "manual hierarchy is unknown")
            continue
        request_id = hashlib.sha256(str(path).encode()).hexdigest()
        requests[request_id] = {
            "id": request_id,
            "path": str(path),
            "root": str(hierarchy_root),
        }
        labels[request_id] = label
    items = list(requests.items())
    for offset in range(0, len(items), 256):
        responses = run_jsonl_profile_batch(
            profiler, dict(items[offset : offset + 256]), timeout, "semantic"
        )
        for request_id, response in responses.items():
            label = labels[request_id]
            if response.get("schema") != PROFILE_SCHEMA:
                yield Finding(label, "hard-failure", [], "unsupported profiler schema")
                continue
            if isinstance(response.get("error"), str):
                yield Finding(label, "hard-failure", [], str(response["error"]))
                continue
            entries = response.get("entries")
            entry_counts = response.get("entryCounts")
            ordinal_entries = response.get("ordinalEntries")
            ordinal_definitions = response.get("ordinalDefinitions")
            empty_entries = response.get("emptyEntries")
            aliasless_count = response.get("aliaslessGenericTermCount")
            aliasless_samples = response.get("aliaslessGenericTermSamples")
            note_count = response.get("noteLikeEntryCount")
            note_samples = response.get("noteLikeEntrySamples")
            value_domain_violations = response.get("valueDomainViolations")
            violations = response.get("violations")
            valid = (
                isinstance(entries, list)
                and all(valid_entry(entry) for entry in entries)
                and isinstance(entry_counts, dict)
                and all(kind in ENTRY_KINDS and isinstance(count, int) and count >= 0 for kind, count in entry_counts.items())
                and sum(entry_counts.values()) == len(entries)
                and isinstance(ordinal_entries, list)
                and all(valid_entry(entry) for entry in ordinal_entries)
                and isinstance(ordinal_definitions, list)
                and all(valid_definition_candidate(item) for item in ordinal_definitions)
                and isinstance(empty_entries, list)
                and all(valid_entry(entry) for entry in empty_entries)
                and isinstance(aliasless_count, int)
                and aliasless_count >= 0
                and isinstance(aliasless_samples, list)
                and all(valid_entry(entry) for entry in aliasless_samples)
                and len(aliasless_samples) <= min(aliasless_count, 32)
                and isinstance(note_count, int)
                and note_count >= 0
                and isinstance(note_samples, list)
                and all(valid_entry(entry) for entry in note_samples)
                and len(note_samples) <= min(note_count, 32)
                and isinstance(value_domain_violations, list)
                and all(isinstance(item, str) for item in value_domain_violations)
                and isinstance(violations, list)
                and all(isinstance(item, str) for item in violations)
                and bool(violations)
                == bool(
                    ordinal_entries
                    or ordinal_definitions
                    or empty_entries
                    or value_domain_violations
                )
            )
            if not valid:
                yield Finding(label, "hard-failure", [], "invalid profiler response")
                continue
            yield Finding(
                label,
                "review" if violations else "clean",
                violations,
                entry_count=len(entries),
                entry_counts=entry_counts,
                ordinal_entries=ordinal_entries,
                ordinal_definitions=ordinal_definitions,
                empty_entries=empty_entries,
                aliasless_generic_term_count=aliasless_count,
                note_like_entry_count=note_count,
                value_domain_violations=value_domain_violations,
            )


def repository_commit() -> str | None:
    result = subprocess.run(
        ["git", "-C", str(ROOT), "rev-parse", "HEAD"],
        check=False,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip() if result.returncode == 0 else None


def file_digest(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def self_check() -> None:
    assert manual_section(Path("git.1.gz")) == "1"
    assert merge_review_status(None, "clean") == "not-required"
    assert merge_review_status(None, "review") == "pending"
    assert valid_entry(
        {
            "id": "option-help",
            "kind": "option",
            "aliases": ["--help"],
            "forms": ["--help"],
            "targets": ["option-help"],
            "containingSection": "options",
            "containingSectionTitle": "OPTIONS",
            "containingSectionSourceLine": 10,
            "nestedDepth": 0,
            "valueDomainOrigin": None,
        }
    )
    assert valid_definition_candidate(
        {
            "form": "1.",
            "identity": "term-1",
            "role": "term",
            "containingSection": "notes",
            "containingSectionTitle": "NOTES",
            "containingSectionSourceLine": 20,
            "irPath": "section[0]/block[0]/definition[0]",
        }
    )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff semantic audit self-check succeeded")
        return 0
    try:
        if arguments.timeout < 1:
            raise ValueError("--timeout must be positive")
        if not arguments.profiler.is_file():
            raise ValueError(
                "semantic profiler not found; run `cargo build -p mant-engine "
                "--example roff_semantic_profile`"
            )
        roots = (
            [path.resolve() for path in arguments.manpath]
            if arguments.manpath
            else [FIXTURE_ROOT]
        )
        corpus = arguments.corpus or ("local-manpath" if arguments.manpath else "fixtures")
        pages = discover_pages(roots)
        pages, unreadable = filter_pages_by_source(
            pages, compile_source_patterns(arguments.source_pattern)
        )
        records = {page: (relative_label(page, roots), source_digest(page)) for page in pages}
        database = read_database(arguments.audit_db)
        if arguments.recorded_only:
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
                if records[page][1] is None
                or (corpus, records[page][0], records[page][1]) not in database
            ]
        pages = stable_sample(pages, arguments.max_pages)
        if arguments.verify and not pages:
            raise ValueError("verification selected no pages")
    except ValueError as error:
        print(f"audit-roff-semantics: {error}", file=sys.stderr)
        return 2

    for path in unreadable:
        print(f"audit-roff-semantics: unreadable source: {path}", file=sys.stderr)
    print("ManT roff semantic-entry precision audit")
    print(f"  pages:  {len(pages)}")
    print(f"  corpus: {corpus}")
    print("  contract: ordinal structure never leaks into semantic definitions or entries")
    print()

    findings = list(profile_findings(pages, roots, arguments.profiler, arguments.timeout))
    by_label = {finding.path: finding for finding in findings}
    summary = Counter(finding.status for finding in findings)
    entry_counts = Counter()
    total_entries = 0
    aliasless_count = 0
    note_count = 0
    ordinal_entry_count = 0
    ordinal_definition_count = 0
    empty_entry_count = 0
    value_domain_violation_count = 0
    verification_failed = False
    for finding in findings:
        total_entries += finding.entry_count
        entry_counts.update(finding.entry_counts or {})
        aliasless_count += finding.aliasless_generic_term_count
        note_count += finding.note_like_entry_count
        ordinal_entry_count += len(finding.ordinal_entries or [])
        ordinal_definition_count += len(finding.ordinal_definitions or [])
        empty_entry_count += len(finding.empty_entries or [])
        value_domain_violation_count += len(finding.value_domain_violations or [])
    for page in pages:
        label, digest = records[page]
        finding = by_label[label]
        if not arguments.findings_only or finding.status != "clean":
            detail = f" — {finding.detail}" if finding.detail else ""
            suffix = f": {'; '.join(finding.violations)}" if finding.violations else ""
            print(f"{finding.status.upper():12} {label}{detail}{suffix}")
        if digest is None:
            continue
        key = (corpus, label, digest)
        previous = database.get(key)
        if arguments.verify:
            if previous is None or previous.scan_status != finding.status:
                verification_failed = True
            continue
        database[key] = AuditRecord(
            corpus=corpus,
            path=label,
            section=manual_section(page) or "",
            digest=digest,
            profile_schema=PROFILE_SCHEMA,
            scan_status=finding.status,
            review_status=merge_review_status(previous, finding.status),
            note=previous.note if previous is not None else "",
        )

    print()
    print(
        f"summary: examined={len(findings)}, clean={summary['clean']}, "
        f"review={summary['review']}, hard={summary['hard-failure']}"
    )
    print(
        f"semantic entries: total={total_entries}, ordinal={ordinal_entry_count}, "
        f"ordinal-definitions={ordinal_definition_count}, "
        f"empty={empty_entry_count}, "
        f"aliasless-generic={aliasless_count}, note-like={note_count}, "
        f"value-domain-violations={value_domain_violation_count}"
    )
    if entry_counts:
        print(
            "entry kinds: "
            + ", ".join(f"{kind}={count}" for kind, count in entry_counts.most_common())
        )
    if arguments.verify:
        print("semantic database verification: " + ("failed" if verification_failed else "passed"))
    else:
        write_database(arguments.audit_db, database.values())
        print(f"semantic database: {arguments.audit_db}")
    if arguments.json:
        arguments.json.parent.mkdir(parents=True, exist_ok=True)
        arguments.json.write_text(
            json.dumps(
                {
                    "schema": PROFILE_SCHEMA,
                    "producerCommit": repository_commit(),
                    "profileSha256": file_digest(arguments.profiler),
                    "scannedAt": datetime.now(timezone.utc).isoformat(),
                    "corpus": corpus,
                    "roots": [str(root) for root in roots],
                    "pageCount": len(findings),
                    "entryCount": total_entries,
                    "entryCounts": dict(entry_counts),
                    "summary": dict(summary),
                    "findings": [asdict(finding) for finding in findings],
                },
                ensure_ascii=False,
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
        print(f"report: {arguments.json}")
    return 1 if summary["hard-failure"] or verification_failed else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
