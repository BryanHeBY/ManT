"""Source-bound explanations of content findings, never punctuation folding.

Call with conservatively framed bodies and the unchanged decoded roff source.
The first rule models only a literal mdoc NAME prefix through the first Nd
line. Pinned CVS mdoc_term.c:termp_nd_pre emits an en dash for the Nd BODY;
ManT's mandoc/inline/scopes.rs emits an em dash. No authored dash is equivalent
on that account. Quotes, bullets, dynamic roff and other constructions are not
explained by this module.

Raw findings survive unchanged. A separate residual comparison changes exactly
one source-proven, positioned reference token, not all occurrences of a glyph.
An ``explained`` result is not pixel equality or complete document acceptance.
"""
from __future__ import annotations

from dataclasses import dataclass
from io import StringIO
import re
import unicodedata

from roff_content_compare import ContentLimits, LEXEME, compare_content, lexemes, visible_text


SCHEMA = "mant.roff-content-explanations/v1"
RULE = "mdoc-literal-NAME-generated-Nd-separator/v1"
_REQUEST = re.compile(r"^\.([A-Za-z]+)(?:[ \t]+(.*))?$")
_ARGUMENT = re.compile(r'"([^"\n]*)"|([^ \t"\n]+)')
_NAME = re.compile(r"[A-Za-z0-9_][A-Za-z0-9_.+:@-]*")


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
                    explanation_limits: ExplanationLimits = ExplanationLimits()) -> dict:
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
    raw = (compare_content(reference, mant, source, limits=limits)
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
    residual = compare_content(rewritten, mant, source, limits=limits)
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
