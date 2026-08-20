"""Shared discovery and identity helpers for local roff audit drivers.

The module owns only deterministic, renderer-independent mechanics. Audit
contracts, ledgers, subprocess protocols, and human-review policy remain in
their individual drivers so evolving one oracle cannot silently change
another oracle's interpretation.
"""

from __future__ import annotations

import argparse
import bz2
import gzip
import hashlib
import lzma
import os
import re
import shutil
import subprocess
from collections import Counter
from pathlib import Path
from typing import Sequence


MANUAL_SUFFIX = re.compile(
    r"\.(?P<section>[1-9][0-9A-Za-z]*|[ln])(?:\.(?:gz|bz2|xz|zst))?$"
)


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


def read_fidelity_identities(path: Path, corpus: str) -> set[tuple[str, str]]:
    import csv

    fields = [
        "corpus",
        "path",
        "section",
        "source_sha256",
        "scan_status",
        "review_status",
        "note",
    ]
    if not path.exists():
        raise ValueError(f"fidelity database does not exist: {path}")
    identities = set()
    with path.open(encoding="utf-8", newline="") as source:
        reader = csv.DictReader(source)
        if reader.fieldnames != fields:
            raise ValueError(
                f"invalid fidelity database header in {path}; expected {','.join(fields)}"
            )
        for row in reader:
            if row["corpus"] == corpus:
                identities.add((row["path"], row["source_sha256"]))
    return identities
