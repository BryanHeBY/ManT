"""Source-owned hard-line observations and cross-renderer layout candidates."""
from __future__ import annotations

import re
from collections import Counter
from dataclasses import dataclass
from typing import Sequence
from scripts.roff.lib.roff_line_observations import SourceRow, observe_hard_lines
from .text import BORDERS, TRANSLATION, ROFF_REQUEST, ROFF_FONT_ESCAPE, strip_terminal_formatting

@dataclass
class LayoutProfile:
    """Geometry observed in one terminal rendering.

    The values are deliberately descriptive.  Groff's absolute columns and
    wrapping are device-dependent, while ManT renders a copyable semantic
    layout; only anchor-relative transitions can become candidates.
    """

    nonblank_lines: int
    blank_line_runs: int
    blank_lines: int
    max_indent: int
    indent_levels: list[int]



@dataclass
class LayoutComparison:
    reference: LayoutProfile
    mant: LayoutProfile
    shared_anchors: int
    reference_baseline_indent: int | None
    mant_baseline_indent: int | None
    candidates: list[str]
    hard_line_observation: dict


@dataclass(frozen=True)
class NoFillSourceLayout:
    """Source obligations narrow enough to compare across renderers.

    A formatter's implicit display gutter is not source content.  Only raw
    indentation authored inside a no-fill region, explicit blank requests,
    and adjacent authored lines are suitable cross-renderer layout signals.
    Flowed source is retained only to reject same-text, different-location
    collisions: it can produce the same visible fragments without losing a
    no-fill boundary.
    """

    anchors: frozenset[str]
    authored_indents: dict[str, int]
    spaced_pairs: frozenset[tuple[str, str]]
    consecutive_pairs: frozenset[tuple[str, str]]
    source_lines: frozenset[str]
    flowed_pairs: frozenset[tuple[str, str]]
    ordered_rows: tuple[SourceRow, ...]
    unclassified_literal_count: int
    unclassified_literal_rows: tuple[dict, ...]


@dataclass(frozen=True)
class LayoutLine:
    number: int
    indent: int
    key: str


def layout_key(value: str) -> str:
    """Return a conservative whole-line anchor for cross-renderer geometry.

    This intentionally does *not* dehyphenate or unwrap: a line-boundary probe
    needs to see whether one renderer joined source-visible lines that the
    other renderer retained.  Typography and bullet glyphs are normalised only
    enough to let the same short display line align across terminal devices.
    """
    value = value.translate(TRANSLATION)
    value = BORDERS.sub(" ", value)
    value = re.sub(r"\[\s+", "[", value)
    value = re.sub(r"\s+\]", "]", value)
    return " ".join(value.split()).casefold()


def leading_columns(value: str) -> int:
    expanded = value.expandtabs(8)
    return len(expanded) - len(expanded.lstrip(" "))


def no_fill_source_layout(source: str) -> NoFillSourceLayout:
    """Find authored no-fill obligations without adopting a formatter's gutter.

    The reference layout is only actionable when the roff source itself asks
    for line-preserving output.  This intentionally handles the common man and
    mdoc display forms plus their simple inline macro operands; a complex macro
    expansion merely produces no candidate, rather than a misleading claim.
    """
    starts = {"nf", "EX", "DS", "CS", "Vb"}
    ends = {"fi", "EE", "YS", "DE", "CE", "Ve"}
    inline_macros = {
        "B",
        "BI",
        "BR",
        "Cm",
        "Em",
        "I",
        "IB",
        "IR",
        "Li",
        "Nm",
        "Op",
        "RI",
        "RB",
        "S",
        "SM",
        "SY",
    }
    depth = 0
    anchors: set[str] = set()
    indent_observations: dict[str, set[int]] = {}
    spaced_pairs: set[tuple[str, str]] = set()
    consecutive_pairs: set[tuple[str, str]] = set()
    source_lines: set[str] = set()
    flowed_pairs: set[tuple[str, str]] = set()
    previous: str | None = None
    flowed_previous: str | None = None
    pending_space = False
    ordered_rows: list[SourceRow] = []
    owner = 0
    island = 0
    unclassified_literal_count = 0
    unclassified_literal_rows: list[dict] = []
    for source_number, raw_line in enumerate(source.splitlines(), 1):
        match = ROFF_REQUEST.fullmatch(raw_line)
        name = match.group("name") if match else None
        arguments = match.group("args") if match and match.group("args") else ""
        if name == "Bd" and "-literal" in arguments.split():
            depth += 1
            owner = source_number
            previous = None
            flowed_previous = None
            pending_space = False
            continue
        if name in starts:
            depth += 1
            owner = source_number
            previous = None
            flowed_previous = None
            pending_space = False
            continue
        if name == "Ed" or name in ends:
            depth = max(0, depth - 1)
            owner = source_number
            previous = None
            flowed_previous = None
            pending_space = False
            continue
        if not raw_line.strip() or name == "sp":
            if depth == 0:
                flowed_previous = None
            else:
                pending_space |= previous is not None
            continue
        if raw_line.lstrip().startswith((r'.\\"', r"'\\\"")):
            if depth == 0:
                flowed_previous = None
            continue
        candidate = arguments if name in inline_macros else raw_line if name is None else ""
        if name in inline_macros:
            # Macro arguments frequently quote an entire visible literal row.
            # Keep the row comparable with terminal text while leaving escaped
            # quotes untouched.
            candidate = re.sub(r'(?<!\\)"([^"]*)"', r"\1", candidate)
        candidate = ROFF_FONT_ESCAPE.sub("", candidate)
        candidate = candidate.replace(r"\&", "").replace(r"\~", " ")
        key = layout_key(candidate)
        if key:
            ordered_rows.append(SourceRow(source_number, owner, key, depth > 0, island))
        elif name not in {"sp", "br", "ft", "ad", "na", "hy", "nh", "ne"}:
            # An unmodeled request may generate visible text; do not claim
            # adjacency across it. All source rows still compete by occurrence.
            island += 1
            if depth > 0:
                unclassified_literal_count += 1
                if len(unclassified_literal_rows) < 256:
                    unclassified_literal_rows.append({"reason": "unclassified-literal-request", "sourceLine": source_number, "owner": owner, "request": name})
        if depth == 0:
            if useful_layout_anchor(key):
                source_lines.add(key)
                if flowed_previous is not None:
                    flowed_pairs.add((flowed_previous, key))
                flowed_previous = key
            elif name is not None:
                flowed_previous = None
            continue
        authored_indent = 0 if name in inline_macros else leading_columns(candidate)
        if useful_layout_anchor(key):
            anchors.add(key)
            source_lines.add(key)
            indent_observations.setdefault(key, set()).add(authored_indent)
            if previous is not None:
                pair = (previous, key)
                consecutive_pairs.add(pair)
                if pending_space:
                    spaced_pairs.add(pair)
            previous = key
            pending_space = False
    return NoFillSourceLayout(
        anchors=frozenset(anchors),
        authored_indents={
            key: next(iter(indents))
            for key, indents in indent_observations.items()
            if len(indents) == 1
        },
        spaced_pairs=frozenset(spaced_pairs),
        consecutive_pairs=frozenset(consecutive_pairs),
        source_lines=frozenset(source_lines),
        flowed_pairs=frozenset(flowed_pairs),
        ordered_rows=tuple(ordered_rows),
        unclassified_literal_count=unclassified_literal_count,
        unclassified_literal_rows=tuple(unclassified_literal_rows),
    )


def layout_lines(value: str) -> tuple[list[LayoutLine], int, int]:
    """Return nonblank terminal lines plus blank-line-run observations."""
    output: list[LayoutLine] = []
    blank_lines = 0
    blank_runs = 0
    in_blank_run = False
    for number, raw_line in enumerate(strip_terminal_formatting(value).splitlines(), 1):
        line = raw_line.expandtabs(8)
        stripped = line.lstrip(" ")
        if not stripped:
            blank_lines += 1
            if not in_blank_run:
                blank_runs += 1
                in_blank_run = True
            continue
        in_blank_run = False
        output.append(LayoutLine(number, len(line) - len(stripped), layout_key(stripped)))
    return output, blank_runs, blank_lines


def useful_layout_anchor(value: str) -> bool:
    # Avoid headings like "NAME" and generic fragments such as "the".  A
    # unique, sufficiently long whole line is a reliable alignment point even
    # when the surrounding prose has been wrapped differently.
    return len(value) >= 8 and sum(character.isalnum() for character in value) >= 4


def modal_indent(values: Sequence[int]) -> int:
    if not values:
        return 0
    counts = Counter(values)
    return min(counts, key=lambda value: (-counts[value], value))


def layout_profile(lines: Sequence[LayoutLine], blank_runs: int, blank_lines: int) -> LayoutProfile:
    indents = sorted({line.indent for line in lines})
    return LayoutProfile(
        nonblank_lines=len(lines),
        blank_line_runs=blank_runs,
        blank_lines=blank_lines,
        max_indent=max(indents, default=0),
        indent_levels=indents,
    )


def layout_comparison(reference: str, mant: str, source: str | None) -> LayoutComparison:
    """Extract conservative layout candidates without treating groff as law.

    `man(1)`/groff normally supplies a page-wide body indentation and wraps
    prose at a device width, while ManT deliberately emits reflow-free semantic
    text.  The comparison therefore aligns only unique whole-line anchors,
    derives each renderer's local body baseline, and reports *relative*
    indentation collapse, adjacent blank-gap divergence, and a short-line
    merge that is exact after whitespace normalisation.  All observations are
    review evidence rather than automatic fidelity failures.
    """
    reference_lines, reference_runs, reference_blanks = layout_lines(reference)
    mant_lines, mant_runs, mant_blanks = layout_lines(mant)
    source_layout = no_fill_source_layout(source) if source is not None else None
    no_fill_anchors = source_layout.anchors if source_layout is not None else frozenset()
    reference_by_key: dict[str, list[LayoutLine]] = {}
    mant_by_key: dict[str, list[LayoutLine]] = {}
    for line in reference_lines:
        if useful_layout_anchor(line.key):
            reference_by_key.setdefault(line.key, []).append(line)
    for line in mant_lines:
        if useful_layout_anchor(line.key):
            mant_by_key.setdefault(line.key, []).append(line)
    pairs = [
        (reference_by_key[key][0], mant_by_key[key][0])
        for key in reference_by_key.keys() & mant_by_key.keys()
        if len(reference_by_key[key]) == len(mant_by_key[key]) == 1
    ]
    pairs.sort(key=lambda pair: pair[0].number)
    reference_positive = [line.indent for line, _ in pairs if line.indent > 0]
    reference_baseline = modal_indent(reference_positive)
    mant_baseline = modal_indent(
        [candidate.indent for line, candidate in pairs if line.indent == reference_baseline]
    )
    candidates: list[str] = []

    collapsed = [
        (reference_line, mant_line)
        for reference_line, mant_line in pairs
        if source_layout is not None
        and source_layout.authored_indents.get(reference_line.key, 0) >= 2
        and reference_line.indent >= source_layout.authored_indents[reference_line.key]
        and mant_line.indent < source_layout.authored_indents[reference_line.key]
    ]
    if collapsed:
        samples = ", ".join(
            f"{reference_line.key[:48]!r} (source +{source_layout.authored_indents[reference_line.key]}, "
            f"reference={reference_line.indent}, ManT={mant_line.indent})"
            for reference_line, mant_line in collapsed[:3]
        )
        candidates.append(
            f"authored relative indentation may collapse for {len(collapsed)} aligned line(s): {samples}"
        )

    common_unique = {reference_line.key for reference_line, _ in pairs}
    mant_positions = {
        line.key: index
        for index, line in enumerate(mant_lines)
        if line.key in common_unique
    }
    spacing = []
    for reference_first, reference_second in zip(reference_lines, reference_lines[1:]):
        if (
            reference_first.key not in common_unique
            or reference_second.key not in common_unique
            or reference_first.key not in mant_positions
            or reference_second.key not in mant_positions
        ):
            continue
        mant_first_index = mant_positions[reference_first.key]
        mant_second_index = mant_positions[reference_second.key]
        if mant_second_index != mant_first_index + 1:
            continue
        reference_gap = reference_second.number - reference_first.number - 1
        mant_first = mant_lines[mant_first_index]
        mant_second = mant_lines[mant_second_index]
        mant_gap = mant_second.number - mant_first.number - 1
        if (
            reference_gap != mant_gap
            and source_layout is not None
            and (reference_first.key, reference_second.key) in source_layout.spaced_pairs
        ):
            spacing.append((reference_first.key, reference_gap, mant_gap))
    if spacing:
        samples = ", ".join(
            f"{key[:40]!r} (reference blank lines={reference_gap}, ManT={mant_gap})"
            for key, reference_gap, mant_gap in spacing[:3]
        )
        candidates.append(
            f"adjacent aligned lines have {len(spacing)} spacing divergence(s): {samples}"
        )

    hard_lines = observe_hard_lines(
        source_layout.ordered_rows if source_layout is not None else (),
        reference_lines,
        mant_lines,
    )
    if source_layout is not None and source_layout.unclassified_literal_count:
        hard_lines["uncoveredCount"] += source_layout.unclassified_literal_count
        hard_lines["uncovered"].extend(source_layout.unclassified_literal_rows[:256 - len(hard_lines["uncovered"])])
        if hard_lines["status"] != "review":
            hard_lines["status"] = "partial"
    if hard_lines["mergedCount"]:
        samples = ", ".join(
            f"owner {item['owner']} at source {item['sourceLines']}: "
            f"{item['first'][:32]!r} + {item['second'][:32]!r}"
            for item in hard_lines["merged"][:3]
        )
        candidates.append(
            f"reference line boundaries may merge in ManT for {hard_lines['mergedCount']} "
            f"source-owned pair(s): {samples}"
        )
    if hard_lines["gapDifferenceCount"]:
        samples = ", ".join(
            f"owner {item['owner']} at source {item['sourceLines']} "
            f"(reference={item['referenceBlankLines']}, ManT={item['mantBlankLines']})"
            for item in hard_lines["gapDifferences"][:3]
        )
        candidates.append(
            f"source-owned literal lines have {hard_lines['gapDifferenceCount']} "
            f"spacing divergence(s): {samples}"
        )
    if hard_lines["uncoveredCount"]:
        candidates.append(
            f"hard-line observation has {hard_lines['uncoveredCount']} "
            "uncovered owner/occurrence obligation(s); inspect hard_line_observation"
        )

    return LayoutComparison(
        reference=layout_profile(reference_lines, reference_runs, reference_blanks),
        mant=layout_profile(mant_lines, mant_runs, mant_blanks),
        shared_anchors=len(pairs),
        reference_baseline_indent=reference_baseline if pairs else None,
        mant_baseline_indent=mant_baseline if pairs else None,
        candidates=candidates,
        hard_line_observation=hard_lines,
    )
