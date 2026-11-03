//! Generated punctuation participates in the same flow as authored operands.
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_inline_node, append_inline_node_with_next,
    append_inline_nodes, first_part_children, inline_children, navigation_anchor, plain_text,
};
use libmandoc_rs::{
    MacroToken::{Man, Mdoc},
    ManMacro, MdocMacro,
};

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
    builder.execution.finish_executed_node(argument);
    builder.restore_definition_geometry(geometry);
}

pub(super) fn manual_reference(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let children = inline_children(node);
    let man_reference = node.macro_token.as_ref() == Some(&Man(ManMacro::Mr));
    let first = children.first();
    if first.is_none() && !man_reference {
        return;
    }
    if man_reference {
        // print_man_node() resets before pre_MR(), which resets again.
        // Both replacements update the independent previous-font register.
        builder.font.select(Font::Regular);
        builder.font.select(Font::Regular);
    }
    // CVS mdoc_xr_pre/man_MR_pre send the raw name and section through
    // html.c::print_encode(norecurse=1), independently of terminal glyphs.
    // Visible operands execute exactly once in the caller's state below.
    let target_name = super::links::identity::link_identity_text(
        first
            .and_then(|node| node.text.as_deref())
            .unwrap_or_default(),
    );
    // print_href() distinguishes a NULL section from an authored empty
    // section. Preserve that invalid empty identity instead of repairing it
    // into an unconstrained manual query.
    let section = children
        .get(1)
        .and_then(|child| child.text.as_deref())
        .map(super::links::identity::link_identity_text);
    let previous_owner = builder.zero_advance.pending_native_owner();
    let mut annotated = false;
    builder.append_scope(
        |builder| {
            if let Some(first) = first {
                builder
                    .with_direct_word_operands(|builder| append_inline_node(builder, first, name));
            }
            // CVS man_term.c::pre_MR() emits parentheses even without a
            // section operand; mdoc termp_xr_pre() returns after the name.
            if children.get(1).is_some() || man_reference {
                if first.is_some() {
                    builder.tighten_next_boundary();
                }
                builder.append_text("(");
                builder.tighten_next_boundary();
                if let Some(section_node) = children.get(1) {
                    builder.with_direct_word_operands(|builder| {
                        append_inline_node(builder, section_node, name);
                    });
                }
                builder.tighten_next_boundary();
                builder.append_text(")");
            }
        },
        |children| {
            if first.is_none() || plain_text(&children).trim().is_empty() {
                return children;
            }
            annotated = true;
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
    if first.is_some() {
        // A section-final bare z can defer the generated ')' until the next
        // word. term_word()/encode1() keep its original ownership; annotate
        // that delayed glyph and rejoin only this occurrence's prefix.
        builder.zero_advance.bind_pending_link(
            previous_owner,
            mant_ir::LinkTarget::Manual {
                name: target_name,
                manual_section: section,
            },
            annotated,
        );
    }
    for child in children.get(2..).unwrap_or_default() {
        builder.tighten_next_boundary();
        builder.with_direct_word_operands(|builder| append_inline_node(builder, child, name));
    }
    if man_reference {
        // print_man_node() also resets after the suffix. A following fP
        // restores that last executed font, rather than the identity decoder.
        builder.font.select(Font::Regular);
    }
}
