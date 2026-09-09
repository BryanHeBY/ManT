//! Resolve source list styles into portable label/body geometry.
use super::{Distance, SourceIndent};
use crate::mandoc::LoweringContext;
use libmandoc_rs::Node;
use mant_ir::{DefinitionLayout, Inline};

#[derive(Clone, Copy)]
pub(in crate::mandoc) enum TermPlacement {
    Fit,
    RunIn,
    Stacked,
}

#[derive(Clone, Copy)]
pub(in crate::mandoc) struct DefinitionGeometry {
    pub(in crate::mandoc) body: Distance,
    pub(in crate::mandoc) placement: TermPlacement,
    pub(in crate::mandoc) gap: u16,
}

impl DefinitionGeometry {
    pub(in crate::mandoc) fn resolve(
        self,
        context: &LoweringContext<'_>,
        node: &Node,
        origin: SourceIndent,
        terms: &[Vec<Inline>],
    ) -> (DefinitionLayout, SourceIndent) {
        let body_origin = context.offset_indent(node, origin, self.body);
        let body_indent_columns = body_origin.offset_from(origin);
        let inline_term = match self.placement {
            TermPlacement::Fit => mant_ir::terms_fit_inline(
                terms,
                usize::try_from(body_indent_columns.saturating_sub(i32::from(self.gap)))
                    .unwrap_or(0),
            ),
            TermPlacement::RunIn => true,
            TermPlacement::Stacked => false,
        };
        (
            DefinitionLayout {
                inline_term,
                body_indent_columns,
                min_term_gap_columns: self.gap,
                spacing_before_lines: None,
            },
            body_origin.content_origin(),
        )
    }
}
