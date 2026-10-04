//! Bound optional producer layout independently of authoritative body content.

use mant_ir::{
    Block, DefinitionItem, Document, Heading, Inline, InlineLayout,
    visit::{self, VisitMut},
};

const DOCUMENT_HINT_BUDGET: usize = 65_536;

/// Layout overflow omits only exceptional positions, never accepted glyphs.
/// The producer must remain readable by the same strict v0.12 wire consumer.
pub(crate) fn bound_layout_hints(document: &mut Document) {
    struct Budget {
        remaining: usize,
        omitted: usize,
    }
    impl Budget {
        fn owner(&mut self, content: &mut Vec<Inline>, layout: &mut InlineLayout) {
            {
                let extracted = crate::mandoc::inline::take_inline_layout(content);
                if !extracted.is_empty() {
                    let mut hints: std::collections::BTreeMap<_, _> = layout
                        .row_hints
                        .iter()
                        .map(|hint| (hint.row, hint.indent_columns))
                        .collect();
                    hints.extend(
                        extracted
                            .row_hints
                            .into_iter()
                            .map(|hint| (hint.row, hint.indent_columns)),
                    );
                    layout.row_hints = hints
                        .into_iter()
                        .map(|(row, indent_columns)| mant_ir::RowLayoutHint {
                            row,
                            indent_columns,
                        })
                        .collect();
                }
            }
            layout.row_hints.retain(|hint| hint.indent_columns != 0);
            let accepted = layout
                .row_hints
                .len()
                .min(mant_ir::MAX_INLINE_ROW_HINTS)
                .min(self.remaining);
            self.omitted = self
                .omitted
                .saturating_add(layout.row_hints.len() - accepted);
            self.remaining -= accepted;
            layout.row_hints.truncate(accepted);
        }
    }
    impl VisitMut for Budget {
        fn visit_heading_mut(&mut self, heading: &mut Heading) {
            self.owner(&mut heading.content, &mut heading.inline_layout);
        }
        fn visit_inline_mut(&mut self, _: &mut Inline) {}
        fn visit_block_mut(&mut self, block: &mut Block) {
            if let Block::Paragraph {
                children,
                inline_layout,
                ..
            }
            | Block::Preformatted {
                children,
                inline_layout,
                ..
            } = block
            {
                self.owner(children, inline_layout);
            }
            visit::walk_block_mut(self, block);
        }
        fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
            for term in &mut item.terms {
                self.owner(&mut term.content, &mut term.inline_layout);
            }
            visit::walk_definition_item_mut(self, item);
        }
    }
    let mut budget = Budget {
        remaining: DOCUMENT_HINT_BUDGET,
        omitted: 0,
    };
    budget.visit_document_mut(document);
    if budget.omitted != 0 {
        document.diagnostics.push(mant_ir::Diagnostic {
            impact: mant_ir::DiagnosticImpact::None,
            level: mant_ir::DiagnosticLevel::Warning,
            code: Some("layout.row-hint-budget".to_owned()),
            message: format!("omitted {} exceptional row layout hints at the layout budget; document text is preserved", budget.omitted),
            source: None,
        });
    }
}

#[cfg(test)]
#[path = "owner_layout/tests.rs"]
mod tests;
