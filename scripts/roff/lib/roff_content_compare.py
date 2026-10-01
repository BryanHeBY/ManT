"""Bounded, bidirectional visible-content evidence for local renderer audits.

Unlike the historical word-presence oracle, retain Unicode, punctuation, short
names, occurrence counts and order. A difference is a review candidate, not a
claim that a reference formatter is authoritative about source semantics.
Nothing here interprets roff execution or imports ManT lowering decisions.
"""
from __future__ import annotations

from bisect import bisect_left
from collections import Counter
from dataclasses import dataclass
from difflib import SequenceMatcher
import re
import unicodedata


SCHEMA = "mant.roff-content-comparison/v1"
# Only SGR is known styling. Cursor movement/erasure and OSC (including title,
# clipboard and links) remain observable controls, not silently stripped text.
SGR = re.compile(r"\x1b\[[0-9;:]*m")
# Keep punctuation, combining marks and ZWJ adjacent to their original word.
# Splitting '-' from 'x' would make '-x' and '- x' falsely equal. Unicode minus,
# hyphens and quotes are distinct spellings unless source review proves a
# specific presentation-only difference; no page-wide folding is justified.
LEXEME = re.compile(r"\S+", re.UNICODE)
# The fixed CVS terminal formatter can split a URI at an internal breakable
# hyphen (term.c:term_fill, ASCII_HYPH) and emits the literal hyphen before its
# physical newline.  Content comparison already treats ordinary soft wrapping
# as whitespace, but that would otherwise turn one URI into two distinct
# lexemes.  This deliberately recognizes only a URI continuation; ordinary
# prose, option names and authored hyphen/newline boundaries stay observable.
_URI_CHARACTER = r"[A-Za-z0-9._~!$&'()*+,;=:@%/?#\[\]-]"
_URI_AT_LINE_END = re.compile(r"(?:https?|ftp)://" + _URI_CHARACTER + r"*-$")
_URI_CONTINUATION = re.compile(r"^[ \t]*(" + _URI_CHARACTER + r"+)")
CONTROL_REQUEST = re.compile(
    r"^[.'](?:ll|po|mc|ti|ad|na|hy|nh|ne|nr|ta|ft|ce|rj|Tg)(?:[ \t]+(.*))?$"
)


@dataclass(frozen=True)
class ContentLimits:
    max_chars: int = 4 * 1024 * 1024
    max_tokens: int = 500_000
    max_window: int = 256
    max_alignment_work: int = 4_000_000
    max_findings: int = 128
    preview_tokens: int = 24

    def __post_init__(self):
        if any(value < 1 for value in self.__dict__.values()):
            raise ValueError("content comparison budgets must be positive")


@dataclass(frozen=True)
class TerminalUriReflow:
    """A bounded physical-row projection with source-proof candidates."""

    text: str
    rows_rejoined: int
    uris: tuple[str, ...]
    evidence_complete: bool


@dataclass(frozen=True)
class TerminalHyphenReflow:
    """A physical-row reflow at a formatter's breakable hyphen."""

    text: str
    rows_rejoined: int
    terms: tuple[str, ...]
    evidence_complete: bool


def visible_text(text: str) -> str:
    """Remove known terminal styling, retaining unexpected control characters.

    Overstriking is resolved by cursor replacement within a physical row. An
    unmatched backspace is retained so it can never disappear from the safety
    signal. The scanner is linear, including adversarial backspace runs.
    """
    text = SGR.sub("", text).replace("\r\n", "\n")
    result: list[str] = []
    row: list[str] = []
    cursor = 0
    pending_motion = 0
    for char in text:
        if char == "\n":
            result.extend(row)
            result.extend("\b" * pending_motion)
            result.append(char)
            row = []
            cursor = 0
            pending_motion = 0
        elif char == "\b":
            if cursor and _single_terminal_cell(row[cursor - 1]):
                cursor -= 1
                pending_motion += 1
            else:
                # Do not guess a cell model for wide/combining/format glyphs,
                # tabs or a cursor already at the row origin. Preserve the
                # control so the caller explicitly reports incomplete safety.
                result.append(char)
        else:
            # Only a subsequent write confirms overstriking. A trailing cursor
            # movement is neither deletion nor styling and remains a control.
            pending_motion = 0
            if cursor < len(row):
                row[cursor] = char
            else:
                row.append(char)
            cursor += 1
    result.extend(row)
    result.extend("\b" * pending_motion)
    return "".join(result)


def _single_terminal_cell(char: str) -> bool:
    return (ord(char) >= 32 and not 127 <= ord(char) < 160
            and unicodedata.category(char) not in {"Mn", "Mc", "Me", "Cf"}
            and unicodedata.east_asian_width(char) not in {"W", "F"})


def lexemes(text: str, *, limit: int = 500_000) -> list[str]:
    """Materialize at most limit+1 tokens; the extra one proves truncation.

    Ordinary spaces and physical soft line wrapping remain interchangeable.
    NFC equivalence is visible equivalence, unlike punctuation or name folding.
    """
    if limit < 1:
        raise ValueError("lexeme budget must be positive")
    output = []
    for match in LEXEME.finditer(unicodedata.normalize("NFC", text)):
        output.append(match[0])
        if len(output) > limit:
            break
    return output


def reflow_terminal_uri_wraps(text: str, *, evidence_limit: int = 128) -> TerminalUriReflow:
    """Rejoin only a URI continuation split at a terminal breakable hyphen.

    This is comparison representation, never document/source normalization.
    CVS term.c replaces ASCII_HYPH with ``-`` before deciding its automatic
    line boundary. The next physical row therefore begins with the next URI
    character, after indentation. A source-authored newline outside a URI or
    a URI followed by an ordinary separate word is left unchanged.
    """
    if evidence_limit < 1:
        raise ValueError("URI reflow evidence budget must be positive")
    rows = text.splitlines()
    trailing_newline = text.endswith("\n")
    output: list[str] = []
    uris: list[str] = []
    evidence_complete = True
    index = 0
    while index < len(rows):
        row = rows[index]
        while index + 1 < len(rows) and _URI_AT_LINE_END.search(row):
            continuation = _URI_CONTINUATION.match(rows[index + 1])
            if continuation is None:
                break
            # Keep any text after the URI run on the physical continuation
            # row. Dropping it would make a comparison projection itself lose
            # evidence when a formatter wraps before trailing prose.
            joined_uri = row + continuation[1]
            row = joined_uri + rows[index + 1][continuation.end():]
            index += 1
            if len(uris) < evidence_limit:
                uris.append(joined_uri)
            else:
                evidence_complete = False
        output.append(row)
        index += 1
    return TerminalUriReflow(
        text="\n".join(output) + ("\n" if trailing_newline else ""),
        rows_rejoined=len(rows) - len(output), uris=tuple(uris),
        evidence_complete=evidence_complete,
    )


def reflow_terminal_hyphen_wraps(text: str, *, evidence_limit: int = 128) -> TerminalHyphenReflow:
    """Rejoin a physical line break immediately after any nonblank hyphen.

    CVS ``term_fill()`` makes every ``ASCII_HYPH`` a possible line break, not
    only hyphens in URIs.  This function deliberately knows *nothing* about
    source meaning: callers must separately prove every rejoined spelling was
    present literally in source before treating this as presentation evidence.
    It is therefore unsuitable for document normalization and is used only by
    the secondary audit projection.
    """
    if evidence_limit < 1:
        raise ValueError("hyphen reflow evidence budget must be positive")
    rows = text.splitlines()
    trailing_newline = text.endswith("\n")
    output: list[str] = []
    terms: list[str] = []
    evidence_complete = True
    index = 0
    while index < len(rows):
        row = rows[index]
        while index + 1 < len(rows):
            match = re.search(r"\S+-$", row)
            continuation = _URI_CONTINUATION.match(rows[index + 1])
            if match is None or continuation is None:
                break
            # Keep prose following the continuation token.  As with URI
            # reflow, an audit representation must never drop a physical row's
            # non-rejoined evidence.
            joined = match[0] + continuation[1]
            row = row[:match.start()] + joined + rows[index + 1][continuation.end():]
            index += 1
            if len(terms) < evidence_limit:
                terms.append(joined)
            else:
                evidence_complete = False
        output.append(row)
        index += 1
    return TerminalHyphenReflow(
        text="\n".join(output) + ("\n" if trailing_newline else ""),
        rows_rejoined=len(rows) - len(output), terms=tuple(terms),
        evidence_complete=evidence_complete,
    )


def _unique_anchors(left: list[str], right: list[str]) -> list[tuple[int, int]]:
    """Patience anchors bound alignment work without a quadratic page-wide diff."""
    counts_left, counts_right = Counter(left), Counter(right)
    positions = {token: index for index, token in enumerate(right) if counts_right[token] == 1}
    pairs = [(index, positions[token]) for index, token in enumerate(left)
             if counts_left[token] == 1 and token in positions]
    tails: list[int] = []
    tail_indices: list[int] = []
    previous: list[int] = []
    for index, (_, position) in enumerate(pairs):
        length = bisect_left(tails, position)
        previous.append(tail_indices[length - 1] if length else -1)
        if length == len(tails):
            tails.append(position)
            tail_indices.append(index)
        else:
            tails[length] = position
            tail_indices[length] = index
    result = []
    index = tail_indices[-1] if tail_indices else -1
    while index >= 0:
        result.append(pairs[index])
        index = previous[index]
    return list(reversed(result))


def compare_content(reference: str, mant: str, source: str | None = None, *,
                    limits: ContentLimits = ContentLimits(),
                    terminal_uri_wraps: bool = False,
                    terminal_hyphen_wraps: bool = False) -> dict:
    """Compare visible streams, with explicit coverage and bounded evidence.

    Reference-only content is not automatically loss; ManT-only content is not
    automatically leakage. Counts and edit locations survive for source review.
    Raw C0/DEL/C1 leaks in the product are hard failures regardless of reference.
    """
    findings: list[dict] = []
    counts: Counter[str] = Counter()
    coverage = {"complete": True, "reasons": [], "alignment_work": 0,
                "uncompared_tokens": 0, "findings_total": 0, "findings_retained": 0}

    def incomplete(reason: str, amount: int = 0):
        coverage["complete"] = False
        if reason not in coverage["reasons"]:
            coverage["reasons"].append(reason)
        coverage["uncompared_tokens"] += amount

    def add(kind: str, **details):
        counts[kind] += 1
        if len(findings) < limits.max_findings:
            findings.append({"kind": kind, **details})
        else:
            incomplete("finding-retention-budget")

    # Never silently compare just a prefix of an oversized document.
    if max(len(reference), len(mant), len(source or "")) > limits.max_chars:
        return {"schema": SCHEMA, "status": "uncovered", "findings": [],
                "counts": {}, "coverage": {**coverage, "complete": False,
                    "reasons": ["input-character-budget"]}}

    visible = {"reference": visible_text(reference), "mant": visible_text(mant)}
    uri_wraps = {"reference": 0, "mant": 0}
    hyphen_wraps = {"reference": 0, "mant": 0}
    if terminal_hyphen_wraps:
        for side, text in visible.items():
            reflow = reflow_terminal_hyphen_wraps(text)
            visible[side], hyphen_wraps[side] = reflow.text, reflow.rows_rejoined
    elif terminal_uri_wraps:
        for side, text in visible.items():
            reflow = reflow_terminal_uri_wraps(text)
            visible[side], uri_wraps[side] = reflow.text, reflow.rows_rejoined
    controls: dict[str, list[dict]] = {}
    for side, text in visible.items():
        bad = Counter(char for char in text if (ord(char) < 32 and char not in "\n\t")
                      or 127 <= ord(char) < 160)
        controls[side] = [{"codepoint": f"U+{ord(char):04X}", "count": count}
                          for char, count in sorted(bad.items())]
        if bad:
            add("control-leak" if side == "mant" else "reference-control", side=side,
                characters=controls[side])
    left = lexemes(visible["reference"], limit=limits.max_tokens)
    right = lexemes(visible["mant"], limit=limits.max_tokens)
    coverage.update(reference_tokens=len(left), mant_tokens=len(right))
    if not left or not right:
        incomplete("empty-comparable-stream")
    if max(len(left), len(right)) > limits.max_tokens:
        coverage["token_counts_are_lower_bounds"] = True
        incomplete("token-budget", len(left) + len(right))
    else:
        coverage["token_counts_are_lower_bounds"] = False
        lc, rc = Counter(left), Counter(right)
        missing, extra = lc - rc, rc - lc
        coverage.update(missing_occurrences=sum(missing.values()),
                        extra_occurrences=sum(extra.values()))
        for token, count in sorted(missing.items()):
            add("missing-occurrence", token=token, count=count,
                reference_count=lc[token], mant_count=rc[token])
        for token, count in sorted(extra.items()):
            add("extra-occurrence", token=token, count=count,
                reference_count=lc[token], mant_count=rc[token])

        if source is not None:
            # A matching token anywhere in the source is not enough evidence
            # that a non-output request leaked it.  For example, ``.ne 2``
            # and an equation's literal ``2`` may coexist.  First retain the
            # exact request operands that are extra in ManT, then check
            # whether each candidate also occurs in source text outside a
            # known non-output request.  The latter remains reviewable but is
            # not promoted as a likely control operand leak.
            request_operands: list[tuple[int, str, list[str]]] = []
            candidate_operands: set[str] = set()
            non_control_lines: list[str] = []
            for line, raw in enumerate(source.splitlines(), 1):
                match = CONTROL_REQUEST.fullmatch(raw)
                if match and match[1]:
                    operands = lexemes(match[1], limit=limits.max_tokens)
                    if len(operands) > limits.max_tokens:
                        incomplete("source-operand-token-budget")
                    payload = [token for token in operands if token in extra]
                    if payload:
                        request_operands.append((line, raw, payload))
                        candidate_operands.update(payload)
                else:
                    non_control_lines.append(raw)
            # This scans the source once, but only remembers candidates.  It
            # therefore remains bounded by the already admitted input size,
            # rather than materializing every source lexeme a second time.
            also_authored: set[str] = set()
            if candidate_operands:
                for raw in non_control_lines:
                    for match in LEXEME.finditer(unicodedata.normalize("NFC", raw)):
                        if match[0] in candidate_operands:
                            also_authored.add(match[0])
            for line, raw, payload in request_operands:
                plausible = [token for token in payload if token not in also_authored]
                ambiguous = [token for token in payload if token in also_authored]
                if plausible:
                    add("possible-control-operand", source_line=line,
                        request=raw[:256], tokens=plausible[:limits.preview_tokens])
                if ambiguous:
                    add("ambiguous-control-operand", source_line=line,
                        request=raw[:256], tokens=ambiguous[:limits.preview_tokens],
                        reason="also-occurs-outside-known-control-request")

        # Only distinct unequal windows invoke a quadratic matcher. Unique
        # anchors are monotone, so reordered content produces deletion/insertion
        # windows instead of being satisfied by equal words elsewhere.
        anchors = [(-1, -1), *_unique_anchors(left, right), (len(left), len(right))]
        for (a0, b0), (a1, b1) in zip(anchors, anchors[1:]):
            a, b = a0 + 1, b0 + 1
            la, rb = left[a:a1], right[b:b1]
            if la == rb:
                continue
            work = len(la) * len(rb)
            if (max(len(la), len(rb)) > limits.max_window or
                    coverage["alignment_work"] + work > limits.max_alignment_work):
                incomplete("alignment-window-budget", len(la) + len(rb))
                # Preserve a candidate even when only counts agree. Never
                # declare unaligned equal-multiset text order-preserving.
                add("unaligned-region", reference_range=[a, a1], mant_range=[b, b1],
                    reference_preview=la[:limits.preview_tokens], mant_preview=rb[:limits.preview_tokens])
                continue
            coverage["alignment_work"] += work
            for tag, a2, a3, b2, b3 in SequenceMatcher(None, la, rb, autojunk=False).get_opcodes():
                if tag != "equal":
                    add("sequence-" + tag, reference_range=[a + a2, a + a3],
                        mant_range=[b + b2, b + b3],
                        reference_preview=la[a2:a3][:limits.preview_tokens],
                        mant_preview=rb[b2:b3][:limits.preview_tokens])
    coverage.update(findings_total=sum(counts.values()), findings_retained=len(findings),
                    terminal_uri_wraps=uri_wraps if terminal_uri_wraps else None,
                    terminal_hyphen_wraps=hyphen_wraps if terminal_hyphen_wraps else None)
    status = ("hard-failure" if controls["mant"] else "review" if counts else
              "covered" if coverage["complete"] else "uncovered")
    return {"schema": SCHEMA, "status": status, "coverage": coverage,
            "counts": dict(sorted(counts.items())), "findings": findings}


def self_check():
    def kinds(a, b, source=None, **kwargs):
        return compare_content(a, b, source, **kwargs)
    assert kinds("café 世界 -x 0 []", "café 世界 -x 0 []")["status"] == "covered"
    assert kinds("Cafe\u0301", "Café")["status"] == "covered"
    assert kinds("ALPHA ALPHA", "ALPHA")["coverage"]["missing_occurrences"] == 1
    assert kinds("ALPHA", "ALPHA ALPHA")["coverage"]["extra_occurrences"] == 1
    for left, right in [("你好 世界", "你好"), ("-x", "-y"), ("[a]", "a"),
                        ("ALPHA BETA GAMMA", "GAMMA BETA ALPHA")]:
        assert kinds(left, right)["status"] == "review"
    for marker in ["\x00", "\x1a", "\x1c", "\x1d", "\x1e", "\x1f", "\x7f", "\x85"]:
        assert kinds("ALPHA", "ALPHA" + marker)["status"] == "hard-failure"
    assert kinds("ALPHA", "\x1b[31mALPHA\x1b[0m")["status"] == "covered"
    assert kinds("ALPHA", "A\bALPHA")["status"] == "covered"
    assert kinds("ALPHA", "\bALPHA")["status"] == "hard-failure"
    leak = kinds("BODY", "50n BODY", ".ll 50n\nBODY\n")
    assert "possible-control-operand" in leak["counts"]
    equation = kinds("BODY", "2 BODY", ".ne 2\n.EQ\nx sup 2\n.EN\n")
    assert "possible-control-operand" not in equation["counts"]
    assert "ambiguous-control-operand" in equation["counts"]
    assert kinds("a b c", "c b a", limits=ContentLimits(max_window=1))["status"] != "covered"
    assert not kinds("a", "a b c", limits=ContentLimits(max_findings=1))["coverage"]["complete"]
    assert kinds("a b", "a b", limits=ContentLimits(max_tokens=1))["status"] == "uncovered"
    assert kinds("", "")["status"] == "uncovered"
    assert kinds("a\nb", "a b")["status"] == "covered"  # soft reflow is not content loss
    assert kinds("https://example.test/container-\n registry/path", "https://example.test/container-registry/path")["status"] == "review"
    assert kinds("https://example.test/container-\n registry/path", "https://example.test/container-registry/path", terminal_uri_wraps=True)["status"] == "covered"
    assert kinds("https://example.test/one-\n two-\n three", "https://example.test/one-two-three", terminal_uri_wraps=True)["status"] == "covered"
    assert kinds("https://example.test/one-\n two trailing", "https://example.test/one-two trailing", terminal_uri_wraps=True)["status"] == "covered"
    assert kinds("NULL-\n terminated", "NULL-terminated", terminal_hyphen_wraps=True)["status"] == "covered"
    assert kinds("word-\n next trailing", "word-next trailing", terminal_hyphen_wraps=True)["status"] == "covered"
    assert kinds("word-\n next", "word-next")["status"] == "review"
    assert kinds("https://example.test/word-\n next", "https://example.test/word- next", terminal_uri_wraps=True)["status"] == "review"
    for left, right in [("--help", "- -help"), ("-x", "- x"),
                        ("👩\u200d💻", "👩 \u200d 💻"), ("क\u093f", "क \u093f"),
                        ("−x", "-x"), ("non\u2011breaking", "non-breaking"), ("‘name’", "'name'")]:
        assert kinds(left, right)["status"] == "review", (left, right)
    for escape in ["\x1b[2J", "\x1b[0K", "\x1b]52;c;eA==\x07", "\x1b]0;title\x07"]:
        assert kinds("A", "A" + escape)["status"] == "hard-failure"
    assert visible_text("ABC\b") == "ABC\b"  # movement alone erases nothing
    assert visible_text("ABC\b\bX") == "AXC"  # untouched suffix remains visible
    assert kinds("AB", "ABC\b")["status"] == "hard-failure"
    assert kinds("AXC", "ABC\b\bX")["status"] == "covered"
    assert kinds("é", "é\bé")["status"] == "covered"
    assert kinds("界", "界\b界")["status"] == "hard-failure"  # unverified cell geometry
    assert len(lexemes("x " * 1000, limit=3)) == 4
    bounded = kinds("x " * 1000, "x " * 1000, limits=ContentLimits(max_tokens=3))
    assert bounded["coverage"]["token_counts_are_lower_bounds"]
    assert bounded["coverage"]["reference_tokens"] == 4


if __name__ == "__main__":
    self_check()
    print("roff content comparison self-check succeeded")
