//! Named-glyph spelling and explicit Unicode sequences; no decoder cursor state.
pub(super) fn unicode_special_characters(name: &str) -> Option<String> {
    let encoded = name.strip_prefix('u')?;
    let mut output = String::new();
    for component in encoded.split('_') {
        if !(4..=6).contains(&component.len())
            || !component
                .chars()
                .all(|character| character.is_ascii_hexdigit())
        {
            return None;
        }
        output.push(char::from_u32(u32::from_str_radix(component, 16).ok()?)?);
    }
    (!output.is_empty()).then_some(output)
}

/// Narrow compatibility fallback for a documented groff spelling absent from
/// the pinned mandoc character table. All characters that the pinned table
/// knows, including typographic quotation marks, must retain that table's
/// exact Unicode scalar instead of being folded for presentation convenience.
pub(super) fn dedicated_special_character(name: &str) -> Option<&'static str> {
    match name {
        // NetBSD's DRM manuals use this long-standing groff-style spelling
        // for a lower-case c with caron.  It is absent from the pinned mandoc
        // table, so preserve the authored character rather than leaking the
        // raw `\[vc]` escape or silently dropping it.
        // The pinned mandoc table omits these documented groff caron
        // spellings.  groff_char(7) specifies vS, vs, vZ, and vz; generated
        // Slovene manuals also use the compact two-character form.  The vc
        // spelling is a historical c-with-caron form in NetBSD DRM manuals.
        // Preserve the authored Unicode instead of leaking formatter source.
        "vC" => Some("Č"),
        "vc" => Some("č"),
        "vS" => Some("Š"),
        "vs" => Some("š"),
        "vZ" => Some("Ž"),
        "vz" => Some("ž"),
        _ => None,
    }
}
