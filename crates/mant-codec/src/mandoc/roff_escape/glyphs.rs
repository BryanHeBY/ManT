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

/// Compatibility folds intentionally chosen by `ManT`. Every other known name
/// comes from the complete catalog pinned by `libmandoc-rs`.
pub(super) fn dedicated_special_character(name: &str) -> Option<&'static str> {
    match name {
        "en" => Some("–"),
        "em" => Some("—"),
        "aq" | "cq" | "oq" => Some("'"),
        "dq" | "lq" | "rq" => Some("\""),
        "co" => Some("©"),
        "rg" => Some("®"),
        "tm" => Some("™"),
        "bu" => Some("•"),
        "ha" => Some("^"),
        "ti" => Some("~"),
        "rs" => Some("\\"),
        // NetBSD's DRM manuals use this long-standing groff-style spelling
        // for a lower-case c with caron.  It is absent from libmandoc
        // 1.14.6's fixed character table, so retain the authored name rather
        // than leaking the raw `\[vc]` escape or silently dropping it like
        // terminal formatters that do not provide the device glyph.
        "vc" => Some("č"),
        _ => None,
    }
}
