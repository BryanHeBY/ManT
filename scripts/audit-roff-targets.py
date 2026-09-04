#!/usr/bin/env python3
"""Audit zero-width libmandoc targets through ManT's AST-to-IR lowering.

The visible fidelity, structure, projection, and layout routes intentionally
cannot observe zero-width anchors. This independent route lowers the same
owned libmandoc parse that supplies its oracle, classifies every deep-link
owner, and compares retained targets with final IR identities and fragments.
Results are candidates until manually reviewed.
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
DEFAULT_PROFILER = ROOT / "target/debug/examples/roff_target_profile"
DEFAULT_AUDIT_DB = ROOT / "tests/fixtures/roff/TARGET_AUDIT.csv"
PROFILE_SCHEMA = "mant.roff-target-profile/v4"
SUPPORTED_PROFILE_SCHEMAS = {
    "mant.roff-target-profile/v1",
    "mant.roff-target-profile/v2",
    "mant.roff-target-profile/v3",
    PROFILE_SCHEMA,
}
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
    observed_occurrences: list[dict[str, object]] | None = None
    matched: list[dict[str, object]] | None = None
    missing: list[dict[str, object]] | None = None
    unexpected_targets: list[str] | None = None
    role_collisions: list[str] | None = None
    identity_violations: list[str] | None = None
    duplicate_target_count: int = 0
    dangling_target_count: int = 0
    target_owner_count: int = 0
    classified_owner_count: int = 0
    logical_owner_count: int = 0
    classified_logical_owner_count: int = 0
    owner_classes: list[dict[str, object]] | None = None
    unclassified_owners: list[dict[str, object]] | None = None


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
            if row["profile_schema"] not in SUPPORTED_PROFILE_SCHEMAS:
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
            "ownerSourceLine": int,
            "ownerMacro": str,
            "ownerKind": str,
            "astPath": str,
            "logicalOwnerKey": str,
            "sectionOrdinal": int,
            "sectionSourceLine": int,
            "expectedRole": str,
            "expectedContainer": str,
            "explicit": bool,
        }.items()
    ) and value.get("expectedRole") in {"section", "anchor"} and value.get(
        "expectedContainer"
    ) in {"section", "item", "content"}


def valid_observed_target(value: object) -> bool:
    return (
        isinstance(value, dict)
        and isinstance(value.get("identity"), str)
        and isinstance(value.get("fragmentAliases"), list)
        and all(isinstance(alias, str) for alias in value["fragmentAliases"])
        and value.get("role") in {"document", "section", "entry", "anchor"}
        and isinstance(value.get("container"), str)
        and isinstance(value.get("sectionOrdinal"), int)
        and value["sectionOrdinal"] >= 0
        and isinstance(value.get("sectionSourceLine"), int)
        and value["sectionSourceLine"] >= 0
        and isinstance(value.get("ownerSourceLine"), int)
        and value["ownerSourceLine"] >= 0
        and isinstance(value.get("ownerPath"), str)
        and isinstance(value.get("irPath"), str)
    )


def valid_matched_target(value: object) -> bool:
    return isinstance(value, dict) and all(
        isinstance(value.get(field), str)
        for field in (
            "logicalOwnerKey",
            "observedIrPath",
            "observedIdentity",
            "matchedBy",
        )
    )


def valid_owner_class(value: object) -> bool:
    if not isinstance(value, dict):
        return False
    return (
        isinstance(value.get("ownerMacro"), str)
        and isinstance(value.get("ownerKind"), str)
        and value.get("disposition") in {"retained", "excluded", "unclassified"}
        and isinstance(value.get("reason"), str)
        and isinstance(value.get("count"), int)
        and value["count"] > 0
    )


def valid_unclassified_owner(value: object) -> bool:
    if not isinstance(value, dict):
        return False
    target = value.get("target")
    return (
        (target is None or isinstance(target, str))
        and isinstance(value.get("sourceLine"), int)
        and value["sourceLine"] >= 0
        and isinstance(value.get("ownerMacro"), str)
        and isinstance(value.get("ownerKind"), str)
        and isinstance(value.get("astPath"), str)
        and isinstance(value.get("logicalOwnerKey"), str)
        and isinstance(value.get("rawOwnerCount"), int)
        and value["rawOwnerCount"] > 0
        and isinstance(value.get("reason"), str)
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
            observed_occurrences = response.get("observedOccurrences")
            matched = response.get("matched")
            observed_identities = response.get("observedIdentities")
            observed_fragments = response.get("observedFragmentAliases")
            observed_entries = response.get("observedEntryIdentities")
            observed_sections = response.get("observedSectionIdentities")
            anchors = response.get("anchors")
            section_links = response.get("sectionLinkTargets")
            missing = response.get("missing")
            unexpected_targets = response.get("unexpectedTargets")
            role_collisions = response.get("roleCollisions")
            identity_violations = response.get("identityViolations")
            duplicate_target_count = response.get("duplicateTargetCount")
            dangling_target_count = response.get("danglingTargetCount")
            target_owner_count = response.get("targetOwnerCount")
            classified_owner_count = response.get("classifiedOwnerCount")
            logical_owner_count = response.get("logicalOwnerCount")
            classified_logical_owner_count = response.get(
                "classifiedLogicalOwnerCount"
            )
            owner_classes = response.get("ownerClasses")
            unclassified_owners = response.get("unclassifiedOwners")
            violations = response.get("violations")
            valid = (
                isinstance(expected, list)
                and all(valid_target(target) for target in expected)
                and isinstance(observed, list)
                and all(isinstance(identity, str) for identity in observed)
                and isinstance(observed_occurrences, list)
                and all(valid_observed_target(item) for item in observed_occurrences)
                and isinstance(matched, list)
                and all(valid_matched_target(item) for item in matched)
                and all(
                    isinstance(collection, list)
                    and all(isinstance(identity, str) for identity in collection)
                    for collection in (
                        observed_identities,
                        observed_fragments,
                        observed_entries,
                        observed_sections,
                        anchors,
                        section_links,
                    )
                )
                and isinstance(missing, list)
                and all(valid_target(target) for target in missing)
                and isinstance(unexpected_targets, list)
                and all(isinstance(target, str) for target in unexpected_targets)
                and isinstance(role_collisions, list)
                and all(isinstance(target, str) for target in role_collisions)
                and isinstance(identity_violations, list)
                and all(isinstance(target, str) for target in identity_violations)
                and isinstance(duplicate_target_count, int)
                and duplicate_target_count >= 0
                and isinstance(dangling_target_count, int)
                and dangling_target_count >= 0
                and isinstance(target_owner_count, int)
                and target_owner_count >= 0
                and isinstance(classified_owner_count, int)
                and 0 <= classified_owner_count <= target_owner_count
                and isinstance(logical_owner_count, int)
                and logical_owner_count >= 0
                and isinstance(classified_logical_owner_count, int)
                and 0 <= classified_logical_owner_count <= logical_owner_count
                and isinstance(violations, list)
                and all(isinstance(item, str) for item in violations)
                and isinstance(owner_classes, list)
                and all(valid_owner_class(item) for item in owner_classes)
                and sum(int(item["count"]) for item in owner_classes)
                == target_owner_count
                and isinstance(unclassified_owners, list)
                and all(valid_unclassified_owner(item) for item in unclassified_owners)
                and sum(int(item["rawOwnerCount"]) for item in unclassified_owners)
                == target_owner_count - classified_owner_count
                and len(unclassified_owners)
                == logical_owner_count - classified_logical_owner_count
                and len(matched) + len(missing) == len(expected)
            )
            has_derived_violation = bool(
                missing
                or unexpected_targets
                or role_collisions
                or identity_violations
                or unclassified_owners
                or duplicate_target_count
                or dangling_target_count
            )
            valid = valid and bool(violations) == has_derived_violation
            if not valid:
                yield Finding(label, "hard-failure", [], "invalid profiler response")
                continue
            yield Finding(
                label,
                "review" if violations else "clean",
                violations,
                expected=expected,
                observed=observed,
                observed_occurrences=observed_occurrences,
                matched=matched,
                missing=missing,
                unexpected_targets=unexpected_targets,
                role_collisions=role_collisions,
                identity_violations=identity_violations,
                duplicate_target_count=duplicate_target_count,
                dangling_target_count=dangling_target_count,
                target_owner_count=target_owner_count,
                classified_owner_count=classified_owner_count,
                logical_owner_count=logical_owner_count,
                classified_logical_owner_count=classified_logical_owner_count,
                owner_classes=owner_classes,
                unclassified_owners=unclassified_owners,
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
            "ownerSourceLine": 2,
            "ownerMacro": "Pp",
            "ownerKind": "element",
            "astPath": "0.1",
            "logicalOwnerKey": "1:0.1:target",
            "sectionOrdinal": 1,
            "sectionSourceLine": 1,
            "expectedRole": "anchor",
            "expectedContainer": "content",
            "explicit": False,
        }
    )
    assert valid_observed_target(
        {
            "identity": "target",
            "fragmentAliases": ["Target"],
            "role": "anchor",
            "container": "paragraph",
            "sectionOrdinal": 1,
            "sectionSourceLine": 1,
            "ownerSourceLine": 2,
            "ownerPath": "section[0]/block[0]",
            "irPath": "section[0]/block[0]/inline[0]",
        }
    )
    assert valid_matched_target(
        {
            "logicalOwnerKey": "1:0.1:target",
            "observedIrPath": "section[0]/block[0]/inline[0]",
            "observedIdentity": "target",
            "matchedBy": "identity",
        }
    )
    assert valid_owner_class(
        {
            "ownerMacro": "IP",
            "ownerKind": "head",
            "disposition": "retained",
            "reason": "validated non-section navigation destination",
            "count": 2,
        }
    )
    assert valid_unclassified_owner(
        {
            "target": "future",
            "sourceLine": 7,
            "ownerMacro": "Future",
            "ownerKind": "element",
            "astPath": "0.3",
            "logicalOwnerKey": "1:0.3:future",
            "rawOwnerCount": 1,
            "reason": "owner macro has no target-conservation policy",
        }
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
    print("  contract: every native target owner is classified and retained targets resolve")
    print()

    findings = list(profile_findings(pages, roots, arguments.profiler, arguments.timeout))
    by_label = {finding.path: finding for finding in findings}
    summary = Counter(finding.status for finding in findings)
    owner_summary = Counter()
    missing_count = 0
    unexpected_count = 0
    role_collision_count = 0
    identity_violation_count = 0
    target_owner_count = 0
    logical_owner_count = 0
    unclassified_logical_owner_count = 0
    unclassified_owner_count = 0
    duplicate_count = 0
    dangling_count = 0
    for finding in findings:
        for owner_class in finding.owner_classes or []:
            key = (
                f"{owner_class['ownerMacro']}/{owner_class['ownerKind']}"
                f"[{owner_class['disposition']}]"
            )
            owner_summary[key] += int(owner_class["count"])
        missing_count += len(finding.missing or [])
        unexpected_count += len(finding.unexpected_targets or [])
        role_collision_count += len(finding.role_collisions or [])
        identity_violation_count += len(finding.identity_violations or [])
        target_owner_count += finding.target_owner_count
        logical_owner_count += finding.logical_owner_count
        unclassified_owner_count += sum(
            int(owner["rawOwnerCount"])
            for owner in (finding.unclassified_owners or [])
        )
        unclassified_logical_owner_count += (
            finding.logical_owner_count - finding.classified_logical_owner_count
        )
        duplicate_count += finding.duplicate_target_count
        dangling_count += finding.dangling_target_count
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
    print(
        f"target differences: missing={missing_count}, unexpected={unexpected_count}, "
        f"role-collision={role_collision_count}, invalid-identity={identity_violation_count}, "
        f"duplicate={duplicate_count}, dangling={dangling_count}"
    )
    print(
        f"target owner classification: total={target_owner_count}, "
        f"classified={target_owner_count - unclassified_owner_count}, "
        f"unclassified={unclassified_owner_count}"
    )
    print(
        f"logical target obligations: total={logical_owner_count}, "
        f"classified={logical_owner_count - unclassified_logical_owner_count}, "
        f"unclassified={unclassified_logical_owner_count}"
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
                    "producerCommit": repository_commit(),
                    "profileSha256": file_digest(arguments.profiler),
                    "scannedAt": datetime.now(timezone.utc).isoformat(),
                    "corpus": corpus,
                    "roots": [str(root) for root in roots],
                    "pageCount": len(findings),
                    "targetOwnerCount": target_owner_count,
                    "logicalOwnerCount": logical_owner_count,
                    "targetDifferences": {
                        "missing": missing_count,
                        "unexpected": unexpected_count,
                        "roleCollision": role_collision_count,
                        "invalidIdentity": identity_violation_count,
                        "duplicate": duplicate_count,
                        "dangling": dangling_count,
                        "unclassifiedRaw": unclassified_owner_count,
                        "unclassifiedLogical": unclassified_logical_owner_count,
                    },
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
