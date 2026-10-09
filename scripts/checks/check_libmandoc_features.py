#!/usr/bin/env python3
"""Check isolated native compatibility combinations and their private ABI."""

import argparse
import itertools
from pathlib import Path
import subprocess

if __package__:
    from .build_environment import verification_environment
else:
    from build_environment import verification_environment


ROOT = Path(__file__).resolve().parents[2]
COMPATIBILITY = ("compat-pandoc", "compat-libbsd", "compat-gnu-eqn")
MATRIX = [
    ("+".join(features) or "minimal",
     ["--no-default-features", *(["--features", ",".join(features)] if features else [])])
    for count in range(len(COMPATIBILITY) + 1)
    for features in itertools.combinations(COMPATIBILITY, count)
] + [
    ("default", []),
    ("render", ["--no-default-features", "--features", "render"]),
    ("serde", ["--no-default-features", "--features", "serde"]),
    ("render+serde", ["--no-default-features", "--features", "render,serde"]),
    ("all", ["--all-features"]),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", choices=[name for name, _ in MATRIX],
                        help="run one configuration instead of the small complete matrix")
    selected = parser.parse_args().case
    for name, flags in MATRIX:
        if selected is not None and selected != name:
            continue
        print(f"[libmandoc-features] {name}", flush=True)
        command = ["cargo", "test", "--locked", "--package", "libmandoc-rs", *flags]
        # Separate Cargo invocations keep normal workspace users from merging
        # compatibility or render features into this crate's disabled controls.
        for target in (["--test", "compatibility"],
                       ["--lib", "matches_the_native_abi"]):
            subprocess.run([*command, *target, "--quiet"], cwd=ROOT,
                           env=verification_environment(), check=True, timeout=180)


if __name__ == "__main__":
    main()
