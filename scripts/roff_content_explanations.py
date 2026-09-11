"""Source-bound and terminal-proven explanation layers for content findings.

Call with conservatively framed bodies and the unchanged decoded roff source.
The first rule models only a literal mdoc NAME prefix through the first Nd
line. Pinned CVS mdoc_term.c:termp_nd_pre emits an en dash for the Nd BODY;
ManT's mandoc/inline/scopes.rs emits an em dash. No authored dash is equivalent
on that account. Quotes, bullets, dynamic roff and other constructions are not
explained by this module.

Raw findings survive unchanged. A terminal presentation projection can rejoin
only the CVS formatter's URI breakable-hyphen rows before a source-bound rule
runs; literal GNU man-ext URI and mail-address blocks share a separately
recorded delimiter projection.  Neither changes the raw result.
A separate residual comparison changes exactly one source-proven, positioned
reference token, not all occurrences of a glyph. An ``explained`` result is
not pixel equality or complete document acceptance.
"""
from __future__ import annotations

from dataclasses import dataclass
from collections import Counter
from io import StringIO
import re
import unicodedata

from roff_content_compare import (
    ContentLimits,
    LEXEME,
    compare_content,
    lexemes,
    reflow_terminal_hyphen_wraps,
    visible_text,
)


SCHEMA = "mant.roff-content-explanations/v1"
RULE = "mdoc-literal-NAME-generated-Nd-separator/v1"
ASSESSMENT_SCHEMA = "mant.roff-content-assessment/v3"
TERMINAL_HYPHEN_RULE = "cvs-terminal-breakable-hyphen/v1"
_REQUEST = re.compile(r"^\.([A-Za-z]+)(?:[ \t]+(.*))?$")
_ARGUMENT = re.compile(r'"([^"\n]*)"|([^ \t"\n]+)')
_NAME = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.+:@-]*")
_BULLET_ESCAPE = re.compile(r"\\(?:\(bu|\[bu\])")
_BR_MANUAL_REFERENCE = re.compile(
    r"^[.']BR[ \t]+([A-Za-z0-9_.:+-]+)[ \t]+\(([1-9][A-Za-z0-9]*)\)[ \t]*$",
    re.MULTILINE,
)
_TERMINAL_MANUAL_REFERENCE = re.compile(r"([A-Za-z0-9_.:+-]+) \(([1-9][A-Za-z0-9]*)\)")
_MDOC_REQUEST = re.compile(r"^[.']([A-Za-z][A-Za-z0-9]*)(?:[ \t]+(.*))?$")
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
_AUDIT_DYNAMIC_REQUESTS = {
    "als", "am", "cc", "c2", "de", "di", "ds", "ec", "el", "ie", "if", "ig",
    "mso", "nr", "rm", "rn", "so", "soquiet", "tr", "wh",
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


def _replace_limited(text: str, original: str, replacement: str, limit: int) -> tuple[str, int]:
    """Replace at most ``limit`` exact display spellings without broadening scope."""
    if limit <= 0:
        return text, 0
    parts = text.split(original)
    available = len(parts) - 1
    count = min(available, limit)
    if count == 0:
        return text, 0
    return replacement.join(parts[:count + 1]) + original.join(parts[count + 1:]), count


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


@dataclass(frozen=True)
class ExplanationLimits:
    source_prefix_lines: int = 4096
    source_prefix_chars: int = 128 * 1024
    name_tokens: int = 256
    description_tokens: int = 64

    def __post_init__(self):
        if any(value < 1 for value in self.__dict__.values()):
            raise ValueError("explanation budgets must be positive")


def _arguments(value: str, limit: int) -> list[str] | None:
    """Literal mdoc arguments only; do not interpret escapes or doubled quotes."""
    if "\\" in value or any(ord(c) < 32 and c != "\t" for c in value):
        return None
    words = []
    end = 0
    for match in _ARGUMENT.finditer(value):
        # Arguments require whitespace separators; reject foo"bar" and """".
        if value[end:match.start()].strip() or (words and match.start() == end):
            return None
        word = match[1] if match[1] is not None else match[2]
        if not word:
            return None
        words.append(word)
        if len(words) > limit:
            return None
        end = match.end()
    return None if value[end:].strip() else words


def _name_prefix(source: str, limits: ExplanationLimits) -> tuple[dict | None, str]:
    """Prove execution only up to a literal first Nd, without evaluating roff.

    A whitelist before Nd rejects definitions, conditions, aliases, includes,
    escape/control-character changes, ignored regions and arbitrary requests.
    Later description macros need not be modeled: they are outside the proof
    prefix and remain in both original and residual comparisons.
    """
    metadata = set()
    in_name = False
    names: list[str] = []
    name_lines = []
    first_name = None
    chars = 0
    for number, raw in enumerate(StringIO(source), 1):
        chars += len(raw)
        if number > limits.source_prefix_lines or chars > limits.source_prefix_chars:
            return None, "source-prefix-budget"
        line = raw.rstrip("\r\n")
        if not line.strip():
            continue
        # Full-line comments only. A trailing escape could continue the line.
        if line.startswith('.\\"') and not line.endswith("\\"):
            continue
        match = _REQUEST.fullmatch(line)
        if not match:
            return None, "unmodeled-source-prefix"
        request, payload = match[1], match[2] or ""
        if not in_name:
            if request in {"Dd", "Dt", "Os"} and request not in metadata:
                args = _arguments(payload, limits.name_tokens)
                if args is None or (request == "Dt" and len(args) < 2):
                    return None, "nonliteral-mdoc-prologue"
                metadata.add(request)
                continue
            if request == "Sh" and _arguments(payload, 1) == ["NAME"] and "Dt" in metadata:
                in_name = True
                continue
            return None, "unmodeled-source-prefix"
        if request == "Nm":
            args = _arguments(payload, 2)
            if args == [] and first_name is not None:
                args = [first_name]
            if (not args or len(args) > 2 or (len(args) == 2 and args[1] != ",")
                    or not _NAME.fullmatch(args[0])
                    # Nm is parsed/callable. Conservatively reject macro-like
                    # names rather than mistaking e.g. Nm Em or Nm Bsx for
                    # literal text (the pinned callable set has 2/3 letters).
                    or re.fullmatch(r"[A-Z][a-z]{1,2}", args[0])):
                return None, "nonliteral-Nm-prefix"
            first_name = first_name or args[0]
            names.append(args[0] + ("," if len(args) == 2 else ""))
            name_lines.append(number)
            if len(names) > limits.name_tokens:
                return None, "name-token-budget"
            continue
        if request == "Nd" and names:
            # Nd itself is MDOC_JOIN, not MDOC_PARSED in pinned mdoc_macro.c;
            # words on this line are literal, unlike later .Em/.Xr requests.
            args = _arguments(payload, limits.description_tokens)
            if not args:
                return None, "nonliteral-or-empty-Nd-line"
            description = lexemes(" ".join(args), limit=limits.description_tokens)
            if len(description) > limits.description_tokens:
                return None, "description-token-budget"
            return {"prefix": ["NAME", *names], "description": description,
                    "sourceLine": number, "nameSourceLines": name_lines}, "literal-NAME-prefix"
        return None, "unmodeled-NAME-prefix"
    return None, "no-literal-first-Nd"


def explain_content(reference: str, mant: str, source: str | None, *,
                    raw_comparison: dict | None = None,
                    limits: ContentLimits = ContentLimits(),
                    explanation_limits: ExplanationLimits = ExplanationLimits(),
                    terminal_uri_wraps: bool = False,
                    terminal_hyphen_wraps: bool = False) -> dict:
    """Return original evidence plus source-bound explanations and residuals.

    ``raw_comparison``, when supplied, must be compare_content's result for
    these exact three inputs and limits. It is trusted caller-owned evidence,
    never mutated; supplying it avoids a duplicate full raw comparison.
    Status is raw status unless a rule applies; then ``explained`` requires a
    covered residual, otherwise residual review/hard-failure/uncovered survives.
    Classification coverage concerns this rule's prefix, NOT all roff source.
    Work is bounded by ContentLimits and a separate source-prefix budget. At
    most one additional bounded content comparison is made.
    """
    raw = (compare_content(reference, mant, source, limits=limits,
                           terminal_uri_wraps=terminal_uri_wraps,
                           terminal_hyphen_wraps=terminal_hyphen_wraps)
           if raw_comparison is None else raw_comparison)
    result = {"schema": SCHEMA, "status": raw["status"], "rawComparison": raw,
              "residualComparison": raw, "explanations": [],
              "coverage": {"comparisonComplete": raw["coverage"]["complete"],
                           "sourceScope": "first-literal-mdoc-NAME-prefix",
                           "sourceModel": "not-attempted", "reasons": []}}

    def stop(reason: str) -> dict:
        result["coverage"]["reasons"].append(reason)
        return result

    if raw["status"] != "review":
        return stop("no-review-or-hard-failure")
    if not raw["coverage"]["complete"]:
        return stop("raw-comparison-incomplete")
    if any(f["kind"] in {"control-leak", "reference-control"} for f in raw["findings"]):
        return stop("unresolved-output-controls")
    if max(len(reference), len(mant), len(source or "")) > limits.max_chars:
        return stop("input-character-budget")
    if source is None:
        return stop("source-unavailable")
    proof, reason = _name_prefix(source, explanation_limits)
    if proof is None:
        result["coverage"]["sourceModel"] = "unsupported"
        return stop(reason)
    result["coverage"]["sourceModel"] = "literal-NAME-prefix"
    left = unicodedata.normalize("NFC", visible_text(reference))
    right = unicodedata.normalize("NFC", visible_text(mant))
    prefix = [unicodedata.normalize("NFC", token) for token in proof["prefix"]]
    index = len(prefix)
    expected_left = [*prefix, "–", *proof["description"]]
    expected_right = [*prefix, "—", *proof["description"]]
    # No search, no anchor borrowing from elsewhere on the page. Both framed
    # streams must begin with the entire independently source-derived prefix.
    left_prefix = lexemes(left, limit=len(expected_left))[:len(expected_left)]
    right_prefix = lexemes(right, limit=len(expected_right))[:len(expected_right)]
    if left_prefix != expected_left or right_prefix != expected_right:
        return stop("source-prefix-not-at-output-origin")
    token = next(match for n, match in enumerate(LEXEME.finditer(left)) if n == index)
    rewritten = left[:token.start()] + "—" + left[token.end():]
    residual = compare_content(rewritten, mant, source, limits=limits,
                               terminal_uri_wraps=terminal_uri_wraps,
                               terminal_hyphen_wraps=terminal_hyphen_wraps)
    result["residualComparison"] = residual
    result["status"] = "explained" if residual["status"] == "covered" else residual["status"]
    result["coverage"]["comparisonComplete"] = residual["coverage"]["complete"]
    result["explanations"].append({
        "rule": RULE, "sourceLine": proof["sourceLine"],
        "nameSourceLines": proof["nameSourceLines"],
        "referenceTokenRange": [index, index + 1], "mantTokenRange": [index, index + 1],
        "reference": "–", "mant": "—", "occurrences": 1,
        "proofTokenRange": [0, len(expected_left)],
        "reason": "Source-proven first Nd BODY separator at the exact literal NAME prefix; no other punctuation occurrence was changed.",
    })
    return result


def _difference_weight(comparison: dict) -> int:
    """Count retained comparison signals without pretending they are defects."""
    return sum(comparison.get("counts", {}).values())


def _eligible_for_secondary_projection(comparison: dict) -> bool:
    """Permit presentation lenses when only the retained sample was capped.

    Token totals, order alignment and control checks are still complete in that
    case; only the serialized finding sample reached its bounded cap.  A
    source/terminal projection may therefore quantify a lower residual, while
    the raw counters and incomplete sample flag remain in the evidence.  Any
    actual comparison coverage gap stays ineligible.
    """
    coverage = comparison.get("coverage", {})
    reasons = set(coverage.get("reasons", ()))
    return (
        comparison.get("status") == "review"
        and reasons <= {"finding-retention-budget"}
        and not coverage.get("token_counts_are_lower_bounds", False)
    )


def _eligible_for_source_consistent_compatibility(comparison: dict) -> bool:
    """Allow exact source-bound display lenses after bounded alignment stops.

    A large unmatched window prevents an order proof, but it does *not* make
    occurrence counts, control scanning, or an exact literal source inventory
    unreliable.  Direct bullet, URI/mail delimiter, manual-reference, and
    column-cell projections only rewrite counted spellings under independent
    source bounds; they can therefore shrink their own presentation residue
    without upgrading an incomplete comparison to ``covered``.  Terminal
    hyphen reflow is deliberately excluded because it rewrites physical row
    boundaries and needs complete order evidence.
    """
    coverage = comparison.get("coverage", {})
    reasons = set(coverage.get("reasons", ()))
    return (
        comparison.get("status") == "review"
        and reasons <= {"finding-retention-budget", "alignment-window-budget"}
        and not coverage.get("token_counts_are_lower_bounds", False)
    )


def _source_is_consistent_with_hyphen_reflows(source: str | None, *reflows) -> bool:
    """Require every rejoined spelling to occur enough times literally in source.

    This is intentionally named *consistent*, rather than source-proven: raw
    text cannot establish which repeated source occurrence rendered on a given
    terminal row, and it cannot evaluate macro/register expansion.  It is only
    sufficient to demote an otherwise fully-explained physical-wrap candidate;
    raw comparison evidence and acceptance remain untouched.
    """
    if source is None:
        return False
    occurrences: dict[str, int] = {}
    for reflow in reflows:
        if not reflow.evidence_complete:
            return False
        for term in reflow.terms:
            occurrences[term] = occurrences.get(term, 0) + 1
    return all(source.count(term) >= count for term, count in occurrences.items())


def _source_consistent_compatibility_projection(reference: str, mant: str,
                                                source: str | None) -> tuple[str, str, list[dict]]:
    """Apply narrow, source-consistent display projections to each renderer.

    These model known terminal compatibility policy rather than roff execution:
    direct ``\\(bu``/``\\[bu]`` markers render as a terminal bullet while ManT's
    text list renderer uses ``-``; a literal ``.BR name (section)`` table cell
    is commonly printed by CVS with a space where ManT renders one atomic
    manual reference.  The source only proves compatible spelling/counts, not
    source-to-output location.  Thus raw evidence remains authoritative and
    this helper is never a semantic acceptance path.
    """
    if source is None:
        return reference, mant, []
    projected_reference = reference
    projected_mant = mant
    evidence: list[dict] = []

    direct_bullet_count = len(_BULLET_ESCAPE.findall(source))
    mdoc_bullet_count = _literal_mdoc_bullet_item_count(source)
    bullet_count = direct_bullet_count + (mdoc_bullet_count or 0)
    reference_bullets = projected_reference.count("•")
    if bullet_count and reference_bullets and reference_bullets <= bullet_count:
        projected_reference = projected_reference.replace("•", "-")
        evidence.append({
            "rule": "source-consistent-bullet-list-marker/v2",
            "sourceMarkers": bullet_count,
            "directSourceMarkers": direct_bullet_count,
            "mdocListItems": mdoc_bullet_count or 0,
            "referenceMarkers": reference_bullets,
            "reason": "Source-proven direct bullet escapes or literal mdoc Bl -bullet items are consistent with CVS UTF-8 bullet output and ManT's hyphen list-marker presentation.",
        })

    authored = Counter((match[1], match[2]) for match in _BR_MANUAL_REFERENCE.finditer(source))
    used: Counter[tuple[str, str]] = Counter()
    replacements = 0

    def replace_manual_reference(match: re.Match[str]) -> str:
        nonlocal replacements
        pair = (match[1], match[2])
        if used[pair] >= authored[pair]:
            return match[0]
        used[pair] += 1
        replacements += 1
        return f"{pair[0]}({pair[1]})"

    if authored:
        projected_reference = _TERMINAL_MANUAL_REFERENCE.sub(
            replace_manual_reference, projected_reference
        )
    if replacements:
        evidence.append({
            "rule": "source-consistent-BR-manual-reference-spacing/v1",
            "sourceReferences": sum(authored.values()),
            "referenceReferences": replacements,
            "reason": "Literal .BR name (section) source cells are consistent with CVS terminal spacing and ManT's atomic manual-reference presentation.",
        })

    external_targets = _literal_man_external_targets(source)
    if external_targets is not None:
        reference_replacements = 0
        mant_replacements = 0
        for target, limit in external_targets.items():
            projected_reference, count = _replace_limited(
                projected_reference, f"<{target}>", target, limit
            )
            reference_replacements += count
            projected_mant, count = _replace_limited(
                projected_mant, f"⟨{target}⟩", target, limit
            )
            mant_replacements += count
        if reference_replacements:
            evidence.append({
            "rule": "source-consistent-man-external-target-delimiters/v2",
            "sourceTargets": sum(external_targets.values()),
            "referenceDelimiters": reference_replacements,
            "mantDelimiters": mant_replacements,
            "reason": "Literal GNU man-ext .UR/.MT targets are bracketed by CVS man_term.c:post_UR while ManT preserves the same typed URI or mail address with compact link presentation.",
        })

    ordinal_markers = _literal_man_ip_ordinals(source)
    if ordinal_markers is not None:
        replacements = 0
        for (authored, canonical), limit in ordinal_markers.items():
            # Require the source inventory to account for every reference
            # spelling.  A matching number elsewhere in prose must keep this
            # page reviewable rather than borrowing list evidence.
            if projected_reference.count(authored) != limit:
                continue
            if projected_mant.count(canonical) < limit:
                continue
            projected_reference, count = _replace_limited(
                projected_reference, authored, canonical, limit
            )
            replacements += count
        if replacements:
            evidence.append({
                "rule": "source-consistent-man-IP-ordinal-marker/v1",
                "sourceMarkers": sum(ordinal_markers.values()),
                "referenceMarkers": replacements,
                "reason": "Literal punctuated man(7) IP tags are source-proven ordered items; CVS prints the authored tag while ManT's portable ordered-list renderer uses the canonical N. marker.",
            })

    column_separators = _literal_mdoc_column_separator_count(source)
    # Source text, CVS output, and the product output must agree on the
    # *complete* separator inventory.  This makes it impossible for this
    # presentation lens to erase a literal pipe or a partially lowered table.
    if (column_separators is not None
            and projected_reference.count(_MDOC_LITERAL_COLUMN_SEPARATOR) == 0
            and projected_mant.count(_MDOC_LITERAL_COLUMN_SEPARATOR) == column_separators):
        projected_mant = projected_mant.replace(_MDOC_LITERAL_COLUMN_SEPARATOR, " ")
        evidence.append({
            "rule": "source-consistent-mdoc-column-table-separators/v1",
            "sourceSeparators": column_separators,
            "mantSeparators": column_separators,
            "reason": "A literal, non-nested mdoc Bl -column stream has one Ta per ManT portable-text separator; CVS terminal alignment presents the same cell boundaries as spacing.",
        })
    return projected_reference, projected_mant, evidence


def assess_content(reference: str, mant: str, source: str | None, *,
                   raw_comparison: dict | None = None,
                   limits: ContentLimits = ContentLimits(),
                   explanation_limits: ExplanationLimits = ExplanationLimits()) -> dict:
    """Layer bounded presentation evidence over an unchanged raw comparison.

    The terminal rule is tied to CVS ``term.c:term_fill``: any breakable
    ``ASCII_HYPH`` may be emitted before automatic wrapping, including ordinary
    compound words as well as URIs.  Every candidate spelling must occur
    literally enough times in source.  This is consistency evidence rather
    than an execution proof: no roff execution, punctuation folding, table
    cell-content normalization, or product document-output mutation is
    performed.  The raw comparison is
    always retained verbatim; callers must use it for acceptance and
    regression evidence.
    """
    raw = (compare_content(reference, mant, source, limits=limits)
           if raw_comparison is None else raw_comparison)
    projection = raw
    projection_used = False
    projection_counts = {"reference": 0, "mant": 0}
    terminal_source_consistent = False
    if _eligible_for_secondary_projection(raw):
        # Inspect first so an unchanged page does not spend a second complete
        # comparison solely to establish that no terminal hyphen rule applies.
        reflows = {
            "reference": reflow_terminal_hyphen_wraps(visible_text(reference)),
            "mant": reflow_terminal_hyphen_wraps(visible_text(mant)),
        }
        projection_counts = {side: reflow.rows_rejoined for side, reflow in reflows.items()}
        terminal_source_consistent = _source_is_consistent_with_hyphen_reflows(source, *reflows.values())
        if any(projection_counts.values()) and terminal_source_consistent:
            projection = compare_content(reference, mant, source, limits=limits,
                                         terminal_hyphen_wraps=True)
            projection_used = True

    compatibility = projection
    compatibility_evidence: list[dict] = []
    compatibility_used = False
    if _eligible_for_source_consistent_compatibility(projection):
        projected_reference, projected_mant, compatibility_evidence = _source_consistent_compatibility_projection(
            visible_text(reference), visible_text(mant), source
        )
        if compatibility_evidence:
            compatibility = compare_content(
                projected_reference, projected_mant, source, limits=limits,
                terminal_hyphen_wraps=projection_used,
            )
            compatibility_used = _difference_weight(compatibility) < _difference_weight(projection)
            if not compatibility_used:
                compatibility = projection
                compatibility_evidence = []

    explained = explain_content(
        reference, mant, source, raw_comparison=compatibility, limits=limits,
        explanation_limits=explanation_limits, terminal_hyphen_wraps=projection_used,
    )
    terminal_helped = (
        projection_used
        and _difference_weight(projection) < _difference_weight(raw)
    )
    explanations = []
    if terminal_helped:
        explanations.append({
            "rule": TERMINAL_HYPHEN_RULE,
            "referenceRowsRejoined": projection_counts["reference"],
            "mantRowsRejoined": projection_counts["mant"],
            "reason": "Pinned CVS term.c emits a literal breakable hyphen before its automatic line break; only source-consistent continuation rows were rejoined for this secondary presentation comparison.",
        })
    explanations.extend(compatibility_evidence)
    explanations.extend(explained["explanations"])
    status = explained["status"]
    if (terminal_helped or compatibility_used) and status == "covered":
        status = "explained"
    return {
        "schema": ASSESSMENT_SCHEMA,
        "status": status,
        "rawComparison": raw,
        "terminalPresentationComparison": projection,
        "compatibilityPresentationComparison": compatibility,
        "residualComparison": explained["residualComparison"],
        "explanations": explanations,
        "coverage": {
            "rawComparisonComplete": raw["coverage"].get("complete", False),
            "terminalPresentationApplied": projection_used,
            "terminalHyphenSourceConsistent": terminal_source_consistent,
            "terminalHyphenRowsRejoined": projection_counts,
            "sourceConsistentCompatibilityApplied": compatibility_used,
            "sourceExplanation": explained["coverage"],
            "reasons": [],
        },
    }
