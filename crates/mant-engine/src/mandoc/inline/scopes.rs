//! Compose semantic wrappers in output order, not by inspecting AST tails.
use super::{
    Inline, InlineBuilder, Node, NodeKind, append_inline_node, append_inline_node_with_next,
    append_inline_nodes, enclosure_marks, first_part_children, inline_children, lower_atomic_node,
    navigation_anchor, plain_text, text_node, visible_text, wrap_emphasis, wrap_strong,
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
            builder.append_scope(
                |builder| append_name(builder, first_part_children(node, NodeKind::Head), name),
                wrap_strong,
            );
            append_inline_nodes(builder, first_part_children(node, NodeKind::Body), name);
        }
        Some("Nm") => {
            builder.append_scope(|builder| append_name(builder, children, name), wrap_strong);
        }
        Some("Fl") => builder.append_scope(
            |builder| {
                builder.append(text_node("-"));
                builder.tighten_next_boundary();
                append_inline_nodes(builder, children, name);
                // A generated dash does not itself request an outside join.
                // Empty Fl still needs that internal boundary consumed.
                if children.is_empty() {
                    builder.clear_tight_boundary();
                }
            },
            wrap_strong,
        ),
        Some("Cm" | "Ic" | "Sy") => builder.append_scope(
            |builder| append_inline_nodes(builder, children, name),
            wrap_strong,
        ),
        Some("Ar" | "Pa" | "Em" | "Va" | "Vt" | "Ft" | "Fa") => builder.append_scope(
            |builder| append_inline_nodes(builder, children, name),
            wrap_emphasis,
        ),
        Some("Li") => builder.append_scope(
            |builder| append_inline_nodes(builder, children, name),
            |children| {
                vec![Inline::Code {
                    value: plain_text(&children),
                }]
            },
        ),
        Some("Sx") => builder.append_scope(
            |builder| append_inline_nodes(builder, children, name),
            section_reference,
        ),
        Some("Nd") => {
            builder.append(text_node("— "));
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
    for (index, child) in node.children.iter().enumerate() {
        match child.kind {
            NodeKind::Head => {
                append_inline_nodes(builder, &child.children, name);
                if !child.children.is_empty() {
                    builder.tighten_next_boundary();
                }
            }
            NodeKind::Tail => {
                if !child.children.is_empty() {
                    builder.tighten_next_boundary();
                }
                append_inline_nodes(builder, &child.children, name);
            }
            NodeKind::Body => append_inline_nodes(builder, &child.children, name),
            _ => append_inline_node_with_next(builder, child, node.children.get(index + 1), name),
        }
    }
}

fn append_name(builder: &mut InlineBuilder, nodes: &[Node], name: Option<&str>) {
    if nodes.is_empty() {
        if let Some(name) = name {
            builder.append(text_node(name));
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
        builder.append(text_node(open));
        builder.tighten_next_boundary();
    }
    append_inline_nodes(builder, nodes, name);
    if !close.is_empty() {
        builder.tighten_next_boundary();
        builder.append(text_node(close));
    }
}
