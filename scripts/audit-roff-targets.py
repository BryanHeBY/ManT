#!/usr/bin/env python3
"""Audit zero-width libmandoc targets through ManT's AST-to-IR lowering.

The visible fidelity, structure, projection, and layout routes intentionally
cannot observe zero-width anchors. This independent route compares targets on
validated libmandoc owners with section, semantic-entry, and inline identities
in the final source-aware IR. Results are candidates until manually reviewed.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import re
import sys
from collections import Counter
from dataclasses import asdict, dataclass
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
DEFAULT_PROFILER = ROOT / "target/debug/examples/roff_target_profile"
DEFAULT_AUDIT_DB = ROOT / "tests/fixtures/roff/TARGET_AUDIT.csv"
PROFILE_SCHEMA = "mant.roff-target-profile/v1"
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
    expected: list[dict[str, object]] | None = None
    observed: list[str] | None = None
    missing: list[dict[str, object]] | None = None
    target_owners: dict[str, int] | None = None


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="compare validated libmandoc targets with ManT IR identities"
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument(
        "--fixtures", action="store_true", help="scan checked-in real fixtures (default)"
    )
    source.add_argument(
        "--manpath", action="append", type=Path, metavar="DIR", help="scan local manual roots"
    )
    parser.add_argument(
        "--source-pattern",
        action="append",
        metavar="REGEX",
        help="scan only sources matching every multiline expression",
    )
    parser.add_argument(
        "--max-pages",
        type=non_negative_integer,
        default=0,
        help="stable path-ordered sample size; zero scans all selected pages",
    )
    parser.add_argument("--corpus", help="stable corpus identity")
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
            raise ValueError(f"invalid target audit header in {path}")
        for number, row in enumerate(reader, 2):
            if SOURCE_DIGEST.fullmatch(row["source_sha256"]) is None:
                raise ValueError(f"invalid source digest at {path}:{number}")
            if row["profile_schema"] != PROFILE_SCHEMA:
                raise ValueError(f"unsupported target profile schema at {path}:{number}")
            if row["scan_status"] not in {"clean", "review", "hard-failure"}:
                raise ValueError(f"invalid scan status at {path}:{number}")
            if row["review_status"] not in REVIEW_STATUSES:
                raise ValueError(f"invalid review status at {path}:{number}")
            key = (row["corpus"], row["path"], row["source_sha256"])
            if key in records:
                raise ValueError(f"duplicate target audit identity at {path}:{number}")
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


def valid_target(value: object) -> bool:
    return isinstance(value, dict) and all(
        isinstance(value.get(field), expected_type)
        for field, expected_type in {
            "id": str,
            "normalizedId": str,
            "sourceLine": int,
            "ownerMacro": str,
            "ownerKind": str,
            "explicit": bool,
        }.items()
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
        batch = dict(items[offset : offset + 256])
        responses = run_jsonl_profile_batch(profiler, batch, timeout, "target")
        for request_id, response in responses.items():
            label = labels[request_id]
            if response.get("schema") != PROFILE_SCHEMA:
                yield Finding(label, "hard-failure", [], "unsupported profiler schema")
                continue
            if isinstance(response.get("error"), str):
                yield Finding(label, "hard-failure", [], str(response["error"]))
                continue
            expected = response.get("expected")
            observed = response.get("observed")
            missing = response.get("missing")
            target_owners = response.get("targetOwners")
            violations = response.get("violations")
            valid = (
                isinstance(expected, list)
                and all(valid_target(target) for target in expected)
                and isinstance(observed, list)
                and all(isinstance(identity, str) for identity in observed)
                and isinstance(missing, list)
                and all(valid_target(target) for target in missing)
                and isinstance(violations, list)
                and all(isinstance(item, str) for item in violations)
                and isinstance(target_owners, dict)
                and all(
                    isinstance(owner, str) and isinstance(count, int) and count >= 0
                    for owner, count in target_owners.items()
                )
            )
            if not valid:
                yield Finding(label, "hard-failure", [], "invalid profiler response")
                continue
            yield Finding(
                label,
                "review" if violations else "clean",
                violations,
                expected=expected,
                observed=observed,
                missing=missing,
                target_owners=target_owners,
            )


def self_check() -> None:
    assert manual_section(Path("git.1.gz")) == "1"
    assert manual_hierarchy_root(
        Path("/usr/share/man/fr/man3/printf.3.gz"), [Path("/usr/share/man")]
    ) == Path("/usr/share/man/fr")
    assert merge_review_status(None, "clean") == "not-required"
    assert merge_review_status(None, "review") == "pending"
    assert valid_target(
        {
            "id": "target",
            "normalizedId": "target",
            "sourceLine": 1,
            "ownerMacro": "Pp",
            "ownerKind": "element",
            "explicit": False,
        }
    )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff target audit self-check succeeded")
        return 0
    try:
        if arguments.timeout < 1:
            raise ValueError("--timeout must be positive")
        if not arguments.profiler.is_file():
            raise ValueError(
                "target profiler not found; run `cargo build -p mant-engine "
                "--example roff_target_profile`"
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
        print(f"audit-roff-targets: {error}", file=sys.stderr)
        return 2

    for path in unreadable:
        print(f"audit-roff-targets: unreadable source: {path}", file=sys.stderr)
    print("ManT roff target-conservation audit")
    print(f"  pages:  {len(pages)}")
    print(f"  corpus: {corpus}")
    print("  contract: validated zero-width targets must survive as IR identities")
    print()

    findings = list(profile_findings(pages, roots, arguments.profiler, arguments.timeout))
    by_label = {finding.path: finding for finding in findings}
    summary = Counter(finding.status for finding in findings)
    owner_summary = Counter()
    for finding in findings:
        owner_summary.update(finding.target_owners or {})
    verification_failed = False
    for page in pages:
        label, digest = records[page]
        finding = by_label[label]
        if not arguments.findings_only or finding.status != "clean":
            detail = f" — {finding.detail}" if finding.detail else ""
            violations = "; ".join(finding.violations)
            suffix = f": {violations}" if violations else ""
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
    if owner_summary:
        owners = ", ".join(
            f"{owner}={count}" for owner, count in owner_summary.most_common()
        )
        print(f"target owners: {owners}")
    if arguments.verify:
        print("target database verification: " + ("failed" if verification_failed else "passed"))
    else:
        write_database(arguments.audit_db, database.values())
        print(f"target database: {arguments.audit_db}")
    if arguments.json:
        arguments.json.parent.mkdir(parents=True, exist_ok=True)
        arguments.json.write_text(
            json.dumps(
                {
                    "schema": PROFILE_SCHEMA,
                    "corpus": corpus,
                    "roots": [str(root) for root in roots],
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
