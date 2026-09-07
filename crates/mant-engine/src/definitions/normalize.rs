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
        let description_origin = base_indent.saturating_add(4);
        while pending
            .front()
            .is_some_and(|next| block_indent(next) > base_indent)
        {
            let mut nested = pending.pop_front().expect("front exists");
            shift_block_indent(&mut nested, description_origin);
            last_item.description.push(nested);
        }
        normalized.push(block);
    }

    *blocks = normalized;
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
            if matches!(next, Block::VerticalSpace { .. }) {
                if description.is_empty() {
                    break;
                }
                description.push(pending.pop_front().expect("front exists"));
                continue;
            }
            if block_indent(next) <= term_indent {
                break;
            }
            description.push(pending.pop_front().expect("front exists"));
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
        let description_origin = term_indent.saturating_add(4);
        for child in &mut description {
            shift_block_indent(child, description_origin);
        }
        let terms = vec![children];
        normalized.push(Block::DefinitionList {
            items: vec![DefinitionItem {
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

fn block_indent(block: &Block) -> u16 {
    block_layout(block).map_or(0, |layout| layout.indent_columns)
}

fn shift_block_indent(block: &mut Block, origin: u16) {
    if let Some(layout) = block_layout_mut(block) {
        layout.indent_columns = layout.indent_columns.saturating_sub(origin);
    }
}
