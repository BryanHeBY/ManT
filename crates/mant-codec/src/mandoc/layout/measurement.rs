//! Literal mdoc units and the existing reading projection of width samples.

use super::Distance;

/// Interpret a unit-qualified literal, otherwise measure its reading sample.
/// This pure helper does not execute the sample in the live formatter. Its
/// reading-text approximation is distinct from `roff_escape::width_sample()`,
/// which implements the native `term_strlen()` measurement contract.
pub(in crate::mandoc) fn mdoc_distance(text: &str) -> Option<Distance> {
    // mdoc requires a unit: a bare number is a printable width sample.
    if let Some((number, unit)) = text
        .trim()
        .split_at_checked(text.trim().len().saturating_sub(1))
        && matches!(
            unit,
            "n" | "m" | "u" | "c" | "f" | "i" | "M" | "P" | "v" | "p"
        )
        && number.parse::<f64>().is_ok()
    {
        return Distance::parse(text);
    }
    let visible = crate::mandoc::inline::plain_text(&crate::mandoc::inline::parse_roff_text(text));
    Some(Distance::cells(mant_ir::geometry::coordinate(
        mant_ir::geometry::text_width(&visible),
    )))
}
