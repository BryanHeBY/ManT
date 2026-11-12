"""Source proofs for literal GNU named and composite glyphs."""
from __future__ import annotations
from collections import Counter
import re
from .syntax import (
    _AUDIT_DYNAMIC_REQUESTS,
    _DYNAMIC_ESCAPE,
    _MDOC_REQUEST,
)

# The product's roff escape decoder deliberately preserves this narrow group
# of documented groff caron spellings even though pinned CVS mandoc does not
# provide them in ``chars.c``. Keep the audit projection equally narrow: this
# is not a general roff interpreter or a promise that arbitrary groff
# character names survive lowering. The mapping mirrors the source-preserving
# compatibility fact in ``mant-codec/src/mandoc/roff_escape/glyphs.rs``.
_GROFF_NAMED_GLYPHS = {
    "vC": "Č",
    "vc": "č",
    "vS": "Š",
    "vs": "š",
    "vZ": "Ž",
    "vz": "ž",
}
_GROFF_NAMED_GLYPH_IN_WORD = re.compile(
    r"(?<![\\A-Za-z0-9_.+:/-])"
    r"(?P<prefix>[A-Za-z0-9_.+:/-]*)"
    r"\\(?:(?P<compact>\()(?P<compact_name>v[CcSsZz])|\[(?P<bracket_name>v[CcSsZz])\])"
    r"(?P<suffix>[A-Za-z0-9_.+:/-]*)"
    r"(?![A-Za-z0-9_.+:/-])"
)
_GROFF_DEFAULT_COMPOSITE_ACCENTS = {
    "ga": "\u0300", "`": "\u0300",
    "aa": "\u0301", "'": "\u0301",
    "a^": "\u0302", "^": "\u0302",
    "a~": "\u0303", "~": "\u0303",
    "a-": "\u0304", "-": "\u0304",
    "ab": "\u0306",
    "a.": "\u0307", ".": "\u0307",
    "ad": "\u0308", ":": "\u0308",
    "ao": "\u030A",
    "a\"": "\u030B", "\"": "\u030B",
    "ah": "\u030C",
    "ac": "\u0327", ",": "\u0327",
    "ho": "\u0328",
}
_GROFF_COMPOSITE_IN_WORD = re.compile(
    r"(?<![\\A-Za-z0-9_.+:/-])"
    r"(?P<prefix>[A-Za-z0-9_.+:/-]*)"
    r"\\\[(?P<name>[^]\n]+)\]"
    r"(?P<suffix>[A-Za-z0-9_.+:/-]*)"
    r"(?![A-Za-z0-9_.+:/-])"
)
# A literal composite has groff's shipped default meaning only while no source
# operation has replaced the character table or escape grammar before it.
# String/register definitions do not themselves alter a direct `\\[base …]`
# spelling, so keeping this list narrow avoids throwing away valid evidence for
# documentation that merely teaches those ordinary requests.
_GROFF_CHARACTER_MUTATORS = {
    "cc", "c2", "char", "composite", "ec", "fchar", "mso", "so", "soquiet", "tr",
}


def _literal_groff_named_glyph_words(source: str) -> Counter[tuple[str, str]] | None:
    """Return literal groff-only named-glyph words, or decline execution.

    The groff spellings listed above are not present in CVS ``chars.c`` (so
    its terminal formatter drops them), while groff accepts them and ManT
    deliberately preserves their authored Unicode. A source inventory is
    useful only when no request or escape can mutate the text stream, and only
    when both the fallback and decoded spellings are complete whole-word
    inventories. This prevents an unrelated literal fallback word from
    borrowing the source proof.
    """
    spellings: Counter[tuple[str, str]] = Counter()
    for raw in source.splitlines():
        if raw.endswith("\\") or _DYNAMIC_ESCAPE.search(raw):
            return None
        if len(raw) >= 3 and raw[0] in ".'" and raw[1] == "\\" and raw[2] == '"':
            continue
        request = _MDOC_REQUEST.fullmatch(raw)
        if request is not None and request[1].islower() and request[1] in _AUDIT_DYNAMIC_REQUESTS:
            return None
        for match in _GROFF_NAMED_GLYPH_IN_WORD.finditer(raw):
            name = match["compact_name"] or match["bracket_name"]
            glyph = _GROFF_NAMED_GLYPHS[name]
            fallback = match["prefix"] + match["suffix"]
            decoded = match["prefix"] + glyph + match["suffix"]
            # The content comparator deliberately ignores short punctuation
            # fragments.  Only complete comparable words can justify a
            # source-bound renderer compatibility projection.
            if len(fallback) >= 3 and len(decoded) >= 3:
                spellings[(fallback, decoded)] += 1
    return spellings or None


def _literal_groff_default_composite_words(source: str) -> Counter[tuple[str, str]] | None:
    """Return direct default-composite words before character-table mutation.

    groff's shipped ``composite.tmac`` defines the documented accent names;
    CVS mandoc intentionally has no dynamic equivalent and drops the complete
    composite escape in terminal text. This scanner recognizes only the same
    static mapping retained by the codec. It stops permanently after a source
    request that can replace character, escape, or translation semantics, and
    exact reference/Mant inventories remain mandatory at the call site.
    """
    spellings: Counter[tuple[str, str]] = Counter()
    character_state_is_default = True
    for raw in source.splitlines():
        if len(raw) >= 3 and raw[0] in ".'" and raw[1] == "\\" and raw[2] == '"':
            continue
        request = _MDOC_REQUEST.fullmatch(raw)
        if request is not None and request[1].lower() in _GROFF_CHARACTER_MUTATORS:
            character_state_is_default = False
        if not character_state_is_default or raw.endswith("\\"):
            continue
        for match in _GROFF_COMPOSITE_IN_WORD.finditer(raw):
            parts = match["name"].split()
            if len(parts) < 2 or len(parts[0]) != 1:
                continue
            accents = []
            for accent in parts[1:]:
                combining = _GROFF_DEFAULT_COMPOSITE_ACCENTS.get(accent)
                if combining is None:
                    accents = []
                    break
                accents.append(combining)
            if not accents:
                continue
            fallback = match["prefix"] + match["suffix"]
            decoded = match["prefix"] + parts[0] + "".join(accents) + match["suffix"]
            # The content comparator only gives a word-level comparison
            # meaning to complete words. Do not let punctuation fragments or
            # an empty base borrow a composite proof.
            if len(fallback) >= 3 and len(decoded) >= 3:
                spellings[(fallback, decoded)] += 1
    return spellings or None
