"""Shared scheduling, resource limits and batching for local roff audits.

The drivers supply their own audit contracts, ledger formats and response
interpretation. The shared review-state transition is used by the target and
semantic audits.
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass, asdict
import json
import math
import os
import subprocess
import tempfile
from collections import Counter
from pathlib import Path
from typing import Callable, Iterable, Protocol, Sequence, TypeVar

# Existing drivers import source helpers through this common entry point.
from scripts.roff.lib.roff_audit_sources import (
    MANUAL_SUFFIX as MANUAL_SUFFIX,
    compile_source_patterns as compile_source_patterns,
    discover_pages as discover_pages,
    filter_pages_by_source as filter_pages_by_source,
    manual_compression as manual_compression,
    manual_hierarchy_root as manual_hierarchy_root,
    manual_section as manual_section,
    manual_transport_error as manual_transport_error,
    observe_sources as observe_sources,
    read_verified_manual as read_verified_manual,
    relative_label as relative_label,
    source_bytes as source_bytes,
    source_digest as source_digest,
    uncovered_sources as uncovered_sources,
    verify_source_digest,
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
class SourceReview:
    line: int
    column: int
    reason: str


def is_nonnegative_integer(value: object) -> bool:
    """Wire counts and coordinates exclude JSON booleans."""
    return type(value) is int and value >= 0


def source_reviews(positions: object, reason: str) -> list[SourceReview] | None:
    """Validate the source's coordinates and retain every occurrence."""
    if not isinstance(positions, list):
        return None
    reviews = []
    for position in positions:
        if (not isinstance(position, list) or len(position) != 2
                or any(type(value) is not int or value < 0 for value in position)):
            return None
        reviews.append(SourceReview(*position, reason=reason))
    return reviews


def ordinal_source_reviews(response: dict[str, object]) -> list[SourceReview] | None:
    return source_reviews(
        response.get("ordinalCasesForReview"),
        "ordinal list HEAD or BODY has unproved roff execution effects",
    )


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


class _ProfileFinding(Protocol):
    @property
    def path(self) -> str: ...


_Finding = TypeVar("_Finding", bound=_ProfileFinding)


def verified_profile_findings(
    pages: Sequence[Path],
    sources: dict[Path, tuple[str, str | None]],
    failures: dict[Path, str],
    profile: Callable[[list[Path]], Iterable[_Finding]],
    reject: Callable[[str, str], _Finding],
) -> list[_Finding]:
    """Observe compatible files, with transport failures outside ledger identity."""
    readable = [page for page in pages if page not in failures]
    findings = {item.path: item for item in profile(readable)} if readable else {}
    result = []
    for page in pages:
        label, digest = sources[page]
        if page in failures:
            result.append(reject(label, f"could not decode source: {failures[page]}"))
        elif digest is None or not verify_source_digest(page, digest):
            sources[page] = label, None
            result.append(reject(label, "source changed during audit"))
        else:
            result.append(findings[label])
    return result


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


class RefreshQueue:
    """Age repeated observations without changing the source-identity ledger."""

    def __init__(self, database: Path, profile: str, corpus: str, roots: Sequence[Path],
                 repeated: Iterable[Path], *, enabled: bool):
        self.path = database.with_suffix(database.suffix + ".refresh.json")
        self.scope = json.dumps([profile, corpus, *map(str, roots)])
        self.labels = {page: relative_label(page, roots) for page in repeated} if enabled else {}
        self.scopes: dict[str, list[str]] = {}
        if self.labels and self.path.exists():
            try:
                data = json.loads(self.path.read_text(encoding="utf-8"))
            except (OSError, UnicodeError) as error:
                raise ValueError(f"cannot read refresh order: {self.path}: {error}") from error
            if not isinstance(data, dict) or any(
                not isinstance(labels, list) or not all(isinstance(label, str) for label in labels)
                for labels in data.values()
            ):
                raise ValueError(f"invalid refresh order: {self.path}")
            self.scopes = data

    def order(self, pages: list[Path]) -> None:
        ages = {label: index for index, label in enumerate(self.scopes.get(self.scope, []))}
        # Keep each driver's priority among unseen inputs. Repeated inputs
        # then run least recently attempted first, including failures.
        pages.sort(key=lambda page: ages.get(self.labels.get(page, ""), -1))

    def record(self, pages: Sequence[Path]) -> None:
        attempted = dict.fromkeys(self.labels[page] for page in pages if page in self.labels)
        if not attempted:
            return
        # Use logical names so replacing or restoring the stored bytes does
        # not reset a path's turn. Keep previously filtered paths in order.
        order = self.scopes.get(self.scope, [])
        self.scopes[self.scope] = [*(label for label in order if label not in attempted), *attempted]
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=self.path.parent,
                                         prefix=f".{self.path.name}.", delete=False) as stream:
            temporary = Path(stream.name)
            try:
                json.dump(self.scopes, stream, ensure_ascii=False)
                stream.write("\n")
                stream.close()
                temporary.replace(self.path)
            finally:
                temporary.unlink(missing_ok=True)


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


def _profile_jsonl_records(output: str) -> list[str]:
    """JSONL is LF-framed; U+0085/U+2028/U+2029 remain JSON string data.

    Remove only the record terminator, preserving invalid interior blank
    records for the existing transport validation instead of skipping them.
    """
    return output.removesuffix("\n").split("\n") if output else []


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
    for number, line in enumerate(_profile_jsonl_records(result.stdout), 1):
        try:
            response = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(
                f"{profile_name} profiler returned invalid JSON on line {number}"
            ) from error
        if not isinstance(response, dict):
            raise ValueError(f"{profile_name} profiler returned a non-object JSON record on line {number}")
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
    from scripts.roff.lib.roff_reference import reference_environment, run_renderer

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
            for line in _profile_jsonl_records(output.decode('utf-8')):
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
