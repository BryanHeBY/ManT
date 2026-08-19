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
from collections import Counter
from dataclasses import asdict, dataclass
from fractions import Fraction
from pathlib import Path
from typing import Iterable, Sequence


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_MANT = ROOT / "target/debug/mant"
DEFAULT_SYNTAX_PROFILER = ROOT / "target/debug/examples/roff_ast_profile"
SYNTAX_CACHE_VERSION = 1
AUDIT_DATABASE_FIELDS = [
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
ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))")
TOKEN = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.+:/-]{2,}")
# Only join a wrapped URL/path component after a non-slash component. A bare
# slash at the end of an unrelated token (for example Perl's `tr//` followed
# by a new sentence) must not consume the semantic line boundary.
URL_WRAP = re.compile(
    r"(https?://[^\s\n]*/)[ \t]*\n[ \t]*(?=[^\s\n]*[./_~?&=%-])"
)
DEHYPHENATE = re.compile(r"-[ \t]*\n[ \t]*")
BORDERS = re.compile(r"[\u2500-\u257f\u2022\u00b7]")
UNICODE_ESCAPE = re.compile(
    r"\\\[u[0-9A-Fa-f]{4,6}(?:_[0-9A-Fa-f]{4,6})*\]"
)
GLUED_MARKER = re.compile(r"^[ \t]*\u2022[A-Za-z(\"']", re.MULTILINE)
INTERNAL_MARKER = re.compile("[\u001d-\u001f]")
MDOC_NAME_DESCRIPTION = re.compile(r"^[.']Nd(?:\s|$)", re.MULTILINE)
MDOC_FUNCTION_DECLARATION = re.compile(r"^[.'](?:Fn|Fo)(?:\s|$)", re.MULTILINE)
EM_DASH_ATTACHED_TO_WORD = re.compile(r"—(?=\w)")

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
        "--mant",
        type=Path,
        default=DEFAULT_MANT,
        help=f"ManT executable (default: {DEFAULT_MANT.relative_to(ROOT)})",
    )
    parser.add_argument(
        "--reference",
        default="man",
        help="man(1)-compatible reference command (default: man)",
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
            "prefer pages with rare or not-yet-recorded libmandoc AST features "
            "when bounded sampling is active"
        ),
    )
    parser.add_argument(
        "--syntax-profiler",
        type=Path,
        default=DEFAULT_SYNTAX_PROFILER,
        metavar="FILE",
        help=(
            "batch AST profiler built from the libmandoc-rs example "
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
        "--audit-db",
        type=Path,
        metavar="FILE",
        help=(
            "skip unchanged pages already listed in the CSV database and merge "
            "this run into it"
        ),
    )
    parser.add_argument(
        "--corpus",
        metavar="NAME",
        help="stable source name stored in --audit-db (default: fixtures or local-manpath)",
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
    if payload.get("version") != SYNTAX_CACHE_VERSION:
        raise ValueError(
            f"unsupported syntax cache version in {path}; "
            f"expected {SYNTAX_CACHE_VERSION}"
        )
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
            not isinstance(request_id, str)
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
                (Fraction(1, frequencies[feature]) for feature in features if not coverage[feature]),
                start=Fraction(),
            )
            balance = sum(
                (
                    Fraction(
                        1,
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
    command: Sequence[str], timeout: int, environment: dict[str, str]
) -> tuple[int, str, str]:
    try:
        result = subprocess.run(
            command,
            stdin=subprocess.DEVNULL,
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
    return "\n".join(lines)


def normalized_visible_text(value: str) -> str:
    value = strip_terminal_formatting(value).translate(TRANSLATION)
    value = URL_WRAP.sub(r"\1", value)
    value = DEHYPHENATE.sub("", value)
    value = BORDERS.sub(" ", value)
    return " ".join(value.split())


def tokens(value: str) -> list[str]:
    return TOKEN.findall(normalized_visible_text(value))


def token_lines(value: str) -> list[list[str]]:
    value = strip_terminal_formatting(value).translate(TRANSLATION)
    value = URL_WRAP.sub(r"\1", value)
    value = DEHYPHENATE.sub("", value)
    value = BORDERS.sub(" ", value)
    return [TOKEN.findall(line) for line in value.splitlines()]


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
        hard.append("internal libmandoc marker leaked")
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


def read_audit_database(
    path: Path,
) -> dict[tuple[str, str, str], AuditRecord]:
    if not path.exists():
        return {}
    entries: dict[tuple[str, str, str], AuditRecord] = {}
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != AUDIT_DATABASE_FIELDS:
            raise ValueError(
                f"invalid audit database header in {path}; expected "
                f"{','.join(AUDIT_DATABASE_FIELDS)}"
            )
        for number, row in enumerate(reader, 2):
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
            )
            entries[(entry.corpus, entry.path, entry.digest)] = entry
    return entries


def write_audit_database(
    path: Path,
    entries: Iterable[AuditRecord],
) -> None:
    rows = sorted(entries, key=lambda entry: (entry.corpus, entry.path, entry.digest))
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp")
    with temporary.open("w", encoding="utf-8", newline="") as destination:
        writer = csv.DictWriter(
            destination,
            fieldnames=AUDIT_DATABASE_FIELDS,
            lineterminator="\n",
        )
        writer.writeheader()
        for entry in rows:
            writer.writerow(
                {
                    "corpus": entry.corpus,
                    "path": entry.path,
                    "section": entry.section,
                    "source_sha256": entry.digest,
                    "scan_status": entry.scan_status,
                    "review_status": entry.review_status,
                    "note": entry.note,
                }
            )
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
    path: Path, reference: str, hierarchy_root: Path | None
) -> list[str]:
    """Render aliases through the reference index when `man -l` cannot."""
    if hierarchy_root is not None:
        section = manual_section(path)
        topic = manual_topic(path)
        if section is not None and topic is not None:
            return [reference, section, topic]
    return [reference, "-l", str(path)]


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
    timeout: int,
    ngram: int,
) -> Finding:
    environment = reference_environment()
    mant_command, hierarchy_root = mant_render_command(path, roots, mant)
    if hierarchy_root is not None:
        environment["MANT_MANPATH"] = str(hierarchy_root)
        # GNU man resolves redirect-only `.so` pages through MANPATH even
        # with `-l`. Keep the reference on the same localized hierarchy as
        # ManT instead of accidentally comparing a translated alias with the
        # default-language target.
        environment["MANPATH"] = str(hierarchy_root)
    mant_status, mant_output, mant_error = run_renderer(
        mant_command,
        timeout,
        environment,
    )
    if mant_status != 0:
        return Finding(
            path=label,
            status="hard-failure",
            signatures=["ManT failed to render the page"],
            detail=mant_error.strip() or f"exit status {mant_status}",
        )

    reference_status, reference_output, reference_error = run_renderer(
        reference_render_command(path, reference, hierarchy_root), timeout, environment
    )
    if reference_status != 0:
        return Finding(
            path=label,
            status="hard-failure",
            signatures=["reference renderer failed; page was not compared"],
            detail=reference_error.strip() or f"reference exit status {reference_status}",
        )

    reference_output = strip_reference_chrome(reference_output)
    reference_lines = token_lines(reference_output)
    reference_tokens = [value for line in reference_lines for value in line]
    mant_tokens = tokens(mant_output)
    if not reference_tokens:
        return Finding(
            path=label,
            status="hard-failure",
            reference_tokens=len(reference_tokens),
            mant_tokens=len(mant_tokens),
            signatures=["reference renderer produced no comparable visible tokens"],
            detail="the page cannot be classified as clean without a reference corpus",
        )
    if not mant_tokens:
        return Finding(
            path=label,
            status="hard-failure",
            reference_tokens=len(reference_tokens),
            mant_tokens=0,
            signatures=["ManT produced no comparable visible tokens"],
            detail="the reference renderer produced visible content",
        )

    missing = missing_token_candidates(reference_tokens, mant_tokens)
    phrases = broken_phrase_candidates(reference_lines, mant_tokens, ngram)
    hard_signatures, review_signatures = fidelity_signatures(mant_output)
    raw_source = source_bytes(path)
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
    return Finding(
        path=label,
        status=status,
        reference_tokens=len(reference_tokens),
        mant_tokens=len(mant_tokens),
        missing_tokens=missing,
        broken_phrases=phrases,
        signatures=signatures,
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
    summary: AuditSummary,
    findings: Sequence[Finding],
) -> None:
    report = {
        "tool": "mant-roff-fidelity-audit/v1",
        "roots": [str(root) for root in roots],
        "mant": str(mant),
        "reference": reference,
        "summary": asdict(summary),
        "findings": [asdict(finding) for finding in findings],
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def write_syntax_report(
    path: Path,
    roots: Sequence[Path],
    corpus: str,
    all_pages: Sequence[Path],
    recorded_pages: set[Path],
    selected_pages: Sequence[Path],
    page_records: dict[Path, tuple[str, str | None]],
    profiles: dict[Path, SyntaxProfile],
) -> None:
    corpus_counts = feature_counts(all_pages, profiles)
    recorded_counts = feature_counts(recorded_pages, profiles)
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
        "tool": "mant-roff-syntax-coverage/v1",
        "corpus": corpus,
        "roots": [str(root) for root in roots],
        "summary": {
            "corpusPages": len(all_pages),
            "uniqueSources": len(unique_sources),
            "recordedPages": len(recorded_pages),
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
        "selectedPaths": [page_records[page][0] for page in selected_pages],
        "features": features,
        "errors": errors,
    }
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )


def self_check() -> None:
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


def validate_tools(mant: Path, reference: str) -> None:
    if not mant.is_file():
        raise ValueError(f"ManT executable not found: {mant}; run `cargo build -p mant`")
    if os.sep not in reference and shutil.which(reference) is None:
        raise ValueError(f"reference command not found: {reference}")


def validate_syntax_profiler(path: Path) -> None:
    if not path.is_file():
        raise ValueError(
            f"syntax profiler not found: {path}; run "
            "`cargo build -p libmandoc-rs --example roff_ast_profile`"
        )


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff fidelity audit self-check succeeded")
        return 0

    try:
        if arguments.recorded_only and arguments.audit_db is None:
            raise ValueError("--recorded-only requires --audit-db")
        if arguments.retry_skipped and arguments.audit_db is None:
            raise ValueError("--retry-skipped requires --audit-db")
        if arguments.pending_only and arguments.audit_db is None:
            raise ValueError("--pending-only requires --audit-db")
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
        validate_tools(arguments.mant, arguments.reference)
        syntax_enabled = (
            arguments.syntax_priority
            or arguments.syntax_report is not None
            or arguments.syntax_cache is not None
        )
        if syntax_enabled:
            validate_syntax_profiler(arguments.syntax_profiler)
        roots = [path.resolve() for path in arguments.manpath] if arguments.manpath else [FIXTURE_ROOT]
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
        database = (
            read_audit_database(arguments.audit_db) if arguments.audit_db else {}
        )
        page_records = {
            path: (relative_label(path, roots), source_digest(path)) for path in pages
        }
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
        if arguments.pending_only:
            pages = [path for path in pages if path in pending]
        elif arguments.retry_skipped:
            pages = [path for path in pages if path in incomplete]
        elif arguments.recorded_only:
            pages = [path for path in pages if path in recorded]
        elif not arguments.recheck_recorded:
            pages = [path for path in pages if path not in completed]
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
            coverage = feature_counts(completed, profiles)
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
    if not pages and arguments.audit_db and completed:
        print(
            "audit-roff-fidelity: no new or changed manual pages; "
            f"{len(completed)} unchanged pages are already complete"
        )
        return 0
    if not pages:
        print("audit-roff-fidelity: no manual pages discovered", file=sys.stderr)
        return 2

    print("ManT roff fidelity audit")
    print(f"  mant:      {arguments.mant}")
    print(f"  reference: {arguments.reference}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    if arguments.audit_db:
        print(
            f"  audit db:  {arguments.audit_db} "
            f"({len(completed)} completed pages skipped from {corpus}; "
            f"{len(incomplete)} historical skips selected again)"
        )
    if arguments.man_section:
        print(f"  sections:  {', '.join(arguments.man_section)}")
    if arguments.source_pattern:
        print(f"  source:    {' AND '.join(arguments.source_pattern)}")
    print(
        "  contract:  visible tokens, continuity, and source-conditioned punctuation; "
        "layout is intentionally ignored"
    )
    print()

    findings: list[Finding] = []
    summary = AuditSummary()
    for path in pages:
        label, digest = page_records[path]
        finding = audit_page(
            path,
            label,
            roots,
            arguments.mant,
            arguments.reference,
            arguments.timeout,
            arguments.ngram,
        )
        findings.append(finding)
        update_summary(summary, finding)
        if not arguments.findings_only or finding.status in {"review", "hard-failure"}:
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
            summary,
            findings,
        )
        print(f"report: {arguments.json}")
    if arguments.audit_db:
        write_audit_database(arguments.audit_db, database.values())
        print(f"audit database: {arguments.audit_db}")
    return 1 if summary.hard_failures else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
