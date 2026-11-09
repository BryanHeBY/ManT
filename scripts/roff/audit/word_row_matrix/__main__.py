"""Enumerate and record the frozen finite word/row review inputs."""
import argparse
import json
from pathlib import Path
import signal

from .manifest import freeze
from .oracle import collect
from .product import collect_product, compare_products


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("action", choices=("freeze", "oracle", "product", "compare"))
    parser.add_argument("--calibration", action="store_true")
    parser.add_argument("--cohort", action="append", default=[])
    parser.add_argument("--jobs", type=int, default=8)
    parser.add_argument("--wall-seconds", type=int, default=900)
    parser.add_argument("--artifact-mib", type=int, default=4096)
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--name", choices=("baseline", "candidate"), default="candidate")
    parser.add_argument("--follow", action="store_true")
    args = parser.parse_args()
    if args.action == "freeze":
        result = freeze(args.directory)
    elif args.action == "compare":
        result = compare_products(args.directory)
    elif args.action == "product":
        if args.binary is None:
            parser.error("product requires --binary")
        result = collect_product(args.directory, args.binary, args.name,
                                 wall_budget=args.wall_seconds,
                                 artifact_budget=args.artifact_mib * 2**20, follow=args.follow)
    else:
        result = collect(args.directory, jobs=args.jobs, wall_budget=args.wall_seconds,
                         artifact_budget=args.artifact_mib * 2**20,
                         calibration=args.calibration, cohorts=tuple(sorted(set(args.cohort))))
    print(json.dumps(result, ensure_ascii=False, indent=2))


def interrupted(*_):
    raise KeyboardInterrupt


if __name__ == "__main__":
    signal.signal(signal.SIGTERM, interrupted)
    main()
