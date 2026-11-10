"""Check owner-content transfers through the real Markdown loader and exporter."""
import argparse
from itertools import product
import json
from pathlib import Path
import subprocess

from .fidelity import chain_obligations, compare_direct

BOUNDARIES = ("paragraphs", "section-heading-body", "root-heading-body",
              "parent-body-child-heading")
DIRECTIONS = ("left-to-right", "right-to-left")
PAYLOADS = ("ascii", "cjk", "combining")


def require(observation, axis, status, context):
    actual = observation[axis][0]
    if actual != status:
        raise AssertionError(f"{context}: {axis} expected {status}, got {observation[axis]!r}")


def verify(records):
    expected = set(product(BOUNDARIES, DIRECTIONS, PAYLOADS))
    direct = first = second = stability = 0
    for record in records:
        coordinate = (record["boundary"], record["direction"], record["payload"])
        if coordinate not in expected:
            raise ValueError(f"unexpected or duplicate owner mutation coordinate {coordinate!r}")
        expected.remove(coordinate)
        control, mutant = record["control"], record["mutant"]
        observations = {**mutant, "roff": control["roff"]}
        direct_result = compare_direct(control["roff"], mutant["roff"])
        for axis, status in (("glyph", "exact"), ("owner", "exact"),
                             ("owner-content", "difference")):
            require(direct_result, axis, status, (coordinate, "direct"))
        direct += 1

        control_obligations = chain_obligations(control)
        obligations = chain_obligations(observations)
        for route in ("default", "addressable"):
            one, two = f"{route}-1", f"{route}-2"
            for stage in (one, two):
                require(control_obligations[stage]["from-source"], "owner-content", "exact",
                        (coordinate, stage, "unchanged"))
                result = obligations[stage]["from-source"]
                for axis, status in (("glyph", "exact"), ("owner", "exact"),
                                     ("owner-content", "difference")):
                    require(result, axis, status, (coordinate, stage, "mutated"))
            require(obligations[two]["from-first"], "owner-content", "exact",
                    (coordinate, two, "stable mutation"))
            first += 1
            second += 1
            stability += 1
    if expected:
        raise ValueError(f"missing {len(expected)} owner mutation coordinates: {sorted(expected)!r}")
    return dict(mutants=direct, firstRound=direct + first, secondFromSource=second,
                secondFromFirst=stability)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("profiler", type=Path, help="built markdown_owner_profile example")
    options = parser.parse_args()
    result = subprocess.run([str(options.profiler.resolve())], check=True,
                            stdout=subprocess.PIPE, text=True, encoding="utf-8", timeout=30)
    print(json.dumps(verify(json.loads(line) for line in result.stdout.splitlines())))


if __name__ == "__main__":
    main()
