#!/usr/bin/env python3
"""Find likely roff fidelity gaps without treating layout as a contract.

The audit compares ManT's plain manual rendering with a local man(1)/groff
reference. It is deliberately a developer and release-time discovery tool:
ordinary CI keeps the focused, deterministic Rust regressions derived from
confirmed findings instead of installing or trusting a host reference renderer.
Pages containing .so requests are rendered through ManT's indexed-manual path
so aliases exercise the same bounded hierarchy resolution as product queries.
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
from fractions import Fraction
from pathlib import Path, PurePosixPath
from typing import Iterable, Sequence


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_MANT = ROOT / "target/debug/mant"
DEFAULT_SYNTAX_PROFILER = ROOT / "target/debug/examples/roff_structure_profile"
DEFAULT_SOURCE_LEDGER = ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv"
SYNTAX_PROFILE_SCHEMA = "mant.roff-ast-profile/v2"
SYNTAX_CACHE_VERSION = 3
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

MANUAL_SUFFIX = re.compile(
    r"\.(?P<section>[1-9][0-9A-Za-z]*|[ln])(?:\.(?:gz|bz2|xz|zst))?$"
)
ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))")
TOKEN = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.+:/-]{2,}")
# Only join a wrapped URL/path component after a non-slash component. A bare
# slash at the end of an unrelated token (for example Perl's `tr//` followed
# by a new sentence) must not consume the semantic line boundary.
URL_WRAP = re.compile(
    r"(https?://[^\s\n]*/)[ \t]*\n[ \t]*(?=[A-Za-z0-9%_~?&=][^\s\n]*[./_~?&=%-])"
)
DEHYPHENATE = re.compile(r"-[ \t]*\n[ \t]*")
BORDERS = re.compile(r"[\u2500-\u257f\u2022\u00b7]")
ANGLE_LINK = re.compile(r"<((?:https?|mailto):[^<>]{1,4096})>", re.DOTALL)
RUNNING_HEADER = re.compile(
    r"^\s*(?P<label>\S+\([^)\s]+\))\s+.+\s+(?P=label)\s*$",
    re.IGNORECASE,
)
UNICODE_ESCAPE = re.compile(
    r"\\\[u[0-9A-Fa-f]{4,6}(?:_[0-9A-Fa-f]{4,6})*\]"
)
GLUED_MARKER = re.compile(r"^[ \t]*\u2022[A-Za-z(\"']", re.MULTILINE)
INTERNAL_MARKER = re.compile("[\u001d-\u001f]")
MDOC_NAME_DESCRIPTION = re.compile(r"^[.']Nd(?:\s|$)", re.MULTILINE)
MDOC_FUNCTION_DECLARATION = re.compile(r"^[.'](?:Fn|Fo)(?:\s|$)", re.MULTILINE)
MDOC_MULTI_OPERAND_FA = re.compile(
    r'''^[.']Fa(?:[ \t]+"(?:[^"\\]|\\.)*"){2,}[ \t]*$''',
    re.MULTILINE,
)
EM_DASH_ATTACHED_TO_WORD = re.compile(r"—(?=\w)")
EXTERNAL_ROFF_CONTEXT = re.compile(
    rb"(?:^|[ \t])[.'](?:so|mso)(?:[ \t]|$)", re.MULTILINE
)
ROFF_REQUEST = re.compile(r"^[.'](?P<name>[A-Za-z][A-Za-z0-9]*)(?:[ \t]+(?P<args>.*))?$")
INLINE_ROFF_CONTROL = re.compile(r"(?<=[ \t])(?P<body>[.'][A-Za-z][A-Za-z0-9]*(?:[ \t]+.*)?)$")
ROFF_FONT_ESCAPE = re.compile(r"\\f(?:\[[^]]*]|.)")

TRANSLATION = str.maketrans(
    {
        "\u2010": "-",
        "\u2011": "-",
        "\u2212": "-",
        "\u00ad": "",
        "\u00a0": " ",
        "\u2018": "'",
        "\u2019": "'",
        "\u201c": '"',
        "\u201d": '"',
        "\u2013": "-",
        "\u2014": "-",
        "\u2022": " ",
        "\u00b7": " ",
    }
)


@dataclass
class Finding:
    path: str
    status: str
    reference_tokens: int = 0
    mant_tokens: int = 0
    missing_tokens: list[str] | None = None
    broken_phrases: list[str] | None = None
    signatures: list[str] | None = None
    detail: str | None = None
    layout: "LayoutComparison | None" = None


@dataclass
class LayoutProfile:
    """Geometry observed in one terminal rendering.

    The values are deliberately descriptive.  Groff's absolute columns and
    wrapping are device-dependent, while ManT renders a copyable semantic
    layout; only anchor-relative transitions can become candidates.
    """

    nonblank_lines: int
    blank_line_runs: int
    blank_lines: int
    max_indent: int
    indent_levels: list[int]


@dataclass
class LayoutComparison:
    reference: LayoutProfile
    mant: LayoutProfile
    shared_anchors: int
    reference_baseline_indent: int | None
    mant_baseline_indent: int | None
    candidates: list[str]


@dataclass(frozen=True)
class NoFillSourceLayout:
    """Source obligations narrow enough to compare across renderers.

    A formatter's implicit display gutter is not source content.  Only raw
    indentation authored inside a no-fill region, explicit blank requests,
    and adjacent authored lines are suitable cross-renderer layout signals.
    Flowed source is retained only to reject same-text, different-location
    collisions: it can produce the same visible fragments without losing a
    no-fill boundary.
    """

    anchors: frozenset[str]
    authored_indents: dict[str, int]
    spaced_pairs: frozenset[tuple[str, str]]
    consecutive_pairs: frozenset[tuple[str, str]]
    source_lines: frozenset[str]
    flowed_pairs: frozenset[tuple[str, str]]


@dataclass
class AuditArtifact:
    """One comparison result plus the local evidence used to review it.

    The normal JSON report remains intentionally compact and contains only
    normalized findings. An explicitly requested review bundle keeps the raw
    decompressed source and both visible renderings together so a human can
    decide whether a candidate is semantic loss or presentation drift.
    """

    finding: Finding
    source: bytes | None
    reference_output: str | None
    mant_output: str | None


@dataclass
class AuditSummary:
    examined: int = 0
    clean: int = 0
    review: int = 0
    hard_failures: int = 0
    skipped: int = 0


@dataclass(frozen=True)
class AuditRecord:
    corpus: str
    path: str
    section: str
    digest: str
    scan_status: str
    review_status: str
    note: str
    reference_kind: str = "man"
    reference_id: str = ""


@dataclass(frozen=True)
class SyntaxProfile:
    features: frozenset[str]
    diagnostics: int
    error: str | None = None


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "compare ManT and man(1) visible semantics over existing fixtures "
            "or a local manual tree"
        )
    )
    source = parser.add_mutually_exclusive_group()
    source.add_argument(
        "--fixtures",
        action="store_true",
        help="audit the checked-in real roff fixture catalogue (default)",
    )
    source.add_argument(
        "--manpath",
        action="append",
        type=Path,
        metavar="DIR",
        help="audit one or more local manual roots instead of checked-in fixtures",
    )
    parser.add_argument(
        "--pages-file",
        type=Path,
        metavar="FILE",
        help=(
            "audit the newline-delimited absolute manual paths in FILE; "
            "bypass discovery sampling and ledger selection while retaining "
            "the selected source roots"
        ),
    )
    parser.add_argument(
        "--replay-source-records",
        action="store_true",
        help=(
            "audit exactly the unchanged source identities recorded for "
            "--corpus in --source-ledger"
        ),
    )
    parser.add_argument(
        "--source-ledger",
        type=Path,
        default=DEFAULT_SOURCE_LEDGER,
        metavar="FILE",
        help=(
            "historical source-identity ledger used only with "
            f"--replay-source-records (default: {DEFAULT_SOURCE_LEDGER.relative_to(ROOT)})"
        ),
    )
    parser.add_argument(
        "--mant",
        type=Path,
        default=DEFAULT_MANT,
        help=f"ManT executable (default: {DEFAULT_MANT.relative_to(ROOT)})",
    )
    parser.add_argument(
        "--reference",
        default="man",
        help="reference renderer command (default: man)",
    )
    parser.add_argument(
        "--reference-kind",
        choices=("man", "mandoc"),
        default="man",
        help=(
            "reference command interface: man uses indexed topic/section or "
            "-l; mandoc renders exact source files (default: man)"
        ),
    )
    parser.add_argument(
        "--reference-id",
        metavar="IDENTITY",
        help=(
            "stable renderer/package identity recorded in mandoc reports and "
            "ledgers, for example mandoc-1.14.6-1"
        ),
    )
    sampling = parser.add_mutually_exclusive_group()
    sampling.add_argument(
        "--max-pages",
        type=non_negative_integer,
        default=0,
        help="stable sample size; zero audits every discovered page",
    )
    sampling.add_argument(
        "--max-pages-per-section",
        type=positive_integer,
        default=0,
        help="stable sample size for each discovered manual section",
    )
    parser.add_argument(
        "--man-section",
        action="append",
        metavar="SECTION",
        help="audit only an exact manual section; may be repeated",
    )
    parser.add_argument(
        "--source-pattern",
        action="append",
        metavar="REGEX",
        help=(
            "audit only sources matching this multiline regular expression; "
            "repeat to require every pattern"
        ),
    )
    parser.add_argument(
        "--seed",
        default="mant-fidelity-v1",
        help="stable sampling seed used with bounded sampling",
    )
    parser.add_argument(
        "--syntax-priority",
        action="store_true",
        help=(
            "retired compatibility option; use audit-roff-structure.py for "
            "native AST-to-IR topology sampling"
        ),
    )
    parser.add_argument(
        "--syntax-profiler",
        type=Path,
        default=DEFAULT_SYNTAX_PROFILER,
        metavar="FILE",
        help=(
            "retired AST profiler path; use audit-roff-structure.py instead "
            f"(default: {DEFAULT_SYNTAX_PROFILER.relative_to(ROOT)})"
        ),
    )
    parser.add_argument(
        "--syntax-cache",
        type=Path,
        metavar="FILE",
        help="reuse and update content-addressed AST profiles in a JSON cache",
    )
    parser.add_argument(
        "--syntax-report",
        type=Path,
        metavar="FILE",
        help="write AST feature coverage for the discovered corpus",
    )
    parser.add_argument(
        "--syntax-timeout",
        type=positive_integer,
        default=600,
        help="seconds allowed for the complete batch AST profile (default: 600)",
    )
    parser.add_argument(
        "--ngram",
        type=positive_integer,
        default=4,
        help="token length of phrase-continuity probes (default: 4)",
    )
    parser.add_argument(
        "--show",
        type=non_negative_integer,
        default=8,
        help="maximum token and phrase candidates printed per page",
    )
    parser.add_argument(
        "--findings-only",
        action="store_true",
        help="print only REVIEW and HARD pages while retaining the full summary and JSON",
    )
    parser.add_argument(
        "--layout-signals",
        action="store_true",
        help=(
            "include conservative reference-versus-ManT line-boundary, spacing, "
            "and relative-indentation evidence in JSON; it never changes the "
            "content-fidelity result"
        ),
    )
    parser.add_argument(
        "--timeout",
        type=positive_integer,
        default=30,
        help="seconds allowed for each renderer invocation",
    )
    parser.add_argument(
        "--json",
        type=Path,
        metavar="FILE",
        help="also write the complete machine-readable report",
    )
    parser.add_argument(
        "--review-dir",
        type=Path,
        metavar="DIR",
        help=(
            "write decompressed source, reference text, ManT text, and a "
            "manifest for every selected page to this local review bundle"
        ),
    )
    parser.add_argument(
        "--audit-db",
        type=Path,
        metavar="FILE",
        help=(
            "skip unchanged pages already listed in the CSV database and merge "
            "this run into it"
        ),
    )
    parser.add_argument(
        "--checkpoint-every",
        type=positive_integer,
        default=100,
        metavar="PAGES",
        help=(
            "atomically checkpoint --audit-db after this many examined pages "
            "during a long run (default: 100)"
        ),
    )
    parser.add_argument(
        "--corpus",
        metavar="NAME",
        help="stable source name stored in --audit-db (default: fixtures or local-manpath)",
    )
    parser.add_argument(
        "--dedupe-across-corpora",
        action="store_true",
        help=(
            "skip context-independent topic/section sources whose exact bytes "
            "already have a completed record in another corpus"
        ),
    )
    parser.add_argument(
        "--recheck-recorded",
        action="store_true",
        help="audit pages already present in --audit-db instead of skipping them",
    )
    parser.add_argument(
        "--recorded-only",
        action="store_true",
        help="audit only unchanged pages already present in --audit-db",
    )
    parser.add_argument(
        "--retry-skipped",
        action="store_true",
        help="audit only unchanged historical skipped rows from --audit-db",
    )
    parser.add_argument(
        "--pending-only",
        action="store_true",
        help="audit only unchanged rows awaiting human review in --audit-db",
    )
    parser.add_argument(
        "--self-check",
        action="store_true",
        help=argparse.SUPPRESS,
    )
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


def compile_source_patterns(values: Sequence[str] | None) -> list[re.Pattern[str]]:
    patterns = []
    for value in values or []:
        try:
            patterns.append(re.compile(value, re.MULTILINE))
        except re.error as error:
            raise ValueError(f"invalid --source-pattern {value!r}: {error}") from error
    return patterns


def discover_pages(roots: Sequence[Path]) -> list[Path]:
    pages: set[Path] = set()
    for root in roots:
        if not root.is_dir():
            raise ValueError(f"manual root is not a directory: {root}")
        for path in root.rglob("*"):
            if (path.is_file() or path.is_symlink()) and MANUAL_SUFFIX.search(path.name):
                pages.add(path)
    return sorted(pages, key=lambda path: path.as_posix())


def explicit_pages(path: Path, roots: Sequence[Path]) -> list[Path]:
    """Read an exact audit set without loosening the selected manual roots.

    A caller that already selected pages (for example the structure audit) must
    not accidentally re-run a different sample because its own CSV completion
    state differs from the fidelity ledger.  Paths are deliberately absolute
    and must lexically remain below one supplied root; this keeps labels and
    `.so` hierarchy resolution identical to normal discovery.
    """
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


def stable_sample(pages: Sequence[Path], maximum: int, seed: str) -> list[Path]:
    if maximum == 0 or len(pages) <= maximum:
        return list(pages)
    ranked = sorted(
        pages,
        key=lambda path: hashlib.sha256(
            f"{seed}\0{path.as_posix()}".encode("utf-8")
        ).digest(),
    )
    return sorted(ranked[:maximum], key=lambda path: path.as_posix())


def manual_section(path: Path) -> str | None:
    match = MANUAL_SUFFIX.search(path.name)
    return match.group("section") if match is not None else None


def manual_topic(path: Path) -> str | None:
    match = MANUAL_SUFFIX.search(path.name)
    return path.name[: match.start()] if match is not None else None


def manual_hierarchy_root(path: Path, roots: Sequence[Path]) -> Path | None:
    """Return the narrow hierarchy root owning an exact manual leaf.

    A localized page such as ``ROOT/fr/man1/demo.1`` belongs to ``ROOT/fr``.
    Using that root with MANT_MANPATH selects the exact localized leaf while
    keeping ``.so man1/...`` redirects inside the same approved hierarchy.
    """
    section = manual_section(path)
    if section is None:
        return None
    for root in roots:
        try:
            relative = path.relative_to(root)
        except ValueError:
            continue
        parts = relative.parts
        candidates = [
            index
            for index, part in enumerate(parts[:-1])
            if part.startswith("man")
            and part[3:]
            and section.startswith(part[3:])
        ]
        if not candidates:
            return root
        index = candidates[-1]
        return root.joinpath(*parts[:index])
    return None


def stable_sample_by_section(
    pages: Sequence[Path], maximum: int, seed: str
) -> list[Path]:
    grouped: dict[str, list[Path]] = {}
    for path in pages:
        section = manual_section(path)
        if section is not None:
            grouped.setdefault(section, []).append(path)
    sampled = [
        path
        for section in sorted(grouped)
        for path in stable_sample(grouped[section], maximum, f"{seed}\0{section}")
    ]
    return sorted(sampled, key=lambda path: path.as_posix())


def syntax_cache_key(corpus: str, label: str, digest: str) -> tuple[str, str, str]:
    return corpus, label, digest


def read_syntax_cache(
    path: Path | None,
) -> dict[tuple[str, str, str], SyntaxProfile]:
    if path is None or not path.exists():
        return {}
    try:
        if path.name.endswith(".gz"):
            with gzip.open(path, "rt", encoding="utf-8") as source:
                payload = json.load(source)
        else:
            payload = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot read syntax cache {path}: {error}") from error
    if (
        payload.get("version") != SYNTAX_CACHE_VERSION
        or payload.get("profileSchema") != SYNTAX_PROFILE_SCHEMA
    ):
        print(
            f"warning: ignoring incompatible syntax cache {path}; "
            "the profiler feature schema changed",
            file=sys.stderr,
        )
        return {}
    profiles: dict[tuple[str, str, str], SyntaxProfile] = {}
    for number, row in enumerate(payload.get("profiles", []), 1):
        try:
            corpus = row["corpus"]
            label = row["path"]
            digest = row["sourceSha256"]
            features = row["features"]
            diagnostics = row["diagnostics"]
            error = row.get("error")
        except (KeyError, TypeError) as cache_error:
            raise ValueError(
                f"invalid syntax cache entry {number} in {path}"
            ) from cache_error
        if not all(isinstance(value, str) for value in [corpus, label, digest]):
            raise ValueError(f"invalid syntax cache identity at {path}:{number}")
        if not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise ValueError(f"invalid syntax cache digest at {path}:{number}")
        if not isinstance(features, list) or not all(
            isinstance(feature, str) for feature in features
        ):
            raise ValueError(f"invalid syntax features at {path}:{number}")
        if not isinstance(diagnostics, int) or diagnostics < 0:
            raise ValueError(f"invalid diagnostic count at {path}:{number}")
        if error is not None and not isinstance(error, str):
            raise ValueError(f"invalid syntax error at {path}:{number}")
        profiles[syntax_cache_key(corpus, label, digest)] = SyntaxProfile(
            frozenset(features), diagnostics, error
        )
    return profiles


def write_syntax_cache(
    path: Path,
    profiles: dict[tuple[str, str, str], SyntaxProfile],
) -> None:
    rows = []
    for (corpus, label, digest), profile in sorted(profiles.items()):
        rows.append(
            {
                "corpus": corpus,
                "path": label,
                "sourceSha256": digest,
                "features": sorted(profile.features),
                "diagnostics": profile.diagnostics,
                "error": profile.error,
            }
        )
    payload = {
        "tool": "mant-roff-ast-profile-cache",
        "version": SYNTAX_CACHE_VERSION,
        "profileSchema": SYNTAX_PROFILE_SCHEMA,
        "profiles": rows,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    if path.name.endswith(".gz"):
        with gzip.open(temporary, "wt", encoding="utf-8") as destination:
            json.dump(payload, destination, ensure_ascii=False, separators=(",", ":"))
            destination.write("\n")
    else:
        temporary.write_text(
            json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
        )
    temporary.replace(path)


def run_syntax_profile_batch(
    requests: dict[str, str],
    profiler: Path,
    timeout: int,
) -> dict[str, dict[str, object]]:
    if not requests:
        return {}
    try:
        result = subprocess.run(
            [str(profiler)],
            input=("\n".join(requests.values()) + "\n").encode("utf-8"),
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired:
        result = None
    if result is None or result.returncode != 0:
        if len(requests) == 1:
            request_id = next(iter(requests))
            detail = (
                f"profiler timed out after {timeout}s"
                if result is None
                else f"profiler exited with status {result.returncode}"
            )
            return {request_id: {"id": request_id, "error": detail}}
        items = list(requests.items())
        midpoint = len(items) // 2
        return {
            **run_syntax_profile_batch(dict(items[:midpoint]), profiler, timeout),
            **run_syntax_profile_batch(dict(items[midpoint:]), profiler, timeout),
        }

    responses: dict[str, dict[str, object]] = {}
    for number, line in enumerate(result.stdout.splitlines(), 1):
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(
                f"syntax profiler returned invalid JSON on line {number}"
            ) from error
        request_id = response.get("id")
        if (
            response.get("schema") != SYNTAX_PROFILE_SCHEMA
            or not isinstance(request_id, str)
            or request_id not in requests
            or request_id in responses
        ):
            raise ValueError(f"syntax profiler returned an invalid id on line {number}")
        responses[request_id] = response
    for request_id in requests.keys() - responses.keys():
        responses[request_id] = {
            "id": request_id,
            "error": "profiler returned no response",
        }
    return responses


def syntax_profiles(
    pages: Sequence[Path],
    roots: Sequence[Path],
    page_records: dict[Path, tuple[str, str | None]],
    corpus: str,
    profiler: Path,
    timeout: int,
    cache: dict[tuple[str, str, str], SyntaxProfile],
) -> dict[Path, SyntaxProfile]:
    profiles: dict[Path, SyntaxProfile] = {}
    missing: dict[str, tuple[Path, tuple[str, str, str]]] = {}
    for page in pages:
        label, digest = page_records[page]
        if digest is None:
            profiles[page] = SyntaxProfile(
                frozenset(), 0, "source could not be decompressed"
            )
            continue
        key = syntax_cache_key(corpus, label, digest)
        if key in cache:
            profiles[page] = cache[key]
            continue
        request_id = hashlib.sha256("\0".join(key).encode("utf-8")).hexdigest()
        missing[request_id] = (page, key)

    if missing:
        requests: dict[str, str] = {}
        for request_id, (page, _) in missing.items():
            hierarchy_root = manual_hierarchy_root(page, roots)
            if hierarchy_root is None:
                profiles[page] = SyntaxProfile(
                    frozenset({"profile:error"}), 0, "manual hierarchy is unknown"
                )
                continue
            requests[request_id] = json.dumps(
                {
                    "id": request_id,
                    "path": str(page),
                    "root": str(hierarchy_root),
                },
                ensure_ascii=False,
                )
        responses: dict[str, dict[str, object]] = {}
        request_items = list(requests.items())
        for offset in range(0, len(request_items), 256):
            responses.update(
                run_syntax_profile_batch(
                    dict(request_items[offset : offset + 256]), profiler, timeout
                )
            )
        for request_id, (page, key) in missing.items():
            if page in profiles:
                continue
            response = responses.get(request_id)
            if response is None:
                profile = SyntaxProfile(
                    frozenset({"profile:error"}), 0, "profiler returned no response"
                )
            elif isinstance(response.get("error"), str):
                profile = SyntaxProfile(
                    frozenset({"profile:error"}), 0, str(response["error"])
                )
            else:
                features = response.get("features")
                diagnostics = response.get("diagnostics")
                if not isinstance(features, list) or not all(
                    isinstance(feature, str) for feature in features
                ):
                    raise ValueError("syntax profiler returned invalid features")
                if not isinstance(diagnostics, int) or diagnostics < 0:
                    raise ValueError("syntax profiler returned invalid diagnostics")
                profile = SyntaxProfile(frozenset(features), diagnostics)
            profiles[page] = profile
            cache[key] = profile
    return profiles


def feature_counts(
    pages: Iterable[Path], profiles: dict[Path, SyntaxProfile]
) -> Counter[str]:
    return Counter(
        feature
        for page in pages
        for feature in profiles.get(
            page, SyntaxProfile(frozenset({"profile:error"}), 0)
        ).features
    )


def rare_feature_sample(
    pages: Sequence[Path],
    maximum: int,
    seed: str,
    profiles: dict[Path, SyntaxProfile],
    frequencies: Counter[str],
    coverage: Counter[str],
) -> list[Path]:
    if maximum == 0 or len(pages) <= maximum:
        selected = list(pages)
        coverage.update(feature_counts(selected, profiles))
        return selected
    remaining = set(pages)
    selected: list[Path] = []
    while remaining and len(selected) < maximum:
        def score(page: Path) -> tuple[Fraction, Fraction, bytes]:
            features = profiles[page].features
            uncovered = sum(
                (
                    Fraction(syntax_feature_weight(feature), frequencies[feature])
                    for feature in features
                    if not coverage[feature]
                ),
                start=Fraction(),
            )
            balance = sum(
                (
                    Fraction(
                        syntax_feature_weight(feature),
                        frequencies[feature] * (coverage[feature] + 1),
                    )
                    for feature in features
                ),
                start=Fraction(),
            )
            tie = hashlib.sha256(
                f"{seed}\0{page.as_posix()}".encode("utf-8")
            ).digest()
            return uncovered, balance, tie

        chosen = max(remaining, key=score)
        remaining.remove(chosen)
        selected.append(chosen)
        coverage.update(profiles[chosen].features)
    return sorted(selected, key=lambda path: path.as_posix())


def syntax_feature_weight(feature: str) -> int:
    return 3 if feature.startswith("interaction:") else 1


def rare_feature_sample_by_section(
    pages: Sequence[Path],
    maximum: int,
    seed: str,
    profiles: dict[Path, SyntaxProfile],
    frequencies: Counter[str],
    coverage: Counter[str],
) -> list[Path]:
    grouped: dict[str, list[Path]] = {}
    for page in pages:
        section = manual_section(page)
        if section is not None:
            grouped.setdefault(section, []).append(page)
    sampled = [
        page
        for section in sorted(grouped)
        for page in rare_feature_sample(
            grouped[section],
            maximum,
            f"{seed}\0{section}",
            profiles,
            frequencies,
            coverage,
        )
    ]
    return sorted(sampled, key=lambda path: path.as_posix())


def run_renderer(
    command: Sequence[str],
    timeout: int,
    environment: dict[str, str],
    input_bytes: bytes | None = None,
) -> tuple[int, str, str]:
    try:
        result = subprocess.run(
            command,
            input=input_bytes,
            stdin=subprocess.DEVNULL if input_bytes is None else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            env=environment,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return 124, "", f"timed out after {timeout}s"
    return (
        result.returncode,
        result.stdout.decode("utf-8", errors="replace"),
        result.stderr.decode("utf-8", errors="replace"),
    )


def reference_environment() -> dict[str, str]:
    environment = os.environ.copy()
    environment.update(
        {
            "MANWIDTH": "200",
            "MANROFFOPT": "-rHY=0",
            "MANPAGER": "cat",
            "PAGER": "cat",
            "GROFF_NO_SGR": "1",
            "TERM": "dumb",
            "LC_ALL": environment.get("LC_ALL", "C.UTF-8"),
        }
    )
    return environment


def strip_terminal_formatting(value: str) -> str:
    value = ANSI.sub("", value)
    while "\b" in value:
        updated = re.sub(r"[^\n]\x08", "", value)
        if updated == value:
            break
        value = updated
    return value.replace("\r\n", "\n").replace("\r", "\n")


def strip_reference_chrome(value: str) -> str:
    lines = strip_terminal_formatting(value).splitlines()
    visible = [index for index, line in enumerate(lines) if line.strip()]
    if len(visible) >= 3:
        lines[visible[0]] = ""
        lines[visible[-1]] = ""
        for index in visible[:4]:
            if RUNNING_HEADER.fullmatch(lines[index]):
                lines[index] = ""
    return "\n".join(lines)


def unwrap_angle_links(value: str) -> str:
    return ANGLE_LINK.sub(
        lambda match: re.sub(r"[ \t]*\n[ \t]*", "", match.group(1)), value
    )


def normalized_visible_text(value: str) -> str:
    value = strip_terminal_formatting(value).translate(TRANSLATION)
    value = unwrap_angle_links(value)
    value = URL_WRAP.sub(r"\1", value)
    value = DEHYPHENATE.sub("", value)
    value = BORDERS.sub(" ", value)
    return " ".join(value.split())


def tokens(value: str) -> list[str]:
    return TOKEN.findall(normalized_visible_text(value))


def token_lines(value: str) -> list[list[str]]:
    value = strip_terminal_formatting(value).translate(TRANSLATION)
    value = unwrap_angle_links(value)
    value = URL_WRAP.sub(r"\1", value)
    value = DEHYPHENATE.sub("", value)
    value = BORDERS.sub(" ", value)
    return [TOKEN.findall(line) for line in value.splitlines()]


def labeled_mdoc_links(source: str) -> list[tuple[tuple[str, ...], tuple[str, ...]]]:
    """Return ``.Lk`` destinations whose authored label remains visible.

    Mandoc's terminal renderer appends a labelled link's destination after
    the label, while ManT's text projection keeps the destination in the link
    object and prints only its authored label.  This is not a fidelity loss.
    Keep the label tokens as a line-local guard so an unlabelled occurrence of
    the same URL is never ignored.
    """
    links = []
    logical_lines = []
    continued = ""
    for physical_line in source.splitlines():
        continued += physical_line
        if continued.endswith("\\"):
            continued = continued[:-1]
            continue
        logical_lines.append(continued)
        continued = ""
    if continued:
        logical_lines.append(continued)

    for line in logical_lines:
        if not line.startswith((".", "'")):
            continue
        match = re.search(
            r"(?:^[.']Lk|[ \t]Lk)[ \t]+(?:\"([^\"]+)\"|(\S+))(?:[ \t]+(.*))?$",
            line,
        )
        if match is None:
            continue
        destination = match.group(1) or match.group(2)
        label = (match.group(3) or "").strip()
        # A final punctuation operand is formatter syntax, not a link label.
        label = re.sub(r"(?:^|[ \t]+)[.,:;!?]$", "", label).strip()
        # These transparent source escapes change neither the authored label
        # nor mandoc's visible token boundaries.
        label = label.replace(r"\-", "-").replace(r"\&", "")
        label_tokens = tuple(token_key(value) for value in tokens(label))
        destination_tokens = tuple(token_key(value) for value in tokens(destination))
        if destination_tokens and label_tokens:
            links.append((destination_tokens, label_tokens))
    return links


def omit_mandoc_labeled_link_destinations(
    lines: Sequence[Sequence[str]], source: str
) -> list[list[str]]:
    """Drop mandoc-only URL display tokens beside their authored labels."""
    labeled_links = labeled_mdoc_links(source)
    output = []
    for line in lines:
        keys = [token_key(value) for value in line]
        omitted = {
            destination
            for destinations, label in labeled_links
            for destination in destinations
            if all(value in keys for value in destinations)
            and all(value in keys for value in label)
        }
        output.append(
            [value for value in line if token_key(value) not in omitted]
        )
    return output


@dataclass(frozen=True)
class LayoutLine:
    number: int
    indent: int
    key: str


def layout_key(value: str) -> str:
    """Return a conservative whole-line anchor for cross-renderer geometry.

    This intentionally does *not* dehyphenate or unwrap: a line-boundary probe
    needs to see whether one renderer joined source-visible lines that the
    other renderer retained.  Typography and bullet glyphs are normalised only
    enough to let the same short display line align across terminal devices.
    """
    value = value.translate(TRANSLATION)
    value = BORDERS.sub(" ", value)
    value = re.sub(r"\[\s+", "[", value)
    value = re.sub(r"\s+\]", "]", value)
    return " ".join(value.split()).casefold()


def leading_columns(value: str) -> int:
    expanded = value.expandtabs(8)
    return len(expanded) - len(expanded.lstrip(" "))


def no_fill_source_layout(source: str) -> NoFillSourceLayout:
    """Find authored no-fill obligations without adopting a formatter's gutter.

    The reference layout is only actionable when the roff source itself asks
    for line-preserving output.  This intentionally handles the common man and
    mdoc display forms plus their simple inline macro operands; a complex macro
    expansion merely produces no candidate, rather than a misleading claim.
    """
    starts = {"nf", "EX", "DS", "CS", "Vb"}
    ends = {"fi", "EE", "YS", "DE", "CE", "Ve"}
    inline_macros = {
        "B",
        "BI",
        "BR",
        "Cm",
        "Em",
        "I",
        "IB",
        "IR",
        "Li",
        "Nm",
        "Op",
        "RI",
        "RB",
        "S",
        "SM",
        "SY",
    }
    depth = 0
    anchors: set[str] = set()
    indent_observations: dict[str, set[int]] = {}
    spaced_pairs: set[tuple[str, str]] = set()
    consecutive_pairs: set[tuple[str, str]] = set()
    source_lines: set[str] = set()
    flowed_pairs: set[tuple[str, str]] = set()
    previous: str | None = None
    flowed_previous: str | None = None
    pending_space = False
    for raw_line in source.splitlines():
        match = ROFF_REQUEST.fullmatch(raw_line)
        name = match.group("name") if match else None
        arguments = match.group("args") if match and match.group("args") else ""
        if name == "Bd" and "-literal" in arguments.split():
            depth += 1
            previous = None
            flowed_previous = None
            pending_space = False
            continue
        if name in starts:
            depth += 1
            previous = None
            flowed_previous = None
            pending_space = False
            continue
        if name == "Ed" or name in ends:
            depth = max(0, depth - 1)
            previous = None
            flowed_previous = None
            pending_space = False
            continue
        if not raw_line.strip() or name == "sp":
            if depth == 0:
                flowed_previous = None
            else:
                pending_space |= previous is not None
            continue
        if raw_line.lstrip().startswith((r'.\\"', r"'\\\"")):
            if depth == 0:
                flowed_previous = None
            continue
        candidate = arguments if name in inline_macros else raw_line if name is None else ""
        if name in inline_macros:
            # Macro arguments frequently quote an entire visible literal row.
            # Keep the row comparable with terminal text while leaving escaped
            # quotes untouched.
            candidate = re.sub(r'(?<!\\)"([^"]*)"', r"\1", candidate)
        candidate = ROFF_FONT_ESCAPE.sub("", candidate)
        candidate = candidate.replace(r"\&", "").replace(r"\~", " ")
        key = layout_key(candidate)
        if depth == 0:
            if useful_layout_anchor(key):
                source_lines.add(key)
                if flowed_previous is not None:
                    flowed_pairs.add((flowed_previous, key))
                flowed_previous = key
            elif name is not None:
                flowed_previous = None
            continue
        authored_indent = 0 if name in inline_macros else leading_columns(candidate)
        if useful_layout_anchor(key):
            anchors.add(key)
            source_lines.add(key)
            indent_observations.setdefault(key, set()).add(authored_indent)
            if previous is not None:
                pair = (previous, key)
                consecutive_pairs.add(pair)
                if pending_space:
                    spaced_pairs.add(pair)
            previous = key
            pending_space = False
    return NoFillSourceLayout(
        anchors=frozenset(anchors),
        authored_indents={
            key: next(iter(indents))
            for key, indents in indent_observations.items()
            if len(indents) == 1
        },
        spaced_pairs=frozenset(spaced_pairs),
        consecutive_pairs=frozenset(consecutive_pairs),
        source_lines=frozenset(source_lines),
        flowed_pairs=frozenset(flowed_pairs),
    )


def layout_lines(value: str) -> tuple[list[LayoutLine], int, int]:
    """Return nonblank terminal lines plus blank-line-run observations."""
    output: list[LayoutLine] = []
    blank_lines = 0
    blank_runs = 0
    in_blank_run = False
    for number, raw_line in enumerate(strip_terminal_formatting(value).splitlines(), 1):
        line = raw_line.expandtabs(8)
        stripped = line.lstrip(" ")
        if not stripped:
            blank_lines += 1
            if not in_blank_run:
                blank_runs += 1
                in_blank_run = True
            continue
        in_blank_run = False
        output.append(LayoutLine(number, len(line) - len(stripped), layout_key(stripped)))
    return output, blank_runs, blank_lines


def useful_layout_anchor(value: str) -> bool:
    # Avoid headings like "NAME" and generic fragments such as "the".  A
    # unique, sufficiently long whole line is a reliable alignment point even
    # when the surrounding prose has been wrapped differently.
    return len(value) >= 8 and sum(character.isalnum() for character in value) >= 4


def modal_indent(values: Sequence[int]) -> int:
    if not values:
        return 0
    counts = Counter(values)
    return min(counts, key=lambda value: (-counts[value], value))


def layout_profile(lines: Sequence[LayoutLine], blank_runs: int, blank_lines: int) -> LayoutProfile:
    indents = sorted({line.indent for line in lines})
    return LayoutProfile(
        nonblank_lines=len(lines),
        blank_line_runs=blank_runs,
        blank_lines=blank_lines,
        max_indent=max(indents, default=0),
        indent_levels=indents,
    )


def layout_comparison(reference: str, mant: str, source: str | None) -> LayoutComparison:
    """Extract conservative layout candidates without treating groff as law.

    `man(1)`/groff normally supplies a page-wide body indentation and wraps
    prose at a device width, while ManT deliberately emits reflow-free semantic
    text.  The comparison therefore aligns only unique whole-line anchors,
    derives each renderer's local body baseline, and reports *relative*
    indentation collapse, adjacent blank-gap divergence, and a short-line
    merge that is exact after whitespace normalisation.  All observations are
    review evidence rather than automatic fidelity failures.
    """
    reference_lines, reference_runs, reference_blanks = layout_lines(reference)
    mant_lines, mant_runs, mant_blanks = layout_lines(mant)
    source_layout = no_fill_source_layout(source) if source is not None else None
    no_fill_anchors = source_layout.anchors if source_layout is not None else frozenset()
    reference_by_key: dict[str, list[LayoutLine]] = {}
    mant_by_key: dict[str, list[LayoutLine]] = {}
    for line in reference_lines:
        if useful_layout_anchor(line.key):
            reference_by_key.setdefault(line.key, []).append(line)
    for line in mant_lines:
        if useful_layout_anchor(line.key):
            mant_by_key.setdefault(line.key, []).append(line)
    pairs = [
        (reference_by_key[key][0], mant_by_key[key][0])
        for key in reference_by_key.keys() & mant_by_key.keys()
        if len(reference_by_key[key]) == len(mant_by_key[key]) == 1
    ]
    pairs.sort(key=lambda pair: pair[0].number)
    reference_positive = [line.indent for line, _ in pairs if line.indent > 0]
    reference_baseline = modal_indent(reference_positive)
    mant_baseline = modal_indent(
        [candidate.indent for line, candidate in pairs if line.indent == reference_baseline]
    )
    candidates: list[str] = []

    collapsed = [
        (reference_line, mant_line)
        for reference_line, mant_line in pairs
        if source_layout is not None
        and source_layout.authored_indents.get(reference_line.key, 0) >= 2
        and reference_line.indent >= source_layout.authored_indents[reference_line.key]
        and mant_line.indent < source_layout.authored_indents[reference_line.key]
    ]
    if collapsed:
        samples = ", ".join(
            f"{reference_line.key[:48]!r} (source +{source_layout.authored_indents[reference_line.key]}, "
            f"reference={reference_line.indent}, ManT={mant_line.indent})"
            for reference_line, mant_line in collapsed[:3]
        )
        candidates.append(
            f"authored relative indentation may collapse for {len(collapsed)} aligned line(s): {samples}"
        )

    common_unique = {reference_line.key for reference_line, _ in pairs}
    mant_positions = {
        line.key: index
        for index, line in enumerate(mant_lines)
        if line.key in common_unique
    }
    spacing = []
    for reference_first, reference_second in zip(reference_lines, reference_lines[1:]):
        if (
            reference_first.key not in common_unique
            or reference_second.key not in common_unique
            or reference_first.key not in mant_positions
            or reference_second.key not in mant_positions
        ):
            continue
        mant_first_index = mant_positions[reference_first.key]
        mant_second_index = mant_positions[reference_second.key]
        if mant_second_index != mant_first_index + 1:
            continue
        reference_gap = reference_second.number - reference_first.number - 1
        mant_first = mant_lines[mant_first_index]
        mant_second = mant_lines[mant_second_index]
        mant_gap = mant_second.number - mant_first.number - 1
        if (
            reference_gap != mant_gap
            and source_layout is not None
            and (reference_first.key, reference_second.key) in source_layout.spaced_pairs
        ):
            spacing.append((reference_first.key, reference_gap, mant_gap))
    if spacing:
        samples = ", ".join(
            f"{key[:40]!r} (reference blank lines={reference_gap}, ManT={mant_gap})"
            for key, reference_gap, mant_gap in spacing[:3]
        )
        candidates.append(
            f"adjacent aligned lines have {len(spacing)} spacing divergence(s): {samples}"
        )

    mant_whole_lines = {line.key for line in mant_lines if useful_layout_anchor(line.key)}
    merged = []
    for first, second in zip(reference_lines, reference_lines[1:]):
        if (
            useful_layout_anchor(first.key)
            and useful_layout_anchor(second.key)
            and source_layout is not None
            and (first.key, second.key) in source_layout.consecutive_pairs
            and f"{first.key} {second.key}" not in source_layout.source_lines
            and (first.key, second.key) not in source_layout.flowed_pairs
            and len(first.key) <= 96
            and len(second.key) <= 96
            and f"{first.key} {second.key}" in mant_whole_lines
        ):
            merged.append((first.key, second.key))
    if merged:
        samples = ", ".join(
            f"{first[:32]!r} + {second[:32]!r}" for first, second in merged[:3]
        )
        candidates.append(
            f"reference line boundaries may merge in ManT for {len(merged)} short pair(s): {samples}"
        )

    return LayoutComparison(
        reference=layout_profile(reference_lines, reference_runs, reference_blanks),
        mant=layout_profile(mant_lines, mant_runs, mant_blanks),
        shared_anchors=len(pairs),
        reference_baseline_indent=reference_baseline if pairs else None,
        mant_baseline_indent=mant_baseline if pairs else None,
        candidates=candidates,
    )


def token_key(value: str) -> str:
    # Sentence punctuation is not a semantic part of a token, and reference
    # renderers disagree about whether punctuation abuts inline markup. Keep
    # dots, slashes, and colons inside a token so qualified names and URLs stay
    # useful discovery probes.
    return value.casefold().rstrip(".:/").replace("-", "")


def reference_font_escape_key(value: str) -> str | None:
    """Return the token behind a leaked groff font escape such as ``fBgit``."""
    if len(value) > 3 and value[0] == "f" and value[1] in "BIRP":
        return token_key(value[2:])
    return None


def reference_glued_words(value: str, mine_keys: set[str]) -> bool:
    """Recognize two ManT tokens glued together by reference layout."""
    return any(
        token_key(value[:offset]) in mine_keys
        and token_key(value[offset:]) in mine_keys
        for offset in range(1, len(value))
    )


def missing_token_candidates(reference: Sequence[str], mine: Sequence[str]) -> list[str]:
    mine_keys = {token_key(value) for value in mine}
    output: list[str] = []
    seen: set[str] = set()
    for value in reference:
        key = token_key(value)
        escaped_key = reference_font_escape_key(value)
        if escaped_key is not None and escaped_key in mine_keys:
            continue
        if reference_glued_words(value, mine_keys):
            continue
        if key not in mine_keys and key not in seen:
            output.append(value)
            seen.add(key)
    return output


def appears_with_small_insertions(
    phrase: tuple[str, ...], mine: Sequence[str], maximum_insertions: int = 2
) -> bool:
    """Accept a reference phrase separated only by a few ManT-owned tokens.

    Markdown link targets and structural labels may be visible in ManT while a
    terminal man renderer omits them. Those additions are worth inspecting in
    the rendered output, but they should not flood the fidelity audit with
    overlapping broken-phrase candidates.
    """
    maximum_width = len(phrase) + maximum_insertions
    for start, value in enumerate(mine):
        if value != phrase[0]:
            continue
        cursor = 1
        for candidate in mine[start + 1 : start + maximum_width]:
            if candidate == phrase[cursor]:
                cursor += 1
                if cursor == len(phrase):
                    return True
    return False


def broken_phrase_candidates(
    reference_lines: Sequence[Sequence[str]], mine: Sequence[str], width: int
) -> list[str]:
    if len(mine) < width:
        return []
    mine_keys = [token_key(value) for value in mine]
    mine_tokens = set(mine_keys)
    mine_phrases = {
        tuple(mine_keys[index : index + width])
        for index in range(len(mine_keys) - width + 1)
    }
    output: list[str] = []
    seen: set[tuple[str, ...]] = set()
    for line in reference_lines:
        for index in range(len(line) - width + 1):
            display = line[index : index + width]
            key = tuple(token_key(value) for value in display)
            if (
                key in mine_phrases
                or key in seen
                or not all(value in mine_tokens for value in key)
                or appears_with_small_insertions(key, mine_keys)
            ):
                continue
            output.append(" ".join(display))
            seen.add(key)
    return output


def fidelity_signatures(value: str) -> tuple[list[str], list[str]]:
    hard: list[str] = []
    review: list[str] = []
    if UNICODE_ESCAPE.search(value):
        # Manuals about roff deliberately print Unicode escape examples. A
        # visible escape is evidence to inspect, not proof that the parser
        # leaked control syntax.
        review.append("bracketed Unicode escape is visible; verify documented syntax")
    if INTERNAL_MARKER.search(value):
        hard.append("internal roff marker leaked")
    if GLUED_MARKER.search(value):
        review.append("list or enumeration marker may be glued to following text")
    return hard, review


def differential_signatures(
    source: str, reference: str, mant: str
) -> list[str]:
    """Return source-conditioned rendering differences worth human review.

    Token presence deliberately ignores punctuation and whitespace. These
    probes cover formatter-owned mdoc syntax where those characters carry
    semantics, while keeping the general comparison tolerant of wrapping and
    layout differences.
    """
    review = []
    reference = strip_terminal_formatting(reference)
    mant = strip_terminal_formatting(mant)
    reference_visible_lines = [
        " ".join(line.split()) for line in reference.splitlines() if line.strip()
    ]
    mant_visible_lines = [
        " ".join(line.split()) for line in mant.splitlines() if line.strip()
    ]
    leaked_controls = []
    for source_line in source.splitlines():
        if not re.match(r"^[.'](?:if|ie|while)(?:[ \t]|$)", source_line):
            continue
        # A selected one-line conditional is reparsed as roff input.  Inspect
        # every control-looking suffix because nested conditionals can contain
        # another request (for example `.if ... .if ... .nr ...`).
        cursor = 0
        while match := INLINE_ROFF_CONTROL.search(source_line, cursor):
            candidate = " ".join(match.group("body").split())
            if (
                any(candidate in line for line in mant_visible_lines)
                and not any(candidate in line for line in reference_visible_lines)
                and not any(candidate in existing for existing in leaked_controls)
            ):
                leaked_controls.append(candidate)
            cursor = match.start("body") + 1
    for candidate in leaked_controls[:3]:
        review.append(
            "selected conditional leaked an authored roff control line: "
            f"{candidate!r}"
        )
    if MDOC_NAME_DESCRIPTION.search(source):
        reference_attached = len(EM_DASH_ATTACHED_TO_WORD.findall(reference))
        mant_attached = len(EM_DASH_ATTACHED_TO_WORD.findall(mant))
        if mant_attached > reference_attached:
            review.append(
                "mdoc Nd separator is attached to its description "
                f"(reference={reference_attached}, mant={mant_attached})"
            )
    if MDOC_FUNCTION_DECLARATION.search(source):
        reference_terminators = reference.count(");")
        mant_terminators = mant.count(");")
        if reference_terminators > mant_terminators:
            review.append(
                "mdoc synopsis function terminators may be missing "
                f"(reference={reference_terminators}, mant={mant_terminators})"
            )
    if MDOC_MULTI_OPERAND_FA.search(source):
        reference_commas = reference.count(",")
        mant_commas = mant.count(",")
        if reference_commas > mant_commas:
            review.append(
                "mdoc multi-operand Fa separators may be missing "
                f"(reference={reference_commas}, mant={mant_commas})"
            )
    return review


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


def filter_pages_by_source(
    pages: Sequence[Path], patterns: Sequence[re.Pattern[str]]
) -> tuple[list[Path], list[Path]]:
    if not patterns:
        return list(pages), []
    selected = []
    unreadable = []
    for path in pages:
        raw = source_bytes(path)
        if raw is None:
            unreadable.append(path)
            continue
        source = raw.decode("utf-8", errors="replace")
        if all(pattern.search(source) for pattern in patterns):
            selected.append(path)
    return selected, unreadable


def source_digest(path: Path) -> str | None:
    source = source_bytes(path)
    return hashlib.sha256(source).hexdigest() if source is not None else None


def source_audit_identity(label: str, digest: str) -> tuple[str, str, str] | None:
    path = Path(label)
    topic = manual_topic(path)
    section = manual_section(path)
    if topic is None or section is None:
        return None
    return digest, topic, section


def source_is_context_independent(path: Path) -> bool:
    source = source_bytes(path)
    return source is not None and source_bytes_are_context_independent(source)


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


def source_bytes_are_context_independent(source: bytes) -> bool:
    return EXTERNAL_ROFF_CONTEXT.search(source) is None


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


def contains_so_request(path: Path) -> bool:
    """Return whether a page needs an approved hierarchy for a .so request."""
    source = source_bytes(path)
    if source is None:
        return False
    return any(
        re.match(rb"^[.']so(?:[ \t]|$)", line) is not None
        for line in source.splitlines()
    )


def contains_external_request(source: bytes) -> bool:
    return any(
        re.match(rb"^[.'](?:so|mso)(?:[ \t]|$)", line) is not None
        for line in source.splitlines()
    )


def redirect_only_target(source: bytes) -> str | None:
    """Return the sole relative target of a redirect-only roff source.

    mandoc intentionally reports a top-level ``.so`` request instead of
    following it. Resolve only the unambiguous alias shape used by manual
    hierarchies; embedded includes remain non-comparable rather than growing a
    second roff interpreter in this audit script.
    """
    meaningful = []
    for raw_line in source.splitlines():
        line = raw_line.strip()
        if not line or line.startswith((b'.\\"', b"'\\\"")):
            continue
        meaningful.append(line)
    if len(meaningful) != 1:
        return None
    match = re.fullmatch(rb"[.']so[ \t]+([^ \t\x00]+)[ \t]*", meaningful[0])
    if match is None:
        return None
    return match.group(1).decode("utf-8", errors="surrogateescape")


def confined_redirect_target(
    path: Path,
    hierarchy_root: Path,
    target: str,
) -> Path | None:
    """Resolve one mandoc alias target inside its owning manual hierarchy."""
    logical = PurePosixPath(target)
    if (
        logical.is_absolute()
        or not logical.parts
        or any(part in {"", ".", ".."} for part in logical.parts)
        or "\\" in target
    ):
        return None
    base = hierarchy_root if logical.parts[0].startswith("man") else path.parent
    candidate = base.joinpath(*logical.parts)
    try:
        candidate.relative_to(hierarchy_root)
    except ValueError:
        return None
    candidates = [candidate]
    if not candidate.name.endswith((".gz", ".bz2", ".xz", ".zst")):
        candidates.extend(
            candidate.with_name(f"{candidate.name}{suffix}")
            for suffix in (".gz", ".bz2", ".xz", ".zst")
        )
    return next(
        (value for value in candidates if value.is_file() or value.is_symlink()),
        None,
    )


def mant_render_command(
    path: Path, roots: Sequence[Path], mant: Path
) -> tuple[list[str], Path | None]:
    """Select standalone or indexed rendering without weakening --input.

    Any source containing a .so request needs an approved manual hierarchy.
    Redirect-only aliases then resolve normally; unsupported embedded includes
    become visible hard failures instead of silently leaving the audit corpus.
    """
    if contains_so_request(path):
        section = manual_section(path)
        topic = manual_topic(path)
        root = manual_hierarchy_root(path, roots)
        if section is not None and topic is not None and root is not None:
            return (
                [
                    str(mant),
                    f"manual/{section}/{topic}",
                    "--manual",
                    "--format",
                    "man",
                ],
                root,
            )
    return (
        [
            str(mant),
            "--input",
            str(path),
            "--input-format",
            "roff",
            "--format",
            "man",
        ],
        None,
    )


def reference_render_command(
    path: Path,
    reference: str,
    reference_kind: str,
    hierarchy_root: Path | None,
    source: bytes | None,
) -> tuple[list[str] | None, bytes | None, str | None]:
    """Build a renderer invocation or explain why the source is not comparable."""
    if reference_kind == "mandoc":
        reference_path = path
        if source is not None and contains_external_request(source):
            target = redirect_only_target(source)
            if target is None:
                return None, None, "mandoc does not expand embedded .so/.mso requests"
            if hierarchy_root is None:
                return None, None, "redirect-only page has no owning manual hierarchy"
            resolved = confined_redirect_target(path, hierarchy_root, target)
            if resolved is None:
                return None, None, f"redirect target is absent or outside the hierarchy: {target}"
            reference_path = resolved
        reference_source = source_bytes(reference_path)
        if reference_source is None:
            return None, None, f"cannot decompress mandoc reference source: {reference_path}"
        return [reference, "-T", "utf8", "-O", "width=200"], reference_source, None
    if hierarchy_root is not None:
        section = manual_section(path)
        topic = manual_topic(path)
        if section is not None and topic is not None:
            return [reference, section, topic], None, None
    return [reference, "-l", str(path)], None, None


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


def audit_page(
    path: Path,
    label: str,
    roots: Sequence[Path],
    mant: Path,
    reference: str,
    reference_kind: str,
    timeout: int,
    ngram: int,
    layout_signals: bool,
) -> AuditArtifact:
    environment = reference_environment()
    raw_source = source_bytes(path)
    mant_command, hierarchy_root = mant_render_command(path, roots, mant)
    if hierarchy_root is not None:
        environment["MANT_MANPATH"] = str(hierarchy_root)
        # GNU man resolves redirect-only `.so` pages through MANPATH even
        # with `-l`. Keep the reference on the same localized hierarchy as
        # ManT instead of accidentally comparing a translated alias with the
        # default-language target.
        environment["MANPATH"] = str(hierarchy_root)
    reference_command, reference_input, skip_detail = reference_render_command(
        path,
        reference,
        reference_kind,
        hierarchy_root,
        raw_source,
    )
    if reference_command is None:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="skipped",
                signatures=["source is not comparable with the selected reference"],
                detail=skip_detail,
            ),
            source=raw_source,
            reference_output=None,
            mant_output=None,
        )
    mant_status, mant_output, mant_error = run_renderer(
        mant_command,
        timeout,
        environment,
    )
    if mant_status != 0:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                signatures=["ManT failed to render the page"],
                detail=mant_error.strip() or f"exit status {mant_status}",
            ),
            source=raw_source,
            reference_output=None,
            mant_output=mant_output,
        )

    reference_status, reference_output, reference_error = run_renderer(
        reference_command, timeout, environment, reference_input
    )
    if reference_status != 0:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                signatures=["reference renderer failed; page was not compared"],
                detail=reference_error.strip() or f"reference exit status {reference_status}",
            ),
            source=raw_source,
            reference_output=reference_output,
            mant_output=mant_output,
        )

    reference_output = strip_reference_chrome(reference_output)
    reference_lines = token_lines(reference_output)
    if reference_kind == "mandoc" and raw_source is not None:
        reference_lines = omit_mandoc_labeled_link_destinations(
            reference_lines,
            raw_source.decode("utf-8", errors="replace"),
        )
    reference_tokens = [value for line in reference_lines for value in line]
    mant_tokens = tokens(mant_output)
    if not reference_tokens:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                reference_tokens=len(reference_tokens),
                mant_tokens=len(mant_tokens),
                signatures=["reference renderer produced no comparable visible tokens"],
                detail="the page cannot be classified as clean without a reference corpus",
            ),
            source=raw_source,
            reference_output=reference_output,
            mant_output=mant_output,
        )
    if not mant_tokens:
        return AuditArtifact(
            finding=Finding(
                path=label,
                status="hard-failure",
                reference_tokens=len(reference_tokens),
                mant_tokens=0,
                signatures=["ManT produced no comparable visible tokens"],
                detail="the reference renderer produced visible content",
            ),
            source=raw_source,
            reference_output=reference_output,
            mant_output=mant_output,
        )

    missing = missing_token_candidates(reference_tokens, mant_tokens)
    phrases = broken_phrase_candidates(reference_lines, mant_tokens, ngram)
    hard_signatures, review_signatures = fidelity_signatures(mant_output)
    if raw_source is not None:
        review_signatures.extend(
            differential_signatures(
                raw_source.decode("utf-8", errors="replace"),
                reference_output,
                mant_output,
            )
        )
    if not mant_output.strip():
        hard_signatures.append("ManT produced empty output")
    signatures = hard_signatures + review_signatures
    status = "hard-failure" if hard_signatures else "review" if missing or phrases or review_signatures else "clean"
    layout = (
        layout_comparison(
            reference_output,
            mant_output,
            raw_source.decode("utf-8", errors="replace") if raw_source is not None else None,
        )
        if layout_signals
        else None
    )
    return AuditArtifact(
        finding=Finding(
            path=label,
            status=status,
            reference_tokens=len(reference_tokens),
            mant_tokens=len(mant_tokens),
            missing_tokens=missing,
            broken_phrases=phrases,
            signatures=signatures,
            layout=layout,
        ),
        source=raw_source,
        reference_output=reference_output,
        mant_output=mant_output,
    )


def print_finding(finding: Finding, show: int) -> None:
    badge = {
        "clean": "CLEAN",
        "review": "REVIEW",
        "hard-failure": "HARD",
        "skipped": "SKIP",
    }[finding.status]
    print(
        f"{badge:6} {finding.path} "
        f"[tokens: reference={finding.reference_tokens}, mant={finding.mant_tokens}]"
    )
    if finding.detail:
        print(f"       detail: {finding.detail}")
    if finding.signatures:
        for signature in finding.signatures:
            print(f"       signature: {signature}")
    if finding.missing_tokens:
        values = ", ".join(finding.missing_tokens[:show])
        suffix = " …" if len(finding.missing_tokens) > show else ""
        print(f"       missing tokens ({len(finding.missing_tokens)}): {values}{suffix}")
    if finding.broken_phrases:
        print(f"       broken phrases ({len(finding.broken_phrases)}):")
        for phrase in finding.broken_phrases[:show]:
            print(f"         - {phrase}")
        if len(finding.broken_phrases) > show:
            print("         - …")
    if finding.layout and finding.layout.candidates:
        print(f"       layout anchors: {finding.layout.shared_anchors}")
        for candidate in finding.layout.candidates:
            print(f"       layout: {candidate}")


def update_summary(summary: AuditSummary, finding: Finding) -> None:
    summary.examined += 1
    if finding.status == "clean":
        summary.clean += 1
    elif finding.status == "review":
        summary.review += 1
    elif finding.status == "hard-failure":
        summary.hard_failures += 1
    else:
        summary.skipped += 1


def write_json_report(
    path: Path,
    roots: Sequence[Path],
    mant: Path,
    reference: str,
    reference_kind: str,
    reference_id: str,
    layout_signals: bool,
    summary: AuditSummary,
    findings: Sequence[Finding],
) -> None:
    report = {
        "tool": "mant-roff-fidelity-audit/v1",
        "roots": [str(root) for root in roots],
        "mant": str(mant),
        "reference": reference,
        "referenceKind": reference_kind,
        "referenceId": reference_id,
        "layout_signals": layout_signals,
        "summary": asdict(summary),
        "findings": [asdict(finding) for finding in findings],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def review_bundle_name(label: str) -> str:
    """Return one stable, path-safe directory name for a reviewed page."""
    leaf = re.sub(r"[^A-Za-z0-9._-]+", "-", Path(label).name).strip(".-")
    readable = leaf or "manual"
    digest = hashlib.sha256(label.encode("utf-8")).hexdigest()[:16]
    return f"{readable}-{digest}"


def write_review_bundle(
    directory: Path,
    artifacts: Sequence[tuple[str, AuditArtifact]],
) -> None:
    """Write local source/render evidence without changing the audit ledger.

    The bundle is intentionally opt-in and is not a fixture format: it may
    contain third-party manuals and renderer-specific text.  A stable hash of
    the logical path avoids interpreting a source label as a filesystem path.
    """
    pages = directory / "pages"
    pages.mkdir(parents=True, exist_ok=True)
    entries = []
    for label, artifact in sorted(artifacts, key=lambda value: value[0]):
        name = review_bundle_name(label)
        page_directory = pages / name
        page_directory.mkdir(parents=True, exist_ok=True)
        files: dict[str, str] = {}
        if artifact.source is not None:
            (page_directory / "source.roff").write_bytes(artifact.source)
            files["source"] = f"pages/{name}/source.roff"
        if artifact.reference_output is not None:
            (page_directory / "reference.txt").write_text(
                artifact.reference_output,
                encoding="utf-8",
            )
            files["reference"] = f"pages/{name}/reference.txt"
        if artifact.mant_output is not None:
            (page_directory / "mant.txt").write_text(
                artifact.mant_output,
                encoding="utf-8",
            )
            files["mant"] = f"pages/{name}/mant.txt"
        (page_directory / "finding.json").write_text(
            json.dumps(asdict(artifact.finding), indent=2, ensure_ascii=False) + "\n",
            encoding="utf-8",
        )
        files["finding"] = f"pages/{name}/finding.json"
        entries.append(
            {
                "path": label,
                "status": artifact.finding.status,
                "files": files,
            }
        )
    manifest = {
        "tool": "mant-roff-fidelity-review/v1",
        "pages": entries,
    }
    (directory / "manifest.json").write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n",
        encoding="utf-8",
    )


def write_syntax_report(
    path: Path,
    roots: Sequence[Path],
    corpus: str,
    all_pages: Sequence[Path],
    recorded_pages: set[Path],
    cross_corpus_pages: dict[Path, AuditRecord],
    selected_pages: Sequence[Path],
    page_records: dict[Path, tuple[str, str | None]],
    profiles: dict[Path, SyntaxProfile],
) -> None:
    corpus_counts = feature_counts(all_pages, profiles)
    covered_pages = recorded_pages | set(cross_corpus_pages)
    recorded_counts = feature_counts(covered_pages, profiles)
    selected_counts = feature_counts(selected_pages, profiles)
    examples: dict[str, list[str]] = {}
    for page in all_pages:
        label, _ = page_records[page]
        for feature in profiles[page].features:
            values = examples.setdefault(feature, [])
            if len(values) < 3:
                values.append(label)
    features = [
        {
            "feature": feature,
            "corpusPages": corpus_counts[feature],
            "recordedPages": recorded_counts[feature],
            "selectedPages": selected_counts[feature],
            "examples": examples.get(feature, []),
        }
        for feature in sorted(corpus_counts)
    ]
    errors = [
        {
            "path": page_records[page][0],
            "error": profiles[page].error,
        }
        for page in all_pages
        if profiles[page].error is not None
    ]
    unique_sources = {
        digest for _, digest in page_records.values() if digest is not None
    }
    payload = {
        "tool": "mant-roff-syntax-coverage/v2",
        "corpus": corpus,
        "roots": [str(root) for root in roots],
        "summary": {
            "corpusPages": len(all_pages),
            "uniqueSources": len(unique_sources),
            "recordedPages": len(recorded_pages),
            "reusedCrossCorpusPages": len(cross_corpus_pages),
            "coveredPages": len(covered_pages),
            "selectedPages": len(selected_pages),
            "features": len(corpus_counts),
            "recordedFeatures": sum(
                1 for feature in corpus_counts if recorded_counts[feature]
            ),
            "selectedNewFeatures": sum(
                1
                for feature in corpus_counts
                if not recorded_counts[feature] and selected_counts[feature]
            ),
            "uncoveredFeaturesAfterSelection": sum(
                1
                for feature in corpus_counts
                if not recorded_counts[feature] and not selected_counts[feature]
            ),
            "profileErrors": len(errors),
        },
        "reusedCrossCorpusSources": [
            {
                "path": page_records[page][0],
                "fromCorpus": record.corpus,
                "fromPath": record.path,
            }
            for page, record in sorted(
                cross_corpus_pages.items(),
                key=lambda item: page_records[item[0]][0],
            )
        ],
        "selectedPaths": [page_records[page][0] for page in selected_pages],
        "features": features,
        "errors": errors,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


def self_check() -> None:
    digest = "0" * 64
    assert source_audit_identity("share/man/man1/git.1.gz", digest) == (
        digest,
        "git",
        "1",
    )
    assert not source_bytes_are_context_independent(b".so man1/git.1\n")
    assert not source_bytes_are_context_independent(b".if n .mso fallback.tmac\n")
    assert source_bytes_are_context_independent(b".SH DESCRIPTION\nordinary text\n")
    assert strip_reference_chrome(
        "delim $$\nMM2GV(1) General Commands Manual MM2GV(1)\ncontent\nfooter\n"
    ).strip() == "content"
    assert normalized_visible_text(
        "read <https://example.test/api/\nversion.3.html> now"
    ) == "read https://example.test/api/version.3.html now"
    assert token_key("line-break") == token_key("linebreak")
    assert manual_section(Path("git.1.gz")) == "1"
    assert manual_section(Path("SSL_read.3ssl")) == "3ssl"
    assert manual_topic(Path("SSL_read.3ssl.gz")) == "SSL_read"
    assert manual_hierarchy_root(
        Path("/usr/share/man/fr/man3/printf.3.gz"),
        [Path("/usr/share/man")],
    ) == Path("/usr/share/man/fr")
    assert manual_hierarchy_root(
        Path("/usr/share/man/man3/printf.3bsd.gz"),
        [Path("/usr/share/man")],
    ) == Path("/usr/share/man")
    assert relative_label(
        Path("/opt/tools/share/man/man1/demo.1"),
        [Path("/opt/tools/man"), Path("/opt/tools/share/man")],
    ) == "share/man/man1/demo.1"
    assert source_digest(Path(__file__)) is not None
    profile_a = SyntaxProfile(frozenset({"common", "rare-a"}), 0)
    profile_b = SyntaxProfile(frozenset({"common"}), 0)
    profile_c = SyntaxProfile(frozenset({"common", "rare-c"}), 0)
    sample_pages = [Path("a.1"), Path("b.1"), Path("c.1")]
    sample_profiles = {
        sample_pages[0]: profile_a,
        sample_pages[1]: profile_b,
        sample_pages[2]: profile_c,
    }
    sample_frequencies = feature_counts(sample_pages, sample_profiles)
    sample_coverage = Counter({"common": 1, "rare-a": 1})
    assert rare_feature_sample(
        sample_pages,
        1,
        "self-check",
        sample_profiles,
        sample_frequencies,
        sample_coverage,
    ) == [Path("c.1")]
    interaction_profiles = {
        sample_pages[0]: SyntaxProfile(frozenset({"common", "rare-atomic"}), 0),
        sample_pages[1]: SyntaxProfile(
            frozenset({"common", "interaction:macro:SY+flag:no-fill"}), 0
        ),
    }
    interaction_frequencies = feature_counts(
        sample_pages[:2], interaction_profiles
    )
    assert rare_feature_sample(
        sample_pages[:2],
        1,
        "interaction-self-check",
        interaction_profiles,
        interaction_frequencies,
        Counter({"common": 1}),
    ) == [Path("b.1")]
    assert token_key("alloca.") == token_key("alloca")
    assert token_key("docs.example/path") != token_key("docs.example")
    assert tokens("one line-\nbreak here") == ["one", "linebreak", "here"]
    assert missing_token_candidates(["alpha", "missing"], ["alpha"]) == ["missing"]
    assert missing_token_candidates(["fBpackage.json"], ["package.json"]) == []
    assert missing_token_candidates(["defsReport"], ["defs", "Report"]) == []
    assert missing_token_candidates(["PATHList"], ["PATH", "List"]) == []
    assert missing_token_candidates(["CPANCPAN"], ["CPAN"]) == []
    assert tokens("tr//\nA new feature") == ["tr//", "new", "feature"]
    assert tokens("https://example.test/path/\nW3C title") == [
        "https://example.test/path/",
        "W3C",
        "title",
    ]
    assert tokens("https://example.test/path/\nnext-part") == [
        "https://example.test/path/next-part"
    ]
    assert tokens("https://example.test/path/\n- next item") == [
        "https://example.test/path/",
        "next",
        "item",
    ]
    labeled_links = labeled_mdoc_links(
        '.Lk https://example.test/path "Example label" .\n'
        ".Lk https://example.test/unlabelled\n"
    )
    assert labeled_links == [
        ((token_key("https://example.test/path"),), ("example", "label"))
    ]
    assert labeled_mdoc_links(
        '.Lk https://example.test/continued\\\n "Continued label" .\n'
        '.Pq Lk decode.html#peer "peer status word" .\n'
    ) == [
        (("https://example.test/continued",), ("continued", "label")),
        (("decode.html", "peer"), ("peer", "status", "word")),
    ]
    assert omit_mandoc_labeled_link_destinations(
        [
            ["Example", "label:", "https://example.test/path"],
            ["https://example.test/path"],
        ],
        '.Lk https://example.test/path "Example label" .',
    ) == [["Example", "label:"], ["https://example.test/path"]]
    assert broken_phrase_candidates(
        [["one", "two", "three", "four"]],
        ["one", "two", "inserted", "three", "four"],
        4,
    ) == []
    assert broken_phrase_candidates(
        [["one", "two", "three", "four"]],
        ["one", "two", "four", "elsewhere", "three"],
        4,
    ) == ["one two three four"]
    hard, review = fidelity_signatures(r"text \[u2192]")
    assert not hard
    assert review == [
        "bracketed Unicode escape is visible; verify documented syntax"
    ]
    assert differential_signatures(
        ".Nd description\n", "name — description", "name —description"
    ) == [
        "mdoc Nd separator is attached to its description (reference=0, mant=1)"
    ]
    assert differential_signatures(
        ".Fo function\n.Fc\n", "function();", "function()"
    ) == [
        "mdoc synopsis function terminators may be missing (reference=1, mant=0)"
    ]
    assert not differential_signatures(
        ".Fn function\n", "function();", "function();"
    )
    assert differential_signatures(
        ".if \\n(.g .if rF .nr rF 1\n",
        "NAME\n",
        "prefix .if rF .nr rF 1 suffix\nNAME\n",
    ) == [
        "selected conditional leaked an authored roff control line: '.if rF .nr rF 1'"
    ]
    assert differential_signatures(
        '.Fo function\n.Fa "int first" "int second"\n.Fc\n',
        "function(int first, int second);",
        "function(int first int second);",
    )[-1] == "mdoc multi-operand Fa separators may be missing (reference=1, mant=0)"
    assert redirect_only_target(b'.\\" alias\n.so man1/target.1\n') == "man1/target.1"
    assert redirect_only_target(b".so man1/target.1\ntext\n") is None
    layout_source = ".EX\nplain first\n  plain second\n.EE\n"
    synopsis_source = (
        ".EX\n.SY #!\\f[I]interpreter\\f[]\n.RI [ optional-arg ]\n.YS\n.EE\n"
    )
    synopsis_layout = no_fill_source_layout(synopsis_source)
    assert synopsis_layout.anchors == {"#!interpreter", "[optional-arg]"}
    assert synopsis_layout.authored_indents["#!interpreter"] == 0
    assert synopsis_layout.consecutive_pairs == {("#!interpreter", "[optional-arg]")}
    collapsed_layout = layout_comparison(
        "  plain first\n    plain second\n",
        "plain first\nplain second\n",
        layout_source,
    )
    assert collapsed_layout.shared_anchors == 2
    assert any("authored relative indentation may collapse" in item for item in collapsed_layout.candidates)
    merged_layout = layout_comparison(
        "  plain first\n  plain second\n",
        "plain first plain second\n",
        layout_source,
    )
    assert any("line boundaries may merge" in item for item in merged_layout.candidates)
    same_text_elsewhere = layout_comparison(
        "  plain first\n  plain second\n",
        "plain first plain second\n",
        "plain first\nplain second\n.EX\nplain first\nplain second\n.EE\n",
    )
    assert not any("line boundaries may merge" in item for item in same_text_elsewhere.candidates)
    assert "plain first plain second" in no_fill_source_layout(
        '.EX\n.B "plain first plain second"\n.EE\n'
    ).source_lines
    spacing_layout = layout_comparison(
        "  plain first\n  plain second\n",
        "plain first\n\nplain second\n",
        ".EX\nplain first\n\nplain second\n.EE\n",
    )
    assert any("spacing divergence" in item for item in spacing_layout.candidates)
    assert len(compile_source_patterns([r"^\.Dd", r"^\.Fn"])) == 2
    clean = AuditRecord(
        "host", "man/demo.1", "1", "0" * 64, "clean", "not-required", ""
    )
    pending = AuditRecord(
        "host", "man/demo.1", "1", "0" * 64, "review", "pending", ""
    )
    assert merged_review_status(None, "review") == "pending"
    assert merged_review_status(clean, "hard-failure") == "pending"
    assert merged_review_status(pending, "clean") == "not-required"
    artifact = AuditArtifact(
        finding=Finding(path="manual/1/demo", status="clean"),
        source=b".TH DEMO 1\n",
        reference_output="DEMO(1)\n",
        mant_output="DEMO\n",
    )
    with tempfile.TemporaryDirectory() as temporary:
        bundle = Path(temporary) / "review"
        write_review_bundle(bundle, [(artifact.finding.path, artifact)])
        manifest = json.loads((bundle / "manifest.json").read_text(encoding="utf-8"))
        assert manifest["tool"] == "mant-roff-fidelity-review/v1"
        assert len(manifest["pages"]) == 1
        page = manifest["pages"][0]
        assert page["path"] == "manual/1/demo"
        assert page["status"] == "clean"
        assert set(page["files"]) == {"source", "reference", "mant", "finding"}
        assert (bundle / page["files"]["source"]).read_bytes() == artifact.source
        assert (bundle / page["files"]["reference"]).read_text(encoding="utf-8") == artifact.reference_output
        assert (bundle / page["files"]["mant"]).read_text(encoding="utf-8") == artifact.mant_output
        mandoc_database = Path(temporary) / "mandoc.csv"
        mandoc_record = AuditRecord(
            "fixture",
            "man3/example.3",
            "3",
            "1" * 64,
            "clean",
            "not-required",
            "",
            "mandoc",
            "mandoc-test",
        )
        write_audit_database(
            mandoc_database,
            [mandoc_record],
            "mandoc",
            "mandoc-test",
        )
        assert read_audit_database(
            mandoc_database,
            "mandoc",
            "mandoc-test",
        ) == {("fixture", "man3/example.3", "1" * 64): mandoc_record}
        hierarchy = Path(temporary) / "share/man"
        alias = hierarchy / "man1/alias.1"
        target = hierarchy / "man1/target.1.gz"
        alias.parent.mkdir(parents=True)
        alias.write_bytes(b".so man1/target.1\n")
        target.write_bytes(b"not actually compressed")
        assert confined_redirect_target(alias, hierarchy, "man1/target.1") == target
        assert confined_redirect_target(alias, hierarchy, "../outside.1") is None
        source_ledger = Path(temporary) / "source-ledger.csv"
        source_ledger.write_text(
            ",".join(AUDIT_DATABASE_FIELDS)
            + "\n"
            + f"known,man/man1/example.1,1,{'2' * 64},clean,not-required,\n",
            encoding="utf-8",
        )
        assert read_source_identities(source_ledger, "known") == {
            ("man/man1/example.1", "2" * 64)
        }
    reusable_page = ROOT / "tests/fixtures/roff/nested-fl-mdoc.1"
    reusable_digest = source_digest(reusable_page)
    assert reusable_digest is not None
    reusable_record = AuditRecord(
        "prior",
        "man1/nested-fl-mdoc.1",
        "1",
        reusable_digest,
        "clean",
        "not-required",
        "",
    )
    assert reusable_cross_corpus_sources(
        [reusable_page],
        {reusable_page: ("share/man1/nested-fl-mdoc.1", reusable_digest)},
        {
            (
                reusable_record.corpus,
                reusable_record.path,
                reusable_record.digest,
            ): reusable_record
        },
        "current",
    ) == {reusable_page: reusable_record}


def validate_tools(mant: Path, reference: str) -> None:
    if not mant.is_file():
        raise ValueError(f"ManT executable not found: {mant}; run `cargo build -p mant`")
    if os.sep not in reference and shutil.which(reference) is None:
        raise ValueError(f"reference command not found: {reference}")


def validate_syntax_profiler(path: Path) -> None:
    raise ValueError(
        "syntax-priority profiling was retired with the C oracle; use "
        "`python3 scripts/audit-roff-structure.py --fixtures` instead"
    )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff fidelity audit self-check succeeded")
        return 0

    try:
        explicit_page_set = arguments.pages_file is not None
        if arguments.recorded_only and arguments.audit_db is None:
            raise ValueError("--recorded-only requires --audit-db")
        if arguments.retry_skipped and arguments.audit_db is None:
            raise ValueError("--retry-skipped requires --audit-db")
        if arguments.pending_only and arguments.audit_db is None:
            raise ValueError("--pending-only requires --audit-db")
        if arguments.dedupe_across_corpora and arguments.audit_db is None:
            raise ValueError("--dedupe-across-corpora requires --audit-db")
        if arguments.replay_source_records and not arguments.corpus:
            raise ValueError("--replay-source-records requires an explicit --corpus")
        if (
            arguments.reference_kind == "mandoc"
            and arguments.audit_db is not None
            and not arguments.reference_id
        ):
            raise ValueError("mandoc --audit-db requires a stable --reference-id")
        if arguments.recorded_only and arguments.recheck_recorded:
            raise ValueError("--recorded-only and --recheck-recorded are mutually exclusive")
        exclusive_database_selections = sum(
            int(selected)
            for selected in [
                arguments.retry_skipped,
                arguments.pending_only,
                arguments.recorded_only,
                arguments.recheck_recorded,
            ]
        )
        if exclusive_database_selections > 1:
            raise ValueError(
                "--retry-skipped, --pending-only, --recorded-only, and "
                "--recheck-recorded are mutually exclusive"
            )
        if arguments.dedupe_across_corpora and exclusive_database_selections:
            raise ValueError(
                "--dedupe-across-corpora cannot be combined with an explicit "
                "audit database recheck mode"
            )
        if explicit_page_set and (
            arguments.max_pages
            or arguments.max_pages_per_section
            or arguments.man_section
            or arguments.source_pattern
            or arguments.syntax_priority
            or arguments.syntax_cache is not None
            or arguments.syntax_report is not None
            or arguments.dedupe_across_corpora
            or exclusive_database_selections
        ):
            raise ValueError(
                "--pages-file cannot be combined with sampling, source filters, "
                "syntax selection, or ledger selection options"
            )
        if arguments.replay_source_records and (
            arguments.max_pages
            or arguments.max_pages_per_section
            or arguments.man_section
            or arguments.source_pattern
            or arguments.syntax_priority
            or arguments.dedupe_across_corpora
        ):
            raise ValueError(
                "--replay-source-records cannot be combined with sampling, "
                "source filters, syntax-priority selection, or cross-corpus deduplication"
            )
        validate_tools(arguments.mant, arguments.reference)
        syntax_enabled = (
            arguments.syntax_priority
            or arguments.syntax_report is not None
            or arguments.syntax_cache is not None
        )
        if syntax_enabled:
            validate_syntax_profiler(arguments.syntax_profiler)
        roots = [path.resolve() for path in arguments.manpath] if arguments.manpath else [FIXTURE_ROOT]
        if explicit_page_set:
            all_pages = explicit_pages(arguments.pages_file, roots)
            source_filter_errors: list[Path] = []
        else:
            all_pages = discover_pages(roots)
            if arguments.man_section:
                selected_sections = set(arguments.man_section)
                all_pages = [
                    path for path in all_pages if manual_section(path) in selected_sections
                ]
            source_patterns = compile_source_patterns(arguments.source_pattern)
            all_pages, source_filter_errors = filter_pages_by_source(
                all_pages, source_patterns
            )
        pages = list(all_pages)
        corpus = arguments.corpus or (
            "fixtures" if not arguments.manpath else "local-manpath"
        )
        reference_id = arguments.reference_id or arguments.reference
        database = (
            read_audit_database(
                arguments.audit_db,
                arguments.reference_kind,
                reference_id,
            )
            if arguments.audit_db
            else {}
        )
        page_records = {
            path: (relative_label(path, roots), source_digest(path)) for path in pages
        }
        replay_targets: set[tuple[str, str]] = set()
        if arguments.replay_source_records:
            replay_targets = read_source_identities(arguments.source_ledger, corpus)
            available = {
                (label, digest)
                for label, digest in page_records.values()
                if digest is not None
            }
            missing = sorted(replay_targets - available)
            if missing:
                examples = ", ".join(
                    f"{label}@{digest[:12]}" for label, digest in missing[:5]
                )
                raise ValueError(
                    f"selected roots do not reproduce {len(missing)} of "
                    f"{len(replay_targets)} source identities for {corpus}: {examples}"
                )
            pages = [
                path
                for path in pages
                if (page_records[path][0], page_records[path][1]) in replay_targets
            ]
        cross_corpus_duplicates = (
            reusable_cross_corpus_sources(
                pages, page_records, database, corpus
            )
            if arguments.dedupe_across_corpora and not explicit_page_set
            else {}
        )
        recorded = {
            path
            for path, (label, digest) in page_records.items()
            if digest is not None and (corpus, label, digest) in database
        }
        completed = set()
        for path in recorded:
            label, digest = page_records[path]
            if digest is not None and database[
                (corpus, label, digest)
            ].scan_status != "skipped":
                completed.add(path)
        incomplete = recorded - completed
        pending = set()
        for path in recorded:
            label, digest = page_records[path]
            if digest is not None and database[
                (corpus, label, digest)
            ].review_status == "pending":
                pending.add(path)
        if explicit_page_set:
            # Exact page sets are callers' explicit intent.  They may still
            # update an audit database, but never become empty simply because
            # the same source was audited in an earlier broad scan.
            pass
        elif arguments.pending_only:
            pages = [path for path in pages if path in pending]
        elif arguments.retry_skipped:
            pages = [path for path in pages if path in incomplete]
        elif arguments.recorded_only:
            pages = [path for path in pages if path in recorded]
        elif not arguments.recheck_recorded:
            pages = [
                path
                for path in pages
                if path not in completed and path not in cross_corpus_duplicates
            ]
        profiles: dict[Path, SyntaxProfile] = {}
        if syntax_enabled:
            syntax_cache = read_syntax_cache(arguments.syntax_cache)
            profiles = syntax_profiles(
                all_pages,
                roots,
                page_records,
                corpus,
                arguments.syntax_profiler,
                arguments.syntax_timeout,
                syntax_cache,
            )
            if arguments.syntax_cache:
                write_syntax_cache(arguments.syntax_cache, syntax_cache)
            frequencies = feature_counts(all_pages, profiles)
            coverage = feature_counts(
                completed | set(cross_corpus_duplicates), profiles
            )
            pages = (
                rare_feature_sample_by_section(
                    pages,
                    arguments.max_pages_per_section,
                    arguments.seed,
                    profiles,
                    frequencies,
                    coverage,
                )
                if arguments.max_pages_per_section
                else rare_feature_sample(
                    pages,
                    arguments.max_pages,
                    arguments.seed,
                    profiles,
                    frequencies,
                    coverage,
                )
            )
            if arguments.syntax_report:
                write_syntax_report(
                    arguments.syntax_report,
                    roots,
                    corpus,
                    all_pages,
                    completed,
                    cross_corpus_duplicates,
                    pages,
                    page_records,
                    profiles,
                )
        else:
            pages = (
                stable_sample_by_section(
                    pages, arguments.max_pages_per_section, arguments.seed
                )
                if arguments.max_pages_per_section
                else stable_sample(pages, arguments.max_pages, arguments.seed)
            )
    except ValueError as error:
        print(f"audit-roff-fidelity: {error}", file=sys.stderr)
        return 2
    if arguments.syntax_report:
        print(f"syntax report: {arguments.syntax_report}")
    if arguments.syntax_cache:
        print(f"syntax cache:  {arguments.syntax_cache}")
    for path in source_filter_errors:
        print(
            "audit-roff-fidelity: source pattern could not inspect unreadable path: "
            f"{path}",
            file=sys.stderr,
        )
    if not pages and arguments.audit_db and (completed or cross_corpus_duplicates):
        print(
            "audit-roff-fidelity: no new or changed manual pages; "
            f"{len(completed)} unchanged pages are already complete and "
            f"{len(cross_corpus_duplicates)} sources were reused across corpora"
        )
        return 0
    if not pages:
        print("audit-roff-fidelity: no manual pages discovered", file=sys.stderr)
        return 2

    print("ManT roff fidelity audit")
    print(f"  mant:      {arguments.mant}")
    print(f"  reference: {arguments.reference}")
    print(f"  profile:   {arguments.reference_kind}/{reference_id}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    if arguments.pages_file:
        print(f"  page set:  {arguments.pages_file}")
    if arguments.replay_source_records:
        print(
            f"  replay:    {len(replay_targets)} exact identities from "
            f"{arguments.source_ledger}"
        )
    if arguments.audit_db:
        print(
            f"  audit db:  {arguments.audit_db} "
            f"({len(completed)} completed pages skipped from {corpus}; "
            f"{len(incomplete)} historical skips selected again; "
            f"{len(cross_corpus_duplicates)} cross-corpus sources reused)"
        )
    if arguments.man_section:
        print(f"  sections:  {', '.join(arguments.man_section)}")
    if arguments.source_pattern:
        print(f"  source:    {' AND '.join(arguments.source_pattern)}")
    print(
        "  contract:  visible tokens, continuity, selected-control leakage, and "
        "source-conditioned punctuation; "
        "layout is intentionally ignored unless --layout-signals is selected"
    )
    print()

    findings: list[Finding] = []
    review_artifacts: list[tuple[str, AuditArtifact]] = []
    summary = AuditSummary()
    for path in pages:
        label, digest = page_records[path]
        artifact = audit_page(
            path,
            label,
            roots,
            arguments.mant,
            arguments.reference,
            arguments.reference_kind,
            arguments.timeout,
            arguments.ngram,
            arguments.layout_signals,
        )
        finding = artifact.finding
        findings.append(finding)
        if arguments.review_dir is not None:
            review_artifacts.append((label, artifact))
        update_summary(summary, finding)
        if (
            not arguments.findings_only
            or finding.status in {"review", "hard-failure"}
            or (finding.layout is not None and finding.layout.candidates)
        ):
            print_finding(finding, arguments.show)
        if arguments.audit_db and digest is not None:
            key = (corpus, label, digest)
            previous = database.get(key)
            database[key] = AuditRecord(
                corpus=corpus,
                path=label,
                section=manual_section(path) or "",
                digest=digest,
                scan_status=finding.status,
                review_status=merged_review_status(previous, finding.status),
                note=previous.note if previous is not None else "",
                reference_kind=arguments.reference_kind,
                reference_id=reference_id,
            )
            if summary.examined % arguments.checkpoint_every == 0:
                write_audit_database(
                    arguments.audit_db,
                    database.values(),
                    arguments.reference_kind,
                    reference_id,
                )

    print()
    print(
        "summary: "
        f"examined={summary.examined}, clean={summary.clean}, review={summary.review}, "
        f"hard={summary.hard_failures}, skipped={summary.skipped}"
    )
    print("REVIEW findings are candidates, not failures; confirm them before adding a Rust regression.")
    if arguments.json:
        write_json_report(
            arguments.json,
            roots,
            arguments.mant,
            arguments.reference,
            arguments.reference_kind,
            reference_id,
            arguments.layout_signals,
            summary,
            findings,
        )
        print(f"report: {arguments.json}")
    if arguments.review_dir:
        write_review_bundle(arguments.review_dir, review_artifacts)
        print(f"review bundle: {arguments.review_dir}")
    if arguments.audit_db:
        write_audit_database(
            arguments.audit_db,
            database.values(),
            arguments.reference_kind,
            reference_id,
        )
        print(f"audit database: {arguments.audit_db}")
    return 1 if summary.hard_failures else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
