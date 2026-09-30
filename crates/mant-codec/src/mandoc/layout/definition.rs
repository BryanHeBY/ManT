//! Resolve source list styles into portable label/body geometry.
use super::{Distance, SourceIndent};
use crate::mandoc::LoweringContext;
use libmandoc_rs::Node;
use mant_ir::{DefinitionLayout, HeadBodyRelation, Inline};

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
    /// The head field's content capacity: the resolved list width distance
    /// before it is composed with the list offset (mdoc_term.c:846-856:
    /// `rmargin = offset + width`). Zero means the style sets no field
    /// width (inset/diagnostic/overhang keep the page right margin).
    pub(in crate::mandoc) head_field_columns: u16,
    /// Exact native HEAD capacity before character-column rounding. The
    /// character device keeps `a2width() + term_len(p, 2)` in basic units
    /// (mdoc_term.c:735-747,846-856); `term_flushln()`'s half-EN tolerance
    /// must compare against that distance (term.c:250-253). `None` keeps
    /// the existing device margin for styles without a measured HEAD.
    pub(in crate::mandoc) native_head_field_units: Option<usize>,
    /// Execution-proven head/body row relation overriding the static
    /// placement decision (`.nf`/`.fi` NOSPACE joins and filled cleared
    /// fields). `None` keeps the placement-derived relation.
    pub(in crate::mandoc) relation_override: Option<mant_ir::HeadBodyRelation>,
}

impl DefinitionGeometry {
    pub(in crate::mandoc) fn body_columns(self) -> u16 {
        u16::try_from(self.body.position_columns().max(0)).unwrap_or(u16::MAX)
    }

    pub(in crate::mandoc) fn body_origin(
        self,
        context: &LoweringContext<'_>,
        node: &Node,
        origin: SourceIndent,
    ) -> SourceIndent {
        context
            .offset_indent(node, origin, self.body)
            .content_origin()
    }

    pub(in crate::mandoc) fn layout(
        self,
        origin: SourceIndent,
        body_origin: SourceIndent,
        terms: &[Vec<Inline>],
    ) -> DefinitionLayout {
        let body_indent_columns = body_origin.offset_from(origin);
        let head_body_relation = self.relation_override.unwrap_or(match self.placement {
            TermPlacement::Fit
                if !mant_ir::terms_fit_inline(
                    terms,
                    usize::try_from(body_indent_columns.saturating_sub(i32::from(self.gap)))
                        .unwrap_or(0),
                ) =>
            {
                HeadBodyRelation::Separate
            }
            TermPlacement::Fit | TermPlacement::RunIn => HeadBodyRelation::RunIn,
            TermPlacement::Stacked => HeadBodyRelation::Separate,
        });
        DefinitionLayout {
            head_body_relation,
            body_indent_columns,
            min_term_gap_columns: self.gap,
            spacing_before_lines: None,
        }
    }
}
