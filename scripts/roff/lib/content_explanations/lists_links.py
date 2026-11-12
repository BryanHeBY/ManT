"""Source proofs for list markers, columns and external targets."""
from __future__ import annotations
from collections import Counter
import re
from .syntax import (
    _AUDIT_DYNAMIC_REQUESTS,
    _MDOC_REQUEST,
)

_BULLET_ESCAPE = re.compile(r"\\(?:\(bu|\[bu\])")
_BR_MANUAL_REFERENCE = re.compile(
    r"^[.']BR[ \t]+([A-Za-z0-9_.:+-]+)[ \t]+\(([1-9][A-Za-z0-9]*)\)[ \t]*$",
    re.MULTILINE,
)
_MDOC_LITERAL_COLUMN_SEPARATOR = " | "
# CVS registers UR/UE and MT/ME as the same expanded-block/closing-block
# macro pair (man_macro.c) and uses the same pre_UR/post_UR terminal handlers
# for both (man_term.c).  The source proof below can consequently model their
# literal heads through one deliberately narrow grammar.
_MAN_EXTERNAL_TARGET = re.compile(r"^[.'](?:UR|MT)[ \t]+([^ \t]+)[ \t]*$")
_MAN_IP_ORDINAL = re.compile(r"^[.']IP[ \t]+(\([1-9][0-9]*\)|\[[1-9][0-9]*\]|[1-9][0-9]*[.)])(?:[ \t]+[^ \t]+)?[ \t]*$")
_MAN_EXTERNAL_DYNAMIC_ESCAPE = re.compile(r"\\(?:\*|n|g|V|\$|\[|\()")
_MAN_EXTERNAL_LITERAL_ESCAPES = {
    # GNU groff parses \: as a zero-width break (input.cpp), and CVS
    # mandoc's terminal character table likewise emits no visible glyph for
    # this special escape.  It is commonly used after an already-authored
    # URI colon, for example `http:\://…`; retaining another colon would
    # manufacture `http:://…` and invalidate the source proof below.
    r"\:": "",
    r"\-": "-",
    r"\%": "",
    r"\&": "",
}


def _literal_mdoc_bullet_item_count(source: str) -> int | None:
    """Count literal mdoc ``Bl -bullet`` items without executing roff.

    CVS ``mdoc_term.c`` renders each such item with a bullet while ManT's
    portable text renderer deliberately uses ``-``.  This is a presentation
    distinction, but only a fully balanced, non-dynamic list stream can prove
    that a particular document's bullets originated from that choice.  Nested
    lists are safe because ownership follows the innermost active ``Bl``;
    conditionals, definitions, includes and other execution-changing requests
    make the small source model decline the whole document.
    """
    stack: list[bool] = []
    items = 0
    for raw in source.splitlines():
        if raw.endswith("\\"):
            return None
        match = _MDOC_REQUEST.fullmatch(raw)
        if match is None:
            continue
        request, payload = match[1], match[2] or ""
        # roff requests are lowercase while mdoc macro names are capitalized;
        # never turn `.El` into the unrelated conditional `.el` merely by
        # folding its spelling during this non-executing inspection.
        if request.islower() and request in _AUDIT_DYNAMIC_REQUESTS:
            return None
        if request == "Bl":
            # mdoc option parsing is substantially broader than this bounded
            # source model.  The literal token is enough to prove the one
            # presentation policy we model; all other list options remain
            # irrelevant to bullet ownership.
            stack.append("-bullet" in payload.split())
            continue
        if request == "El":
            if not stack:
                return None
            stack.pop()
            continue
        if request == "It" and stack and stack[-1]:
            items += 1
    return items if not stack and items else None


def _literal_man_external_targets(source: str) -> Counter[str] | None:
    """Return direct GNU man-ext ``.UR``/``.MT`` targets, or decline unsafe source.

    CVS registers URI and mail targets with the same expanded block parser and
    sends both through ``man_term.c:post_UR``, which deliberately writes angle
    brackets around the head target.  ManT keeps the same typed target but
    chooses its own compact link presentation.  This small source model proves
    only literal, single-token heads; it refuses roff execution that could
    change source syntax or expand the target before the native parser sees it.
    """
    targets: Counter[str] = Counter()
    for raw in source.splitlines():
        if raw.endswith("\\"):
            return None
        match = _MDOC_REQUEST.fullmatch(raw)
        if match is not None and match[1].lower() in _AUDIT_DYNAMIC_REQUESTS:
            return None
        match = _MAN_EXTERNAL_TARGET.fullmatch(raw)
        if match is None:
            continue
        target = match[1]
        if _MAN_EXTERNAL_DYNAMIC_ESCAPE.search(target):
            return None
        for escaped, visible in _MAN_EXTERNAL_LITERAL_ESCAPES.items():
            target = target.replace(escaped, visible)
        if "\\" in target or not target:
            return None
        targets[target] += 1
    return targets or None


def _literal_man_ip_ordinals(source: str) -> Counter[tuple[str, str]] | None:
    """Return directly authored ``IP`` ordinal spellings and their canonical marks.

    ManT's source-proven ordered-list lowering intentionally represents all
    accepted man(7) ordinal styles as the portable ``N.`` marker.  The CVS
    terminal renderer prints the authored tag, such as ``(1)``.  This model is
    deliberately limited to literal ``IP`` heads and refuses all syntax- or
    expansion-changing roff requests before treating that difference as
    presentational.
    """
    markers: Counter[tuple[str, str]] = Counter()
    for raw in source.splitlines():
        if raw.endswith("\\"):
            return None
        match = _MDOC_REQUEST.fullmatch(raw)
        if match is not None and match[1].lower() in _AUDIT_DYNAMIC_REQUESTS:
            return None
        match = _MAN_IP_ORDINAL.fullmatch(raw)
        if match is None:
            continue
        authored = match[1]
        digits = authored.strip("()[]").rstrip(".)")
        markers[(authored, f"{digits}.")] += 1
    return markers or None


def _literal_mdoc_column_separator_count(source: str) -> int | None:
    """Count a deliberately small, non-executing subset of mdoc column cells.

    ManT's portable text renderer deliberately joins dense IR table cells with
    ``" | "`` while CVS terminal output aligns those cells with spaces.  This
    is presentation, but arbitrary ``|`` text is content.  Only accept a page
    when each expected separator comes from a literal ``.Bl -column`` / ``.It
    ... Ta ...`` stream: no nested list, macro mutation, continuation, or
    standalone text is interpreted here.  Callers must still require the
    exact resulting separator count before changing a comparison view.
    """
    if source.count(_MDOC_LITERAL_COLUMN_SEPARATOR):
        return None
    stack: list[bool] = []
    separators = 0
    unsafe = {"als", "am", "cc", "c2", "de", "di", "ds", "ec", "if", "ie", "el", "ig", "mso", "nr", "rm", "rn", "so", "soquiet", "tr"}
    for raw in source.splitlines():
        if raw.endswith("\\"):
            return None
        match = _MDOC_REQUEST.fullmatch(raw)
        if match is None:
            # Raw text inside a column list can be a valid cell continuation,
            # but then the short source model cannot bind its row separators.
            if stack and stack[-1]:
                return None
            continue
        request, payload = match[1], match[2] or ""
        if request == "Bl":
            if stack:
                # Nested list/table cells need full mdoc execution to map
                # their separator ownership.  Refuse rather than guess.
                return None
            options = payload.split()
            stack.append("-column" in options)
            continue
        if request == "El":
            if not stack:
                return None
            stack.pop()
            continue
        if stack and stack[-1] and request.lower() in unsafe:
            return None
        if not stack or not stack[-1]:
            continue
        if request != "It":
            return None
        # A quoted ``Ta`` is literal text; this simple scanner therefore
        # counts only whitespace-delimited, unquoted macro words.  ``\\&`` is
        # mdoc's zero-width, no-call escape and cannot create or consume a
        # macro word; other escapes can, so decline those rows altogether.
        payload = payload.replace("\\&", "")
        if "\\" in payload:
            return None
        quoted = False
        words: list[tuple[str, bool]] = []
        for word in payload.split():
            if word.startswith('"'):
                quoted = True
            words.append((word.strip('"'), quoted))
            if word.endswith('"'):
                quoted = False
        if quoted:
            return None
        separators += sum(1 for word, was_quoted in words if word == "Ta" and not was_quoted)
    return separators if not stack and separators else None
