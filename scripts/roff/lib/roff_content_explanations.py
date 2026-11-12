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
not pixel equality or complete document acceptance."""
from __future__ import annotations
import unicodedata
from scripts.roff.lib.roff_content_compare import ContentLimits, LEXEME, compare_content, lexemes, reflow_terminal_hyphen_wraps, visible_text
from .content_explanations.compatibility import (
    _source_consistent_compatibility_projection,
)
from .content_explanations.name_prefix import (
    ExplanationLimits,
    _name_prefix,
)


IMPLEMENTATION_FILES = (
    'scripts/roff/lib/roff_content_explanations.py',
    'scripts/roff/lib/content_explanations/__init__.py',
    'scripts/roff/lib/content_explanations/compatibility.py',
    'scripts/roff/lib/content_explanations/equations.py',
    'scripts/roff/lib/content_explanations/glyphs.py',
    'scripts/roff/lib/content_explanations/lists_links.py',
    'scripts/roff/lib/content_explanations/name_prefix.py',
    'scripts/roff/lib/content_explanations/syntax.py',
)

SCHEMA = "mant.roff-content-explanations/v1"
RULE = "mdoc-literal-NAME-generated-Nd-separator/v1"
ASSESSMENT_SCHEMA = "mant.roff-content-assessment/v3"
TERMINAL_HYPHEN_RULE = "cvs-terminal-breakable-hyphen/v1"


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
