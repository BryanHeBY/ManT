//! Lowers man and mdoc list and definition structures.

use libmandoc_rs::{DefinitionListStyle, Node, NodeKind, NormalizedListKind};
use mant_ir::{
    Block, DefinitionItem, Inline, ListItem, ListKind, TableAlignment as AstTableAlignment,
    TableCell as AstTableCell, TableRow,
};

#[cfg(test)]
use super::super::inline::terms_fit_inline;
use super::super::{
    LoweringContext, first_part_children,
    inline::InlineBuilder,
    layout::{block_indent, layout, layout_with_spacing, paragraph_distance_lines},
    part_child_groups, source_span, targets,
};
use super::{is_inline_equation, is_inline_equation_quote_artifact, lower_blocks_with_predecessor};

mod definition;
mod evidence;
pub(super) mod man;
mod mdoc;
#[cfg(test)]
use definition::split_definition_terms;
use definition::{
    DefinitionFlow, DefinitionHeadFlow, PendingDefinitionItem, RunInHeadStyle, definition_item,
    prepend_definition_heads,
};
use man::ordered::{
    ManListState, append_pending_ordered, ordinal_marker_text, pending_ordinal_sequence,
};
pub(super) use man::{ManDefinitionState, lower_man_definition};

pub(super) use mdoc::lower_mdoc_list;

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint};

    fn text(value: &str) -> Vec<Inline> {
        vec![crate::test_content::text(value.to_owned())]
    }

    fn draft(value: &str) -> Vec<crate::mandoc::inline::DraftInline> {
        vec![crate::mandoc::inline::DraftInline::Text {
            value: value.to_owned(),
        }]
    }

    fn definition(term: &str, description: &str) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry: None,
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
                ..Default::default()
            },
            terms: vec![text(term)],
            description: vec![Block::Paragraph {
                children: text(description),
                layout: LayoutHint::default(),
                source: None,
            }],
        }
    }

    #[test]
    fn prepending_heads_preserves_unknown_sources_without_inventing_a_range() {
        let later_source = Some(mant_ir::SourceSpan {
            source: mant_ir::SourceKey::FIRST,
            byte_range: None,
            line: 9,
            column: 1,
            end_line: None,
            end_column: None,
        });
        let mut item = definition("--all", "body");
        item.source = later_source;
        super::prepend_definition_heads(&mut item, std::iter::empty());
        assert_eq!(item.source, later_source);
        super::prepend_definition_heads(&mut item, std::iter::once(definition("-a", "")));
        assert_eq!(item.source, None);
        assert_eq!(item.terms.len(), 2);
    }

    #[test]
    fn short_terms_hang_inline_but_long_ones_do_not() {
        // Matches man(1): a tag that fits the default hanging indent shares the
        // first description line; wider tags take their own line.
        assert!(super::terms_fit_inline(&[draft("space")], 6));
        assert!(super::terms_fit_inline(&[draft("* / %")], 6));
        assert!(!super::terms_fit_inline(
            &[draft("--listed-incremental")],
            6
        ));
        assert!(!super::terms_fit_inline(&[], 6));
    }

    #[test]
    fn extended_definition_terms_split_only_at_semantic_line_breaks() {
        let terms = super::split_definition_terms(vec![
            crate::mandoc::inline::DraftInline::Text {
                value: "first".to_owned(),
            },
            crate::mandoc::inline::DraftInline::LineBreak,
            crate::mandoc::inline::DraftInline::Strong {
                children: draft("second"),
            },
        ]);

        assert_eq!(
            terms,
            [
                draft("first"),
                vec![crate::mandoc::inline::DraftInline::Strong {
                    children: draft("second")
                }]
            ]
        );
    }
}
