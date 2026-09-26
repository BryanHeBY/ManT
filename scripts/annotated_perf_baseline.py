#!/usr/bin/env python3
"""Reproducible, interleaved annotated/old/native process benchmark.

Only frozen binaries and decoded roff inputs are accepted.  This script does
not build, decompress, or mutate either of them.  See the PERF00 README for
the manifest and the limits of process-level measurements.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import statistics
import subprocess
import sys
import time
from typing import Any


ROOT = Path(__file__).resolve().parent.parent
TARGET = (ROOT / "target").resolve()
OUTPUT_ROUNDS = 12
QUERY_ROUNDS = 8
OUTPUT_ROUTES = ("native", "old", "annotated")
QUERY_ROUTES = ("old", "annotated")
OPERATIONS = ("explain", "search")
TIME_MARKER = "__MANT_PERF_RSS_KIB__"
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def canonical_path(value: str, base: Path) -> Path:
    path = Path(value)
    return (path if path.is_absolute() else base / path).resolve()


def checked_artifact(value: Any, label: str, base: Path) -> tuple[Path, str]:
    if not isinstance(value, dict) or not isinstance(value.get("path"), str):
        raise ValueError(f"{label} needs a path and sha256")
    expected = value.get("sha256")
    if not isinstance(expected, str) or SHA256_RE.fullmatch(expected) is None:
        raise ValueError(f"{label} needs a lowercase SHA-256")
    path = canonical_path(value["path"], base)
    if not path.is_file():
        raise ValueError(f"{label} is not a file: {path}")
    return path, expected


def artifacts(manifest: dict[str, Any], base: Path) -> dict[str, tuple[Path, str]]:
    binaries = manifest.get("binaries")
    fixtures = manifest.get("fixtures")
    if not isinstance(binaries, dict) or not isinstance(fixtures, dict) or not fixtures:
        raise ValueError("manifest needs binaries and at least one fixture")
    found: dict[str, tuple[Path, str]] = {}
    for route in ("mant", "native"):
        found[route] = checked_artifact(binaries.get(route), f"binaries.{route}", base)
    for page, fixture in fixtures.items():
        if not isinstance(page, str) or not re.fullmatch(r"[a-zA-Z0-9_-]+", page):
            raise ValueError(f"invalid fixture key: {page!r}")
        if not isinstance(fixture, dict) or not isinstance(fixture.get("query"), str):
            raise ValueError(f"fixtures.{page} needs a query")
        found[f"fixture:{page}"] = checked_artifact(fixture, f"fixtures.{page}", base)
    if "oracle" in manifest:
        found["oracle"] = checked_artifact(manifest["oracle"], "oracle", base)
    return found


def frozen_directory_manifest(folder: Path, head: str) -> dict[str, Any]:
    """Freeze the conventional four-page directory into an in-memory manifest."""
    mant_files = tuple(folder.glob("mant-*"))
    if len(mant_files) != 1 or not mant_files[0].is_file():
        raise ValueError(f"expected exactly one frozen mant-* binary in {folder}")
    revision = mant_files[0].name.removeprefix("mant-")
    if re.fullmatch(r"[0-9a-f]{8,40}", revision) and not head.startswith(revision):
        raise ValueError(
            f"frozen binary name {mant_files[0].name} does not match checkout {head}; "
            "use an explicit manifest for cross-revision comparisons"
        )

    def artifact(filename: str) -> dict[str, str]:
        path = folder / filename
        if not path.is_file():
            raise ValueError(f"missing frozen artifact: {path}")
        return {"path": str(path), "sha256": digest(path)}

    return {
        "schemaVersion": 1,
        "label": folder.name,
        "sourceHead": head,
        "binaries": {"mant": artifact(mant_files[0].name), "native": artifact("mandoc-cvs")},
        "fixtures": {
            page: {**artifact(f"{page}.1"), "query": query}
            for page, query in (
                ("gcc", "-x"), ("git", "--help"),
                ("clang", "-help"), ("rclone", "--help"),
            )
        },
        "build": {"note": "Supply exact release feature/C flags and native build recipe in the tracked PERF00 record"},
    }


def verify_artifacts(found: dict[str, tuple[Path, str]], stage: str) -> None:
    for label, (path, expected) in found.items():
        actual = digest(path)
        if actual != expected:
            raise RuntimeError(f"{stage}: {label} SHA-256 changed: {path}: {actual} != {expected}")


def environment() -> dict[str, str]:
    env = os.environ.copy()
    env.update(
        LC_ALL="C.UTF-8",
        LANG="C.UTF-8",
        TZ="UTC",
        TERM="dumb",
        MANWIDTH="78",
        COLUMNS="78",
        NO_COLOR="1",
    )
    return env


def output_command(found: dict[str, tuple[Path, str]], page: str, route: str) -> list[str]:
    source = str(found[f"fixture:{page}"][0])
    if route == "native":
        return [str(found["native"][0]), "-Tutf8", "-O", "width=78", source]
    args = [
        str(found["mant"][0]), "--input", source, "--input-format", "roff",
        "--format", "text", "--display", "direct", "--color", "never",
    ]
    if route == "annotated":
        args.insert(1, "--annotated-preview")
    return args


def query_command(
    found: dict[str, tuple[Path, str]], page: str, route: str,
    operation: str, query: str,
) -> list[str]:
    args = [
        str(found["mant"][0]), "--input", str(found[f"fixture:{page}"][0]),
        "--input-format", "roff", "--format", "json", "--compact",
    ]
    if route == "annotated":
        args.insert(1, "--annotated-preview")
    if operation == "explain":
        args.append(f"--explain={query}")
    else:
        args.extend((f"--search={query}", "--limit", "10"))
    return args


def outline_command(found: dict[str, tuple[Path, str]], page: str, route: str) -> list[str]:
    args = [
        str(found["mant"][0]), "--input", str(found[f"fixture:{page}"][0]),
        "--input-format", "roff", "--outline", "--outline-entries", "all",
        "--outline-references", "none", "--format", "json", "--compact",
    ]
    if route == "annotated":
        args.insert(1, "--annotated-preview")
    return args


def json_summary(raw: bytes) -> dict[str, Any]:
    wire = json.loads(raw)
    if not isinstance(wire, dict):
        raise ValueError("expected a JSON object from the CLI")
    fields = ("total", "returned", "counts", "semanticsComplete", "coverageComplete")
    summary = {name: wire[name] for name in fields if name in wire}
    # The digest remains the complete response identity; this intentionally
    # records only a small semantic summary rather than duplicating page data.
    return summary


def output_identity(args: list[str], env: dict[str, str], *, json_output: bool) -> dict[str, Any]:
    result = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env, check=False)
    if result.returncode != 0:
        tail = result.stderr[-1000:].decode("utf-8", "replace")
        raise RuntimeError(f"identity command failed ({result.returncode}): {args!r}: {tail}")
    identity: dict[str, Any] = {
        "exitCode": result.returncode,
        "stdoutBytes": len(result.stdout),
        "stdoutSha256": hashlib.sha256(result.stdout).hexdigest(),
        "stderrBytes": len(result.stderr),
        "stderrSha256": hashlib.sha256(result.stderr).hexdigest(),
    }
    if json_output:
        identity["semanticSummary"] = json_summary(result.stdout)
    return identity


def sample(args: list[str], env: dict[str, str]) -> dict[str, float | int]:
    started = time.perf_counter_ns()
    result = subprocess.run(
        ["/usr/bin/time", "-f", TIME_MARKER + "%M:%U:%S", *args],
        stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, env=env, check=False,
    )
    wall_ms = (time.perf_counter_ns() - started) / 1_000_000
    stderr = result.stderr.decode("utf-8", "replace")
    values = re.findall(rf"{TIME_MARKER}(\d+):([0-9.]+):([0-9.]+)", stderr)
    if result.returncode != 0 or len(values) != 1:
        raise RuntimeError(
            f"sample command failed ({result.returncode}): {args!r}: {stderr[-1000:]}"
        )
    rss, user_seconds, system_seconds = values[0]
    return {
        "exitCode": result.returncode,
        "wallMs": wall_ms,
        "cpuMs": (float(user_seconds) + float(system_seconds)) * 1000,
        "rssKiB": int(rss),
    }


def distribution(values: list[float | int]) -> dict[str, float | int]:
    ordered = sorted(values)
    middle = len(ordered) // 2
    low = statistics.median(ordered[:middle])
    high = statistics.median(ordered[(len(ordered) + 1) // 2:])
    return {
        "median": statistics.median(ordered), "q1": low, "q3": high,
        "iqr": high - low, "min": ordered[0], "max": ordered[-1],
    }


def summarize(trials: list[dict[str, float | int]]) -> dict[str, Any]:
    return {
        "wallMs": distribution([trial["wallMs"] for trial in trials]),
        "cpuMs": distribution([trial["cpuMs"] for trial in trials]),
        "rssKiB": distribution([trial["rssKiB"] for trial in trials]),
    }


def git_state() -> dict[str, Any]:
    def git(*args: str) -> str:
        return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()

    diff = subprocess.check_output(["git", "diff", "--binary", "HEAD"], cwd=ROOT)
    untracked = subprocess.check_output(
        ["git", "ls-files", "--others", "--exclude-standard", "-z"], cwd=ROOT,
    )
    untracked_hashes = {
        name.decode("utf-8", "surrogateescape"): digest(ROOT / name.decode("utf-8", "surrogateescape"))
        for name in untracked.split(b"\0") if name
    }
    return {
        "head": git("rev-parse", "HEAD"),
        "status": git("status", "--porcelain", "--untracked-files=all"),
        "trackedDiffSha256": hashlib.sha256(diff).hexdigest(),
        "untrackedSha256": untracked_hashes,
    }


def system_identity() -> dict[str, Any]:
    cpu_model = "unknown"
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.is_file():
        for line in cpuinfo.read_text(encoding="utf-8", errors="replace").splitlines():
            if line.startswith("model name"):
                cpu_model = line.partition(":")[2].strip()
                break
    return {
        "platform": platform.platform(), "python": sys.version.split()[0],
        "cpuModel": cpu_model, "logicalCpus": os.cpu_count(),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, help="manifest JSON; paths are relative to repo root or --frozen-dir")
    parser.add_argument(
        "--frozen-dir", type=Path,
        help="directory containing one mant-* binary, mandoc-cvs, and gcc/git/clang/rclone .1 files",
    )
    parser.add_argument("--out", required=True, type=Path, help="new directory below repository target/")
    parser.add_argument("--allow-dirty", action="store_true", help="label a WIP candidate explicitly")
    args = parser.parse_args()

    if args.manifest is None and args.frozen_dir is None:
        parser.error("supply --manifest or --frozen-dir")
    frozen_dir = (
        canonical_path(str(args.frozen_dir), ROOT) if args.frozen_dir is not None else None
    )
    manifest_path = (
        canonical_path(str(args.manifest), ROOT) if args.manifest is not None else None
    )
    start_repo = git_state()
    if manifest_path is not None:
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    else:
        assert frozen_dir is not None
        manifest = frozen_directory_manifest(frozen_dir, start_repo["head"])
    if not isinstance(manifest, dict) or manifest.get("schemaVersion") != 1:
        raise ValueError("manifest must be a schemaVersion 1 JSON object")
    if not isinstance(manifest.get("label"), str) or not manifest["label"]:
        raise ValueError("manifest needs a nonempty label")
    found = artifacts(manifest, frozen_dir or ROOT)
    manifest_hash = (
        digest(manifest_path) if manifest_path is not None
        else hashlib.sha256(json.dumps(manifest, sort_keys=True).encode("utf-8")).hexdigest()
    )
    verify_artifacts(found, "before")
    if manifest.get("sourceHead") != start_repo["head"]:
        raise ValueError("manifest sourceHead does not match the current checkout")
    if start_repo["status"] and not args.allow_dirty:
        raise ValueError("worktree is dirty; pass --allow-dirty for an explicit WIP measurement")
    if not Path("/usr/bin/time").is_file():
        raise RuntimeError("/usr/bin/time is required for peak RSS")

    out = (args.out if args.out.is_absolute() else ROOT / args.out).resolve()
    if out == TARGET or not out.is_relative_to(TARGET):
        raise ValueError("--out must name a new child directory of repository target/")
    out.mkdir(parents=True, exist_ok=False)

    env = environment()
    pages = tuple(sorted(manifest["fixtures"]))
    output_identity_map: dict[str, Any] = {}
    query_identity_map: dict[str, Any] = {}
    outline_identity_map: dict[str, Any] = {}
    output_trials: dict[str, Any] = {}
    query_trials: dict[str, Any] = {}

    for page in pages:
        query = manifest["fixtures"][page]["query"]
        output_identity_map[page] = {}
        query_identity_map[page] = {operation: {} for operation in OPERATIONS}
        outline_identity_map[page] = {}
        output_trials[page] = {route: [] for route in OUTPUT_ROUTES}
        query_trials[page] = {
            operation: {route: [] for route in QUERY_ROUTES} for operation in OPERATIONS
        }
        for route in OUTPUT_ROUTES:
            command = output_command(found, page, route)
            output_identity_map[page][route] = output_identity(command, env, json_output=False)
            sample(command, env)
        for route in QUERY_ROUTES:
            outline_identity_map[page][route] = output_identity(
                outline_command(found, page, route), env, json_output=True,
            )
            for operation in OPERATIONS:
                command = query_command(found, page, route, operation, query)
                query_identity_map[page][operation][route] = output_identity(
                    command, env, json_output=True,
                )
                sample(command, env)

    route_orders = (
        OUTPUT_ROUTES, ("annotated", "native", "old"), ("old", "annotated", "native"),
    )
    for round_index in range(OUTPUT_ROUNDS):
        ordered_pages = pages if round_index % 2 == 0 else tuple(reversed(pages))
        for page in ordered_pages:
            for route in route_orders[round_index % len(route_orders)]:
                output_trials[page][route].append(sample(output_command(found, page, route), env))
        print(f"output round {round_index + 1}/{OUTPUT_ROUNDS}", flush=True)

    for round_index in range(QUERY_ROUNDS):
        ordered_pages = pages if round_index % 2 == 0 else tuple(reversed(pages))
        operations = OPERATIONS if round_index % 2 == 0 else tuple(reversed(OPERATIONS))
        routes = QUERY_ROUTES if round_index % 2 == 0 else tuple(reversed(QUERY_ROUTES))
        for page in ordered_pages:
            query = manifest["fixtures"][page]["query"]
            for operation in operations:
                for route in routes:
                    query_trials[page][operation][route].append(
                        sample(query_command(found, page, route, operation, query), env)
                    )
        print(f"query round {round_index + 1}/{QUERY_ROUNDS}", flush=True)

    verify_artifacts(found, "after")
    if manifest_path is not None and digest(manifest_path) != manifest_hash:
        raise RuntimeError("manifest changed during the benchmark")
    end_repo = git_state()
    if end_repo != start_repo:
        raise RuntimeError("repository changed during the benchmark")
    report = {
        "schemaVersion": 1,
        "label": manifest["label"],
        "rounds": {"output": OUTPUT_ROUNDS, "query": QUERY_ROUNDS},
        "manifestSha256": manifest_hash,
        "manifest": manifest,
        "artifacts": {
            label: {"path": str(path), "sha256": expected}
            for label, (path, expected) in found.items()
        },
        "build": manifest.get("build"),
        "oracleIdentity": manifest.get("oracleIdentity"),
        "repository": start_repo,
        "system": system_identity(),
        "environment": {
            key: env[key] for key in ("LC_ALL", "LANG", "TZ", "TERM", "MANWIDTH", "COLUMNS", "NO_COLOR")
        },
        "method": (
            "fresh process per sample; warm-up once per output/query command; "
            "interleaved routes and reversed page order; stdout to /dev/null; "
            "wall includes /usr/bin/time wrapper, startup, rendering, write, exit; "
            "CPU is /usr/bin/time user + system at 10 ms resolution; "
            "RSS is /usr/bin/time maximum resident set size"
        ),
        "commands": {
            page: {
                "output": {route: output_command(found, page, route) for route in OUTPUT_ROUTES},
                "outline": {route: outline_command(found, page, route) for route in QUERY_ROUTES},
                "query": {
                    operation: {
                        route: query_command(
                            found, page, route, operation, manifest["fixtures"][page]["query"],
                        ) for route in QUERY_ROUTES
                    } for operation in OPERATIONS
                },
            } for page in pages
        },
        "outputIdentity": output_identity_map,
        "outlineIdentity": outline_identity_map,
        "queryIdentity": query_identity_map,
        "outputTrials": output_trials,
        "queryTrials": query_trials,
        "outputSummary": {
            page: {route: summarize(output_trials[page][route]) for route in OUTPUT_ROUTES}
            for page in pages
        },
        "querySummary": {
            page: {
                operation: {route: summarize(query_trials[page][operation][route]) for route in QUERY_ROUTES}
                for operation in OPERATIONS
            } for page in pages
        },
    }
    result_path = out / "results.json"
    with result_path.open("x", encoding="utf-8") as result_file:
        json.dump(report, result_file, indent=2, ensure_ascii=False)
        result_file.write("\n")
    print(f"wrote {result_path}")
    for page in pages:
        values = report["outputSummary"][page]
        formatted = ", ".join(
            f"{route}={values[route]['wallMs']['median']:.2f} ms/{values[route]['rssKiB']['median']:.0f} KiB"
            for route in OUTPUT_ROUTES
        )
        print(f"{page}: {formatted}")


if __name__ == "__main__":
    main()
