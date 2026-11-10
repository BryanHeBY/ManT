"""Finite marker positions that exercise a real section owner handoff."""

from itertools import product

from .core import MAN

HEADER = MAN.removesuffix(".SH DESCRIPTION\n") + ".SH FIRST\n.PD 0\n"
END = ".SH ENDTEST\nFINISH\n"
SUFFIXES = ("", " ", r"\&", r"\:", r"\c", " BETA")


def pending_sources():
    # The extra PD in the first cohort deliberately retains the reviewed
    # source coordinates. Keep distinct logical coordinates even if future
    # control templates happen to produce identical bytes.
    for repeat, prefix, suffix, pd, head, literal in product(
        range(1, 4), ("", "ALPHA"), SUFFIXES, range(3), ("SS", "SH"), (False, True),
    ):
        word = prefix + r"\p" * repeat + suffix
        source = (HEADER + f".PD {pd}\n" + (".nf\n" if literal else "")
                  + f"{word}\n.{head} NEXT\nAFTERMARK\n" + END)
        yield "PH-core", [repeat, prefix, suffix, pd, head, literal], source
    for request, heads in ((".br\n", ("SS", "SH")), (".br\n\\&\n", ("SS",))):
        cohort = "PH-br" if len(heads) == 2 else "PH-retained-br"
        for repeat, prefix, suffix, head, literal in product(
            range(1, 4), ("", "ALPHA"), SUFFIXES, heads, (False, True),
        ):
            word = prefix + r"\p" * repeat + suffix
            source = (HEADER + (".nf\n" if literal else "") + f"{word}\n"
                      + request + f".{head} NEXT\nAFTERMARK\n" + END)
            yield cohort, [repeat, prefix, suffix, 0, head, literal], source
