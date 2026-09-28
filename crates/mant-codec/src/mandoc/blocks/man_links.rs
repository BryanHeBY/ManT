//! Man link labels are annotations over normally executed block content.
use super::{Block, Inline, Node, NodeKind, first_part_children, source_span};
use crate::mandoc::inline::{
    InlineBuilder, append_inline_nodes, lower_inline_nodes_with_spacing, plain_text,
};
use crate::mandoc::roff_escape::RoffFont as Font;
use mant_ir::LinkTarget;

impl super::BlockLowerer<'_, '_> {
    pub(super) fn push_man_link(&mut self, node: &Node) {
        let head = first_part_children(node, NodeKind::Head);
        let body = first_part_children(node, NodeKind::Body);
        let target_text = plain_text(&lower_inline_nodes_with_spacing(
            head,
            self.context.default_name,
            self.state.spacing_enabled(),
        ));
        let target = if node.macro_name.as_deref() == Some("MT") {
            LinkTarget::Email {
                address: target_text.clone(),
            }
        } else {
            LinkTarget::External {
                uri: target_text.clone(),
            }
        };

        // The label can contain PP, IP, nested UR/MT, and fi/nf requests.
        // man_term.c::print_man_node() executes them with the ordinary node
        // driver; only the output annotation is specific to UR/MT.
        let output_start = self.state.link_output_cursor();
        self.state
            .formatter
            .execution
            .zero_advance
            .begin_output_owner();
        self.state.formatter.font.select(Font::Regular); // BLOCK pre
        self.state.formatter.font.select(Font::Regular); // HEAD pre
        self.state.formatter.font.select(Font::Regular); // HEAD post
        self.state.formatter.font.select(Font::Regular); // BODY pre
        self.push_nodes(body);
        self.state.formatter.font.select(Font::Regular); // BODY post

        if target_text.is_empty() {
            self.state
                .formatter
                .execution
                .zero_advance
                .end_output_owner();
        } else {
            // The first generated word's native boundary may settle a final
            // BODY \z glyph. Execute that boundary before choosing the
            // semantic label, while a tight \c still permits overstrike.
            self.push_link_word(node, InlineBuilder::prepare_generated_word);
            let prior_glyph_emitted = self
                .state
                .formatter
                .execution
                .zero_advance
                .end_output_owner();
            if !self
                .state
                .wrap_first_link_since(output_start, &target, prior_glyph_emitted)
            {
                self.push_link_word(node, |builder| {
                    builder.append(vec![Inline::Link {
                        target: target.clone(),
                        title: None,
                        children: Vec::new(),
                    }]);
                });
            }
            self.push_link_word(node, |builder| {
                builder.append_prepared_text("⟨");
                builder.tighten_next_boundary();
                append_inline_nodes(builder, head, self.context.default_name);
                builder.tighten_next_boundary();
                builder.append_text("⟩");
            });
        }
        self.state.formatter.font.select(Font::Regular); // BLOCK post
    }

    fn push_link_word(&mut self, node: &Node, append: impl FnOnce(&mut InlineBuilder)) {
        if self.state.formatter.no_fill {
            self.push_no_fill_generated(node, false, false, false, append);
        } else {
            self.state
                .push_inline_with(source_span(node), false, false, append);
        }
    }
}

pub(super) fn wrap_first_visible_inline(
    nodes: &mut Vec<Inline>,
    target: &LinkTarget,
    start: usize,
    skip_visible: &mut usize,
) -> bool {
    let Some(first) = nodes
        .iter()
        .enumerate()
        .skip(start)
        .find_map(|(index, node)| {
            if !mant_ir::has_printable_character(std::slice::from_ref(node)) {
                return None;
            }
            if *skip_visible > 0 {
                *skip_visible -= 1;
                return None;
            }
            Some(index)
        })
    else {
        return false;
    };
    let children = nodes.split_off(first);
    nodes.push(Inline::Link {
        target: target.clone(),
        title: None,
        children,
    });
    true
}

pub(super) fn wrap_first_visible_block(
    block: &mut Block,
    target: &LinkTarget,
    start: usize,
    skip_visible: &mut usize,
) -> bool {
    match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            wrap_first_visible_inline(children, target, start, skip_visible)
        }
        Block::List { items, .. } => items.iter_mut().any(|item| {
            item.blocks
                .iter_mut()
                .any(|block| wrap_first_visible_block(block, target, 0, skip_visible))
        }),
        Block::DefinitionList { items, .. } => items.iter_mut().any(|item| {
            item.terms
                .iter_mut()
                .any(|term| wrap_first_visible_inline(term, target, 0, skip_visible))
                || item
                    .description
                    .iter_mut()
                    .any(|block| wrap_first_visible_block(block, target, 0, skip_visible))
        }),
        Block::Table { rows, .. } => rows.iter_mut().any(|row| {
            row.cells.iter_mut().any(|cell| {
                cell.blocks
                    .iter_mut()
                    .any(|block| wrap_first_visible_block(block, target, 0, skip_visible))
            })
        }),
        Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => false,
    }
}
