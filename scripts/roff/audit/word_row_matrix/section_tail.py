"""Accepted empty passes followed by visible, invisible, or rejected input."""

from itertools import product

from .pending_handoff import END, HEADER


def render(body, *, literal, heading="NEXT", macro="SS", distance=0):
    header = HEADER.removesuffix(".PD 0\n") + f".PD {distance}\n"
    boundary = f".{macro} {heading}\n" if heading is not None else ""
    return header + (".nf\n" if literal else "") + body + boundary + "AFTERMARK\n" + END


def word(blanks, prefix, tail, marker=""):
    if prefix:
        prefix += r"\p " if blanks else " "
    return prefix + r"\p\& " * blanks + tail + marker


def wrapped(text, wrapper):
    if wrapper.startswith("f"):
        return "\\" + wrapper + text + "\\fR\n"
    if wrapper == "UR":
        return f".UR https://audit.test/\n{text}\n.UE\n"
    if wrapper == "MT":
        return f".MT audit@example.test\n{text}\n.ME\n"
    return f".{wrapper} {text}\n"


def section_tail_sources():
    # Write empty passes *before* the final word, rather than offering
    # NBRZW and a new word as mutually exclusive suffixes.
    for blanks, prefix, tail, marker, literal, macro, distance in product(
        range(4), ("", "ALPHA"), ("", "BETA", r"\zX", r"\p BETA"),
        ("", r"\p"), (False, True), ("SS", "SH"), range(2),
    ):
        source = render(word(blanks, prefix, tail, marker) + "\n",
                        literal=literal, macro=macro, distance=distance)
        yield "PHT-core", [blanks, prefix, tail, marker, literal, macro, distance], source
    for blanks, literal, wrapper in product((1, 2), (False, True),
                                             ("fI", "fB", "I", "B", "UR", "MT")):
        source = render(wrapped(word(blanks, "ALPHA", "BETA"), wrapper), literal=literal)
        yield "PHT-wrapped", [blanks, literal, wrapper], source
    for tail, literal, head in product(
        ("BETA", r"\zX"), (False, True),
        (r"\&", r"\p", r"\p NEXT", r"\&\p NEXT", r"NEXT\zX", r"\zNEXT",
         r"NEXT\p WORD", r"NEXT\p \p WORD"),
    ):
        source = render(word(1, "ALPHA", tail) + "\n", literal=literal, heading=head)
        yield "PHT-head", [tail, literal, head], source
    for tail, literal, (request, heading) in product(
        ("BETA", r"\zX"), (False, True),
        ((".br\n", "NEXT"), (".br\n\\&\n", "NEXT"), (".sp 1\n", "NEXT"), (".br\n", None)),
    ):
        source = render(word(1, "ALPHA", tail) + "\n" + request,
                        literal=literal, heading=heading)
        yield "PHT-boundary", [tail, literal, request, heading], source
    for tail, literal, heading in product(
        (r"BETA\p \p\& GAMMA", r"BETA\p \p\&", r"\zX\c"),
        (False, True), ("NEXT", r"NEXT\p WORD"),
    ):
        source = render(word(1, "ALPHA", tail) + "\n", literal=literal, heading=heading)
        yield "PHT-chain", [tail, literal, heading], source
    for (tail, heading), literal in product(
        ((r"\zX", r"\h'-1n'NEXT"), (r"\p BETA", r"\h'-2n'NEXT")), (False, True),
    ):
        source = render(word(1, "ALPHA", tail) + "\n", literal=literal, heading=heading)
        yield "PHT-rewind", [tail, literal, heading], source
