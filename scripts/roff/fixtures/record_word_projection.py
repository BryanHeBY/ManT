"""Freeze source-word ownership and printed separator rows from pristine CVS."""

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
import gzip
from itertools import product
import json
from pathlib import Path

from scripts.roff.fixtures import record_word_receipts as oracle

FIXTURE = oracle.FIXTURE.parent.parent / "word_projection/cases.json.gz"
END = ".Sh END\nENDMARK\n"
RETIRES = {
    "br": ".br\n.No OMEGA\n",
    "empty-br": '.No ""\n.br\n.No OMEGA\n',
    "sp-zero": ".sp 0\n.No OMEGA\n",
    "next-word": ".No OMEGA\n",
    "eof": "",
}
FIRST = {
    "ordinary": "ALPHA",
    "bare-arm": r"ALPHA\z",
    "cached-glyph": r"ALPHA\zX",
    "cached-and-armed": r"ALPHA\zX\z",
}
MIDDLE = {
    "none": "", "space": " ", "tab": "\t", "nbsp": r"\~",
    "zero-width": r"\&", "breakpoint": r"\:", "word-end-break": r"\p",
    "marker-breakpoint": r"\p\:", "zero-marker-breakpoint": r"\&\p\:",
    "font-only": r"\fB", "zero-space": r"\& ", "space-zero": r" \&",
}


def quoted(word):
    return f'"{word}"'


def operands(first, last, seam):
    if seam in ("same-tight", "same-source-space"):
        boundary = " " if seam == "same-source-space" else ""
        return f".No {quoted(first + boundary + last)}\n"
    if seam == "same-macro":
        return f".No {quoted(first)} No {quoted(last)}\n"
    if seam == "raw-physical-line":
        return f".No {quoted(first)}\n{last}\n"
    between = '.No ""\n' * {"empty-one": 1, "empty-two": 2}.get(seam, 0)
    return f".No {quoted(first)}\n{between}.No {quoted(last)}\n"


def case(identifier, cohort, body, comparison=None, *, end=END):
    source = oracle.MDOC + body + end
    comparison = comparison or ("device-tab" if "\t" in source else "exact")
    return oracle.specification(identifier, cohort, source, comparison)


def core():
    seams = ("same-tight", "same-source-space", "next-No", "same-macro",
             "raw-physical-line", "empty-one", "empty-two")
    for first, control, seam, retire in product(FIRST, MIDDLE, seams, RETIRES):
        # A leading raw space on a physical line ends the preceding source
        # row. It must never be compared as a quoted-space operand.
        body = operands(FIRST[first], MIDDLE[control] + "BETA", seam)
        yield case(f"core/{first}/{control}/{seam}/{retire}", "core",
                   body + RETIRES[retire], end="" if retire == "eof" else END)


def prefixes():
    prefixes = (r" \fB ", r"\fB \fI ", r"\& \fB ", r" \& \fB ",
                r"\:\&  ", r"  \:", " \t ", r"\&\p\:\& ",
                r"\p\:\& ", r" \~ ", r"\~ \& ", r"\z  ",
                r"\z \& ", r"\z \p\: ", r"\z \&\p ", r" \z \fB ")
    initials = (r"ALPHA\zX", r"ALPHA\zX\p\: ", r"ALPHA\zX\p\: \zY")
    combinations = product(initials, (0, 1), prefixes, ("No", "Em", "Lk"), (0, 1))
    for index, (first, middle, prefix, wrapper, tail) in enumerate(combinations):
        target = " https://link.test" if wrapper == "Lk" else ""
        body = (f".No {quoted(first)}\n" + '.No ""\n' * middle
                + f".{wrapper}{target} {quoted(prefix + 'BETA')}\n"
                + '.No ""\n' * tail + ".br\n.No OMEGA\n")
        yield case(f"prefix-{index}", "reported-prefixes", body)


def scope():
    probes = (("overwrite", r"\:BETA"), ("unprinted", r"\&\p\: BETA"),
              ("rearm", r"\z \p BETA"))
    first = r"ALPHA\zX"
    last = '.No ""\n.br\n.No OMEGA\n'
    for family, payload in probes:
        for wrapper, empty in product(("No", "Em", "Li", "Lk"), (0, 1)):
            target = " https://link.test" if wrapper == "Lk" else ""
            yield case(f"scope/{family}/{wrapper}/{empty}", "scope",
                       f".No {quoted(first)}\n.{wrapper}{target} {quoted(payload)}\n"
                       + '.No ""\n' * empty + ".br\n.No OMEGA\n")
        for seam in ("same-tight", "same-source-space", "next-No", "same-macro"):
            yield case(f"scope/{family}/literal/{seam}", "scope",
                       ".nf\n" + operands(first, payload, seam) + last + ".fi\n")
        for mode, start, end in (("keep", ".Bk -words\n", ".Ek\n"),
                                 ("spacing-off", ".Sm off\n", ".Sm on\n")):
            yield case(f"scope/{family}/{mode}", "scope",
                       start + operands(first, payload, "next-No") + last + end)
        for kind in ("tag", "hang", "inset", "diag", "column"):
            width = ' "20n" "10n"' if kind == "column" else (
                " -width 20n" if kind in ("tag", "hang") else "")
            item = ".It HEAD Ta\n" if kind == "column" else ".It HEAD\n"
            yield case(f"field/{family}/{kind}-body", "field",
                       f".Bl -{kind}{width}\n{item}" + operands(first, payload, "next-No")
                       + '.No ""\n.El\n.No OMEGA\n', "native-field")
        # In an explicit extended HEAD the parser visits each No as a
        # macro. A -diag HEAD would instead display No as plain text.
        for kind in ("tag", "hang"):
            body = (f".Bl -{kind} -width 20n\n.It Xo\n"
                    + operands(first, payload, "next-No")
                    + '.No ""\n.Xc\nBODY\n.El\n.No OMEGA\n')
            yield case(f"field/{family}/{kind}-head", "field", body, "native-field")


def separators():
    markers = (r"\&\p\:", r"\p\:", r"\:\p", r"\p\&\:", r"\&\p\:\p\:")
    blanks = ("", " ", "  ", "\t", r"\~", r"\fB ")
    for marker, blank, tail in product(markers, blanks, RETIRES):
        body = operands(r"ALPHA\zX", marker + blank + "BETA", "next-No") + RETIRES[tail]
        yield case(f"separator/{marker}/{blank!r}/{tail}", "separator", body,
                   end="" if tail == "eof" else END)
    for marker, glyph in product(markers, ("", r"\zY", r"Y\p\:Z", r"Y\p\:\p REJECTED")):
        body = operands(r"ALPHA\zX", marker + glyph, "next-No") + ".br\n.No OMEGA\n"
        yield case(f"separator/suffix/{marker}/{glyph}", "separator-suffix", body)
    for marker, prefix in product(markers, (r"ALPHA\z.", r"ALPHA\z")):
        # A raw source line with a trailing dot supplies NODE_EOS, unlike a
        # space inserted by the portable renderer.
        body = prefix + "\n" + f".No {quoted(marker + ' BETA')}\n" + RETIRES["empty-br"]
        yield case(f"separator/sentence/{marker}/{prefix}", "separator-sentence", body)


def tight_empty_words():
    for marker, count, literal in product((r"\:", r"\&", r"\p"), range(1, 4), (False, True)):
        body = '.No "ALPHA\\zX"' + ' No ""' * count + f' Ns No "{marker}BETA"\n'
        if literal:
            body = f".nf\n{body}.fi\n"
        yield case(f"scope/tight-empty/{marker}/{count}/{literal}", "tight-empty",
                   body + RETIRES["empty-br"])


def consumer_sources():
    for source in sorted(FIXTURE.parent.glob("*.1")):
        yield oracle.specification(f"consumer/{source.stem}", "consumer",
                                   source.read_bytes().decode(), "exact")


def specifications():
    seen = set()
    cohorts = (prefixes, scope, core, separators, tight_empty_words, consumer_sources)
    for cohort in cohorts:
        for spec in cohort():
            if spec["source"] in seen:
                continue
            seen.add(spec["source"])
            yield spec


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--evidence", type=Path)
    args = parser.parse_args()
    registration = oracle.verified_reference(oracle.ROOT, oracle.REFERENCE)
    with ThreadPoolExecutor(max_workers=8) as executor:
        receipts = list(executor.map(oracle.record, specifications()))
    cases = [spec for spec, _ in receipts]
    fixture = dict(oracle_identity=registration["identity"],
                   oracle_sha256=oracle.digest(oracle.REFERENCE.read_bytes()),
                   terminal_width=160, cases=cases)
    encoded = (json.dumps(fixture, ensure_ascii=False, indent=2) + "\n").encode()
    if args.write:
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_bytes(gzip.compress(encoded, mtime=0))
    elif gzip.decompress(FIXTURE.read_bytes()) != encoded:
        raise ValueError("word projection fixture differs from pristine profiles")
    if args.evidence:
        args.evidence.parent.mkdir(parents=True, exist_ok=True)
        with args.evidence.open("w") as output:
            for spec, profiles in receipts:
                print(json.dumps(dict(spec, raw=profiles), ensure_ascii=False), file=output)
    print(dict(Counter(spec["cohort"] for spec in cases)))


if __name__ == "__main__":
    main()
