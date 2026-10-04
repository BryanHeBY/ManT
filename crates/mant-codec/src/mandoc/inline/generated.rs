//! Generated punctuation participates in the same flow as authored operands.
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_inline_node, append_inline_node_with_next,
    append_inline_nodes, first_part_children, inline_children, navigation_anchor, plain_text,
};
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

pub(super) fn function(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let block = node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fo));
    if block && let Some(end) = node.scope_end {
        // CVS mdoc_html.c::print_mdoc_node() visits a BODY-end marker's
        // children, then mdoc_fo_post(), before the enclosing body unwinds.
        append_inline_nodes(builder, &node.children, name);
        if let Some(synopsis) = builder.scope_posts.function_suffix(end.body_id) {
            builder.tighten_next_boundary();
            builder.append_text(if synopsis { ");" } else { ")" });
            builder.scope_posts.finish(end.body_id);
        }
        return;
    }
    let (head, body) = if block {
        (
            first_part_children(node, NodeKind::Head),
            first_part_children(node, NodeKind::Body),
        )
    } else {
        let children = inline_children(node);
        children.split_at(usize::from(!children.is_empty()))
    };
    if head.is_empty() && !block {
        append_inline_nodes(builder, body, name);
        return;
    }
    let synopsis = node.flags.synopsis_pretty
        || node
            .children
            .iter()
            .any(|child| child.kind == NodeKind::Body && child.flags.synopsis_pretty);
    let body_id = block
        .then(|| {
            node.children
                .iter()
                .find(|child| child.kind == NodeKind::Body && child.scope_end.is_none())
                .map(|body| body.id)
        })
        .flatten();
    if let Some(body_id) = body_id {
        builder.scope_posts.register_function(body_id, synopsis);
    }
    if block
        && let Some(target) = super::super::targets::part_target_with_source(node, NodeKind::Head)
    {
        builder.append(vec![target.into_inline()]);
    }
    if !head.is_empty() {
        builder.with_font_scope(Font::Strong, |builder| {
            append_inline_nodes(builder, head, name);
        });
    }
    builder.tighten_next_boundary();
    builder.append_text("(");
    builder.tighten_next_boundary();
    for (index, argument) in body.iter().enumerate() {
        if argument.flags.no_print {
            append_inline_node(builder, argument, name);
        } else if !block || argument.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fa)) {
            if block {
                // Fa owns NODE_LINE; its operand children normally do not.
                // Generated comma handling must not bypass that executed
                // wrapper event when consuming the operands directly.
                builder.begin_executed_node(argument);
            }
            if let Some(anchor) = navigation_anchor(argument) {
                builder.append(vec![anchor]);
            }
            let operands = if block {
                inline_children(argument)
            } else {
                std::slice::from_ref(argument)
            };
            for (operand_index, operand) in operands.iter().enumerate() {
                if operand.flags.delimiter_close {
                    append_inline_node(builder, operand, name);
                    continue;
                }
                builder.with_font_scope(Font::Emphasis, |builder| {
                    append_inline_node(builder, operand, name);
                });
                // Emit punctuation before intervening controls, so Sm off
                // preserves the boundary following an already written comma.
                let next_operand = crate::mandoc::adjacency::next(&operands[operand_index + 1..]);
                let comma = next_operand.map_or_else(
                    || {
                        crate::mandoc::adjacency::next(&body[index + 1..]).is_some_and(|node| {
                            !block || node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Fa))
                        })
                    },
                    |node| !node.flags.delimiter_close,
                );
                if comma {
                    builder.tighten_next_boundary();
                    builder.append_text(",");
                }
            }
        } else {
            append_inline_node_with_next(builder, argument, body.get(index + 1), name);
        }
    }
    if body_id.is_none_or(|id| !builder.scope_posts.ended(id)) {
        builder.tighten_next_boundary();
        builder.append_text(if synopsis { ");" } else { ")" });
    }
}

pub(in crate::mandoc) fn function_argument(
    builder: &mut InlineBuilder,
    argument: &Node,
    comma_after: bool,
    name: Option<&str>,
) {
    // CVS mdoc_html.c::mdoc_fa_pre() iterates each direct Fo argument and
    // inserts commas between its operands and before a following Fa sibling.
    builder.begin_executed_node(argument);
    let geometry = builder.definition_geometry_checkpoint(argument);
    if let Some(anchor) = navigation_anchor(argument) {
        builder.append(vec![anchor]);
    }
    let operands = inline_children(argument);
    for (index, operand) in operands.iter().enumerate() {
        if operand.flags.delimiter_close {
            append_inline_node(builder, operand, name);
            continue;
        }
        builder.with_font_scope(Font::Emphasis, |builder| {
            append_inline_node(builder, operand, name);
        });
        let comma = crate::mandoc::adjacency::next(&operands[index + 1..])
            .is_some_and(|next| !next.flags.delimiter_close)
            || (index + 1 == operands.len() && comma_after);
        if comma {
            builder.tighten_next_boundary();
            builder.append_text(",");
        }
    }
    builder.restore_definition_geometry(geometry);
}

pub(super) fn manual_reference(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let children = inline_children(node);
    let Some(first) = children.first() else {
        return;
    };
    // Only the target identity is read independently. Visible operands are
    // decoded exactly once in the caller's font/spacing state below.
    let target_name = super::visible_text(first.text.as_deref().unwrap_or_default());
    let section = children
        .get(1)
        .and_then(|child| child.text.as_deref())
        .map(super::visible_text)
        .filter(|value| !value.is_empty());
    builder.append_scope(
        |builder| {
            builder.with_direct_word_operands(|builder| append_inline_node(builder, first, name));
            if let Some(section_node) = children.get(1) {
                builder.tighten_next_boundary();
                builder.append_text("(");
                builder.tighten_next_boundary();
                builder.with_direct_word_operands(|builder| {
                    append_inline_node(builder, section_node, name);
                });
                builder.tighten_next_boundary();
                builder.append_text(")");
            }
        },
        |children| {
            if plain_text(&children).trim().is_empty() {
                return children;
            }
            vec![Inline::Link {
                target: mant_ir::LinkTarget::Manual {
                    name: target_name.clone(),
                    manual_section: section.clone(),
                },
                title: None,
                children,
            }]
        },
    );
    for child in children.get(2..).unwrap_or_default() {
        builder.tighten_next_boundary();
        builder.with_direct_word_operands(|builder| append_inline_node(builder, child, name));
    }
}
