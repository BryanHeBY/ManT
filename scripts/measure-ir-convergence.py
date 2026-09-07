#!/usr/bin/env python3
"""Local fixed-input baseline/comparison; never compiles or creates a worktree."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--native", type=Path, required=True)
    parser.add_argument("--reference-label", help="Alternate each round with retained reference binaries")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    fixture = root / "tests/fixtures/roff/real/archlinux/gcc.1.gz"
    output = root / "target/ir-convergence" / args.label
    output.mkdir(parents=True, exist_ok=True)
    modes = {
        "full": [],
        "outline": ["--outline", "--outline-entries", "all"],
        "explain": ["--explain=--help"],
    }
    commands = {
        name: [str(args.binary.resolve()), "--input", str(fixture),
               "--format", "json", "--display", "direct", *flags]
        for name, flags in modes.items()
    }
    commands["native"] = [str(args.native.resolve()), str(fixture)]
    reference = None
    if args.reference_label:
        reference = json.loads((root / "target/ir-convergence" / args.reference_label / "measurements.json").read_text())
    record = {
        "producer": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
        "os": platform.platform(),
        "toolchain": subprocess.check_output(["rustc", "-Vv"], text=True),
        "profile": "release", "features": "default",
        "gzipSha256": hashlib.sha256(fixture.read_bytes()).hexdigest(),
        "sourceSha256": hashlib.sha256(gzip.decompress(fixture.read_bytes())).hexdigest(),
        "binarySha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "nativeSha256": hashlib.sha256(args.native.read_bytes()).hexdigest(),
        "referenceProducer": reference["producer"] if reference else None,
        "referenceBinarySha256": reference["binarySha256"] if reference else None,
        "modes": {},
    }
    env = {**os.environ, "LC_ALL": "C.UTF-8", "TERM": "dumb"}
    for name, command in commands.items():
        subprocess.run(command, stdout=subprocess.DEVNULL, check=True, env=env)
        samples = []
        reference_samples = []
        pair = [("candidate", command, samples)]
        if reference:
            old = reference["modes"][name]["argv"]
            subprocess.run(old, stdout=subprocess.DEVNULL, check=True, env=env)
            pair.append(("reference", old, reference_samples))
        for ordinal in range(7):
            for side, argv, destination in pair[::1 if ordinal % 2 == 0 else -1]:
                rss = output / "rss.txt"
                start = time.perf_counter()
                result = subprocess.run(["/usr/bin/time", "-f", "%M", "-o", str(rss),
                                         *argv], capture_output=True, check=True, env=env)
                sample = {"milliseconds": (time.perf_counter() - start) * 1000,
                          "peakRssKiB": int(rss.read_text().strip())}
                if name == "native":
                    sample.update(json.loads(result.stdout))
                destination.append(sample)
                filename = f"{name}.json" if side == "candidate" else f"reference-{name}.json"
                (output / filename).write_bytes(result.stdout)
        record["modes"][name] = {
            "argv": command, "samples": samples,
            "medianMilliseconds": statistics.median(x["milliseconds"] for x in samples),
            "peakRssKiB": max(x["peakRssKiB"] for x in samples),
            "referenceSamples": reference_samples,
        }
    for fmt in ("text", "markdown"):
        result = subprocess.run([str(args.binary.resolve()), "--input", str(fixture),
                                 "--format", fmt, "--display", "direct"],
                                capture_output=True, check=True, env=env)
        (output / f"full.{fmt}").write_bytes(result.stdout)
    (output / "measurements.json").write_text(json.dumps(record, indent=2) + "\n")
    print(json.dumps(record, indent=2))


if __name__ == "__main__":
    main()
