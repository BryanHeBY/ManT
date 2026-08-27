#!/usr/bin/env python3
"""Audit conservative groff/man layout signals without revisiting AST coverage.

This is deliberately separate from ``audit-roff-structure.py`` and its ledger.
It reuses the fidelity auditor's exact hierarchy handling and terminal cleanup,
but records only renderer-layout observations: no-fill line-boundary merges,
spacing between adjacent no-fill lines, and anchor-relative indentation loss.
Existing content and structure rows are never rewritten or made stale.
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
import tempfile
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Sequence


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
FIDELITY_AUDITOR = ROOT / "scripts/audit-roff-fidelity.py"
DEFAULT_MANT = ROOT / "target/debug/mant"
DEFAULT_AUDIT_DB = ROOT / "tests/fixtures/roff/LAYOUT_AUDIT.csv"
DEFAULT_FIDELITY_DB = ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv"
DEFAULT_MANDOC_AUDIT_DB = ROOT / "tests/fixtures/roff/MANDOC_LAYOUT_AUDIT.csv"
DEFAULT_MANDOC_FIDELITY_DB = ROOT / "tests/fixtures/roff/MANDOC_FIDELITY_AUDIT.csv"
LAYOUT_SCHEMA = "mant.roff-layout-audit/v3"
MANUAL_SUFFIX = re.compile(r"\.(?P<section>[1-9][0-9A-Za-z]*|[ln])(?:\.(?:gz|bz2|xz|zst))?$")
DATABASE_FIELDS = [
    "corpus",
    "path",
    "section",
    "source_sha256",
    "layout_schema",
    "scan_status",
    "review_status",
    "note",
]
MANDOC_DATABASE_FIELDS = ["reference_kind", "reference_id", *DATABASE_FIELDS]
FIDELITY_DATABASE_FIELDS = [
    "corpus",
    "path",
    "section",
    "source_sha256",
    "scan_status",
    "review_status",
    "note",
]
MANDOC_FIDELITY_DATABASE_FIELDS = [
    "reference_kind",
    "reference_id",
    *FIDELITY_DATABASE_FIELDS,
]


@dataclass(frozen=True)
class AuditRecord:
    corpus: str
    path: str
    section: str
    digest: str
    schema: str
    scan_status: str
    review_status: str
    note: str
    reference_kind: str = "man"
    reference_id: str = ""


@dataclass
class Finding:
    path: str
    status: str
    reference_status: str | None
    candidates: list[str]
    layout: dict[str, object] | None
    detail: str | None = None


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="compare local man(1)/groff layout signals with ManT semantic text"
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument(
        "--fixtures",
        action="store_true",
        help="scan checked-in real roff fixtures (default)",
    )
    source.add_argument(
        "--manpath",
        action="append",
        type=Path,
        metavar="DIR",
        help="scan one or more local manual roots instead of fixtures",
    )
    parser.add_argument(
        "--pages-file",
        type=Path,
        metavar="FILE",
        help="audit exactly the absolute manual paths in FILE",
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
        "--mant",
        type=Path,
        default=DEFAULT_MANT,
        metavar="FILE",
        help=f"ManT executable (default: {DEFAULT_MANT.relative_to(ROOT)})",
    )
    parser.add_argument(
        "--reference",
        default="man",
        metavar="COMMAND",
        help="reference renderer command (default: man)",
    )
    parser.add_argument(
        "--reference-kind",
        choices=("man", "mandoc"),
        default="man",
        help="reference command interface passed to the fidelity auditor",
    )
    parser.add_argument(
        "--reference-id",
        metavar="IDENTITY",
        help="stable mandoc renderer/package identity",
    )
    parser.add_argument(
        "--timeout",
        type=positive_integer,
        default=30,
        metavar="SECONDS",
        help="per-page renderer timeout (default: 30)",
    )
    parser.add_argument(
        "--audit-db",
        type=Path,
        default=None,
        metavar="FILE",
        help=(
            "independent renderer-layout ledger (default selected by "
            "--reference-kind)"
        ),
    )
    parser.add_argument("--corpus", metavar="NAME", help="stable ledger corpus name")
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument(
        "--recorded-only",
        action="store_true",
        help="scan only unchanged rows already present in the layout ledger",
    )
    selection.add_argument(
        "--recheck-recorded",
        action="store_true",
        help="scan every selected page, including completed layout-ledger rows",
    )
    selection.add_argument(
        "--recheck-review-recorded",
        action="store_true",
        help="scan only unchanged selected rows whose latest layout result requires review",
    )
    selection.add_argument(
        "--replay-fidelity-records",
        action="store_true",
        help=(
            "select only unchanged completed rows from --fidelity-db for the "
            "chosen corpus; never modifies that content-fidelity ledger"
        ),
    )
    parser.add_argument(
        "--fidelity-db",
        type=Path,
        default=None,
        metavar="FILE",
        help=(
            "content-fidelity ledger used only with --replay-fidelity-records "
            "(default selected by --reference-kind)"
        ),
    )
    parser.add_argument(
        "--findings-only",
        action="store_true",
        help="print only REVIEW and HARD pages while retaining the summary",
    )
    parser.add_argument("--json", type=Path, metavar="FILE", help="write a complete JSON report")
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


def manual_section(path: Path) -> str | None:
    match = MANUAL_SUFFIX.search(path.name)
    return match.group("section") if match else None


def discover_pages(roots: Sequence[Path]) -> list[Path]:
    pages: set[Path] = set()
    for root in roots:
        if not root.is_dir():
            raise ValueError(f"manual root is not a directory: {root}")
        pages.update(
            path
            for path in root.rglob("*")
            if (path.is_file() or path.is_symlink()) and MANUAL_SUFFIX.search(path.name)
        )
    return sorted(pages, key=lambda path: path.as_posix())


def explicit_pages(path: Path, roots: Sequence[Path]) -> list[Path]:
    """Read an exact page set without relaxing its selected manual roots."""
    try:
        lines = path.read_text(encoding="utf-8").splitlines()
    except OSError as error:
        raise ValueError(f"cannot read --pages-file {path}: {error}") from error
    pages: set[Path] = set()
    for number, value in enumerate(lines, 1):
        if not value:
            continue
        candidate = Path(value)
        if not candidate.is_absolute():
            raise ValueError(f"--pages-file {path}:{number} is not an absolute path")
        if not MANUAL_SUFFIX.search(candidate.name):
            raise ValueError(f"--pages-file {path}:{number} is not a manual page")
        if not any(candidate.is_relative_to(root) for root in roots):
            raise ValueError(
                f"--pages-file {path}:{number} is outside the selected manual roots"
            )
        if not (candidate.is_file() or candidate.is_symlink()):
            raise ValueError(f"--pages-file {path}:{number} is not a readable manual page")
        pages.add(candidate)
    if not pages:
        raise ValueError(f"--pages-file {path} did not select any manual pages")
    return sorted(pages, key=lambda candidate: candidate.as_posix())


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


def read_database(
    path: Path,
    reference_kind: str = "man",
    reference_id: str = "",
) -> dict[tuple[str, str, str], AuditRecord]:
    if not path.exists():
        return {}
    entries: dict[tuple[str, str, str], AuditRecord] = {}
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        fields = MANDOC_DATABASE_FIELDS if reference_kind == "mandoc" else DATABASE_FIELDS
        if reader.fieldnames != fields:
            raise ValueError(
                f"invalid layout database header in {path}; expected {','.join(fields)}"
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
            if row["layout_schema"] != LAYOUT_SCHEMA:
                raise ValueError(f"invalid layout schema at {path}:{number}")
            statuses = {"clean", "review", "hard-failure"}
            if reference_kind == "mandoc":
                statuses.add("skipped")
            if row["scan_status"] not in statuses:
                raise ValueError(f"invalid scan status at {path}:{number}")
            if row["review_status"] not in {
                "not-required",
                "pending",
                "false-positive",
                "confirmed-open",
                "confirmed-fixed",
            }:
                raise ValueError(f"invalid review status at {path}:{number}")
            if not re.fullmatch(r"[0-9a-f]{64}", row["source_sha256"]):
                raise ValueError(f"invalid source digest at {path}:{number}")
            entry = AuditRecord(
                row["corpus"],
                row["path"],
                row["section"],
                row["source_sha256"],
                row["layout_schema"],
                row["scan_status"],
                row["review_status"],
                row["note"],
                reference_kind,
                reference_id,
            )
            entries[(entry.corpus, entry.path, entry.digest)] = entry
    return entries


def read_fidelity_records(
    path: Path,
    reference_kind: str = "man",
    reference_id: str = "",
) -> set[tuple[str, str, str]]:
    """Return completed content-audit identities without altering their ledger.

    The layout probe can deliberately revisit exactly the source bytes that
    were already rendered for fidelity.  It does not treat a historical
    ``skipped`` or ``hard-failure`` row as comparable evidence: neither had a
    completed two-renderer baseline. It needs no further fidelity detail beyond
    selecting the immutable `(corpus, path, digest)` identity.
    """
    if not path.is_file():
        raise ValueError(f"content-fidelity database does not exist: {path}")
    identities: set[tuple[str, str, str]] = set()
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        fields = (
            MANDOC_FIDELITY_DATABASE_FIELDS
            if reference_kind == "mandoc"
            else FIDELITY_DATABASE_FIELDS
        )
        if reader.fieldnames != fields:
            raise ValueError(
                f"invalid content-fidelity database header in {path}; "
                f"expected {','.join(fields)}"
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
            digest = row["source_sha256"]
            status = row["scan_status"]
            if not re.fullmatch(r"[0-9a-f]{64}", digest):
                raise ValueError(f"invalid content-fidelity digest at {path}:{number}")
            if status not in {"clean", "review", "hard-failure", "skipped"}:
                raise ValueError(f"invalid content-fidelity status at {path}:{number}")
            if status in {"clean", "review"}:
                identities.add((row["corpus"], row["path"], digest))
    return identities


def write_database(
    path: Path,
    entries: Iterable[AuditRecord],
    reference_kind: str = "man",
    reference_id: str = "",
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    with temporary.open("w", encoding="utf-8", newline="") as destination:
        fields = MANDOC_DATABASE_FIELDS if reference_kind == "mandoc" else DATABASE_FIELDS
        writer = csv.DictWriter(destination, fieldnames=fields, lineterminator="\n")
        writer.writeheader()
        for entry in sorted(entries, key=lambda item: (item.corpus, item.path, item.digest)):
            row = {
                "corpus": entry.corpus,
                "path": entry.path,
                "section": entry.section,
                "source_sha256": entry.digest,
                "layout_schema": entry.schema,
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


def merge_review_status(previous: AuditRecord | None, status: str) -> str:
    if previous is None:
        return "pending" if status in {"review", "hard-failure"} else "not-required"
    if previous.review_status == "not-required" and status in {"review", "hard-failure"}:
        return "pending"
    if previous.review_status == "pending" and status == "clean":
        return "not-required"
    return previous.review_status


def valid_layout(value: object) -> bool:
    if not isinstance(value, dict) or not isinstance(value.get("shared_anchors"), int):
        return False
    if not all(isinstance(item, str) for item in value.get("candidates", [])):
        return False
    for renderer in ("reference", "mant"):
        profile = value.get(renderer)
        if not isinstance(profile, dict):
            return False
        if not all(
            isinstance(profile.get(field), int)
            for field in ("nonblank_lines", "blank_line_runs", "blank_lines", "max_indent")
        ):
            return False
        if not all(isinstance(indent, int) for indent in profile.get("indent_levels", [])):
            return False
    return True


def run_layout_auditor(
    pages: Sequence[Path], roots: Sequence[Path], arguments: argparse.Namespace
) -> tuple[dict[str, Finding], str | None]:
    labels = {relative_label(path, roots) for path in pages}
    with tempfile.TemporaryDirectory(prefix="mant-roff-layout-") as directory:
        temporary = Path(directory)
        page_set = temporary / "pages.txt"
        report = temporary / "fidelity.json"
        page_set.write_text("".join(f"{path}\n" for path in pages), encoding="utf-8")
        command = [
            sys.executable,
            str(FIDELITY_AUDITOR),
            "--pages-file",
            str(page_set),
            "--layout-signals",
            "--mant",
            str(arguments.mant),
            "--reference",
            arguments.reference,
            "--reference-kind",
            arguments.reference_kind,
            "--timeout",
            str(arguments.timeout),
            "--json",
            str(report),
            "--findings-only",
        ]
        if arguments.reference_id:
            command.extend(["--reference-id", arguments.reference_id])
        if arguments.manpath:
            for root in roots:
                command.extend(["--manpath", str(root)])
        else:
            command.append("--fixtures")
        try:
            result = subprocess.run(
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=max(60, len(pages) * arguments.timeout * 2 + 30),
                check=False,
            )
        except subprocess.TimeoutExpired:
            detail = "renderer layout child timed out before returning its report"
            return ({label: Finding(label, "hard-failure", None, [], None, detail) for label in labels}, detail)
        try:
            payload = json.loads(report.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            output = result.stderr.strip() or result.stdout.strip()
            detail = output or f"renderer layout child did not write a valid report: {error}"
            return ({label: Finding(label, "hard-failure", None, [], None, detail) for label in labels}, detail)
    raw_findings = payload.get("findings")
    if not isinstance(raw_findings, list):
        detail = "renderer layout child report has no findings array"
        return ({label: Finding(label, "hard-failure", None, [], None, detail) for label in labels}, detail)
    results: dict[str, Finding] = {}
    for raw in raw_findings:
        if not isinstance(raw, dict) or not isinstance(raw.get("path"), str):
            continue
        label = raw["path"]
        reference_status = raw.get("status")
        layout = raw.get("layout")
        if label not in labels or reference_status not in {
            "clean",
            "review",
            "hard-failure",
            "skipped",
        }:
            continue
        if reference_status == "skipped":
            results[label] = Finding(
                label,
                "skipped",
                reference_status,
                [],
                None,
                raw.get("detail") if isinstance(raw.get("detail"), str) else None,
            )
            continue
        if reference_status == "hard-failure" or not valid_layout(layout):
            results[label] = Finding(
                label,
                "hard-failure",
                reference_status,
                [],
                None,
                raw.get("detail") if isinstance(raw.get("detail"), str) else "no valid layout signal",
            )
            continue
        candidates = layout["candidates"]
        results[label] = Finding(
            label,
            "review" if candidates else "clean",
            reference_status,
            candidates,
            layout,
        )
    missing = labels - results.keys()
    if missing:
        detail = "renderer layout child omitted this selected page from its report"
        for label in missing:
            results[label] = Finding(label, "hard-failure", None, [], None, detail)
        return results, detail
    return results, None


def self_check() -> None:
    assert manual_section(Path("git.1.gz")) == "1"
    assert manual_section(Path("SSL_read.3ssl.zst")) == "3ssl"
    assert merge_review_status(None, "review") == "pending"
    assert merge_review_status(None, "clean") == "not-required"
    assert FIDELITY_DATABASE_FIELDS[-1] == "note"
    with tempfile.TemporaryDirectory(prefix="mant-roff-layout-self-check-") as directory:
        database = Path(directory) / "fidelity.csv"
        database.write_text(
            "\n".join(
                [
                    ",".join(FIDELITY_DATABASE_FIELDS),
                    f"known,man/man1/known.1,1,{'0' * 64},clean,not-required,",
                    f"failed,man/man1/failed.1,1,{'2' * 64},hard-failure,pending,",
                    f"skipped,man/man1/skipped.1,1,{'1' * 64},skipped,pending,",
                    "",
                ]
            ),
            encoding="utf-8",
        )
        assert read_fidelity_records(database) == {
            ("known", "man/man1/known.1", "0" * 64)
        }
    assert valid_layout(
        {
            "shared_anchors": 2,
            "candidates": ["candidate"],
            "reference": {
                "nonblank_lines": 2,
                "blank_line_runs": 0,
                "blank_lines": 0,
                "max_indent": 4,
                "indent_levels": [2, 4],
            },
            "mant": {
                "nonblank_lines": 2,
                "blank_line_runs": 0,
                "blank_lines": 0,
                "max_indent": 2,
                "indent_levels": [0, 2],
            },
        }
    )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff layout audit self-check succeeded")
        return 0
    try:
        if arguments.reference_kind == "mandoc" and not arguments.reference_id:
            raise ValueError("mandoc layout audits require a stable --reference-id")
        reference_id = arguments.reference_id or arguments.reference
        if arguments.audit_db is None:
            arguments.audit_db = (
                DEFAULT_MANDOC_AUDIT_DB
                if arguments.reference_kind == "mandoc"
                else DEFAULT_AUDIT_DB
            )
        if arguments.fidelity_db is None:
            arguments.fidelity_db = (
                DEFAULT_MANDOC_FIDELITY_DB
                if arguments.reference_kind == "mandoc"
                else DEFAULT_FIDELITY_DB
            )
        if not FIDELITY_AUDITOR.is_file():
            raise ValueError(f"fidelity auditor not found: {FIDELITY_AUDITOR}")
        if not arguments.mant.is_file():
            raise ValueError(f"ManT executable not found: {arguments.mant}; run `cargo build -p mant`")
        roots = [path.resolve() for path in arguments.manpath] if arguments.manpath else [FIXTURE_ROOT]
        corpus = arguments.corpus or ("fixtures" if not arguments.manpath else "local-manpath")
        explicit_page_set = arguments.pages_file is not None
        if explicit_page_set and (
            arguments.max_pages
            or arguments.max_pages_per_section
            or arguments.man_section
            or arguments.source_pattern
            or arguments.recorded_only
            or arguments.recheck_recorded
            or arguments.recheck_review_recorded
            or arguments.replay_fidelity_records
        ):
            raise ValueError(
                "--pages-file cannot be combined with sampling, source filters, or ledger selection options"
            )
        if explicit_page_set:
            pages = explicit_pages(arguments.pages_file, roots)
            unreadable: list[Path] = []
        else:
            pages = discover_pages(roots)
            if arguments.man_section:
                sections = set(arguments.man_section)
                pages = [path for path in pages if manual_section(path) in sections]
            pages, unreadable = filter_pages_by_source(
                pages, compile_source_patterns(arguments.source_pattern)
            )
        records = {path: (relative_label(path, roots), source_digest(path)) for path in pages}
        database = read_database(
            arguments.audit_db,
            arguments.reference_kind,
            reference_id,
        )
        if arguments.recorded_only:
            pages = [
                path for path in pages
                if records[path][1] is not None
                and (corpus, records[path][0], records[path][1]) in database
            ]
        elif arguments.recheck_review_recorded:
            pages = [
                path for path in pages
                if records[path][1] is not None
                and (record := database.get((corpus, records[path][0], records[path][1]))) is not None
                and record.scan_status == "review"
            ]
        elif arguments.replay_fidelity_records:
            fidelity_records = read_fidelity_records(
                arguments.fidelity_db,
                arguments.reference_kind,
                reference_id,
            )
            pages = [
                path for path in pages
                if records[path][1] is not None
                and (corpus, records[path][0], records[path][1]) in fidelity_records
                and (corpus, records[path][0], records[path][1]) not in database
            ]
        elif not arguments.recheck_recorded:
            pages = [
                path for path in pages
                if records[path][1] is None
                or (corpus, records[path][0], records[path][1]) not in database
            ]
        if not explicit_page_set:
            pages = (
                stable_sample_by_section(pages, arguments.max_pages_per_section)
                if arguments.max_pages_per_section
                else stable_sample(pages, arguments.max_pages)
            )
    except ValueError as error:
        print(f"audit-roff-layout: {error}", file=sys.stderr)
        return 2
    for path in unreadable:
        print(f"audit-roff-layout: source pattern could not inspect unreadable path: {path}", file=sys.stderr)
    if not pages:
        print("audit-roff-layout: no selected manual pages")
        return 0

    print("ManT roff renderer-layout audit")
    print(f"  mant:      {arguments.mant}")
    print(f"  reference: {arguments.reference}")
    print(f"  profile:   {arguments.reference_kind}/{reference_id}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    if arguments.pages_file:
        print(f"  page set:  {arguments.pages_file}")
    print(f"  corpus:    {corpus}")
    if arguments.replay_fidelity_records:
        print(f"  replay:    completed rows from {arguments.fidelity_db}")
    elif arguments.recheck_review_recorded:
        print("  replay:    current review rows from the layout ledger")
    print("  contract:  source-gated line boundaries, spacing, and relative indentation")
    print()

    findings, child_error = run_layout_auditor(pages, roots, arguments)
    summary = Counter(finding.status for finding in findings.values())
    for path in pages:
        label, digest = records[path]
        finding = findings[label]
        if not arguments.findings_only or finding.status != "clean":
            detail = f" — {finding.detail}" if finding.detail else ""
            print(f"{finding.status.upper():12} {label}{detail}")
            for candidate in finding.candidates:
                print(f"  layout: {candidate}")
        if digest is not None:
            key = (corpus, label, digest)
            previous = database.get(key)
            database[key] = AuditRecord(
                corpus,
                label,
                manual_section(path) or "",
                digest,
                LAYOUT_SCHEMA,
                finding.status,
                merge_review_status(previous, finding.status),
                previous.note if previous is not None else "",
                arguments.reference_kind,
                reference_id,
            )
    print()
    print(
        "summary: "
        f"examined={len(findings)}, clean={summary['clean']}, review={summary['review']}, "
        f"hard={summary['hard-failure']}, skipped={summary['skipped']}"
    )
    if child_error:
        print(f"reference note: {child_error}")
    print("REVIEW is renderer evidence, not a regression until source and IR are inspected.")
    write_database(
        arguments.audit_db,
        database.values(),
        arguments.reference_kind,
        reference_id,
    )
    print(f"layout database: {arguments.audit_db}")
    if arguments.json:
        arguments.json.parent.mkdir(parents=True, exist_ok=True)
        arguments.json.write_text(
            json.dumps(
                {
                    "schema": LAYOUT_SCHEMA,
                    "corpus": corpus,
                    "roots": [str(root) for root in roots],
                    "reference": arguments.reference,
                    "referenceKind": arguments.reference_kind,
                    "referenceId": reference_id,
                    "replay_fidelity_records": arguments.replay_fidelity_records,
                    "fidelity_db": str(arguments.fidelity_db)
                    if arguments.replay_fidelity_records
                    else None,
                    "summary": dict(summary),
                    "findings": [asdict(findings[relative_label(path, roots)]) for path in pages],
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
