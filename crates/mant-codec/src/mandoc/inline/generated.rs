// Copyright (c) 2010, 2012-2020, 2022, 2025, 2026
//               Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
// Copyright (c) 2013 Franco Fichtner <franco@lastsummer.de>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! Generated punctuation participates in the same flow as authored operands.
use super::{
    Font, Inline, InlineBuilder, Node, append_isolated_inline_node, inline_children,
    navigation_anchor, plain_text,
};
use libmandoc_rs::{MacroToken::Man, ManMacro};

/// Fn pre executes direct `term_word` operands, rather than entering their
/// `NODE_LINE/EOS` lifecycle (mdoc_term.c:1345-1405). Fo uses the shared
/// container driver so its HEAD/BODY and explicit Fc retain their source order.
pub(super) fn function(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let Some((head, arguments)) = inline_children(node).split_first() else {
        return;
    };
    let append_name = |builder: &mut InlineBuilder| {
        builder.with_font_scope(Font::Strong, |builder| {
            builder.with_direct_word_operands(|builder| {
                append_isolated_inline_node(builder, head, name);
            });
        });
    };
    if node.flags.synopsis_pretty {
        builder.with_function_name_field(append_name);
    } else {
        append_name(builder);
    }
    builder.tighten_next_boundary();
    builder.append_text("(");
    builder.tighten_next_boundary();
    for (index, argument) in arguments.iter().enumerate() {
        builder.with_font_scope(Font::Emphasis, |builder| {
            if node.flags.synopsis_pretty {
                builder.with_nonbreaking_word_operand(|builder| {
                    append_isolated_inline_node(builder, argument, name);
                });
            } else {
                builder.with_direct_word_operands(|builder| {
                    append_isolated_inline_node(builder, argument, name);
                });
            }
        });
        if index + 1 < arguments.len() {
            builder.tighten_next_boundary();
            builder.append_text(",");
        }
    }
    builder.tighten_next_boundary();
    builder.append_text(if node.flags.synopsis_pretty {
        ");"
    } else {
        ")"
    });
    if node.flags.synopsis_pretty {
        builder.flush_native_field();
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
            append_isolated_inline_node(builder, operand, name);
            continue;
        }
        builder.with_font_scope(Font::Emphasis, |builder| {
            builder.with_nonbreaking_word_operand(|builder| {
                append_isolated_inline_node(builder, operand, name);
            });
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
    // term_word's automatic/field prefix precedes the first source operand;
    // only the source spelling belongs to Xr/MR's Link. The shared marker
    // boundary retains authored leading spaces without classifying Text.
    let (annotated, membership) = builder.append_semantic_scope(
        node.id,
        |builder| {
            if let Some(first) = first {
                builder.with_direct_word_operands(|builder| {
                    append_isolated_inline_node(builder, first, name);
                });
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
                        append_isolated_inline_node(builder, section_node, name);
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
        builder.zero_advance.bind_deferred(
            membership,
            &super::ownership::Annotation::Link {
                target: mant_ir::LinkTarget::Manual {
                    name: target_name,
                    manual_section: section,
                },
                title: None,
            },
            annotated,
        );
    }
    for child in children.get(2..).unwrap_or_default() {
        builder.tighten_next_boundary();
        builder
            .with_direct_word_operands(|builder| append_isolated_inline_node(builder, child, name));
    }
    if man_reference {
        // print_man_node() also resets after the suffix. A following fP
        // restores that last executed font, rather than the identity decoder.
        builder.font.select(Font::Regular);
    }
}
