//! Definition normalize policy; coordinated by the parent discovery passes.
use super::{
    context::DefinitionContext,
    syntax::{environment_names_from_terms, option_names_from_terms},
};
use crate::{
    block::{block_layout, block_layout_mut},
    inline::{DEFAULT_INLINE_TERM_MAX_WIDTH, plain_text, terms_fit_inline},
};
use mant_ir::{Block, DefinitionItem, LayoutHint};
use std::{collections::VecDeque, mem};

/// Reattach source-neutral indented continuations to their owning definition.
///
/// libmandoc can retain man(7) `.RS` continuations as later sibling blocks
/// whose absolute indentation is greater than the preceding definition list.
/// They remain visually correct in that flat form, but the topology loses the
/// command → parameter → value relationship needed by semantic navigation.
/// Move the run under the last definition and translate its layout to the
/// description's relative coordinate system so rendering is unchanged.
pub(super) fn normalize_definition_nesting(blocks: &mut Vec<Block>) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(mut block) = pending.pop_front() {
        let Some(base_indent) = block_definition_indent(&block) else {
            normalized.push(block);
            continue;
        };
        let Some(last_item) = last_definition_mut(&mut block) else {
            normalized.push(block);
            continue;
        };
        let description_origin =
            base_indent.saturating_add(DefinitionItem::DESCRIPTION_INDENT_COLUMNS);
        while let Some(length) = indented_continuation_len(&pending, base_indent) {
            for mut nested in pending.drain(..length) {
                shift_block_indent(&mut nested, description_origin);
                last_item.description.push(nested);
            }
        }
        normalized.push(block);
    }

    *blocks = normalized;
}

/// Whitespace has no indentation and cannot independently establish or end
/// ownership. Carry a run of it only when the next substantive block proves
/// an indented continuation. Do not cross other layout-less blocks, equal or
/// shallower content, or the end of this container. Consume a whole run at once
/// so even long sequences of explicit spacing are examined only once.
fn indented_continuation_len(pending: &VecDeque<Block>, base_indent: u16) -> Option<usize> {
    let (index, block) = pending
        .iter()
        .enumerate()
        .find(|(_, block)| !matches!(block, Block::VerticalSpace { .. }))?;
    block_layout(block)
        .is_some_and(|layout| layout.indent_columns > base_indent)
        .then_some(index + 1)
}

fn block_definition_indent(block: &Block) -> Option<u16> {
    match block {
        Block::DefinitionList { layout, .. } => Some(layout.indent_columns),
        _ => None,
    }
}

fn last_definition_mut(block: &mut Block) -> Option<&mut DefinitionItem> {
    match block {
        Block::DefinitionList { items, .. } => items.last_mut(),
        _ => None,
    }
}

/// Turn renderer-neutral hanging-indent runs into semantic definitions.
///
/// Some man(7) generators use `.PP` followed by `.RS` instead of `.TP` for
/// options and environment variables. Native parsers correctly retain that
/// layout, but neither representation is a definition list on its own.
/// Recognising the shared visible shape here keeps identity independent of
/// the source macro set or source parser used by the query pipeline.
pub(super) fn normalize_hanging_definitions(blocks: &mut Vec<Block>, context: DefinitionContext) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(block) = pending.pop_front() {
        let Some(term_indent) = hanging_term_indent(&block, context) else {
            normalized.push(block);
            continue;
        };

        let mut description = Vec::new();
        while let Some(next) = pending.front() {
            if hanging_term_indent(next, context) == Some(term_indent) {
                break;
            }
            if matches!(next, Block::VerticalSpace { .. }) && description.is_empty() {
                break;
            }
            let Some(length) = indented_continuation_len(&pending, term_indent) else {
                break;
            };
            description.extend(pending.drain(..length));
        }

        if description.is_empty() {
            normalized.push(block);
            continue;
        }

        let Block::Paragraph {
            children,
            layout,
            source,
        } = block
        else {
            unreachable!("option_term_indent only accepts paragraphs");
        };
        let description_origin =
            term_indent.saturating_add(DefinitionItem::DESCRIPTION_INDENT_COLUMNS);
        for child in &mut description {
            shift_block_indent(child, description_origin);
        }
        let terms = vec![children];
        normalized.push(Block::DefinitionList {
            items: vec![DefinitionItem {
                source: None,
                identity: None,
                inline_term: terms_fit_inline(&terms, DEFAULT_INLINE_TERM_MAX_WIDTH),
                terms,
                description,
                spacing_before_lines: Some(layout.spacing_before_lines),
            }],
            compact: true,
            layout: LayoutHint {
                indent_columns: term_indent,
                spacing_before_lines: 0,
            },
            source,
        });
    }

    *blocks = normalized;
}

fn hanging_term_indent(block: &Block, context: DefinitionContext) -> Option<u16> {
    let Block::Paragraph {
        children, layout, ..
    } = block
    else {
        return None;
    };
    let recognized = match context {
        DefinitionContext::EnvironmentVariables => {
            !environment_names_from_terms(std::slice::from_ref(children)).is_empty()
        }
        DefinitionContext::Generic | DefinitionContext::Parameters => {
            let text = plain_text(children);
            text.trim_start().starts_with('-')
                && !option_names_from_terms(std::slice::from_ref(children)).is_empty()
        }
        DefinitionContext::Commands
        | DefinitionContext::Variables
        | DefinitionContext::ConfigurationKeys
        | DefinitionContext::Values => false,
    };
    recognized.then_some(layout.indent_columns)
}

fn shift_block_indent(block: &mut Block, origin: u16) {
    if let Some(layout) = block_layout_mut(block) {
        layout.indent_columns = layout.indent_columns.saturating_sub(origin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::Inline;

    fn paragraph(text: &str, indent_columns: u16) -> Block {
        Block::Paragraph {
            children: vec![Inline::Text { value: text.into() }],
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines: 0,
            },
            source: None,
        }
    }

    fn definition(indent_columns: u16) -> Block {
        Block::DefinitionList {
            items: vec![DefinitionItem {
                source: None,
                identity: None,
                terms: vec![vec![Inline::Text {
                    value: "--owner".into(),
                }]],
                description: vec![paragraph("Initial description.", 4)],
                inline_term: false,
                spacing_before_lines: None,
            }],
            compact: false,
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines: 0,
            },
            source: None,
        }
    }

    fn space(lines: u16) -> Block {
        Block::VerticalSpace {
            lines,
            source: None,
        }
    }

    fn text(blocks: Vec<Block>) -> String {
        let mut query = crate::query_markdown_text("Body.\n", None).unwrap();
        query.document.as_mut().unwrap().blocks = blocks;
        crate::render_query_text(&query)
    }

    #[test]
    fn moving_spaced_continuations_preserves_text_geometry_and_blank_lines() {
        for base in [0, 7] {
            for inline_term in [false, true] {
                for label in ["-a", "--long-option", "界", "e\u{301}"] {
                    let mut owner = definition(base);
                    let Block::DefinitionList { items, .. } = &mut owner else {
                        unreachable!()
                    };
                    items[0].inline_term = inline_term;
                    items[0].terms = vec![vec![Inline::Text {
                        value: label.into(),
                    }]];
                    let mut blocks = vec![
                        owner,
                        space(1),
                        space(3),
                        paragraph("First continuation.", base + 4),
                        space(2),
                        definition(base + 8),
                        space(1),
                        paragraph("Last continuation.", base + 4),
                        space(2),
                        paragraph("Outside.", base),
                    ];
                    let before = text(blocks.clone());
                    let outside = blocks[8..].to_vec();
                    normalize_definition_nesting(&mut blocks);
                    assert_eq!(text(blocks.clone()), before, "base indent {base}");
                    assert_eq!(&blocks[1..], outside);
                    let Block::DefinitionList { items, .. } = &blocks[0] else {
                        unreachable!()
                    };
                    assert_eq!(items[0].description.len(), 8);
                    let once = blocks.clone();
                    normalize_definition_nesting(&mut blocks);
                    assert_eq!(blocks, once);
                }
            }
        }
    }

    #[test]
    fn whitespace_does_not_cross_an_outer_or_nonlayout_boundary() {
        for boundary in [
            vec![],
            vec![paragraph("Same level.", 4)],
            vec![paragraph("Outer level.", 0)],
            vec![
                Block::ThematicBreak { source: None },
                paragraph("Deep but unrelated.", 8),
            ],
        ] {
            let mut blocks = vec![definition(4), space(1), space(2)];
            blocks.extend(boundary);
            let before = blocks.clone();
            normalize_definition_nesting(&mut blocks);
            assert_eq!(blocks, before);
        }
    }

    #[test]
    fn hanging_definitions_share_the_boundary_without_stealing_trailing_spacing() {
        let mut blocks = vec![
            paragraph("--owner", 0),
            paragraph("Description.", 4),
            space(2),
            paragraph("Continuation.", 4),
            space(3),
            paragraph("--next", 0),
            paragraph("Next description.", 4),
            space(5),
        ];
        normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[1], space(3));
        assert_eq!(blocks[3], space(5));
        let Block::DefinitionList { items, .. } = &blocks[0] else {
            unreachable!()
        };
        assert_eq!(items[0].description.len(), 3);
        assert_eq!(items[0].description[1], space(2));
    }
}
