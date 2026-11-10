"""Exact source adapters for the horizontal rewind follow-up."""
from itertools import product

from .core import MAN, MDOC

PREFIX = ("ALPHA", r"ALPHA\zX", r"ALPHA\zX\z", "ALPHA\\z ")
HORIZ = ("", r"\h'0n'", r"\h'-1n'", r"\h'-2n'", r"\h'-3n'", r"\h'-24u'",
         r"\h'-25u'", r"\h'-47u'", r"\h'-48u'")
SUFFIX = (
    "BETA", " BETA", "  BETA", "   BETA", "    BETA",
    r"\pBETA", r"\p BETA", r"\p   BETA", r"\: BETA", r"\p\: BETA",
    r"\:\p BETA", r"\p\& BETA", r"\p\zY BETA", r"\p\~BETA", r"\p\0BETA",
)
WRAPPER = (".No", ".Em", ".Lk https://example.test")
MODE = (("", ""), (".Sm off\n", ".Sm on\n"), (".Bk -words\n", ".Ek\n"),
        (".Bd -literal\n", ".Ed\n"))
TAIL = (".br\n.No OMEGA\n", ".sp 0\n.No OMEGA\n", ".Pp\n.No OMEGA\n", "")
SEEDS = ((2, 3), (3, 3), (2, 6), (3, 6), (2, 9), (3, 9), (1, 9), (3, 8))


def operand(macro, text):
    return f'{macro} "{text}"\n'


def rewind_sources():
    for p, h, s, w, m, e, t in product(*(range(n) for n in (4, 9, 15, 3, 4, 2, 4))):
        enter, leave = MODE[m]
        source = (MDOC + enter + operand(".No", PREFIX[p])
                  + operand(WRAPPER[w], HORIZ[h] + SUFFIX[s])
                  + ('.No ""\n' if e else "") + TAIL[t] + leave)
        yield "W-core", [p, h, s, w, m, e, t], source


def field(kind, head, words):
    if kind == "plain":
        return words
    if kind == "column":
        return ".Bl -column column\n.It Xo\n" + words + ".Xc\n.El\n"
    enter = f".Bl -{kind} -width 4n\n"
    if head:
        return enter + ".It Xo\n" + words + ".Xc\n.No BODYWORD\n.El\n"
    return enter + ".It HEADWORD\n" + words + ".El\n"


def bridge_sources():
    for i, adapter, literal, tail in product(range(8), range(10), (False, True), (False, True)):
        h, s = SEEDS[i]
        first, second = PREFIX[1], HORIZ[h] + SUFFIX[s]
        words = operand(".No", first) + operand(".No", second)
        header = MDOC
        if adapter == 0:
            words = operand(".No", first + " " + second)
        elif adapter in (2, 3):
            words = operand(".No", first) + operand(WRAPPER[adapter - 1], second)
        elif adapter == 4:
            words = ".de XX\n" + words.replace("\\", "\\\\") + "..\n.XX\n"
        elif adapter in (5, 6, 7):
            header = MAN
            words = (first + "\n" + second + "\n" if adapter == 5 else
                     f'.{"BR" if adapter == 6 else "BI"} "{first}" "{second}"\n')
        elif adapter in (8, 9):
            words = field("tag", adapter == 8, words)
        source = (header + (".nf\n" if literal else "") + words
                  + ((".br\nOMEGA\n" if header == MAN else TAIL[0]) if tail else "")
                  + (".fi\n" if literal else ""))
        yield "W-adapter", [i, adapter, literal, tail], source
    for i, (kind, head), tail in product(
        range(8), (("plain", False), ("tag", True), ("tag", False), ("hang", True),
                   ("hang", False), ("column", False)), (False, True),
    ):
        h, s = SEEDS[i]
        words = operand(".No", PREFIX[1]) + operand(".No", HORIZ[h] + SUFFIX[s])
        yield "W-field", [i, kind, head, tail], MDOC + field(
            kind, head, words + (TAIL[0] if tail else ""))


def scale_sources():
    for length, seed, wrapper, literal in product(
        (8, 32, 128, 512, 2048, 8192), (0, 5), WRAPPER, (False, True),
    ):
        h, s = SEEDS[seed]
        enter, leave = MODE[3 if literal else 0]
        source = (MDOC + enter + operand(".No", "A" * length + r"LPHA\zX")
                  + operand(wrapper, HORIZ[h] + SUFFIX[s]) + leave)
        yield "W-scale", [length, seed, wrapper, literal], source
