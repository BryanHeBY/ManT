"""Freeze the two ordered-marker audit cohorts against pristine CVS."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import gzip
from itertools import product
import json
from pathlib import Path

from scripts.roff.fixtures import record_word_receipts as oracle


FIXTURE = oracle.FIXTURE.parent.parent / "word_passes" / "cases.json.gz"
EMPTY_BREAK = '.No ""\n.br\n.No OMEGA\n'
CONTROL_TAIL = '.No "\\&\\p\\: GAMMA"\n.br\n.No OMEGA\n'
PRIORS = (r"ALPHA\zX", r"ALPHA\zX\p\: \zY", r"ALPHA\z.", r"ALPHA\p\:\zX")
FOLLOWERS = (
    r"\&\p\: BETA", r"\z \p BETA", r"\p\:\&\p\: BETA", r"\& \p\:\& BETA",
    r" \p\:\&\p\:\& BETA", r"\z \p\:\zY BETA", r"\&\p\: \~BETA",
    r"\&\p\: \p REJECTED",
)
SPACING = (("", ""), (".Sm off\n", ".Sm on\n"), (".Bk -words\n", ".Ek\n"))


def cross_cases():
    endings = ("", EMPTY_BREAK, CONTROL_TAIL, '.No "\\z \\p GAMMA"\n.br\n.No OMEGA\n')
    for index, (prior, follower, ending, (open_scope, close_scope), empty) in enumerate(
            product(PRIORS, FOLLOWERS, endings, SPACING, range(2))):
        body = (open_scope + f'.No "{prior}"\n' + '.No ""\n' * empty
                + f'.No "{follower}"\n' + ending + close_scope)
        yield oracle.specification(f"cross/{index}", "cross", oracle.MDOC + body)


def wrapper_cases():
    priors = (PRIORS[0], PRIORS[1], r"ALPHA\zX\z", PRIORS[3])
    endings = (EMPTY_BREAK, ".Pp\n.No OMEGA\n", CONTROL_TAIL)
    arrangements = ("deferred", "rearmed", "alternating")
    for index, (prior, count, arrangement, wrapper, ending) in enumerate(
            product(priors, (2, 3, 4), arrangements, ("No", "Em", "Lk"), endings)):
        if arrangement == "deferred":
            operand = r"\&" + r"\p\: " * (count - 1) + r"\p BETA"
        elif arrangement == "rearmed":
            operand = r"\&\p\: \zY" + r"\p\: " * (count - 2) + r"\p BETA"
        else:
            stages = (r"\p\:  " if i % 2 == 0 else r"\p\& " for i in range(count))
            operand = r"\&" + "".join(stages) + "BETA"
        target = " https://matrix.test/w" if wrapper == "Lk" else ""
        body = f'.No "{prior}"\n.{wrapper}{target} "{operand}"\n' + ending
        yield oracle.specification(f"wrapper/{index}", "wrapper", oracle.MDOC + body + oracle.END)


def specifications():
    yield from cross_cases()
    yield from wrapper_cases()
    for source in sorted(FIXTURE.parent.glob("*.1")):
        yield oracle.specification(f"consumer/{source.stem}", "consumer",
                                   source.read_bytes().decode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    registration = oracle.verified_reference(oracle.ROOT, oracle.REFERENCE)
    cases = []
    with ThreadPoolExecutor(max_workers=8) as executor:
        receipts = executor.map(oracle.record, specifications())
        if args.evidence:
            args.evidence.parent.mkdir(parents=True, exist_ok=True)
            with args.evidence.open("w") as output:
                for case, profiles in receipts:
                    cases.append(case)
                    print(json.dumps(dict(case, raw=profiles), ensure_ascii=False), file=output)
        else:
            cases.extend(case for case, _ in receipts)
    fixture = dict(oracle_identity=registration["identity"],
                   oracle_sha256=oracle.digest(oracle.REFERENCE.read_bytes()),
                   terminal_width=160, cases=cases)
    encoded = (json.dumps(fixture, ensure_ascii=False, indent=2) + "\n").encode()
    if args.write:
        FIXTURE.write_bytes(gzip.compress(encoded, mtime=0))
    elif gzip.decompress(FIXTURE.read_bytes()) != encoded:
        raise ValueError("word pass fixture differs from pristine profiles")
    print(dict(Counter(case["cohort"] for case in cases)))


if __name__ == "__main__":
    main()
