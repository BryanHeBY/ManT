"""Fidelity command options and stable checkout-relative defaults."""
from __future__ import annotations

import argparse
import re
from pathlib import Path
from typing import Sequence

ROOT = Path(__file__).resolve().parents[4]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_MANT = ROOT / "target/debug/mant"
DEFAULT_SYNTAX_PROFILER = ROOT / "target/debug/examples/roff_ast_profile"
DEFAULT_SOURCE_LEDGER = ROOT / "tests/fixtures/roff/FIDELITY_AUDIT.csv"
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
    parser.add_argument("--oracle-attestation", type=Path)
    parser.add_argument("--oracle-archive", type=Path)
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
