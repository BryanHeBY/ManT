"""Finite source inputs for section entry and literal-owner handoff."""

from itertools import product

from .core import MAN, MDOC

MAN_HEADER = MAN.partition(".SH NAME")[0]
MDOC_HEADER = MDOC.partition(".Sh NAME")[0]
MDOC_END = ".Sh END\n.No ENDMARK\n"
PREDECESSORS = (
    "none", "visible", "transparent", "empty-peer", "transparent-peer",
    "empty-word", "invisible-word",
)
REQUESTS = ("", ".sp 0\n", ".sp -1\n", ".sp -2\n")
OWNER_CONTROLS = ("", ".br\n", ".sp 0\n", ".sp 1\n")
OWNER_PAYLOADS = ("", "\\&\n", "\\z\n", "\\zX\n", "\n", '.B ""\n',
                  "ALPHA\n", "ALPHA\\c\n")
MAN_EMPTY_WORD = ".B \\fR\n"


def section_entries():
    for macro in ("SH", "SS", "Sh", "Ss"):
        man = macro.isupper()
        top = macro.lower() == "sh"
        head = MAN_HEADER if man else MDOC_HEADER
        chapter = "SH" if man else "Sh"
        transparent = ".ft R\n" if man else ".Sm off\n"
        empty_word = MAN_EMPTY_WORD if man else '.No ""\n'
        for previous, control, literal, tail, pd in product(
            PREDECESSORS, REQUESTS, (False, True), ("word", "empty-then-space", "eof"),
            range(3) if man else (None,),
        ):
            source = head + (f".PD {pd}\n" if man else "")
            if not top:
                source += f".{chapter} FIRST\n"
            if previous in ("empty-peer", "transparent-peer"):
                source += f".{macro} PREVIOUS\n"
            elif top and previous != "none":
                source += f".{chapter} PREVIOUS\n"
            if literal:
                source += ".nf\n"
            source += {
                "visible": "BEFOREMARK\n",
                "transparent": transparent,
                "transparent-peer": transparent,
                "empty-word": empty_word,
                "invisible-word": "\\&\n",
            }.get(previous, "")
            source += control + f".{macro} NEXT\n"
            if tail == "word":
                source += "AFTERMARK\n"
            elif tail == "empty-then-space":
                source += empty_word + ".sp 1\nAFTERMARK\n"
            yield "SEC-entry", [macro, previous, control, literal, tail, pd], source


def section_owner_sources():
    for pd, literal, payload, control, empty_first in product(
        range(3), (False, True), OWNER_PAYLOADS, OWNER_CONTROLS, (False, True),
    ):
        source = (MAN.removesuffix(".SH DESCRIPTION\n") + f".SH FIRST\n.PD {pd}\n"
                  + (".nf\n" if literal else "") + payload + control + ".SS NEXT\n"
                  + (MAN_EMPTY_WORD if empty_first else "") + "AFTERMARK\n")
        yield "SEC-owner", [pd, literal, payload, control, empty_first], source
    for payload in OWNER_PAYLOADS:
        source = (MAN.removesuffix(".SH DESCRIPTION\n") + ".SH FIRST\n.PD 0\n.nf\n"
                  + payload + ".SH NEXT\nAFTERMARK\n")
        yield "SEC-owner-sh", [payload], source


def section_controls():
    for heading, count in product(("Sh", "Ss"), range(1, 5)):
        source = MDOC + "".join(f".{heading} EMPTY{i}\n" for i in range(count))
        yield "SEC-chain", [heading, count], source + f".{heading} NEXT\n.No AFTERMARK\n" + MDOC_END
    for heading, debt in product(("Sh", "Ss"), range(3)):
        spacing = f".sp -{debt}\n" if debt else ""
        prefix = MDOC + f".No BEFOREMARK\n{spacing}.{heading} NEXT\n"
        for name, tail in (
            ("positive", ".No AFTERMARK\n.sp 1\n.No TAILMARK\n"),
            ("emptyword-positive", '.No ""\n.sp 1\n.No AFTERMARK\n'),
            ("next-section", ".No AFTERMARK\n.Ss FOLLOW\n.No TAILMARK\n"),
            ("nofill-positive", ".nf\n.No AFTERMARK\n.sp 1\n.No TAILMARK\n.fi\n"),
        ):
            yield "SEC-reset", [heading, debt, name], prefix + tail + MDOC_END
        source = MDOC + f".Bd -literal\n.No BEFOREMARK\n{spacing}.Ed\n"
        yield "SEC-display", [heading, debt], source + f".{heading} NEXT\n.No AFTERMARK\n" + MDOC_END
    for payload, heading, debt, literal in product(
        ("BEFOREMARK", "BEFOREMARK\\c", "BEFOREMARK\\z", "\\z", "\\zX", "\\&", ""),
        ("Sh", "Ss"), range(3), (False, True),
    ):
        source = (MDOC + (".nf\n" if literal else "") + f'.No "{payload}"\n'
                  + (f".sp -{debt}\n" if debt else "")
                  + f".{heading} NEXT\n.No AFTERMARK\n" + MDOC_END)
        yield "SEC-pending", [payload, heading, debt, literal], source


def shared_heading_controls():
    for literal, payload in product(
        (False, True), (
            "\\p\n", "\\&\n\\&\n", "\\& \\&\n", "\\:\n", "\\&\\p\n", "\\p\\&\n",
            "\\p\\&\n\\p\\&\n", "\\p\\& \\p\\&\n", "\\~\n", ".sp 1\n\\&\n",
        ),
    ):
        source = (MAN.removesuffix(".SH DESCRIPTION\n") + ".SH FIRST\n.PD 0\n"
                  + (".nf\n" if literal else "") + payload + ".SS NEXT\nAFTERMARK\n")
        yield "SEC-shared-control", [literal, payload], source


def section_sources():
    yield from section_entries()
    yield from section_owner_sources()
    yield from section_controls()
    yield from shared_heading_controls()
