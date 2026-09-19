//! Resolve source list styles into portable label/body geometry.
use super::{Distance, SourceIndent};
use crate::mandoc::LoweringContext;
use libmandoc_rs::Node;
use mant_ir::{DefinitionLayout, DefinitionPlacement};

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct DefinitionGeometry {
    pub(in crate::mandoc) body: Distance,
    pub(in crate::mandoc) placement: DefinitionPlacement,
    pub(in crate::mandoc) gap: u16,
}

impl DefinitionGeometry {
    pub(in crate::mandoc) fn body_columns(self) -> u16 {
        u16::try_from(self.body.position_columns().max(0)).unwrap_or(u16::MAX)
    }

    pub(in crate::mandoc) fn resolve(
        self,
        context: &LoweringContext<'_>,
        node: &Node,
        origin: SourceIndent,
    ) -> (DefinitionLayout, SourceIndent) {
        let body_origin = context.offset_indent(node, origin, self.body);
        let body_indent_columns = body_origin.offset_from(origin);
        (
            DefinitionLayout {
                placement: self.placement,
                body_indent_columns,
                min_term_gap_columns: self.gap,
                term_continuation_indent_columns: 0,
                fit_constraint: None,
                spacing_before_lines: None,
            },
            body_origin.content_origin(),
        )
    }
}
