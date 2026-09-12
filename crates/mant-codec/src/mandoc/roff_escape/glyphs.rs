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

/// Decode the documented default mappings from groff's `composite.tmac`.
///
/// `groff_char(7)` defines `\\[base accent ...]` as a composite glyph and its
/// shipped `composite.tmac` maps the accent names below to Unicode combining
/// scalars.  mandoc deliberately has no equivalent dynamic character table,
/// so those escapes remain in native text nodes.  Keep this decoder limited to
/// groff's built-in mappings: user-defined `composite` requests must remain
/// visible source rather than being guessed at this boundary.
pub(super) fn documented_groff_composite_character(name: &str) -> Option<String> {
    let mut components = name.split_ascii_whitespace();
    let base = components.next()?;
    let base = (base.chars().count() == 1).then_some(base)?;
    let mut output = base.to_owned();
    let mut has_accent = false;

    for accent in components {
        let combining = match accent {
            "ga" | "`" => '\u{0300}',
            "aa" | "'" => '\u{0301}',
            "a^" | "^" => '\u{0302}',
            "a~" | "~" => '\u{0303}',
            "a-" | "-" => '\u{0304}',
            "ab" => '\u{0306}',
            "a." | "." => '\u{0307}',
            "ad" | ":" => '\u{0308}',
            "ao" => '\u{030A}',
            "a\"" | "\"" => '\u{030B}',
            "ah" => '\u{030C}',
            "ac" | "," => '\u{0327}',
            "ho" => '\u{0328}',
            _ => return None,
        };
        output.push(combining);
        has_accent = true;
    }

    has_accent.then_some(output)
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
