#!/usr/bin/env python3
"""Compare two frozen annotated CLIs in one interleaved output session.

This complements annotated_perf_baseline.py when a small optimization is
otherwise indistinguishable from drift between two separate 12/8 reports.
It measures output only; query identities and timings remain in those full
reports. Never build or mutate a binary while this sampler is running.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import annotated_perf_baseline as baseline


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", required=True, type=Path, help="frozen earlier ManT binary")
    parser.add_argument("--candidate", required=True, type=Path, help="frozen candidate ManT binary")
    parser.add_argument("--fixtures", required=True, type=Path, help="directory with four decoded .1 pages")
    parser.add_argument("--out", required=True, type=Path, help="new directory under repository target/")
    parser.add_argument("--rounds", type=int, default=12)
    args = parser.parse_args()
    if args.rounds < 4:
        parser.error("--rounds must be at least 4")

    binaries = {
        label: baseline.canonical_path(str(path), baseline.ROOT)
        for label, path in (("base", args.base), ("candidate", args.candidate))
    }
    fixture_dir = baseline.canonical_path(str(args.fixtures), baseline.ROOT)
    pages = ("gcc", "git", "clang", "rclone")
    fixtures = {page: fixture_dir / f"{page}.1" for page in pages}
    artifacts = {**binaries, **{f"fixture:{page}": path for page, path in fixtures.items()}}
    for label, path in artifacts.items():
        if not path.is_file() or not path.is_relative_to(baseline.TARGET):
            parser.error(f"{label} must be a frozen file below repository target/: {path}")
    start_hashes = {label: baseline.digest(path) for label, path in artifacts.items()}
    start_repository = baseline.git_state()
    out = baseline.canonical_path(str(args.out), baseline.ROOT)
    if out == baseline.TARGET or not out.is_relative_to(baseline.TARGET):
        parser.error("--out must be a new child directory of repository target/")
    out.mkdir(parents=True, exist_ok=False)

    env = baseline.environment()

    def command(page: str, label: str) -> list[str]:
        return [
            str(binaries[label]), "--annotated-preview", "--input", str(fixtures[page]),
            "--input-format", "roff", "--format", "text", "--display", "direct",
            "--color", "never",
        ]

    identities: dict[str, dict[str, object]] = {}
    trials: dict[str, dict[str, list[dict[str, float | int]]]] = {}
    for page in pages:
        identities[page] = {
            label: baseline.output_identity(command(page, label), env, json_output=False)
            for label in binaries
        }
        if identities[page]["base"] != identities[page]["candidate"]:
            raise RuntimeError(f"{page}: candidate output or diagnostics differ from base")
        trials[page] = {label: [] for label in binaries}
        for label in binaries:
            baseline.sample(command(page, label), env)

    for round_index in range(args.rounds):
        ordered_pages = pages if round_index % 2 == 0 else tuple(reversed(pages))
        labels = ("base", "candidate") if round_index % 2 == 0 else ("candidate", "base")
        for page in ordered_pages:
            for label in labels:
                trials[page][label].append(baseline.sample(command(page, label), env))
        print(f"paired output round {round_index + 1}/{args.rounds}", flush=True)

    if {label: baseline.digest(path) for label, path in artifacts.items()} != start_hashes:
        raise RuntimeError("a frozen binary or fixture changed during paired sampling")
    if baseline.git_state() != start_repository:
        raise RuntimeError("repository changed during paired sampling")
    report = {
        "schemaVersion": 1,
        "method": "alternating fresh-process annotated output; /usr/bin/time; 78 columns",
        "rounds": args.rounds,
        "repository": start_repository,
        "system": baseline.system_identity(),
        "environment": {key: env[key] for key in ("LC_ALL", "TZ", "TERM", "MANWIDTH", "COLUMNS")},
        "artifacts": {
            label: {"path": str(path), "sha256": start_hashes[label]}
            for label, path in artifacts.items()
        },
        "identities": identities,
        "summary": {
            page: {label: baseline.summarize(values) for label, values in routes.items()}
            for page, routes in trials.items()
        },
        "pairedDelta": {
            page: {
                metric: baseline.distribution([
                    candidate[metric] - base[metric]
                    for base, candidate in zip(routes["base"], routes["candidate"], strict=True)
                ])
                for metric in ("wallMs", "cpuMs", "rssKiB")
            } | {
                "candidateFasterRounds": sum(
                    candidate["wallMs"] < base["wallMs"]
                    for base, candidate in zip(routes["base"], routes["candidate"], strict=True)
                )
            }
            for page, routes in trials.items()
        },
        "trials": trials,
    }
    (out / "results.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {out / 'results.json'}")


if __name__ == "__main__":
    main()
