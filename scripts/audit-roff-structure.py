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
import bz2
import csv
import gzip
import hashlib
import json
import lzma
import os
import re
import shutil
import subprocess
import sys
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Sequence

from roff_audit_common import run_jsonl_profile_batch


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_PROFILER = ROOT / "target/debug/examples/roff_structure_profile"
DEFAULT_AUDIT_DB = ROOT / "tests/fixtures/roff/STRUCTURE_AUDIT.csv"
DEFAULT_FIDELITY_DB = ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv"
PROFILE_SCHEMA = "mant.roff-structure-profile/v4"
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
MANUAL_SUFFIX = re.compile(
    r"\.(?P<section>[1-9][0-9A-Za-z]*|[ln])(?:\.(?:gz|bz2|xz|zst))?$"
)


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
        default=DEFAULT_AUDIT_DB,
        metavar="FILE",
        help=(
            "incremental structure-ledger CSV "
            f"(default: {DEFAULT_AUDIT_DB.relative_to(ROOT)})"
        ),
    )
    parser.add_argument(
        "--fidelity-db",
        type=Path,
        default=DEFAULT_FIDELITY_DB,
        metavar="FILE",
        help=(
            "content-audit CSV used by --replay-fidelity-records "
            f"(default: {DEFAULT_FIDELITY_DB.relative_to(ROOT)})"
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
    return parser.parse_args(argv)


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


def discover_pages(roots: Sequence[Path]) -> list[Path]:
    pages: set[Path] = set()
    for root in roots:
        if not root.is_dir():
            raise ValueError(f"manual root is not a directory: {root}")
        for path in root.rglob("*"):
            if (path.is_file() or path.is_symlink()) and MANUAL_SUFFIX.search(path.name):
                pages.add(path)
    return sorted(pages, key=lambda path: path.as_posix())


def manual_section(path: Path) -> str | None:
    match = MANUAL_SUFFIX.search(path.name)
    return match.group("section") if match is not None else None


def relative_label(path: Path, roots: Sequence[Path]) -> str:
    common = Path(os.path.commonpath(roots)) if len(roots) > 1 else None
    for root in roots:
        try:
            relative = path.relative_to(root)
        except ValueError:
            continue
        prefix = root.relative_to(common) if common is not None else Path(root.name)
        return (prefix / relative).as_posix()
    return path.as_posix()


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


def source_bytes(path: Path) -> bytes | None:
    try:
        if path.name.endswith(".gz"):
            return gzip.open(path, "rb").read()
        if path.name.endswith(".bz2"):
            return bz2.open(path, "rb").read()
        if path.name.endswith(".xz"):
            return lzma.open(path, "rb").read()
        if path.name.endswith(".zst"):
            zstd = shutil.which("zstd")
            if zstd is None:
                return None
            result = subprocess.run(
                [zstd, "--decompress", "--stdout", str(path)],
                stdout=subprocess.PIPE,
                stderr=subprocess.DEVNULL,
                check=False,
            )
            return result.stdout if result.returncode == 0 else None
        return path.read_bytes()
    except OSError:
        return None


def source_digest(path: Path) -> str | None:
    source = source_bytes(path)
    return hashlib.sha256(source).hexdigest() if source is not None else None


def compile_source_patterns(values: Sequence[str] | None) -> list[re.Pattern[str]]:
    patterns = []
    for value in values or []:
        try:
            patterns.append(re.compile(value, re.MULTILINE))
        except re.error as error:
            raise ValueError(f"invalid --source-pattern {value!r}: {error}") from error
    return patterns


def filter_pages_by_source(
    pages: Sequence[Path], patterns: Sequence[re.Pattern[str]]
) -> tuple[list[Path], list[Path]]:
    if not patterns:
        return list(pages), []
    selected = []
    unreadable = []
    for path in pages:
        source = source_bytes(path)
        if source is None:
            unreadable.append(path)
        elif all(pattern.search(source.decode("utf-8", errors="replace")) for pattern in patterns):
            selected.append(path)
    return selected, unreadable


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
    pages: Sequence[Path], roots: Sequence[Path], timeout: int, profiler: Path
) -> list[Finding]:
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
    for offset in range(0, len(requests), 256):
        batch = dict(list(requests.items())[offset : offset + 256])
        for request_id, response in run_profile_batch(profiler, batch, timeout).items():
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
                if (
                    not isinstance(violations, list)
                    or not all(isinstance(item, str) for item in violations)
                    or not valid_structure_counts(expected)
                    or not valid_structure_counts(observed)
                    or not valid_structure_topology(topology)
                ):
                    yield Finding(label, "hard-failure", [], "profiler returned invalid violations")
                else:
                    yield Finding(
                        label,
                        "review" if violations else "clean",
                        violations,
                        expected=expected,
                        observed=observed,
                    )


def valid_structure_counts(value: object) -> bool:
    return isinstance(value, dict) and all(
        isinstance(key, str) and isinstance(count, int) and count >= 0
        for key, count in value.items()
    )


def valid_structure_topology(value: object) -> bool:
    if not isinstance(value, dict):
        return False
    for side in ("expected", "observed"):
        topology = value.get(side)
        if not isinstance(topology, dict):
            return False
        if not all(
            isinstance(topology.get(field), list)
            for field in ("lists", "tableRows", "equations")
        ):
            return False
    return True


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
        all_pages = discover_pages(roots)
        if arguments.man_section:
            sections = set(arguments.man_section)
            all_pages = [page for page in all_pages if manual_section(page) in sections]
        all_pages, unreadable = filter_pages_by_source(
            all_pages, compile_source_patterns(arguments.source_pattern)
        )
        records = {
            page: (relative_label(page, roots), source_digest(page)) for page in all_pages
        }
        database = read_structure_database(arguments.audit_db)
        pages = list(all_pages)
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
        print(f"audit-roff-structure: {error}", file=sys.stderr)
        return 2

    for path in unreadable:
        print(
            f"audit-roff-structure: source pattern could not inspect unreadable path: {path}",
            file=sys.stderr,
        )
    if not pages:
        print("audit-roff-structure: no selected manual pages")
        return 0

    print("ManT roff structure audit")
    print(f"  profiler:  {arguments.profiler}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    print(f"  corpus:    {corpus}")
    print("  contract:  AST-to-IR topology; terminal wrapping is intentionally ignored")
    print()

    try:
        findings = list(profile_findings(pages, roots, arguments.timeout, arguments.profiler))
    except ValueError as error:
        print(f"audit-roff-structure: {error}", file=sys.stderr)
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
        f"hard={summary['hard-failure']}"
    )
    print("REVIEW is an AST-to-IR candidate, not a regression until source and IR are inspected.")
    write_structure_database(arguments.audit_db, database.values())
    print(f"structure database: {arguments.audit_db}")
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
    return 1 if summary["hard-failure"] else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
