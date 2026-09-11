"""Shared discovery and identity helpers for local roff audit drivers.

The module owns only deterministic, renderer-independent mechanics plus the
shared failure-isolating JSON-lines transport used by native profilers. Audit
contracts, ledger formats, response interpretation, and the choice of review
policy remain in their individual drivers. The identical target/semantic
per-result review-state transition is shared explicitly; evolving one oracle
must not silently change another oracle's interpretation.
"""

from __future__ import annotations

import argparse
import bz2
from dataclasses import dataclass, asdict
import gzip
import hashlib
import json
import lzma
import math
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

# The renderer boundary gives every child a 1 GiB address-space ceiling.  That
# is a containment limit, not a prediction of normal resident memory, so the
# automatic worker count reserves the same amount per concurrent audit item.
# It deliberately stays conservative on small CI containers while allowing a
# local review host to use the cores it actually has.  An explicit --workers
# remains available for an informed local override, up to the hard cap.
MAX_AUDIT_WORKERS = 16
AUDIT_WORKER_MEMORY_RESERVE = 1024 * 1024 * 1024
AUDIT_FD_HEADROOM = 128
AUDIT_FDS_PER_WORKER = 8


@dataclass(frozen=True)
class AuditParallelism:
    """Resolved local audit concurrency and the bounded-resource inputs.

    ``automaticWorkers`` is an advisory capacity, not a security guarantee:
    source parsing and native renderers have their own per-process resource
    limits, and an explicit command-line value intentionally overrides this
    local recommendation.  Persisting the inputs makes a full-corpus result
    reproducible enough to explain why two hosts chose different defaults.
    """

    workers: int
    requestedWorkers: int | None
    automaticWorkers: int
    cpuLimit: int
    memoryLimit: int | None
    fileDescriptorLimit: int | None
    hardLimit: int
    explicitOverride: bool

    def report(self) -> dict[str, int | bool | None]:
        return asdict(self)


def _read_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="ascii").strip()
    except (OSError, UnicodeError):
        return None


def _positive_int(value: str | None) -> int | None:
    if value is None:
        return None
    try:
        parsed = int(value)
    except ValueError:
        return None
    return parsed if parsed > 0 else None


def audit_cpu_limit() -> int:
    """Return the effective CPU capacity, honoring affinity and cgroup v2."""
    try:
        available = len(os.sched_getaffinity(0))
    except (AttributeError, OSError):
        available = os.cpu_count() or 1
    cgroup = _read_text(Path("/sys/fs/cgroup/cpu.max"))
    if cgroup:
        fields = cgroup.split()
        if len(fields) == 2 and fields[0] != "max":
            quota, period = (_positive_int(field) for field in fields)
            if quota is not None and period is not None:
                available = min(available, max(1, math.ceil(quota / period)))
    return max(1, available)


def audit_memory_available() -> int | None:
    """Return a conservative available-memory bound when this host exposes one."""
    available = None
    meminfo = _read_text(Path("/proc/meminfo"))
    if meminfo:
        for line in meminfo.splitlines():
            field, _, value = line.partition(":")
            if field != "MemAvailable":
                continue
            fields = value.split()
            if fields:
                kib = _positive_int(fields[0])
                if kib is not None:
                    available = kib * 1024
            break
    # cgroup v2 expresses a hard memory capacity separately from the current
    # use.  A container may expose a large host MemAvailable while still being
    # tightly capped, so take the smaller value when both observations exist.
    maximum = _read_text(Path("/sys/fs/cgroup/memory.max"))
    current = _positive_int(_read_text(Path("/sys/fs/cgroup/memory.current")))
    limit = _positive_int(maximum) if maximum != "max" else None
    if limit is not None:
        cgroup_available = max(0, limit - (current or 0))
        available = cgroup_available if available is None else min(available, cgroup_available)
    return available


def audit_file_descriptor_limit() -> int | None:
    try:
        import resource

        limit, _ = resource.getrlimit(resource.RLIMIT_NOFILE)
    except (ImportError, AttributeError, OSError):
        return None
    if limit == getattr(resource, "RLIM_INFINITY", limit):
        return None
    return int(limit) if limit > 0 else None


def resolve_audit_parallelism(
    requested_workers: int | None,
    *,
    hard_limit: int = MAX_AUDIT_WORKERS,
    cpu_limit: int | None = None,
    memory_available: int | None = None,
    file_descriptor_limit: int | None = None,
) -> AuditParallelism:
    """Choose a bounded automatic worker count or validate an explicit one.

    The worker owns one source plus sequential renderer/comparison children;
    batching is resolved separately and never creates extra simultaneous
    workers.  Explicit values preserve the old local tuning escape hatch, but
    cannot exceed the shared hard cap.
    """
    if hard_limit < 1:
        raise ValueError("parallelism hard limit must be positive")
    if requested_workers is not None and not 1 <= requested_workers <= hard_limit:
        raise ValueError(f"workers 1..{hard_limit} required")
    cpu = max(1, cpu_limit if cpu_limit is not None else audit_cpu_limit())
    memory = memory_available if memory_available is not None else audit_memory_available()
    descriptors = file_descriptor_limit if file_descriptor_limit is not None else audit_file_descriptor_limit()
    candidates = [hard_limit, cpu]
    memory_workers = None
    if memory is not None:
        memory_workers = max(1, memory // AUDIT_WORKER_MEMORY_RESERVE)
        candidates.append(memory_workers)
    descriptor_workers = None
    if descriptors is not None:
        descriptor_workers = max(1, (descriptors - AUDIT_FD_HEADROOM) // AUDIT_FDS_PER_WORKER)
        candidates.append(descriptor_workers)
    automatic = max(1, min(candidates))
    workers = requested_workers if requested_workers is not None else automatic
    return AuditParallelism(
        workers=workers,
        requestedWorkers=requested_workers,
        automaticWorkers=automatic,
        cpuLimit=cpu,
        memoryLimit=memory_workers,
        fileDescriptorLimit=descriptor_workers,
        hardLimit=hard_limit,
        explicitOverride=requested_workers is not None and requested_workers != automatic,
    )


def default_audit_batch_size(workers: int, *, maximum: int = 64) -> int:
    """Keep two waves of completed records without growing corpus-sized queues."""
    if workers < 1 or maximum < 1:
        raise ValueError("positive worker and batch limits required")
    return min(maximum, workers * 2)


def merge_clean_review_state(previous: str | None, status: str) -> str:
    """Existing target/semantic transition, after driver-specific validation.

    This only merges one newly scanned result, never rescans or rewrites an
    old ledger. Durable human conclusions remain authoritative. Other drivers
    keep their own candidate/status policies and schema validation.
    """
    if previous is None:
        return "not-required" if status == "clean" else "pending"
    if previous == "not-required" and status != "clean":
        return "pending"
    if previous == "pending" and status == "clean":
        return "not-required"
    return previous


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


def run_jsonl_profile_batch(
    profiler: Path,
    requests: dict[str, dict[str, str]],
    timeout: int,
    profile_name: str,
) -> dict[str, dict[str, object]]:
    """Run a JSON-lines profiler and isolate abnormal exits to one request."""
    if not requests:
        return {}
    payload = "".join(
        json.dumps(request, ensure_ascii=False) + "\n" for request in requests.values()
    )
    try:
        result = subprocess.run(
            [str(profiler)],
            input=payload,
            text=True,
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
                f"{profile_name} profiler timed out after {timeout}s"
                if result is None
                else result.stderr.strip()
                or f"{profile_name} profiler exited with status {result.returncode}"
            )
            return {request_id: {"id": request_id, "error": detail}}
        items = list(requests.items())
        midpoint = len(items) // 2
        return {
            **run_jsonl_profile_batch(
                profiler, dict(items[:midpoint]), timeout, profile_name
            ),
            **run_jsonl_profile_batch(
                profiler, dict(items[midpoint:]), timeout, profile_name
            ),
        }

    responses: dict[str, dict[str, object]] = {}
    for number, line in enumerate(result.stdout.splitlines(), 1):
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(
                f"{profile_name} profiler returned invalid JSON on line {number}"
            ) from error
        request_id = response.get("id")
        if (
            not isinstance(request_id, str)
            or request_id not in requests
            or request_id in responses
        ):
            raise ValueError(
                f"{profile_name} profiler returned an invalid id on line {number}"
            )
        responses[request_id] = response
    for request_id in requests:
        responses.setdefault(
            request_id, {"id": request_id, "error": "profiler returned no response"}
        )
    return responses


def run_bounded_profile_batch(profiler, requests, timeout):
    """Bounded JSONL transport for manifest orchestration, not a new oracle.

    A batch has one wall-time budget, including crash isolation retries. Output
    and child-process memory limits come from the shared reference boundary.
    Transport failures retain a typed execution status separate from findings.
    """
    import time
    from roff_reference import reference_environment, run_renderer

    deadline = time.monotonic() + timeout

    def failure(batch, execution, detail):
        return {key: {"id": key, "error": detail, "_execution": execution}
                for key in batch}

    def run(batch):
        if not batch:
            return {}
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return failure(batch, "budget", "profiler batch wall-time budget exhausted")
        payload = ''.join(json.dumps(value, ensure_ascii=False) + '\n'
                          for value in batch.values()).encode()
        code, output, error = run_renderer([str(profiler)], remaining,
            reference_environment(), payload, binary_output=True)
        if code:
            budget = code in (124, -24, -9) or (code == 125 and
                any(word in error for word in ('exceeds', 'budget', 'memory')))
            if budget:
                return failure(batch, "budget", error or f"profiler resource exit {code}")
            if len(batch) > 1:
                items = list(batch.items()); half = len(items) // 2
                return {**run(dict(items[:half])), **run(dict(items[half:]))}
            return failure(batch, "error", error or f"profiler exit {code}")
        responses = {}
        try:
            for line in output.decode('utf-8').splitlines():
                response = json.loads(line)
                if not isinstance(response, dict):
                    raise ValueError('response is not an object')
                key = response.get('id')
                if not isinstance(key, str) or key not in batch or key in responses:
                    raise ValueError('unknown or duplicate response ID')
                responses[key] = response
        except (UnicodeError, ValueError) as error:
            return failure(batch, 'error', f'invalid profiler transport: {error}')
        for key in batch:
            responses.setdefault(key, failure({key: None}, 'error', 'missing profiler response')[key])
        return responses

    return run(requests)
