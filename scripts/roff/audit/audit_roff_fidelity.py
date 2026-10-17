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
from dataclasses import asdict
from fractions import Fraction
from pathlib import Path, PurePosixPath
from typing import Iterable, Sequence

from scripts.roff.lib.fidelity.arguments import (
    FIXTURE_ROOT,
    ROOT,
    compile_source_patterns,
    parse_arguments,
)
from scripts.roff.lib.fidelity.model import (
    AuditArtifact,
    AuditRecord,
    AuditSummary,
    Finding,
    SyntaxProfile,
)
from scripts.roff.lib.fidelity.layout import (
    layout_comparison,
    no_fill_source_layout,
)
from scripts.roff.lib.fidelity.text import (
    EXTERNAL_ROFF_CONTEXT,
    broken_phrase_candidates,
    differential_signatures,
    fidelity_signatures,
    labeled_mdoc_links,
    missing_token_candidates,
    normalized_visible_text,
    omit_mandoc_labeled_link_destinations,
    portable_table_row_pairs,
    strip_reference_chrome,
    table_phrase_is_only_terminal_geometry,
    token_key,
    token_lines,
    tokens,
)
from scripts.roff.lib.fidelity.report import (
    print_finding,
    update_summary,
    write_json_report,
    write_review_bundle,
)

from scripts.roff.oracle import mandoc_oracle
from scripts.roff.lib.roff_reference import reference_environment, run_renderer
from scripts.roff.lib.roff_reference import self_check as reference_runner_self_check


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
    path: Path, roots: Sequence[Path], mant: Path, *, source: bytes | None = None
) -> tuple[list[str], Path | None]:
    """Select standalone or indexed rendering without weakening --input.

    Any source containing a .so request needs an approved manual hierarchy.
    Redirect-only aliases then resolve normally; unsupported embedded includes
    become visible hard failures instead of silently leaving the audit corpus.
    """
    needs_hierarchy = (contains_so_request(path) if source is None else
        any(re.match(rb"^[.']so(?:[ \t]|$)", line) is not None for line in source.splitlines()))
    if needs_hierarchy:
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
    *, source_reader=None,
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
        reference_source = (source_reader or source_bytes)(reference_path)
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

    return compare_rendered(label, raw_source, reference_output, mant_output,
                            reference_kind, ngram, layout_signals)


def compare_rendered(
    label: str, raw_source: bytes | None, reference_output: str, mant_output: str,
    reference_kind: str, ngram: int = 4, layout_signals: bool = True,
) -> AuditArtifact:
    """The legacy oracle, independent of discovery and subprocess transport.

    Callers must establish successful bounded renders and source identity first.
    No ledger policy or interpretation differs from ``audit_page``.
    """
    reference_output = strip_reference_chrome(reference_output)
    source_text = raw_source.decode("utf-8", errors="replace") if raw_source is not None else None
    reference_lines = token_lines(reference_output, source_text)
    if reference_kind == "mandoc" and raw_source is not None:
        reference_lines = omit_mandoc_labeled_link_destinations(
            reference_lines,
            raw_source.decode("utf-8", errors="replace"),
        )
    reference_tokens = [value for line in reference_lines for value in line]
    mant_tokens = tokens(mant_output, source_text)
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
    phrases = broken_phrase_candidates(
        reference_lines,
        mant_tokens,
        ngram,
        portable_table_row_pairs(source_text, mant_output),
    )
    hard_signatures, review_signatures = fidelity_signatures(mant_output, source_text)
    if raw_source is not None:
        review_signatures.extend(
            differential_signatures(
                source_text,
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
    reference_runner_self_check()
    for marker in ("\x1a", "\x1c", "\x1d", "\x1e", "\x1f"):
        assert fidelity_signatures("before" + marker + "after")[0] == [
            "internal libmandoc marker leaked"
        ]
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
    assert strip_reference_chrome(
        "PROBE(1) Commands PROBE(1)\ncontent\nLinux 6.18\n"
        "   July 3, 2025                  PROBE(1)\n"
    ).strip() == "content"
    assert normalized_visible_text(
        "read <https://example.test/api/\nversion.3.html> now"
    ) == "read https://example.test/api/version.3.html now"
    assert normalized_visible_text(
        "read ⟨https://example.test/api/version.3.html⟩. now"
    ) == "read https://example.test/api/version.3.html. now"
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
    # CVS term_fill() may wrap after an authored trailing hyphen that is an
    # operator rather than a discretionary word break.  The direct source
    # spelling retains that operator; an unproved physical wrap still joins.
    literal_hyphen_source = r"\fIx-\fR enumerates from index"
    assert tokens("x-\n enumerates from index", literal_hyphen_source) == [
        "enumerates",
        "from",
        "index",
    ]
    assert tokens("x-\n enumerates from index") == ["xenumerates", "from", "index"]
    assert tokens("compound-\n word", literal_hyphen_source) == ["compoundword"]
    assert tokens("-W----\nZIO_STAGE_ENCRYPT") == ["W----", "ZIO_STAGE_ENCRYPT"]
    assert tokens("Prikaže café 日本語") == ["Prikaže", "café", "日本語"]
    assert tokens("cafe\u0301") == ["café"]
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
    literal_table_source = """.TS
tab(@);
l l.
T{
alpha beta
T}@T{
gamma delta
T}
.TE
"""
    literal_table_rows = portable_table_row_pairs(
        literal_table_source,
        "alpha beta | gamma delta\n",
    )
    assert len(literal_table_rows) == 1
    assert broken_phrase_candidates(
        [["alpha", "beta", "gamma", "delta"]],
        ["alpha", "beta", "delta", "elsewhere", "gamma"],
        4,
        literal_table_rows,
    ) == []
    geometry_table_source = """.TS
tab(@);
l l.
T{
The color for a folder or a shortcut to a folder as an RGB hex string.
T}@T{
string
T}
.TE
"""
    geometry_table_rows = portable_table_row_pairs(
        geometry_table_source,
        "The color for a folder or a shortcut to a folder as an RGB hex string. | string\n",
    )
    # The reference table formatter can line up `color for folder` beside the
    # next cell's `string`. The exact source/portable row proves that this
    # non-contiguous sequence is geometry, not a missing ManT phrase.
    assert broken_phrase_candidates(
        [["color", "for", "folder", "string"]],
        [
            "the", "color", "for", "folder", "shortcut", "folder",
            "rgb", "hex", "string", "string",
        ],
        4,
        geometry_table_rows,
    ) == []
    # Contiguous prose inside a verified table cell remains a real ordering
    # candidate when ManT does not contain it.
    assert not table_phrase_is_only_terminal_geometry(
        ("color", "for", "folder"), geometry_table_rows[0][0]
    )
    # A page that merely contains a tbl request cannot exempt an unrelated
    # literal pipe in source code: no exact source-table row binds it.
    assert portable_table_row_pairs(
        literal_table_source,
        "alpha beta | ordinary code\n",
    ) == []
    mixed_table_source = """.TS
l l l l.
T{
alpha
T}\t1\tbeta\tT{
gamma
T}
.TE
"""
    assert portable_table_row_pairs(
        mixed_table_source,
        "alpha | (1) | beta | gamma\n",
    ) == [
        (
            (("alpha",), (), ("beta",), ("gamma",)),
            (("alpha",), (), ("beta",), ("gamma",)),
        )
    ]
    raw_prefix_table_source = """.TS
l l l.
Keymap\tT{
complete keyboard description
T}\tT{
aliases for keys
T}
.TE
"""
    assert portable_table_row_pairs(
        raw_prefix_table_source,
        "Keymap | complete keyboard description | aliases for keys\n",
    ) == [
        (
            (
                ("keymap",),
                ("complete", "keyboard", "description"),
                ("aliases", "for", "keys"),
            ),
            (
                ("keymap",),
                ("complete", "keyboard", "description"),
                ("aliases", "for", "keys"),
            ),
        )
    ]
    custom_separator_table_source = """.TS
tab(@);
l l.
left@T{
right
T}
.TE
"""
    assert portable_table_row_pairs(custom_separator_table_source, "left | right\n") == [
        ((("left",), ("right",)), (("left",), ("right",)))
    ]
    # CVS tbl_term.c emits a U+2502 frame between physical cells. The global
    # token comparison still observes both cells; phrase ordering must not
    # borrow that renderer-owned geometry as a missing-content claim.
    assert token_lines("│ one two │ three four │") == []
    assert token_lines("one two three four") == [["one", "two", "three", "four"]]
    hard, review = fidelity_signatures(r"text \[u2192]")
    assert not hard
    assert review == [
        "bracketed Unicode escape is visible; verify documented syntax"
    ]
    hard, review = fidelity_signatures(
        r"literal \[u2192] example", r"author writes \\[u2192] literally"
    )
    assert not hard
    assert not review
    # CVS preconv.c rewrites the raw Ukrainian `е` to `\[u0435]`; the
    # preceding authored backslash remains literal, so this is documentation
    # text rather than an unhandled parser escape.
    hard, review = fidelity_signatures(
        r"literal \[u0435] example", "author writes \\е literally"
    )
    assert not hard
    assert not review
    for source in [
        r"author writes \\[u2192] literally",
        r"author writes \e[u2192] literally",
        r"author writes \[rs][u2192] literally",
    ]:
        hard, review = fidelity_signatures(r"literal \[u2192] example", source)
        assert not hard
        assert not review
    hard, review = fidelity_signatures(r"literal \[u2192] example", r"u2192 appears in prose")
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
    ) == ["mdoc function declaration terminator is missing for function"]
    assert not differential_signatures(
        ".Fn function\n", "function();", "function();"
    )
    assert differential_signatures(
        '.Fo function\n.Fa "int first" "int second"\n.Fc\n',
        "function(int first, int second);",
        "function(int first int second);",
    )[-1] == "mdoc multi-operand Fa separator is missing in int first, int second"
    assert not differential_signatures(
        '.Fo function\n.Fa "int first" "int second"\n.Fc\n',
        "function(int first, int second); prose, has unrelated commas, too.",
        "function(int first, int second);",
    )
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
            and (not arguments.reference_id
                 or arguments.oracle_attestation is None
                 or arguments.oracle_archive is None)
        ):
            raise ValueError("mandoc audits require --reference-id, --oracle-attestation and --oracle-archive")
        if arguments.reference_kind == "mandoc":
            reference_path = Path(arguments.reference)
            if not reference_path.is_absolute():
                raise ValueError("mandoc reference must be an explicit absolute path")
            arguments.oracle_preflight = mandoc_oracle.preflight(
                ROOT, reference_path, arguments.oracle_archive.resolve(),
                arguments.oracle_attestation.resolve(), arguments.reference_id, "utf8",
            )
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
        "  contract:  visible tokens, continuity, and source-conditioned punctuation; "
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
        if arguments.reference_kind == "mandoc":
            after = mandoc_oracle.preflight(
                ROOT, Path(arguments.reference), arguments.oracle_archive.resolve(),
                arguments.oracle_attestation.resolve(), arguments.reference_id, "utf8",
            )
            if after != arguments.oracle_preflight:
                print("audit-roff-fidelity: oracle identity drifted during the audit", file=sys.stderr)
                return 2
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
