"""Source proofs for the admitted literal equation spellings."""
from __future__ import annotations
from collections import Counter
import re
from .syntax import (
    _DYNAMIC_ESCAPE,
)

_EQN_OPEN = re.compile(r"^[.']EQ(?:[ \t]+.*)?$")
_EQN_CLOSE = re.compile(r"^[.']EN(?:[ \t]+.*)?$")
_EQN_LITERAL_SUBSCRIPT = re.compile(
    r"(?<![A-Za-z0-9_.-])(?P<base>[A-Za-z][A-Za-z0-9_.-]*)[ \t]+sub[ \t]+"
    r"(?P<sub>[A-Za-z0-9][A-Za-z0-9_.-]*)(?![A-Za-z0-9_.-])"
)
_EQN_LITERAL_FRACTION = re.compile(
    r"(?<![A-Za-z0-9_.-])(?P<numerator>[A-Za-z][A-Za-z0-9_.-]*)[ \t]+over[ \t]+"
    r"(?P<denominator>[A-Za-z0-9][A-Za-z0-9_.-]*)(?![A-Za-z0-9_.-])"
)
_EQN_LITERAL_BRACED_FROM = re.compile(
    r"(?<![A-Za-z0-9_.-])(?P<base>[A-Za-z][A-Za-z0-9_.-]*)[ \t]+from[ \t]+"
    r"\{[^{}\n]+\}(?![A-Za-z0-9_.-])"
)
_EQN_LITERAL_LDOTS = re.compile(r"(?<![A-Za-z0-9_.-])ldots(?![A-Za-z0-9_.-])")
# CVS eqn.c's static eqnsyms table turns these literal operands into the
# corresponding named roff character before eqn_term.c applies positional
# rendering. Keep the source model deliberately small: it resolves only the
# operands needed to establish a complete, otherwise lexical relation proof.
_EQN_LITERAL_SYMBOLS = {"pi": "π", "PI": "Π"}
_EQN_DYNAMIC_WORDS = {"define", "delim", "gfont", "gsize", "include", "ifdef", "if"}


def _literal_eqn_relation_spellings(source: str) -> Counter[tuple[str, str]] | None:
    """Return source-bound simple eqn relation spellings, or decline execution.

    CVS ``eqn_term.c`` emits positional markers directly against their bases
    (``log_2`` and ``a/b``), whereas ManT retains the same parsed relations
    with explicit copyable spacing.  Both block equations and inline equations
    activated by a literal ``.EQ delim XY .EN`` setting are safe evidence;
    bare punctuation is never treated as eqn.  This is not an interpreter:
    only simple word operands and the GNU ``ldots`` token are accepted, and a
    dynamic eqn request declines the complete source proof.
    """
    spellings: Counter[tuple[str, str]] = Counter()
    in_block = False
    saw_equations = False
    pending_delimiters: tuple[str, str] | None = None
    active_delimiters: tuple[str, str] | None = None

    def collect(expressions: list[str]) -> bool:
        for expression in expressions:
            if _DYNAMIC_ESCAPE.search(expression):
                return False
            words = expression.split()
            if words and words[0].lower() in _EQN_DYNAMIC_WORDS:
                return False
            for match in _EQN_LITERAL_SUBSCRIPT.finditer(expression):
                base = _EQN_LITERAL_SYMBOLS.get(match['base'], match['base'])
                sub = _EQN_LITERAL_SYMBOLS.get(match['sub'], match['sub'])
                spellings[(
                    f"{base}_{sub}",
                    f"{base} _ {sub}",
                )] += 1
            for match in _EQN_LITERAL_FRACTION.finditer(expression):
                numerator = _EQN_LITERAL_SYMBOLS.get(match['numerator'], match['numerator'])
                denominator = _EQN_LITERAL_SYMBOLS.get(match['denominator'], match['denominator'])
                spellings[(
                    f"{numerator}/{denominator}",
                    f"{numerator} / {denominator}",
                )] += 1
            for match in _EQN_LITERAL_BRACED_FROM.finditer(expression):
                # The terminal tokenization of a braced lower limit is
                # renderer-specific: CVS exposes only `base_` while ManT
                # retains a separated marker before the complete operand.
                # We do not interpret that operand; the balanced braces are
                # merely the source fact that makes this attached marker an
                # eqn `from` relation rather than ordinary prose.
                base = _EQN_LITERAL_SYMBOLS.get(match['base'], match['base'])
                spellings[(f"{base}_", f"{base} _")] += 1
            # This is intentionally not a general prose normalization.
            # libmandoc's terminal renderer preserves the GNU token while
            # ManT's bounded owned-AST snapshot intentionally emits the
            # conventional copyable spelling.  The surrounding literal eqn
            # context and the exact inventories below keep that choice local.
            ldots_count = len(_EQN_LITERAL_LDOTS.findall(expression))
            if ldots_count:
                spellings[("ldots", "...")] += ldots_count
        return True

    for raw in source.splitlines():
        if _EQN_OPEN.fullmatch(raw):
            if in_block:
                return None
            in_block = True
            saw_equations = True
            pending_delimiters = None
            continue
        if _EQN_CLOSE.fullmatch(raw):
            if not in_block:
                return None
            in_block = False
            active_delimiters = pending_delimiters
            continue
        if in_block:
            if raw.endswith("\\"):
                return None
            delimiter = raw.strip().split()
            if delimiter[:1] == ["delim"]:
                if len(delimiter) != 2:
                    return None
                if delimiter[1] == "off":
                    pending_delimiters = None
                elif len(delimiter[1]) == 2:
                    pending_delimiters = (delimiter[1][0], delimiter[1][1])
                else:
                    return None
                continue
            if not collect([raw]):
                return None
            continue
        if active_delimiters is None:
            continue
        opening, closing = active_delimiters
        # Delimiters are literal one-character operands from the balanced
        # configuration line above; regex escaping keeps punctuation syntax
        # from broadening this source inspection.
        expressions = re.findall(
            re.escape(opening) + r"([^\n" + re.escape(closing) + r"]*)" + re.escape(closing),
            raw,
        )
        if expressions and not collect(expressions):
            return None
    if in_block:
        return None
    return spellings if saw_equations and spellings else None
