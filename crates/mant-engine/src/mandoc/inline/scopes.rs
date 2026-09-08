//! Compose semantic wrappers in output order, not by inspecting AST tails.
use super::font::coalesce_font_runs;
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_inline_node, append_inline_node_with_next,
    append_inline_nodes, enclosure_marks, first_part_children, inline_children, lower_atomic_node,
    navigation_anchor, plain_text, visible_text,
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
    if node.kind == NodeKind::Equation
        || matches!(
            node.macro_name.as_deref(),
            Some("In" | "Xr" | "MR" | "Lk" | "Mt" | "Bx" | "Fn" | "Fo")
        )
    {
        builder.append(lower_atomic_node(node, name, builder.spacing_enabled()));
        return;
    }
    if let Some(anchor) = navigation_anchor(node) {
        builder.append(vec![anchor]);
    }
    let children = inline_children(node);
    match node.macro_name.as_deref() {
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
        Some("Eo") => authored_enclosure(builder, node, name),
        Some("En") if node.enclosure.is_some() => {
            let enclosure = node.enclosure.as_ref().unwrap();
            enclosed(
                builder,
                children,
                &visible_text(&enclosure.opening),
                &enclosure
                    .closing
                    .as_deref()
                    .map(visible_text)
                    .unwrap_or_default(),
                name,
            );
        }
        Some(macro_name) if enclosure_marks(macro_name).is_some() => {
            let (open, close) = enclosure_marks(macro_name).unwrap();
            enclosed(builder, children, open, close, name);
            // These delimiters are outside the body and occur after its
            // generated closer. Visit them as events, exactly once.
            for child in node
                .children
                .iter()
                .filter(|child| child.flags.delimiter_close)
            {
                append_inline_node(builder, child, name);
            }
        }
        _ => append_inline_nodes(builder, children, name),
    }
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

fn authored_enclosure(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let head = first_part_children(node, NodeKind::Head);
    let body = first_part_children(node, NodeKind::Body);
    let tail = first_part_children(node, NodeKind::Tail);
    for (index, child) in node.children.iter().enumerate() {
        match child.kind {
            NodeKind::Head => {
                append_inline_nodes(builder, &child.children, name);
                if !head.is_empty() && (!body.is_empty() || !tail.is_empty()) {
                    builder.tighten_next_boundary();
                }
            }
            NodeKind::Tail => {
                if !tail.is_empty() && (!head.is_empty() || !body.is_empty()) {
                    builder.tighten_next_boundary();
                }
                append_inline_nodes(builder, &child.children, name);
            }
            NodeKind::Body => {
                append_inline_nodes(builder, &child.children, name);
                // Eo/Ec owns its closing boundary even when no closing glyph
                // was supplied. A completely empty enclosure is a zero-width
                // word, not a transparent target/control scope.
                if head.is_empty() && body.is_empty() && tail.is_empty() {
                    builder.append_word(Vec::new());
                } else if tail.is_empty() {
                    builder.release_next_boundary();
                }
            }
            _ => append_inline_node_with_next(builder, child, node.children.get(index + 1), name),
        }
    }
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

fn enclosed(
    builder: &mut InlineBuilder,
    nodes: &[Node],
    open: &str,
    close: &str,
    name: Option<&str>,
) {
    if !open.is_empty() {
        builder.append_text(open);
        builder.tighten_next_boundary();
    }
    append_inline_nodes(builder, nodes, name);
    if !close.is_empty() {
        builder.tighten_next_boundary();
        builder.append_text(close);
    }
}
