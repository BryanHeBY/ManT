#!/usr/bin/env python3
"""Run independent native mantdoc conformance lanes concurrently.

The native canonical snapshot plus M3, M4, and M6 are independent whole-corpus
tasks. The strict lint, M5, and M9 renderer-golden gates additionally partition their
checksum-ordered corpus by ``case_index % shard_count``. This helper builds the
feature-gated tools once, runs every independent task concurrently, then sums
machine-readable counters. A shard always validates the whole upstream
inventory before selecting its own cases, so no filesystem walk or scheduling
order becomes part of the result.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import os
import pathlib
import subprocess
import sys
from dataclasses import dataclass


ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_LANES = ("canonical", "lint", "m3", "m4", "m5", "m6", "m9")
SHARDED_LANES = frozenset(("lint", "m5", "m9"))
KNOWN_LANES = frozenset(DEFAULT_LANES)
AGGREGATE_COUNTERS = {
    "case_count",
    "canonical_case_count",
    "diagnostic_case_count",
    "renderer_output_count",
    "renderer_equal_output_count",
    "renderer_difference_output_count",
    "renderer_error_output_count",
    "lint_output_count",
    "lint_equal_output_count",
    "lint_difference_output_count",
    "lint_error_output_count",
    "lint_external_output_count",
}


@dataclass
class Outcome:
    lane: str
    shard_index: int
    returncode: int
    output: str


def command_for(
    lane: str,
    archive: pathlib.Path,
    shard: str,
    list_renderer_differences: bool,
) -> list[str]:
    inventory = ROOT / "target" / "debug" / "examples" / "mantdoc-corpus-inventory"
    canonical = ROOT / "target" / "debug" / "examples" / "mantdoc-canonical-snapshot"
    lint_diff = ROOT / "target" / "debug" / "examples" / "mantdoc-lint-diff"
    renderer_diff = ROOT / "target" / "debug" / "examples" / "mantdoc-render-diff"
    if lane == "canonical":
        return [
            str(canonical),
            str(archive),
            "--verify",
            str(ROOT / "crates/mantdoc/tests/conformance/data/mandoc-1.14.6-native-canonical.sha256"),
        ]
    if lane == "m3":
        return [str(inventory), str(archive), "--m3-execution"]
    if lane == "m4":
        return [str(inventory), str(archive), "--m4-man-smoke"]
    if lane == "m5":
        return [str(inventory), str(archive), "--m5-mdoc-smoke-shard", shard]
    if lane == "m6":
        return [str(inventory), str(archive), "--m6-preprocess-smoke"]
    if lane == "lint":
        return [str(lint_diff), str(archive), "--all-shard", shard]
    renderer_mode = (
        "--all-list-differences-shard" if list_renderer_differences else "--all-shard"
    )
    return [str(renderer_diff), str(archive), renderer_mode, shard]


def run_lane_shard(
    archive: pathlib.Path,
    lane: str,
    shard_index: int,
    shard_count: int,
    list_renderer_differences: bool,
) -> Outcome:
    shard = f"{shard_index}/{shard_count}"
    completed = subprocess.run(
        command_for(lane, archive, shard, list_renderer_differences and lane == "m9"),
        cwd=ROOT,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        check=False,
    )
    return Outcome(lane, shard_index, completed.returncode, completed.stdout)


def counters(output: str) -> dict[str, int]:
    values: dict[str, int] = {}
    for line in output.splitlines():
        key, separator, value = line.partition("=")
        if separator and key in AGGREGATE_COUNTERS and value.isdecimal():
            values[key] = int(value)
    return values


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive", type=pathlib.Path, help="pinned mandoc-1.14.6 archive")
    parser.add_argument(
        "--shards",
        type=int,
        default=min(os.cpu_count() or 1, 12),
        help="number of deterministic process shards (default: min(CPU count, 12))",
    )
    parser.add_argument(
        "--jobs",
        type=int,
        default=min(os.cpu_count() or 1, 20),
        help="maximum concurrent worker processes (default: min(CPU count, 20))",
    )
    parser.add_argument(
        "--lanes",
        default=",".join(DEFAULT_LANES),
        help=(
            "comma-separated subset of canonical,lint,m3,m4,m5,m6,m9 "
            "(default: canonical, strict lint, and M3-M6/M9)"
        ),
    )
    parser.add_argument(
        "--list-renderer-differences",
        action="store_true",
        help="print M9 differences from every deterministic shard (requires m9 lane)",
    )
    parser.add_argument("--verbose", action="store_true", help="print every shard report")
    args = parser.parse_args()
    if args.shards < 1:
        parser.error("--shards must be positive")
    if args.jobs is not None and args.jobs < 1:
        parser.error("--jobs must be positive")
    lanes = tuple(lane for lane in args.lanes.split(",") if lane)
    if not lanes or any(lane not in KNOWN_LANES for lane in lanes):
        parser.error("--lanes must be a nonempty subset of canonical,lint,m3,m4,m5,m6,m9")
    if args.list_renderer_differences and "m9" not in lanes:
        parser.error("--list-renderer-differences requires the m9 lane")
    if not args.archive.is_file():
        parser.error(f"archive does not exist: {args.archive}")

    features = []
    if "m9" in lanes:
        features.append("render")
    build_command = [
        "cargo",
        "build",
        "--locked",
        "--package",
        "mantdoc",
        "--examples",
    ]
    if features:
        build_command.extend(["--features", ",".join(features)])
    build = subprocess.run(
        build_command,
        cwd=ROOT,
        check=False,
    )
    if build.returncode:
        return build.returncode

    outcomes: list[Outcome] = []
    tasks = [
        (lane, shard_index, shard_count)
        for lane in lanes
        for shard_index, shard_count in (
            ((index, args.shards) for index in range(args.shards))
            if lane in SHARDED_LANES
            else ((0, 1),)
        )
    ]
    jobs = min(len(tasks), args.jobs)
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as executor:
        # Difference enumeration remains parallel: each M9 worker owns a
        # stable case-index partition and emits only its local findings.
        # This avoids falling back to the slow sequential all-corpus CLI
        # when triaging the next renderer work item.
        pending = [
            executor.submit(
                run_lane_shard,
                args.archive,
                lane,
                index,
                shard_count,
                args.list_renderer_differences,
            )
            for lane, index, shard_count in tasks
        ]
        for future in concurrent.futures.as_completed(pending):
            outcomes.append(future.result())
    outcomes.sort(key=lambda outcome: (outcome.lane, outcome.shard_index))

    failed = [outcome for outcome in outcomes if outcome.returncode]
    # Keep successful routine gates compact.  Printing every successful shard
    # after one failure both buries the actionable output and needlessly slows
    # interactive runs; `--verbose` remains available for full evidence.
    reported = outcomes if args.verbose or args.list_renderer_differences else failed
    for outcome in reported:
        print(f"[{outcome.lane} shard {outcome.shard_index}/{args.shards}]")
        print(outcome.output, end="" if outcome.output.endswith("\n") else "\n")

    for lane in lanes:
        lane_outcomes = [outcome for outcome in outcomes if outcome.lane == lane]
        totals: dict[str, int] = {}
        for outcome in lane_outcomes:
            for key, value in counters(outcome.output).items():
                if key not in {"shard_index", "shard_count"}:
                    totals[key] = totals.get(key, 0) + value
        print(f"lane={lane}")
        for key in sorted(totals):
            print(f"{key}={totals[key]}")

    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
