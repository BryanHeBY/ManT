#!/usr/bin/env python3
"""Find likely roff fidelity gaps without treating layout as a contract.

The audit compares ManT's plain manual rendering with a local man(1)/groff
reference. It is deliberately a developer and release-time discovery tool:
ordinary CI keeps the focused, deterministic Rust regressions derived from
confirmed findings instead of installing or trusting a host reference renderer.
"""

from __future__ import annotations

import argparse
import bz2
import gzip
import hashlib
import json
import lzma
import os
import re
import shutil
import subprocess
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable, Sequence


ROOT = Path(__file__).resolve().parents[1]
FIXTURE_ROOT = ROOT / "tests/fixtures/roff/real"
DEFAULT_MANT = ROOT / "target/debug/mant"

MANUAL_SUFFIX = re.compile(
    r"\.(?:[1-9][0-9A-Za-z]*|[ln])(?:\.(?:gz|bz2|xz|zst))?$"
)
ANSI = re.compile(r"\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07]*(?:\x07|\x1b\\))")
TOKEN = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.+:/-]{2,}")
URL_WRAP = re.compile(r"(?<=/)[ \t]*\n[ \t]*")
DEHYPHENATE = re.compile(r"-[ \t]*\n[ \t]*")
BORDERS = re.compile(r"[\u2500-\u257f\u2022\u00b7]")
UNICODE_ESCAPE = re.compile(
    r"\\\[u[0-9A-Fa-f]{4,6}(?:_[0-9A-Fa-f]{4,6})*\]"
)
GLUED_MARKER = re.compile(
    r"^[ \t]*((?:\u2022|\d{1,3}[.)])[A-Za-z(\"'])", re.MULTILINE
)
INTERNAL_MARKER = re.compile("[\u001d-\u001f]")

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
    parser.add_argument(
        "--max-pages",
        type=non_negative_integer,
        default=0,
        help="stable sample size; zero audits every discovered page",
    )
    parser.add_argument(
        "--seed",
        default="mant-fidelity-v1",
        help="stable sampling seed used with --max-pages",
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
    value = URL_WRAP.sub("", value)
    value = DEHYPHENATE.sub("", value)
    value = BORDERS.sub(" ", value)
    return " ".join(value.split())


def tokens(value: str) -> list[str]:
    return TOKEN.findall(normalized_visible_text(value))


def token_lines(value: str) -> list[list[str]]:
    value = strip_terminal_formatting(value).translate(TRANSLATION)
    value = URL_WRAP.sub("", value)
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
    """Recognize reference-renderer joins at a lower-to-uppercase boundary."""
    parts = re.split(r"(?<=[a-z])(?=[A-Z])", value)
    return len(parts) > 1 and all(token_key(part) in mine_keys for part in parts)


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
        hard.append("bracketed Unicode escape leaked")
    if INTERNAL_MARKER.search(value):
        hard.append("internal libmandoc marker leaked")
    if GLUED_MARKER.search(value):
        review.append("list or enumeration marker may be glued to following text")
    return hard, review


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


def redirect_only_stub(path: Path) -> bool:
    source = source_bytes(path)
    if source is None:
        return False
    lines = []
    for line in source.decode("utf-8", errors="replace").splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith('.\\"'):
            continue
        lines.append(stripped)
    return len(lines) == 1 and lines[0].startswith(".so ")


def relative_label(path: Path, roots: Sequence[Path]) -> str:
    for root in roots:
        try:
            return f"{root.name}/{path.relative_to(root).as_posix()}"
        except ValueError:
            continue
    return path.as_posix()


def audit_page(
    path: Path,
    label: str,
    mant: Path,
    reference: str,
    timeout: int,
    ngram: int,
) -> Finding:
    if redirect_only_stub(path):
        return Finding(path=label, status="skipped", detail="redirect-only .so page")

    environment = reference_environment()
    mant_status, mant_output, mant_error = run_renderer(
        [
            str(mant),
            "--input",
            str(path),
            "--input-format",
            "roff",
            "--format",
            "man",
        ],
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
        [reference, "-l", str(path)], timeout, environment
    )
    if reference_status != 0:
        return Finding(
            path=label,
            status="skipped",
            detail=reference_error.strip() or f"reference exit status {reference_status}",
        )

    reference_output = strip_reference_chrome(reference_output)
    reference_lines = token_lines(reference_output)
    reference_tokens = [value for line in reference_lines for value in line]
    mant_tokens = tokens(mant_output)
    if not reference_tokens or not mant_tokens:
        return Finding(
            path=label,
            status="skipped",
            reference_tokens=len(reference_tokens),
            mant_tokens=len(mant_tokens),
            detail="reference or ManT produced no comparable visible tokens",
        )

    missing = missing_token_candidates(reference_tokens, mant_tokens)
    phrases = broken_phrase_candidates(reference_lines, mant_tokens, ngram)
    hard_signatures, review_signatures = fidelity_signatures(mant_output)
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


def self_check() -> None:
    assert token_key("line-break") == token_key("linebreak")
    assert token_key("alloca.") == token_key("alloca")
    assert token_key("docs.example/path") != token_key("docs.example")
    assert tokens("one line-\nbreak here") == ["one", "linebreak", "here"]
    assert missing_token_candidates(["alpha", "missing"], ["alpha"]) == ["missing"]
    assert missing_token_candidates(["fBpackage.json"], ["package.json"]) == []
    assert missing_token_candidates(["defsReport"], ["defs", "Report"]) == []
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
    assert hard == ["bracketed Unicode escape leaked"]
    assert not review


def validate_tools(mant: Path, reference: str) -> None:
    if not mant.is_file():
        raise ValueError(f"ManT executable not found: {mant}; run `cargo build -p mant`")
    if os.sep not in reference and shutil.which(reference) is None:
        raise ValueError(f"reference command not found: {reference}")


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        print("roff fidelity audit self-check succeeded")
        return 0

    try:
        validate_tools(arguments.mant, arguments.reference)
        roots = [path.resolve() for path in arguments.manpath] if arguments.manpath else [FIXTURE_ROOT]
        pages = stable_sample(
            discover_pages(roots), arguments.max_pages, arguments.seed
        )
    except ValueError as error:
        print(f"audit-roff-fidelity: {error}", file=sys.stderr)
        return 2
    if not pages:
        print("audit-roff-fidelity: no manual pages discovered", file=sys.stderr)
        return 2

    print("ManT roff fidelity audit")
    print(f"  mant:      {arguments.mant}")
    print(f"  reference: {arguments.reference}")
    print(f"  roots:     {', '.join(str(root) for root in roots)}")
    print(f"  pages:     {len(pages)}")
    print("  contract:  visible tokens and token continuity; layout is intentionally ignored")
    print()

    findings: list[Finding] = []
    summary = AuditSummary()
    for path in pages:
        finding = audit_page(
            path,
            relative_label(path, roots),
            arguments.mant,
            arguments.reference,
            arguments.timeout,
            arguments.ngram,
        )
        findings.append(finding)
        update_summary(summary, finding)
        print_finding(finding, arguments.show)

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
    return 1 if summary.hard_failures else 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
