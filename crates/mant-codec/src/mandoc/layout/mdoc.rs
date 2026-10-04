//! mdoc width samples and named display-offset policy.
use libmandoc_rs::Node;
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

use super::Distance;

impl crate::mandoc::LoweringContext<'_> {
    pub(in crate::mandoc) fn measured_mdoc_distance(
        &self,
        node: &Node,
        text: &str,
        fallback: Distance,
    ) -> Distance {
        measured_distance(text).unwrap_or_else(|| self.distance_or(node, text, fallback))
    }

    pub(in crate::mandoc) fn display_offset(&self, node: &Node) -> Distance {
        if matches!(
            node.macro_token.as_ref(),
            Some(Mdoc(MdocMacro::D1 | MdocMacro::Dl))
        ) {
            return Distance::cells(6);
        }
        match node.offset.as_deref() {
            None | Some("left") => Distance::default(),
            Some("indent") => Distance::cells(6),
            Some("indent-two") => Distance::cells(12),
            Some(offset) => self.measured_mdoc_distance(node, offset, Distance::default()),
        }
    }
}

/// The normalized display offset is interpreted once for both native BODY
/// geometry and public layout. Sampling its width executes no formatter word.
pub(in crate::mandoc) fn display_offset_distance(node: &Node) -> Option<Distance> {
    match node.offset.as_deref() {
        None | Some("left") => Some(Distance::default()),
        Some("indent") => Some(Distance::cells(6)),
        Some("indent-two") => Some(Distance::cells(12)),
        Some(offset) => measured_distance(offset),
    }
}

fn measured_distance(text: &str) -> Option<Distance> {
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
