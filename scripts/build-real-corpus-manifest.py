#!/usr/bin/env python3
"""Validate all historical roff ledger sources and emit an oracle JSONL manifest.

The six checked-in ledgers are an immutable source-identity contract, not a
sampling hint. This tool takes explicit ``corpus=ROOT`` mappings, verifies
every decompressed source against its ledger SHA-256, and writes a deterministic
JSONL manifest only when the complete union is present. The manifest is meant
for the isolated migration oracle under ``$HOME/dev/tmp``; it is never a
published-crate asset.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import sys
import tempfile
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from pathlib import Path
from typing import Iterable, Sequence

from roff_audit_common import source_digest


ROOT = Path(__file__).resolve().parents[1]
ROFF_ROOT = ROOT / "tests/fixtures/roff"
DEFAULT_LEDGERS = (
    ROFF_ROOT / "FIDELITY_AUDIT.csv",
    ROFF_ROOT / "PROJECTION_AUDIT.csv",
    ROFF_ROOT / "STRUCTURE_AUDIT.csv",
    ROFF_ROOT / "LAYOUT_AUDIT.csv",
    ROFF_ROOT / "MANDOC_FIDELITY_AUDIT.csv",
    ROFF_ROOT / "MANDOC_LAYOUT_AUDIT.csv",
)
REQUIRED_COLUMNS = frozenset(("corpus", "path", "source_sha256"))
SOURCE_SHA256 = re.compile(r"[0-9a-f]{64}")


@dataclass(frozen=True, order=True)
class Identity:
    """One decompressed manual source declared by at least one ledger."""

    corpus: str
    path: str
    sha256: str


@dataclass(frozen=True)
class Inspection:
    """The verification outcome and, when matched, its authoritative root."""

    status: str
    root: Path | None = None


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--ledger",
        action="append",
        type=Path,
        default=[],
        help="additional CSV ledger; defaults to the six checked-in ledgers",
    )
    parser.add_argument(
        "--source-root",
        action="append",
        default=[],
        metavar="CORPUS=ROOT",
        help=(
            "absolute directory that owns corpus ledger-relative paths; repeat "
            "a corpus to supply ordered, checksum-verified fallback roots"
        ),
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="absolute JSONL output outside the repository; written only when complete",
    )
    parser.add_argument(
        "--unavailable-output",
        type=Path,
        help="absolute JSONL report outside the repository for missing or mismatched identities",
    )
    parser.add_argument(
        "--jobs",
        type=positive_integer,
        default=min(os.cpu_count() or 1, 12),
        help="parallel decompression/hash workers (default: min(CPU count, 12))",
    )
    parser.add_argument(
        "--require-complete",
        action="store_true",
        help="exit unsuccessfully unless every ledger identity verifies",
    )
    parser.add_argument("--self-check", action="store_true", help=argparse.SUPPRESS)
    arguments = parser.parse_args(argv)
    for option in ("output", "unavailable_output"):
        value = getattr(arguments, option)
        if value is not None and not value.is_absolute():
            parser.error(f"--{option.replace('_', '-')} must be an absolute path outside the repository")
    if arguments.output is not None and arguments.output == arguments.unavailable_output:
        parser.error("--output and --unavailable-output must differ")
    return arguments


def positive_integer(value: str) -> int:
    parsed = int(value)
    if parsed < 1:
        raise argparse.ArgumentTypeError("must be positive")
    return parsed


def ledger_paths(arguments: argparse.Namespace) -> tuple[Path, ...]:
    paths = tuple(DEFAULT_LEDGERS) + tuple(arguments.ledger)
    if len(set(paths)) != len(paths):
        raise ValueError("a ledger was supplied more than once")
    return paths


def read_identities(paths: Iterable[Path]) -> tuple[Identity, ...]:
    identities: set[Identity] = set()
    for path in paths:
        if not path.is_file():
            raise ValueError(f"ledger does not exist: {path}")
        with path.open(encoding="utf-8", newline="") as source:
            reader = csv.DictReader(source)
            if reader.fieldnames is None or not REQUIRED_COLUMNS.issubset(reader.fieldnames):
                raise ValueError(f"ledger has no source identity columns: {path}")
            for line, row in enumerate(reader, 2):
                identity = Identity(
                    row["corpus"], row["path"], row["source_sha256"].lower()
                )
                validate_identity(identity, path, line)
                identities.add(identity)
    return tuple(sorted(identities))


def validate_identity(identity: Identity, ledger: Path, line: int) -> None:
    if not identity.corpus or not identity.path or SOURCE_SHA256.fullmatch(identity.sha256) is None:
        raise ValueError(f"invalid source identity at {ledger}:{line}")
    path = Path(identity.path)
    if path.is_absolute() or ".." in path.parts or path.as_posix() != identity.path:
        raise ValueError(f"unsafe ledger-relative source path at {ledger}:{line}: {identity.path}")


def source_roots(values: Iterable[str]) -> dict[str, tuple[Path, ...]]:
    roots: dict[str, list[Path]] = {}
    for value in values:
        corpus, separator, raw_path = value.partition("=")
        path = Path(raw_path)
        if not separator or not corpus or not raw_path or not path.is_absolute():
            raise ValueError(f"--source-root must be CORPUS=absolute-path: {value!r}")
        candidates = roots.setdefault(corpus, [])
        if path in candidates:
            raise ValueError(f"duplicate --source-root mapping: {value!r}")
        candidates.append(path)
    return {corpus: tuple(candidates) for corpus, candidates in roots.items()}


def inspect_identity(
    identity: Identity, roots: dict[str, tuple[Path, ...]]
) -> tuple[Identity, Inspection]:
    candidates = roots.get(identity.corpus)
    if candidates is None:
        return identity, Inspection("missing-root")

    saw_unreadable = False
    saw_mismatch = False
    for root in candidates:
        candidate = root / identity.path
        if not candidate.is_file() and not candidate.is_symlink():
            continue
        observed = source_digest(candidate)
        if observed is None:
            saw_unreadable = True
        elif observed == identity.sha256:
            return identity, Inspection("verified", root)
        else:
            saw_mismatch = True

    if saw_mismatch:
        return identity, Inspection("hash-mismatch")
    if saw_unreadable:
        return identity, Inspection("unreadable")
    return identity, Inspection("missing-path")


def inspect_all(
    identities: Sequence[Identity], roots: dict[str, tuple[Path, ...]], jobs: int
) -> dict[Identity, Inspection]:
    with ThreadPoolExecutor(max_workers=jobs) as executor:
        results = executor.map(lambda identity: inspect_identity(identity, roots), identities)
        return dict(results)


def external_output_path(output: Path) -> Path:
    resolved_output = output.resolve()
    if resolved_output.is_relative_to(ROOT):
        raise ValueError("--output must stay outside the repository")
    if not resolved_output.parent.is_dir():
        raise ValueError(f"output parent does not exist: {resolved_output.parent}")
    return resolved_output


def write_jsonl(output: Path, records: Iterable[dict[str, object]]) -> Path:
    resolved_output = external_output_path(output)
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", newline="", dir=resolved_output.parent, delete=False
    ) as temporary:
        for record in records:
            temporary.write(json.dumps(record, sort_keys=True, separators=(",", ":")) + "\n")
        temporary_path = Path(temporary.name)
    temporary_path.replace(resolved_output)
    return resolved_output


def write_manifest(
    output: Path, identities: Sequence[Identity], results: dict[Identity, Inspection]
) -> Path:
    return write_jsonl(
        output,
        (
            manifest_record(identity, results[identity])
            for identity in identities
        ),
    )


def manifest_record(identity: Identity, inspection: Inspection) -> dict[str, object]:
    if inspection.status != "verified" or inspection.root is None:
        raise ValueError(f"cannot emit an unverified source: {identity.corpus}:{identity.path}")
    return {
        "id": f"{identity.corpus}:{identity.path}",
        "source_name": identity.path,
        "source_path": str((inspection.root / identity.path).resolve()),
        "source_sha256": identity.sha256,
    }


def write_unavailable_report(
    output: Path,
    identities: Sequence[Identity],
    results: dict[Identity, Inspection],
    roots: dict[str, tuple[Path, ...]],
) -> Path:
    def records() -> Iterable[dict[str, object]]:
        for identity in identities:
            inspection = results[identity]
            if inspection.status == "verified":
                continue
            record = {
                "schema": "mantdoc.real-corpus-unavailable/v1",
                "id": f"{identity.corpus}:{identity.path}",
                "corpus": identity.corpus,
                "path": identity.path,
                "source_sha256": identity.sha256,
                "status": inspection.status,
            }
            if candidates := roots.get(identity.corpus):
                record["source_roots"] = [str(root.resolve()) for root in candidates]
            yield record

    return write_jsonl(output, records())


def summarize(identities: Sequence[Identity], results: dict[Identity, Inspection]) -> bool:
    statuses = Counter(inspection.status for inspection in results.values())
    by_corpus = Counter(
        identity.corpus
        for identity, inspection in results.items()
        if inspection.status != "verified"
    )
    print("real_corpus_manifest_schema=mantdoc.real-corpus-manifest/v1")
    print(f"unique_identity_count={len(identities)}")
    for status in ("verified", "missing-root", "missing-path", "unreadable", "hash-mismatch"):
        print(f"{status.replace('-', '_')}_count={statuses[status]}")
    for corpus, count in sorted(by_corpus.items()):
        print(f"unavailable_corpus={corpus} count={count}")
    return all(inspection.status == "verified" for inspection in results.values())


def self_check() -> None:
    first = Identity("a", "man/man1/a.1.gz", "a" * 64)
    duplicate = Identity("a", "man/man1/a.1.gz", "a" * 64)
    second = Identity("b", "man/man2/b.2", "b" * 64)
    assert tuple(sorted({first, duplicate, second})) == (first, second)
    try:
        validate_identity(Identity("a", "../escape.1", "a" * 64), Path("fixture.csv"), 2)
    except ValueError:
        pass
    else:
        raise AssertionError("unsafe source path was accepted")
    digest = hashlib.sha256(b"fixture\n").hexdigest()
    assert SOURCE_SHA256.fullmatch(digest) is not None
    with tempfile.TemporaryDirectory() as temporary:
        root = Path(temporary)
        stale = root / "stale"
        restored = root / "restored"
        stale.mkdir()
        restored.mkdir()
        relative_path = Path("man/man1/fixture.1")
        stale_candidate = stale / relative_path
        restored_candidate = restored / relative_path
        stale_candidate.parent.mkdir(parents=True)
        restored_candidate.parent.mkdir(parents=True)
        stale_candidate.write_bytes(b"stale\n")
        restored_candidate.write_bytes(b"fixture\n")
        identity = Identity("overlay", relative_path.as_posix(), digest)
        result = inspect_identity(identity, {"overlay": (stale, restored)})[1]
        assert result == Inspection("verified", restored)
        assert manifest_record(identity, result)["source_path"] == str(restored_candidate)


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    if arguments.self_check:
        self_check()
        return 0
    try:
        identities = read_identities(ledger_paths(arguments))
        roots = source_roots(arguments.source_root)
        unknown_roots = set(roots) - {identity.corpus for identity in identities}
        if unknown_roots:
            raise ValueError(f"source roots name no ledger corpus: {sorted(unknown_roots)!r}")
        results = inspect_all(identities, roots, arguments.jobs)
        complete = summarize(identities, results)
        if arguments.unavailable_output is not None:
            report = write_unavailable_report(
                arguments.unavailable_output, identities, results, roots
            )
            print(f"unavailable_report={report}")
        if arguments.output is not None:
            if not complete:
                raise ValueError("refusing to write an incomplete oracle manifest")
            manifest = write_manifest(arguments.output, identities, results)
            print(f"manifest={manifest}")
        return 0 if complete or not arguments.require_complete else 1
    except ValueError as error:
        print(f"build-real-corpus-manifest: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
