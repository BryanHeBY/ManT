//! Generated punctuation participates in the same flow as authored operands.
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_inline_node, append_inline_node_with_next,
    append_inline_nodes, first_part_children, inline_children, navigation_anchor, plain_text,
};

pub(super) fn function(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let block = node.macro_name.as_deref() == Some("Fo");
    let (head, body) = if block {
        (
            first_part_children(node, NodeKind::Head),
            first_part_children(node, NodeKind::Body),
        )
    } else {
        let children = inline_children(node);
        children.split_at(usize::from(!children.is_empty()))
    };
    if head.is_empty() {
        append_inline_nodes(builder, body, name);
        return;
    }
    if block
        && let Some(target) = super::super::targets::part_target_with_source(node, NodeKind::Head)
    {
        builder.append(vec![target.into_inline()]);
    }
    builder.with_font_scope(Font::Strong, |builder| {
        append_inline_nodes(builder, head, name);
    });
    builder.tighten_next_boundary();
    builder.append_text("(");
    builder.tighten_next_boundary();
    for (index, argument) in body.iter().enumerate() {
        if argument.flags.no_print {
            append_inline_node(builder, argument, name);
        } else if !block || argument.macro_name.as_deref() == Some("Fa") {
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
                        crate::mandoc::adjacency::next(&body[index + 1..])
                            .is_some_and(|node| !block || node.macro_name.as_deref() == Some("Fa"))
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
    let synopsis = node.flags.synopsis_pretty
        || node
            .children
            .iter()
            .any(|child| child.kind == NodeKind::Body && child.flags.synopsis_pretty);
    builder.tighten_next_boundary();
    builder.append_text(if synopsis { ");" } else { ")" });
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
            append_inline_node(builder, first, name);
            if let Some(section_node) = children.get(1) {
                builder.tighten_next_boundary();
                builder.append_text("(");
                builder.tighten_next_boundary();
                append_inline_node(builder, section_node, name);
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
                    name: target_name,
                    manual_section: section,
                },
                title: None,
                children,
            }]
        },
    );
    for child in children.get(2..).unwrap_or_default() {
        builder.tighten_next_boundary();
        append_inline_node(builder, child, name);
    }
}
