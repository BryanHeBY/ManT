//! Lowers man and mdoc list and definition structures.

use libmandoc_rs::{DefinitionListStyle, Node, NodeKind, NormalizedListKind};
use mant_ir::{
    Block, DefinitionItem, Inline, ListItem, ListKind, TableAlignment as AstTableAlignment,
    TableCell as AstTableCell, TableRow,
};

use super::super::{
    LoweringContext, first_part_children,
    inline::{InlineBuilder, plain_text, terms_fit_inline},
    layout::{
        block_indent, horizontal_distance_columns, layout, layout_with_spacing,
        paragraph_distance_lines,
    },
    part_child_groups, source_span, targets,
};
use super::{is_inline_equation, is_inline_equation_quote_artifact, lower_blocks_with_spacing};
use crate::block::block_layout_mut;

mod definition;
pub(super) mod man;
mod mdoc;
#[cfg(test)]
use definition::split_definition_terms;
use definition::{definition_item, prepend_definition_heads};
#[cfg(test)]
use man::is_ip_bullet_item;
use man::ordered::{
    DefinitionLocation, MAN_DEFINITION_BODY_INDENT, ManListState, append_ordered,
    list_item_from_definition, ordinal_marker, ordinal_sequence,
};
pub(super) use man::{ManDefinitionState, lower_man_definition};

pub(super) use mdoc::lower_mdoc_list;

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint};

    fn text(value: &str) -> Vec<Inline> {
        vec![Inline::Text {
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
    fn only_single_glyph_definition_terms_are_ip_bullets() {
        assert!(super::is_ip_bullet_item(&definition("*", "multiply")));
        assert!(super::is_ip_bullet_item(&definition("o", "item")));
        assert!(!super::is_ip_bullet_item(&definition("&&", "logical and")));
        assert!(!super::is_ip_bullet_item(&definition(
            "-a, --all",
            "show all"
        )));
    }

    #[test]
    fn short_terms_hang_inline_but_long_ones_do_not() {
        // Matches man(1): a tag that fits the default hanging indent shares the
        // first description line; wider tags take their own line.
        assert!(super::terms_fit_inline(&[text("space")], 6));
        assert!(super::terms_fit_inline(&[text("* / %")], 6));
        assert!(!super::terms_fit_inline(&[text("--listed-incremental")], 6));
        assert!(!super::terms_fit_inline(&[], 6));
    }

    #[test]
    fn extended_definition_terms_split_only_at_semantic_line_breaks() {
        let terms = super::split_definition_terms(vec![
            Inline::Text {
                value: "first".to_owned(),
            },
            Inline::LineBreak,
            Inline::Strong {
                children: text("second"),
            },
        ]);

        assert_eq!(
            terms,
            [
                text("first"),
                vec![Inline::Strong {
                    children: text("second")
                }]
            ]
        );
    }
}
