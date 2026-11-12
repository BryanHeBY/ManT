"""Source-consistent display projections; raw evidence stays authoritative."""
from __future__ import annotations
from collections import Counter
from .equations import (
    _literal_eqn_relation_spellings,
)
from .glyphs import (
    _literal_groff_default_composite_words,
    _literal_groff_named_glyph_words,
)
from .lists_links import (
    _BR_MANUAL_REFERENCE,
    _BULLET_ESCAPE,
    _MDOC_LITERAL_COLUMN_SEPARATOR,
    _literal_man_external_targets,
    _literal_man_ip_ordinals,
    _literal_mdoc_bullet_item_count,
    _literal_mdoc_column_separator_count,
)

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
    # Count equality proves that every displayed reference bullet came from
    # the literal inventory.  Also require ManT to have enough portable list
    # markers for the replacement to reduce occurrence residue rather than
    # manufacture a new unmatched hyphen.  A page with conditional or omitted
    # source items must stay reviewable.
    bullet_residue_before = (
        abs(reference_bullets - projected_mant.count("•"))
        + abs(projected_reference.count("-") - projected_mant.count("-"))
    )
    bullet_residue_after = (
        abs(projected_mant.count("•"))
        + abs(projected_reference.count("-") + reference_bullets - projected_mant.count("-"))
    )
    if (bullet_count and reference_bullets == bullet_count
            and bullet_residue_after < bullet_residue_before):
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
    replacements = 0
    # A partial .BR inventory is not evidence for any member of that family:
    # conditionals or table-specific execution can suppress an otherwise
    # literal call.  Keep all manual-reference cells raw unless every source
    # declaration has an exact terminal and compact counterpart.
    if authored and all(
            projected_reference.count(f"{pair[0]} ({pair[1]})") == limit
            and projected_mant.count(f"{pair[0]}({pair[1]})") >= limit
            for pair, limit in authored.items()
    ):
        for pair, limit in authored.items():
            terminal = f"{pair[0]} ({pair[1]})"
            compact = f"{pair[0]}({pair[1]})"
            projected_reference, count = _replace_limited(
                projected_reference, terminal, compact, limit
            )
            replacements += count
    if replacements:
        evidence.append({
            "rule": "source-consistent-BR-manual-reference-spacing/v1",
            "sourceReferences": replacements,
            "sourceCandidates": sum(authored.values()),
            "referenceReferences": replacements,
            "reason": "Only literal .BR name (section) cells whose complete source, CVS, and ManT inventories agree are consistent with CVS terminal spacing and ManT's atomic manual-reference presentation.",
        })

    external_targets = _literal_man_external_targets(source)
    # Apply the same all-or-nothing rule to URI/mail heads.  A single hidden
    # or malformed block must not let a different explicit head borrow its
    # source proof.
    if external_targets is not None and all(
            projected_reference.count(f"<{target}>") == limit
            and max(projected_mant.count(f"⟨{target}⟩"), projected_mant.count(target)) >= limit
            for target, limit in external_targets.items()
    ):
        reference_replacements = 0
        mant_replacements = 0
        for target, limit in external_targets.items():
            reference_target = f"<{target}>"
            mant_target = f"⟨{target}⟩"
            mant_bracketed = projected_mant.count(mant_target)
            mant_plain = projected_mant.count(target)
            # All literal heads have already passed the complete-inventory
            # guard.  ManT's text projection may retain a typed target bare
            # or put it in compact angle brackets, so rewrite only the latter.
            projected_reference, count = _replace_limited(
                projected_reference, reference_target, target, limit
            )
            reference_replacements += count
            if mant_bracketed:
                projected_mant, count = _replace_limited(
                    projected_mant, mant_target, target, limit
                )
                mant_replacements += count
        if reference_replacements:
            evidence.append({
                "rule": "source-consistent-man-external-target-delimiters/v2",
                "sourceTargets": reference_replacements,
                "sourceCandidates": sum(external_targets.values()),
                "referenceDelimiters": reference_replacements,
                "mantDelimiters": mant_replacements,
                "reason": "Only literal GNU man-ext .UR/.MT targets whose complete source, CVS, and ManT inventories agree are consistent with CVS man_term.c:post_UR bracket delimiters and ManT's compact link presentation.",
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

    named_glyph_words = _literal_groff_named_glyph_words(source)
    # CVS mandoc's character table intentionally omits these documented groff
    # caron spellings, while groff and ManT preserve their authored Unicode.
    # Exact inventories on all three sides make this a reference-compatibility
    # distinction rather than an excuse for arbitrary Unicode substitutions.
    if named_glyph_words is not None and all(
            projected_reference.count(fallback) == limit
            and projected_mant.count(decoded) == limit
            for (fallback, decoded), limit in named_glyph_words.items()
    ):
        replacements = 0
        for (fallback, decoded), limit in named_glyph_words.items():
            projected_reference, count = _replace_limited(
                projected_reference, fallback, decoded, limit
            )
            replacements += count
        if replacements:
            evidence.append({
                "rule": "source-consistent-groff-named-character/v1",
                "sourceSpellings": replacements,
                "sourceCandidates": sum(named_glyph_words.values()),
                "referenceSpellings": sorted({fallback for fallback, _ in named_glyph_words}),
                "mantSpellings": sorted({decoded for _, decoded in named_glyph_words}),
                "reason": "Literal groff-only named characters whose complete fallback and decoded word inventories agree are preserved by ManT even when the selected terminal reference omits its unsupported glyph.",
            })

    composite_words = _literal_groff_default_composite_words(source)
    # CVS chars.c has no dynamic composite table, whereas groff's documented
    # default composite.tmac mapping and ManT both retain the base plus
    # combining accents. Require exact inventories so this one compatibility
    # lens cannot erase unrelated text that merely resembles a fallback word.
    if composite_words is not None and all(
            projected_reference.count(fallback) == limit
            and projected_mant.count(decoded) == limit
            for (fallback, decoded), limit in composite_words.items()
    ):
        replacements = 0
        for (fallback, decoded), limit in composite_words.items():
            projected_reference, count = _replace_limited(
                projected_reference, fallback, decoded, limit
            )
            replacements += count
        if replacements:
            evidence.append({
                "rule": "source-consistent-groff-default-composite/v1",
                "sourceSpellings": replacements,
                "sourceCandidates": sum(composite_words.values()),
                "referenceSpellings": sorted({fallback for fallback, _ in composite_words}),
                "mantSpellings": sorted({decoded for _, decoded in composite_words}),
                "reason": "Literal documented groff default composites whose complete fallback and decoded word inventories agree are preserved by ManT even though CVS mandoc does not implement its dynamic composite character table.",
            })

    eqn_relations = _literal_eqn_relation_spellings(source)
    # CVS eqn_term.c writes positional operators under TERMP_NOSPACE while
    # ManT deliberately exposes the same parsed relations in a copyable
    # textual form. Demand full inventories on both sides so an unrelated
    # identifier cannot borrow a proof from a literal equation context.
    if eqn_relations is not None:
        replacements = 0
        confirmed_relations: Counter[tuple[str, str]] = Counter()
        for (reference_spelling, mant_spelling), limit in eqn_relations.items():
            # A configured inline delimiter can occur inside a tbl cell that
            # libmandoc deliberately retains as literal table text. Prove
            # each spelling independently: an unexecuted candidate must not
            # suppress exact source/reference/product evidence for a separate
            # executed relation, and an inexact spelling remains untouched.
            if (
                projected_reference.count(reference_spelling) != limit
                or projected_mant.count(mant_spelling) != limit
            ):
                continue
            projected_reference, count = _replace_limited(
                projected_reference, reference_spelling, mant_spelling, limit
            )
            replacements += count
            if count == limit:
                confirmed_relations[(reference_spelling, mant_spelling)] = limit
        if replacements:
            evidence.append({
                "rule": "source-consistent-eqn-linear-relations/v1",
                "sourceSpellings": replacements,
                "sourceCandidates": sum(eqn_relations.values()),
                "referenceSpellings": sorted({pair[0] for pair in confirmed_relations}),
                "mantSpellings": sorted({pair[1] for pair in confirmed_relations}),
                "reason": "Each reported literal block or configured-inline eqn relation has an exact source/reference/product inventory: CVS eqn_term.c attaches positional markers while ManT retains explicit copyable spacing. Unreported candidates remain in the raw review.",
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
