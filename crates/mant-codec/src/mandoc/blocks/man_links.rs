//! Man link labels are annotations over normally executed block content.
use super::{Block, Inline, Node, NodeKind, first_part_children, source_span};
use crate::mandoc::inline::{
    InlineBuilder, append_inline_nodes, lower_inline_nodes_with_spacing, plain_text,
};
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
        self.state.formatter.font.man_text_boundary(); // BLOCK pre
        self.state.formatter.font.man_text_boundary(); // HEAD pre
        self.state.formatter.font.man_text_boundary(); // HEAD post
        self.state.formatter.font.man_text_boundary(); // BODY pre
        // A link BODY is a new non-RS sibling list. print_bvspace() cannot
        // climb through UR/MT to an earlier paragraph outside this BODY.
        let inherited = std::mem::take(&mut self.man_source_predecessor);
        self.push_nodes(body);
        self.man_source_predecessor = inherited;
        self.state.formatter.font.man_text_boundary(); // BODY post

        // man_term.c::post_UR() always executes its first generated word,
        // even for an empty HEAD. Its boundary may settle a final BODY \z
        // glyph, while a tight \c still permits the bracket to overstrike.
        self.push_link_word(node, InlineBuilder::prepare_generated_word);
        let prior_glyph_emitted = self
            .state
            .formatter
            .execution
            .zero_advance
            .end_output_owner();
        let head_is_label = body.is_empty() && !target_text.is_empty();
        if !target_text.is_empty()
            && !head_is_label
            && !self
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
            if head_is_label {
                builder.append_scope(
                    |builder| append_inline_nodes(builder, head, self.context.default_name),
                    |children| {
                        vec![Inline::Link {
                            target,
                            title: None,
                            children,
                        }]
                    },
                );
            } else {
                append_inline_nodes(builder, head, self.context.default_name);
            }
            builder.tighten_next_boundary();
            builder.append_text("⟩");
        });
        self.state.formatter.font.man_text_boundary(); // BLOCK post
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
    let mut children = nodes.split_off(first);
    // The link annotates label glyphs, not a formatter row terminator. Keep
    // trailing hard breaks outside the wrapper so the literal sink can tell
    // whether that physical row was already closed by fi/nf or a word-end
    // break. Internal breaks between label glyphs remain inside the link.
    let tail_start = children
        .iter()
        .rposition(|node| !matches!(node, Inline::Anchor { .. } | Inline::LineBreak))
        .map_or(0, |index| index + 1);
    let trailing = if children[tail_start..]
        .iter()
        .any(|node| matches!(node, Inline::LineBreak))
    {
        children.split_off(tail_start)
    } else {
        Vec::new()
    };
    nodes.push(Inline::Link {
        target: target.clone(),
        title: None,
        children,
    });
    nodes.extend(trailing);
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
