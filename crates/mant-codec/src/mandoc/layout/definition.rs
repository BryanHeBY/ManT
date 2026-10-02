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
    /// `rmargin = offset + width`). Zero may be a measured empty capacity;
    /// `native_head_field_units` distinguishes it from an unmeasured style
    /// (inset/diagnostic/overhang keep the page right margin).
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
        let mut head_body_relation = self.relation_override.unwrap_or(match self.placement {
            TermPlacement::Fit
                if !mant_ir::terms_fit_inline(
                    terms,
                    usize::try_from(body_indent_columns.saturating_sub(i32::from(self.gap)))
                        .unwrap_or(0),
                ) =>
            {
                HeadBodyRelation::Separate
            }
            TermPlacement::Fit | TermPlacement::RunIn => HeadBodyRelation::from(true),
            TermPlacement::Stacked => HeadBodyRelation::Separate,
        });
        // term_flushln() places a following HANG field after the last printed
        // HEAD row. Resolve this preference once, instead of making each
        // renderer infer it from historical rows. Leading empty rows alone
        // do not consume that origin (term.c:134-136,205-207,235-253).
        if head_body_relation == HeadBodyRelation::from(true)
            && final_head_has_completed_content_row(terms)
        {
            head_body_relation =
                HeadBodyRelation::separated(mant_ir::DefinitionBodyAlignment::AfterTerm);
        }
        DefinitionLayout {
            head_body_relation,
            body_indent_columns,
            min_term_gap_columns: self.gap,
            spacing_before_lines: None,
        }
    }
}

fn final_head_has_completed_content_row(terms: &[Vec<Inline>]) -> bool {
    for term in terms.iter().rev() {
        let mut rows = HeadRows::default();
        rows.observe(term);
        if rows.present || rows.current_cells {
            return rows.completed_cells;
        }
    }
    false
}

#[derive(Default)]
struct HeadRows {
    present: bool,
    current_cells: bool,
    completed_cells: bool,
}

impl HeadRows {
    fn end_row(&mut self) {
        self.present = true;
        self.completed_cells |= self.current_cells;
        self.current_cells = false;
    }

    fn observe(&mut self, nodes: &[Inline]) {
        for node in nodes {
            match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    for character in value.chars() {
                        if character == '\n' {
                            self.end_row();
                        } else {
                            self.current_cells = true;
                        }
                    }
                }
                Inline::LineBreak { .. } => self.end_row(),
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => self.observe(children),
                Inline::Anchor { .. } => {}
            }
        }
    }
}
