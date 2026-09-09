//! mdoc width samples and named display-offset policy.
use libmandoc_rs::Node;

use super::Distance;

impl crate::mandoc::LoweringContext<'_> {
    pub(in crate::mandoc) fn measured_mdoc_distance(
        &self,
        node: &Node,
        text: &str,
        fallback: Distance,
    ) -> Distance {
        // Unlike man distances, mdoc requires a unit; a bare number is a
        // printable width sample. Do not mistake digit-leading samples for
        // malformed numeric distances.
        if let Some((number, unit)) = text
            .trim()
            .split_at_checked(text.trim().len().saturating_sub(1))
            && matches!(
                unit,
                "n" | "m" | "u" | "c" | "f" | "i" | "M" | "P" | "v" | "p"
            )
            && number.parse::<f64>().is_ok()
        {
            return self.distance_or(node, text, fallback);
        }
        let visible =
            crate::mandoc::inline::plain_text(&crate::mandoc::inline::parse_roff_text(text));
        Distance::cells(mant_ir::geometry::coordinate(
            mant_ir::geometry::text_width(&visible),
        ))
    }

    pub(in crate::mandoc) fn display_offset(&self, node: &Node) -> Distance {
        if matches!(node.macro_name.as_deref(), Some("D1" | "Dl")) {
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
