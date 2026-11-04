"""Read versioned syntax caches and collect native profiler observations."""
from __future__ import annotations

import gzip
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Sequence

from .model import SyntaxProfile
from .sources import manual_hierarchy_root


SYNTAX_PROFILE_SCHEMA = "mant.roff-ast-profile/v2"
SYNTAX_CACHE_VERSION = 3


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
