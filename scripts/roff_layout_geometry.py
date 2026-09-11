"""Bounded source-conditioned geometry evidence, independent of ManT IR.

This is not a roff interpreter.  It recognizes a small static source language,
then compares actual formatter line gaps and relative origins.  In particular,
it never counts Pp requests to predict terminal rows: mandoc's termp_pp_pre /
term_vspace and native validation own that behavior.  Unknown source execution,
ambiguous occurrences and exhausted budgets are observable coverage gaps.

The selected contract is docs/manuals/mant-roff.md.  Filled soft wrapping,
page-wide gutters, device alignment, and HP's terminal short-line flush artifact
are not interchangeable with literal row, paragraph-gap or RS-owner evidence.
"""

from collections import defaultdict
from dataclasses import dataclass
import re

from roff_content_compare import visible_text


@dataclass(frozen=True)
class GeometryLimits:
    source_bytes: int = 2 * 1024 * 1024
    rendered_bytes: int = 8 * 1024 * 1024
    anchors: int = 4096
    rendered_tokens: int = 300_000
    alignment_work: int = 4_000_000
    occurrences_per_anchor: int = 256
    findings: int = 256
    coverage_details: int = 256


@dataclass(frozen=True)
class _Anchor:
    words: tuple[str, ...]
    source_line: int
    owner: tuple[int, ...]
    literal: bool
    heading: bool
    authored_indent: int
    gap_before: bool
    island: int
    trusted_indent: bool


@dataclass(frozen=True)
class _Span:
    first: int
    last: int
    first_line: int
    last_line: int
    indent: int


class _Limit(Exception):
    pass


class _Gaps(list):
    """Bound detail memory while keeping omitted coverage explicitly visible."""
    def __init__(self, limit):
        super().__init__()
        self.limit = limit
        self.total = 0

    def append(self, value):
        self.total += 1
        if len(self) < self.limit:
            super().append(value)
        else:
            self[-1] = {"reason": "coverage-details-truncated", "omitted": self.total - self.limit + 1}


_REQUEST = re.compile(r"^[.'](?P<name>\S+)(?:[ \t]+(?P<args>.*))?$")
_FONT = re.compile(r"\\f(?:\[[^\]\\\n]*\]|\([^\n]{2}|[^\n])")
_DYNAMIC = {"de", "de1", "am", "am1", "if", "ie", "el", "while", "so", "soquiet"}
_TRANSPARENT = {"ft", "Tg", "ad", "na", "hy", "nh", "ne"}
_METADATA = {"TH", "Dd", "Dt", "Os"}
_POLICY = {
    "HP": "hp-continuation-and-short-line-flush-require-native-provenance",
    "ce": "documented-device-centering-difference",
    "rj": "documented-device-right-alignment-difference",
    "ti": "documented-temporary-indent-omission",
    "ll": "documented-device-line-length-omission",
    "po": "documented-device-page-origin-omission",
    "in": "executed-indent-state-requires-native-provenance",
    "TS": "documented-table-column-geometry-difference",
    "EQ": "documented-equation-typesetting-difference",
}


def _words(text):
    # Case and typographic quote differences cannot change geometry.  Do not
    # remove executable punctuation, dehyphenate, or interpret roff twice.
    text = text.translate(str.maketrans({"‘": "'", "’": "'", "“": '"', "”": '"'}))
    return tuple(text.casefold().split())


def _source_text(raw):
    """Recognize only transparent, verified source spelling escapes."""
    raw = _FONT.sub("", raw)
    output = []
    index = 0
    while index < len(raw):
        character = raw[index]
        index += 1
        if character != "\\":
            output.append(character)
            continue
        if index == len(raw):
            return None
        escape = raw[index]
        index += 1
        if escape in "&^|":
            continue
        if escape in "e\\":
            output.append("\\")
        elif escape == "-":
            output.append("-")
        elif escape in " ~0":
            output.append(" ")
        else:
            return None
    return "".join(output)


def _source_anchors(source, limits):
    anchors, uncovered = [], _Gaps(limits.coverage_details)
    lines = source.splitlines()
    for number, line in enumerate(lines, 1):
        match = _REQUEST.fullmatch(line)
        if match and match["name"] in _DYNAMIC:
            uncovered.append({"reason": "dynamic-source-requires-native-provenance", "source_line": number})
            return [], uncovered, 0
    owner, stack = (0,), []
    serial, scopes, island = 0, 1, 0
    literal, trusted_indent, gap = False, True, False
    paragraph, paragraph_line = [], 0
    unmodeled_block = None

    def emit(text, number, heading=False):
        nonlocal gap
        words = _words(text)
        if not words:
            return
        if len(anchors) >= limits.anchors:
            raise _Limit("source-anchor-budget")
        anchors.append(_Anchor(words, number, owner, literal, heading,
                               len(text.expandtabs(8)) - len(text.expandtabs(8).lstrip(" ")),
                               gap, island, trusted_indent))
        gap = False

    def flush():
        nonlocal paragraph
        if paragraph:
            emit(" ".join(paragraph), paragraph_line)
            paragraph = []

    def barrier(number, reason):
        nonlocal island, trusted_indent
        flush()
        uncovered.append({"reason": reason, "source_line": number, "owner": list(owner)})
        island += 1
        trusted_indent = False

    for number, raw in enumerate(lines, 1):
        if raw.startswith((r'.\"', r"'\"")):
            continue
        match = _REQUEST.fullmatch(raw)
        name, arguments = (match["name"], match["args"] or "") if match else (None, "")
        if unmodeled_block:
            if name == unmodeled_block:
                barrier(number, "unmodeled-block-end:" + name)
                unmodeled_block = None
            continue
        if name in {"TS", "EQ"}:
            barrier(number, _POLICY[name])
            unmodeled_block = "TE" if name == "TS" else "EN"
            continue
        if name in _METADATA or name in _TRANSPARENT:
            continue
        if name in {"SH", "SS", "Sh", "Ss"}:
            flush()
            if stack:
                uncovered.append({"reason": "unclosed-scope-at-section", "source_line": number})
            serial += 1
            owner, stack, literal, trusted_indent = (serial,), [], False, True
            scopes += 1
            island += 1
            heading = _source_text(arguments.strip('"'))
            if heading is not None:
                emit(heading, number, True)
            else:
                barrier(number, "unclassified-heading-spelling")
            continue
        if name in {"nf", "EX", "fi", "EE"}:
            flush()
            literal = name in {"nf", "EX"}
            gap = True
            scopes += 1
            continue
        if name in {"RS", "Bd"}:
            flush()
            supported = name == "RS" or any(flag in arguments.split() for flag in ["-literal", "-unfilled", "-filled", "-ragged"])
            if not supported:
                barrier(number, "unclassified-or-device-aligned-display")
            # Only literal distances establish static RS/Bd evidence.  No
            # numerical conversion is done here: the real renderer is oracle.
            if name == "RS" and arguments and not re.fullmatch(r"[+-]?\d+(?:\.\d+)?[nmuicpv]?", arguments):
                barrier(number, "nonliteral-relative-scope-distance")
            if name == "Bd" and "-offset" in arguments.split():
                tail = arguments.split("-offset", 1)[1].strip().split()
                if not tail or not re.fullmatch(r"[+-]?\d+(?:\.\d+)?[nmuicpv]?|indent|left", tail[0]):
                    barrier(number, "nonliteral-display-offset")
            stack.append((name, owner, literal, trusted_indent))
            serial += 1
            owner += (serial,)
            scopes += 1
            if name == "Bd":
                literal = any(flag in arguments.split() for flag in ["-literal", "-unfilled"])
            gap = True
            continue
        if name in {"RE", "Ed"}:
            flush()
            expected = "RS" if name == "RE" else "Bd"
            if arguments or not stack or stack[-1][0] != expected:
                barrier(number, "unclassified-scope-close")
            else:
                _, owner, literal, trusted_indent = stack.pop()
            gap = True
            continue
        if name in {"Pp", "PP", "P", "LP", "sp", "br"} or not raw.strip():
            flush()
            gap = True
            continue
        if name is not None:
            if name not in {"B", "I", "SM"}:
                barrier(number, _POLICY.get(name, "unclassified-source-request:" + name))
                continue
            if '"' in arguments:
                if not (arguments.startswith('"') and arguments.endswith('"') and arguments.count('"') == 2):
                    barrier(number, "unclassified-macro-argument-boundaries")
                    continue
                arguments = arguments[1:-1]
            raw = arguments
        visible = _source_text(raw)
        if visible is None:
            barrier(number, "unclassified-source-escape")
            continue
        if literal:
            emit(visible, number)
        else:
            if not paragraph:
                paragraph_line = number
            paragraph.append(visible)
    flush()
    if stack:
        uncovered.append({"reason": "unclosed-source-scope", "source_line": len(lines)})
    if unmodeled_block:
        uncovered.append({"reason": "unclosed-unmodeled-block", "source_line": len(lines)})
    return anchors, uncovered, scopes


def _render_tokens(text, limits):
    words, lines, indents = [], [], []
    index = defaultdict(list)
    # Share only the terminal transport decoder, never content alignment or
    # source/IR decisions. An unverified control remains visible/unmatchable.
    for number, raw in enumerate(visible_text(text).splitlines(), 1):
        raw = raw.expandtabs(8)
        # Native term.c retains authored nonbreaking spaces as U+00A0 in
        # UTF-8. They occupy one terminal cell, just like ordinary spaces.
        # Do not silently drop them while measuring a relative origin.
        indent = len(raw) - len(raw.lstrip(" \u00a0"))
        for word in _words(raw):
            if len(words) >= limits.rendered_tokens:
                raise _Limit("rendered-token-budget")
            index[word].append(len(words))
            words.append(word)
            lines.append(number)
            indents.append(indent)
    return words, lines, indents, index


def _matches(anchor, rendered, limits, budget):
    words, lines, indents, index = rendered
    candidates = index.get(anchor.words[0], [])
    if len(candidates) > limits.occurrences_per_anchor:
        return None
    spans = []
    for start in candidates:
        budget[0] += len(anchor.words)
        if budget[0] > limits.alignment_work:
            raise _Limit("alignment-work-budget")
        end = start + len(anchor.words)
        if tuple(words[start:end]) == anchor.words:
            spans.append(_Span(start, end - 1, lines[start], lines[end - 1], indents[start]))
    return spans


def _resolve(anchors, candidates):
    """Use ordered occurrences between independently unique source landmarks."""
    resolved = {i: choices[0] for i, choices in enumerate(candidates) if choices is not None and len(choices) == 1}
    # Repeated source rows must not all claim one rendered occurrence.
    counts = defaultdict(list)
    for i, span in resolved.items():
        counts[span.first].append(i)
    for indices in counts.values():
        if len(indices) > 1:
            for i in indices:
                resolved.pop(i)
    groups = defaultdict(list)
    for i, anchor in enumerate(anchors):
        if i not in resolved:
            previous = max((j for j in resolved if j < i), default=None)
            following = min((j for j in resolved if j > i), default=None)
            groups[(previous, following, anchor.words)].append(i)
    for (previous, following, _), indices in groups.items():
        low = resolved[previous].last if previous is not None else -1
        high = resolved[following].first if following is not None else float("inf")
        choices = candidates[indices[0]]
        if choices is None:
            continue
        choices = [span for span in choices if low < span.first and span.last < high]
        if len(choices) == len(indices):
            resolved.update(zip(indices, choices))
    return resolved


def compare_layout_geometry(reference, mant, source, *, limits=None):
    """Return JSON-ready findings and honest per-obligation coverage.

    ``covered`` means only the supported static obligations were compared;
    unknown source/control/occurrence cases make the report partial/uncovered.
    Callers must not translate partial or uncovered into a clean audit result.
    """
    limits = limits or GeometryLimits()
    if any(value <= 0 for value in vars(limits).values()):
        raise ValueError("geometry budgets must be positive")
    report = {"schema": "mant.roff-layout-geometry/v1", "status": "uncovered", "findings": [],
              "coverage": {"source_scopes": 0, "source_anchors": 0, "aligned_anchors": 0,
                           "compared_obligations": 0, "uncovered": _Gaps(limits.coverage_details),
                           "uncovered_count": 0, "alignment_work": 0},
              "policy": ["global-renderer-gutter-is-not-an-error", "filled-soft-wrap-is-not-a-hard-line-obligation",
                         "actual-reference-gaps-not-source-request-counts",
                         "source-owner-order-is-output-occurrence-not-IR-ownership"]}
    coverage = report["coverage"]
    if source is None:
        coverage["uncovered"].append({"reason": "source-unavailable"})
        coverage["uncovered_count"] = 1
        return report
    try:
        for value, maximum in [(source, limits.source_bytes), (reference, limits.rendered_bytes), (mant, limits.rendered_bytes)]:
            if len(value) > maximum or len(value.encode("utf-8")) > maximum:
                raise _Limit("input-byte-budget")
        anchors, uncovered, scopes = _source_anchors(source, limits)
        coverage.update(source_scopes=scopes, source_anchors=len(anchors), uncovered=uncovered)
        outputs = [_render_tokens(text, limits) for text in [reference, mant]]
        budget = [0]
        candidates = [[_matches(anchor, output, limits, budget) for anchor in anchors] for output in outputs]
        coverage["alignment_work"] = budget[0]
        resolved = [_resolve(anchors, choices) for choices in candidates]
        common = sorted(resolved[0].keys() & resolved[1].keys())
        coverage["aligned_anchors"] = len(common)

        def finding(kind, i, **details):
            if len(report["findings"]) >= limits.findings:
                raise _Limit("finding-budget")
            anchor = anchors[i]
            report["findings"].append({"kind": kind, "source_line": anchor.source_line,
                                       "owner": list(anchor.owner), "text": " ".join(anchor.words)[:160], **details})

        for i, anchor in enumerate(anchors):
            if i in common:
                continue
            reason = "ambiguous-or-unmatched-source-occurrence"
            if i in resolved[0] and not candidates[1][i]:
                finding("missing-rendered-anchor", i, reference_line=resolved[0][i].first_line)
            elif i in resolved[0] and candidates[1][i]:
                finding("owner-position-or-occurrence-count", i, reference_line=resolved[0][i].first_line)
            coverage["uncovered"].append({"reason": reason, "source_line": anchor.source_line, "owner": list(anchor.owner)})

        for left, right in zip(common, common[1:]):
            for renderer, mapping in zip(["reference", "mant"], resolved):
                if mapping[left].last >= mapping[right].first:
                    finding("source-owner-order", right, renderer=renderer,
                            preceding_source_line=anchors[left].source_line)

        # Compare gaps only across consecutive, fully modeled source content.
        # Duplicate/superseded Pp behavior is measured from the real reference.
        for i in range(1, len(anchors)):
            left, right = anchors[i - 1], anchors[i]
            if left.heading or right.heading or left.island != right.island:
                continue
            if not (right.gap_before or (left.literal and right.literal)):
                continue
            if i - 1 not in common or i not in common:
                continue
            a, b = resolved[0][i - 1], resolved[0][i]
            c, d = resolved[1][i - 1], resolved[1][i]
            if b.first != a.last + 1 or d.first != c.last + 1:
                coverage["uncovered"].append({"reason": "unmodeled-rendered-content-between-anchors", "source_line": right.source_line})
                continue
            coverage["compared_obligations"] += 1
            if b.first_line <= a.last_line:
                coverage["uncovered"].append({"reason": "reference-does-not-retain-source-row-boundary", "source_line": right.source_line})
                continue
            expected, actual = b.first_line - a.last_line - 1, d.first_line - c.last_line - 1
            if actual != expected:
                finding("line-boundary" if actual < 0 else "blank-gap", i,
                        reference_lines=[a.last_line, b.first_line], mant_lines=[c.last_line, d.first_line],
                        reference_blank_lines=expected, mant_blank_lines=actual)

        # Each scope is measured against its nearest matched parent origin;
        # root authored indents use a zero-indent sibling. Subtracting both
        # origins eliminates page-wide gutters without hiding over-indentation.
        bodies = [i for i in common if not anchors[i].heading and anchors[i].trusted_indent]
        for i in bodies:
            anchor = anchors[i]
            parent = anchor.owner[:-1] if len(anchor.owner) > 1 else anchor.owner
            bases = [j for j in bodies if j != i and anchors[j].owner == parent and anchors[j].authored_indent == 0]
            if not bases:
                if len(anchor.owner) > 1 or anchor.authored_indent:
                    coverage["uncovered"].append({"reason": "parent-origin-not-observable", "source_line": anchor.source_line, "owner": list(anchor.owner)})
                continue
            basis = min(bases, key=lambda j: (abs(j - i), j))
            expected = resolved[0][i].indent - resolved[0][basis].indent
            actual = resolved[1][i].indent - resolved[1][basis].indent
            coverage["compared_obligations"] += 1
            if actual != expected:
                finding("relative-origin", i, basis_source_line=anchors[basis].source_line,
                        reference_line=resolved[0][i].first_line, mant_line=resolved[1][i].first_line,
                        reference_delta=expected, mant_delta=actual)
        if not coverage["compared_obligations"]:
            coverage["uncovered"].append({"reason": "no-comparable-geometry-obligations"})
    except _Limit as error:
        coverage["uncovered"].append({"reason": str(error)})
    report["status"] = ("review" if report["findings"] else "partial" if coverage["uncovered"] and coverage["compared_obligations"]
                        else "uncovered" if coverage["uncovered"] else "covered")
    coverage["uncovered_count"] = coverage["uncovered"].total
    return report


def self_check():
    """Small importable smoke gate; full fault injection lives beside this module."""
    source = ".nf\nA\n.Pp\nB\n.fi\n"
    assert compare_layout_geometry(" A\n\n B\n", "A\nB\n", source)["status"] == "review"
    assert compare_layout_geometry(" A\n\n B\n", "A\n\nB\n", source)["status"] == "covered"


if __name__ == "__main__":
    self_check()
    print("roff layout geometry self-check passed")
