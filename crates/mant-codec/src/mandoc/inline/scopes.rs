//! Compose semantic wrappers in output order, not by inspecting AST tails.
use super::font::coalesce_font_runs;
use super::links::{append_bsd_reference, append_link, append_mail_addresses};
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_include, append_inline_nodes,
    first_part_children, inline_children, lower_equation_node, navigation_anchor, plain_text,
};

pub(super) fn append(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    if node.macro_name.as_deref() == Some("Tg") {
        if let Some(anchor) = navigation_anchor(node) {
            builder.append(vec![anchor]);
        }
        return;
    }
    if node.flags.no_print || node.kind == NodeKind::Comment {
        return;
    }
    // Atomic reconstructions own their generated punctuation (references,
    // function declarations, etc.); their output cannot carry a guessed tail
    // effect. Transparent and styled scopes below share the caller's flow.
    if append_atomic(builder, node, name) {
        return;
    }
    if let Some(anchor) = navigation_anchor(node) {
        builder.append(vec![anchor]);
    }
    let mut saved_font = None;
    if crate::mandoc::containers::walk(node, |event| {
        use crate::mandoc::containers::Event;
        match event {
            Event::BeginNode(node) => builder.begin_executed_node(node),
            Event::Break => {
                builder.hard_break();
                builder.reset_source_cursor();
            }
            Event::FlushLine => {
                builder.append(vec![Inline::LineBreak]);
                builder.reset_source_cursor();
            }
            Event::Children(nodes) => append_inline_nodes(builder, nodes, name),
            Event::Glyph(value) => builder.append_text(&value),
            Event::Tight => builder.tighten_next_boundary(),
            Event::Release => builder.release_next_boundary(),
            Event::EmptyWord => builder.execute_empty_word(),
            Event::EnterKeep => builder.enter_keep_words(),
            Event::ExitKeep => builder.exit_keep_words(),
            Event::EnterFont(font) => saved_font = Some(builder.font.push_scope(font)),
            Event::ExitFont => {
                if let Some(saved) = saved_font.take() {
                    builder.font.pop_scope(saved);
                }
            }
        }
    }) {
        return;
    }
    let children = inline_children(node);
    match node.macro_name.as_deref() {
        Some("Fn" | "Fo") => super::generated::function(builder, node, name),
        Some("Xr" | "MR") => super::generated::manual_reference(builder, node, name),
        Some("Nm") if node.kind == NodeKind::Block => {
            builder.with_font_scope(Font::Strong, |builder| {
                append_name(builder, first_part_children(node, NodeKind::Head), name);
            });
            append_inline_nodes(builder, first_part_children(node, NodeKind::Body), name);
        }
        Some("Nm") => {
            builder.with_font_scope(Font::Strong, |builder| append_name(builder, children, name));
        }
        Some("Fl") => builder.append_scope(
            |builder| {
                builder.with_font_scope(Font::Strong, |builder| {
                    builder.append_text("-");
                    builder
                        .with_prefix_join(|builder| append_inline_nodes(builder, children, name));
                });
            },
            coalesce_font_runs,
        ),
        Some("Cm" | "Ic" | "Sy" | "Ms") => builder.with_font_scope(Font::Strong, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Ar" | "Pa" | "Em" | "Va" | "Vt" | "Ft" | "Fa" | "Ad" | "Fr") => builder
            .with_font_scope(Font::Emphasis, |builder| {
                append_inline_nodes(builder, children, name);
            }),
        Some("No" | "Dv") => builder.with_font_scope(Font::Regular, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Li") => builder.with_font_scope(Font::Code, |builder| {
            append_inline_nodes(builder, children, name);
        }),
        Some("Sx") => builder.append_scope(
            |builder| {
                builder.with_font_scope(Font::Emphasis, |builder| {
                    append_inline_nodes(builder, children, name);
                });
            },
            section_reference,
        ),
        Some("Nd") => {
            builder.append_text("— ");
            append_inline_nodes(builder, children, name);
        }
        _ => append_inline_nodes(builder, children, name),
    }
}

/// Atomic syntax owns a semantic wrapper, but its visible components still
/// execute in the caller's formatter stream.  Keep that dispatch separate
/// from ordinary scope lowering so new atomic forms cannot accidentally
/// recreate an isolated zero-advance state.
fn append_atomic(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) -> bool {
    match node.macro_name.as_deref() {
        Some("In") => append_include(builder, node, name),
        Some("Bx") => append_bsd_reference(builder, node, name),
        Some("Lk") => {
            if let Some(anchor) = navigation_anchor(node) {
                builder.append(vec![anchor]);
            }
            append_link(builder, node, name);
        }
        Some("Mt") => {
            if let Some(anchor) = navigation_anchor(node) {
                builder.append(vec![anchor]);
            }
            append_mail_addresses(builder, node, name);
        }
        _ if node.kind == NodeKind::Equation => {
            builder.append(lower_equation_node(node));
        }
        _ => return false,
    }
    true
}

fn section_reference(children: Vec<Inline>) -> Vec<Inline> {
    if children.is_empty() {
        return children;
    }
    vec![Inline::Link {
        target: mant_ir::LinkTarget::Section {
            id: plain_text(&children).trim().into(),
        },
        title: None,
        children,
    }]
}

fn append_name(builder: &mut InlineBuilder, nodes: &[Node], name: Option<&str>) {
    if nodes.is_empty() {
        if let Some(name) = name {
            builder.append_text(name);
        }
    } else {
        append_inline_nodes(builder, nodes, name);
    }
}
