//! mdoc width samples and named display-offset policy.
use libmandoc_rs::Node;
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

use super::{Distance, mdoc_distance};

impl crate::mandoc::LoweringContext<'_> {
    pub(in crate::mandoc) fn measured_mdoc_distance(
        &self,
        node: &Node,
        text: &str,
        fallback: Distance,
    ) -> Distance {
        mdoc_distance(text).unwrap_or_else(|| self.distance_or(node, text, fallback))
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
        Some(offset) => mdoc_distance(offset),
    }
}
