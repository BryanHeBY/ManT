"""Finite source coordinates for section-tail padding and completed rows."""

from itertools import product

from .pending_handoff import HEADER
from .section_tail import render


CONTRIBUTIONS = {
    "none": "",
    "horiz-zero": r"\h'0n'",
    "horiz-one": r"\h'1n'",
    "horiz-two": r"\h'2n'",
    "ascii-space": " ",
    "escaped-space": r"\ ",
    "nbsp": "\u00a0",
    "escaped-nbsp": r"\~",
    "digit-space": r"\0",
    "tab": "\t",
    "zero-width": r"\&",
    "breakpoint": r"\:",
}
SUCCESSORS = ("", r"\c", "BETA", r"\p BETA", r"\p\& BETA")


def operand(blanks, tail, following=""):
    prior = r"ALPHA\p " + " ".join([r"\p\&"] * blanks) if blanks else "ALPHA"
    if tail:
        # ASCII source padding is itself the axis, not a separator plus
        # the axis. Keep exact bytes even when the AST normalizes them.
        prior += ("" if tail[0] in " \t" else " ") + tail
    if following:
        prior += ("" if following == r"\c" else " ") + following
    return prior


def wrapped_operand(text, wrapper, padding, placement):
    if wrapper == "plain":
        return text + " " + padding + "\n"
    if wrapper in ("UR", "MT"):
        target, close = (("https://audit.test/", "UE") if wrapper == "UR"
                         else ("audit@example.test", "ME"))
        if placement == "inside":
            return f".{wrapper} {target}\n{text} {padding}\n.{close}\n"
        return f".{wrapper} {target}\n{text}\n.{close} {padding}\n"
    if placement == "inside":
        return f".{wrapper} {text} {padding}\n"
    return f"\\f{wrapper}{text}\\fR {padding}\n"


def padding_sources():
    # Cross authored/reading padding with the *following* execution
    # event; neither can stand for the earlier accepted empty passes.
    for literal, blanks, tail, following, macro, pd in product(
        (False, True), range(4), CONTRIBUTIONS, SUCCESSORS, ("SH", "SS"), range(2),
    ):
        body = operand(blanks, CONTRIBUTIONS[tail], following) + "\n"
        yield "PHD-core", [literal, blanks, tail, following, macro, pd], render(
            body, literal=literal, macro=macro, distance=pd)

    for blanks, motion, end, case in product(
        (1, 2), (1, 2), ("", r"\c"),
        ("surviving", "new-empty", "rejected", "covered", "zero-advance",
         "padding-glyph-padding", "invisible-heading", "bare-heading", "heading-break",
         "reject-heading", "mixed-rejected-heading", "two-handoffs"),
    ):
        padding = rf"\h'{motion}n'"
        tail, head, after = {
            "surviving": (padding + " BETA", "NEXT", ""),
            "new-empty": (padding + r" BETA\p \p\&", "NEXT", ""),
            "rejected": (padding + r" \p BETA", "NEXT", ""),
            "covered": (padding + r" \zX\c", "NEXT", ""),
            "zero-advance": (r"\zX" + padding, "NEXT", ""),
            "padding-glyph-padding": (padding + " BETA" + padding, "NEXT", ""),
            "invisible-heading": (padding, r"\&", ""),
            "bare-heading": (padding, r"\z", ""),
            "heading-break": (padding, r"NEXT\p WORD", ""),
            "reject-heading": (padding, r"\p NEXT", ""),
            "mixed-rejected-heading": (padding + " BETA", r"\p NEXT", ""),
            "two-handoffs": (padding, "NEXT", r"\p \p\& " + padding + "\n.SS LATER\nAFTERLATER\n"),
        }[case]
        body = operand(blanks, tail) + end + "\n"
        source = render(body, literal=True, heading=head)
        if after:
            source = source.replace("AFTERMARK\n", "AFTERMARK" + after)
        yield "PHD-ordered", [blanks, motion, end, case], source

    for blanks, (wrapper, placement), tail in product(
        (1, 2),
        (("plain", "inside"), ("B", "inside"), ("B", "outside"), ("I", "inside"),
         ("I", "outside"), ("UR", "inside"), ("UR", "outside"),
         ("MT", "inside"), ("MT", "outside")),
        ("horiz-one", "horiz-two", "escaped-space", "horiz-then-word"),
    ):
        padding = (r"\h'1n' BETA" if tail == "horiz-then-word" else CONTRIBUTIONS[tail])
        body = wrapped_operand(operand(blanks, ""), wrapper, padding, placement)
        yield "PHD-wrapped", [blanks, wrapper, placement, tail], render(body, literal=True)

    for blanks, tail, (request, heading) in product(
        (1, 2), ("horiz-one", "horiz-then-word", "ascii-space"),
        (("", "NEXT"), (".br\n", "NEXT"), (".br\n\\&\n", "NEXT"),
         (".sp 1\n", "NEXT"), (".fi\n", "NEXT"), ("", None)),
    ):
        padding = (r"\h'1n' BETA" if tail == "horiz-then-word" else CONTRIBUTIONS[tail])
        body = operand(blanks, padding) + "\n" + request
        yield "PHD-boundary", [blanks, tail, request, heading], render(
            body, literal=True, heading=heading)

    for blanks, tail in product((1, 2), ("horiz-one", "escaped-space", "ascii-space")):
        yield "PHD-eof", [blanks, tail], HEADER + ".nf\n" + operand(
            blanks, CONTRIBUTIONS[tail]) + "\n"

    for blanks, source_event in product((1, 2), ("earlier-nf", "same-coordinate")):
        body = operand(blanks, CONTRIBUTIONS["horiz-one"])
        if source_event == "earlier-nf":
            source = render(".\\\" Earlier fill request\n" + body + "\n", literal=True)
        else:
            # The macro expansion retains source coordinates from one
            # call site. The oracle tree supplies actual NODE_LINE flags.
            expansion = r"\&\c" + "\n" + body
            definition = ".de PAY\n" + expansion.replace("\\", "\\\\") + "\n..\n"
            source = render(definition + ".PAY\n", literal=True)
        yield "PHD-source-event", [blanks, source_event], source

    for blanks, tail in product((1, 2, 16, 128, 1024),
                                 ("generated", "authored", "alternating-fonts")):
        padding = {
            "generated": r"\h'1n'" * 64,
            "authored": r"\~" * 64,
            "alternating-fonts": (r"\fI\h'1n'\fR\~" * 32),
        }[tail]
        body = operand(blanks, padding) + "\n"
        yield "PHD-scale", [blanks, tail], render(body, literal=True)
