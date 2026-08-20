#!/usr/bin/env python3
"""Find IR-to-CommonMark topology gaps in local roff corpora.

This development-only audit is intentionally independent from the AST-to-IR
structure and reference-renderer ledgers. It reparses ManT's public Markdown
projection and checks section identity/order, list kind/item ownership, fenced
block language/ownership, and a deterministic sample of node excerpts.

The ordinary output is a review queue. ``--verify`` turns the selected corpus
into a read-only gate: both review candidates and hard failures make the command
fail, while the audit ledger remains untouched.
"""

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
from pathlib import Path
from typing import Iterable, Sequence

from roff_audit_common import (
    compile_source_patterns,
    discover_pages,
    filter_pages_by_source,
    manual_hierarchy_root,
    manual_section,
    non_negative_integer,
    positive_integer,
    read_fidelity_identities,
    relative_label,
    source_digest,
    stable_sample,
    stable_sample_by_section,
)


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_PROFILER = ROOT / "target/debug/examples/roff_projection_profile"
DEFAULT_AUDIT_DB = ROOT / "tests/fixtures/roff/PROJECTION_AUDIT.csv"
DEFAULT_FIDELITY_DB = ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv"
PROFILE_SCHEMA = "mant.roff-projection-profile/v2"
PROFILE_SCHEMA_PATTERN = re.compile(r"mant\.roff-projection-profile/v[1-9][0-9]*$")
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
    expected: dict[str, int] | None = None
    observed: dict[str, int] | None = None
    excerpt_checks: int = 0


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="reparse ManT CommonMark and compare its topology with native IR"
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
        "--profiler", type=Path, default=DEFAULT_PROFILER, metavar="FILE"
    )
    parser.add_argument(
        "--timeout",
        type=positive_integer,
        default=600,
        help="seconds allowed for the complete profiler batch (default: 600)",
    )
    parser.add_argument("--audit-db", type=Path, default=DEFAULT_AUDIT_DB, metavar="FILE")
    parser.add_argument(
        "--fidelity-db", type=Path, default=DEFAULT_FIDELITY_DB, metavar="FILE"
    )
    parser.add_argument(
        "--corpus", metavar="NAME", help="stable corpus name (default: fixtures or local-manpath)"
    )
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument(
        "--recorded-only",
        action="store_true",
        help="scan only unchanged rows already present in the projection ledger",
    )
    selection.add_argument(
        "--recheck-recorded",
        action="store_true",
        help="scan every selected page, including completed projection rows",
    )
    selection.add_argument(
        "--replay-fidelity-records",
        action="store_true",
        help="scan unchanged inputs recorded for this corpus in the fidelity ledger",
    )
    parser.add_argument("--findings-only", action="store_true")
    parser.add_argument(
        "--verify",
        action="store_true",
        help="do not update the ledger and fail unless every selected page is clean",
    )
    parser.add_argument("--json", type=Path, metavar="FILE")
    parser.add_argument("--self-check", action="store_true", help=argparse.SUPPRESS)
    return parser.parse_args(argv)


def read_database(path: Path) -> dict[tuple[str, str, str], AuditRecord]:
    if not path.exists():
        return {}
    entries = {}
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != DATABASE_FIELDS:
            raise ValueError(
                f"invalid projection database header in {path}; expected "
                f"{','.join(DATABASE_FIELDS)}"
            )
        for number, row in enumerate(reader, 2):
            if row["scan_status"] not in {"clean", "review", "hard-failure"}:
                raise ValueError(f"invalid scan status at {path}:{number}")
            if row["review_status"] not in {
                "not-required",
                "pending",
                "false-positive",
                "confirmed-open",
                "confirmed-fixed",
            }:
                raise ValueError(f"invalid review status at {path}:{number}")
            if not PROFILE_SCHEMA_PATTERN.fullmatch(row["profile_schema"]):
                raise ValueError(f"invalid profile schema at {path}:{number}")
            if not re.fullmatch(r"[0-9a-f]{64}", row["source_sha256"]):
                raise ValueError(f"invalid source digest at {path}:{number}")
            record = AuditRecord(
                corpus=row["corpus"],
                path=row["path"],
                section=row["section"],
                digest=row["source_sha256"],
                profile_schema=row["profile_schema"],
                scan_status=row["scan_status"],
                review_status=row["review_status"],
                note=row["note"],
            )
            entries[(record.corpus, record.path, record.digest)] = record
    return entries


def write_database(path: Path, entries: Iterable[AuditRecord]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    with temporary.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.DictWriter(destination, fieldnames=DATABASE_FIELDS, lineterminator="\n")
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


def merge_review_status(previous: AuditRecord | None, status: str) -> str:
    if previous is None:
        return "pending" if status in {"review", "hard-failure"} else "not-required"
    if previous.review_status == "not-required" and status in {"review", "hard-failure"}:
        return "pending"
    if previous.review_status == "pending" and status == "clean":
        return "not-required"
    return previous.review_status


def audit_exit_status(summary: Counter[str], verify: bool) -> int:
    if summary["hard-failure"]:
        return 1
    if verify and summary["review"]:
        return 1
    return 0


def run_profile_batch(
    profiler: Path, requests: dict[str, dict[str, str]], timeout: int
) -> dict[str, dict[str, object]]:
    payload = "".join(json.dumps(request, ensure_ascii=False) + "\n" for request in requests.values())
    try:
        result = subprocess.run(
            [str(profiler)],
            input=payload,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired as error:
        raise ValueError(f"projection profiler timed out after {timeout}s") from error
    if result.returncode != 0:
        detail = result.stderr.strip() or f"exit status {result.returncode}"
        raise ValueError(f"projection profiler failed: {detail}")
    responses = {}
    for number, line in enumerate(result.stdout.splitlines(), 1):
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"projection profiler returned invalid JSON on line {number}") from error
        request_id = response.get("id")
        if not isinstance(request_id, str):
            raise ValueError(f"projection profiler returned an invalid id on line {number}")
        responses[request_id] = response
    for request_id in requests:
        responses.setdefault(request_id, {"id": request_id, "error": "profiler returned no response"})
    return responses


def valid_counts(value: object) -> bool:
    return (
        isinstance(value, dict)
        and set(value) == {"sections", "listItems", "fences", "entitySpellings"}
        and all(isinstance(count, int) and count >= 0 for count in value.values())
    )


def valid_topology(value: object) -> bool:
    if not isinstance(value, dict):
        return False
    for side in ("expected", "observed"):
        topology = value.get(side)
        if not isinstance(topology, dict):
            return False
        if not all(
            isinstance(topology.get(field), list)
            for field in ("sections", "listItems", "fences", "entitySpellings")
        ):
            return False
    return True


def profile_findings(
    pages: Sequence[Path], roots: Sequence[Path], timeout: int, profiler: Path
) -> Iterable[Finding]:
    requests = {}
    labels = {}
    for path in pages:
        hierarchy_root = manual_hierarchy_root(path, roots)
        label = relative_label(path, roots)
        if hierarchy_root is None:
            yield Finding(label, "hard-failure", [], "manual hierarchy is unknown")
            continue
        request_id = hashlib.sha256(str(path).encode("utf-8")).hexdigest()
        requests[request_id] = {"id": request_id, "path": str(path), "root": str(hierarchy_root)}
        labels[request_id] = label
    items = list(requests.items())
    for offset in range(0, len(items), 256):
        batch = dict(items[offset : offset + 256])
        for request_id, response in run_profile_batch(profiler, batch, timeout).items():
            label = labels[request_id]
            violations = response.get("violations")
            expected = response.get("expected")
            observed = response.get("observed")
            excerpt_checks = response.get("excerptChecks")
            if response.get("schema") != PROFILE_SCHEMA:
                yield Finding(label, "hard-failure", [], "profiler returned an unsupported schema")
            elif isinstance(response.get("error"), str):
                yield Finding(label, "hard-failure", [], str(response["error"]))
            elif (
                not isinstance(violations, list)
                or not all(isinstance(item, str) for item in violations)
                or not valid_counts(expected)
                or not valid_counts(observed)
                or not valid_topology(response.get("topology"))
                or not isinstance(excerpt_checks, int)
                or excerpt_checks < 0
            ):
                yield Finding(label, "hard-failure", [], "profiler returned invalid topology")
            else:
                yield Finding(
                    label,
                    "review" if violations else "clean",
                    violations,
                    expected=expected,
                    observed=observed,
                    excerpt_checks=excerpt_checks,
                )


def self_check() -> None:
    assert manual_section(Path("git.1.gz")) == "1"
    assert manual_section(Path("SSL_read.3ssl.zst")) == "3ssl"
    assert manual_hierarchy_root(
        Path("/usr/share/man/fr/man3/printf.3.gz"), [Path("/usr/share/man")]
    ) == Path("/usr/share/man/fr")
    assert merge_review_status(None, "clean") == "not-required"
    assert merge_review_status(None, "review") == "pending"
    assert audit_exit_status(Counter({"clean": 1}), verify=True) == 0
    assert audit_exit_status(Counter({"review": 1}), verify=False) == 0
    assert audit_exit_status(Counter({"review": 1}), verify=True) == 1
    assert audit_exit_status(Counter({"hard-failure": 1}), verify=False) == 1
    assert valid_counts(
        {"sections": 1, "listItems": 2, "fences": 3, "entitySpellings": 4}
    )
    assert valid_topology(
        {
            "expected": {
                "sections": [],
                "listItems": [],
                "fences": [],
                "entitySpellings": [],
            },
            "observed": {
                "sections": [],
                "listItems": [],
                "fences": [],
                "entitySpellings": [],
            },
        }
    )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff projection audit self-check succeeded")
        return 0
    try:
        if not arguments.profiler.is_file():
            raise ValueError(
                f"projection profiler not found: {arguments.profiler}; run "
                "`cargo build -p mant-engine --example roff_projection_profile`"
            )
        roots = [path.resolve() for path in arguments.manpath] if arguments.manpath else [FIXTURE_ROOT]
        corpus = arguments.corpus or ("fixtures" if not arguments.manpath else "local-manpath")
        pages = discover_pages(roots)
        if arguments.man_section:
            sections = set(arguments.man_section)
            pages = [page for page in pages if manual_section(page) in sections]
        pages, unreadable = filter_pages_by_source(
            pages, compile_source_patterns(arguments.source_pattern)
        )
        records = {page: (relative_label(page, roots), source_digest(page)) for page in pages}
        database = read_database(arguments.audit_db)
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
                if records[page][1] is None
                or (corpus, records[page][0], records[page][1]) not in database
                or database[(corpus, records[page][0], records[page][1])].profile_schema
                != PROFILE_SCHEMA
            ]
        pages = (
            stable_sample_by_section(pages, arguments.max_pages_per_section)
            if arguments.max_pages_per_section
            else stable_sample(pages, arguments.max_pages)
        )
    except ValueError as error:
        print(f"audit-roff-projection: {error}", file=sys.stderr)
        return 2

    for path in unreadable:
        print(f"audit-roff-projection: unreadable source: {path}", file=sys.stderr)
    if not pages:
        print("audit-roff-projection: no selected manual pages")
        return 0

    print("ManT roff CommonMark projection audit")
    print(f"  profiler:  {arguments.profiler}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    print(f"  corpus:    {corpus}")
    print("  contract:  IR topology survives full and sampled node CommonMark projection")
    print()

    try:
        findings = list(profile_findings(pages, roots, arguments.timeout, arguments.profiler))
    except ValueError as error:
        print(f"audit-roff-projection: {error}", file=sys.stderr)
        return 2
    summary = Counter(finding.status for finding in findings)
    by_label = {finding.path: finding for finding in findings}
    for path in pages:
        label, digest = records[path]
        finding = by_label[label]
        if not arguments.findings_only or finding.status != "clean":
            detail = f" — {finding.detail}" if finding.detail else ""
            violations = "; ".join(finding.violations)
            print(f"{finding.status.upper():12} {label}{detail}{': ' + violations if violations else ''}")
        if digest is not None:
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
        f"hard={summary['hard-failure']}, excerpts={sum(item.excerpt_checks for item in findings)}"
    )
    print("REVIEW is a projection candidate until the generated CommonMark is inspected.")
    if arguments.verify:
        print("projection database: unchanged (verification mode)")
    else:
        write_database(arguments.audit_db, database.values())
        print(f"projection database: {arguments.audit_db}")
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
    return audit_exit_status(summary, arguments.verify)


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
