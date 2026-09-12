//! Dialect-specific link execution, separate from pure target construction.
use super::{
    Font, FontState, Inline, InlineBuilder, Node, NodeKind, append_inline_node,
    append_inline_nodes, first_part_children, inline_children, lower_inline_node,
    lower_inline_nodes_with_font_state, lower_inline_nodes_with_spacing, plain_text, text_node,
    visible_text,
};

pub(super) fn lower_link(
    children: &[Node],
    default_name: Option<&str>,
    spacing_enabled: bool,
    font: &mut FontState,
) -> Vec<Inline> {
    let Some(first) = children.first() else {
        return Vec::new();
    };
    // Identity extraction is pure. The formatter executes the label before
    // the URI, even when compact presentation hides the latter's glyphs.
    let address = visible_text(first.text.as_deref().unwrap_or_default());
    if address.is_empty() {
        return Vec::new();
    }
    let label_end = children
        .iter()
        .rposition(|child| !child.flags.delimiter_close)
        .map_or(1, |index| index + 1)
        .max(1);
    let label = if label_end > 1 {
        let saved = font.push_scope(Font::Emphasis);
        let label = lower_inline_nodes_with_font_state(
            &children[1..label_end],
            default_name,
            spacing_enabled,
            font,
        );
        font.pop_scope(saved);
        label
    } else {
        Vec::new()
    };
    let address_nodes = lower_inline_node(first, default_name, spacing_enabled, font);
    let mut output = lower_external_link(address, address_nodes, label, false);
    output.extend(lower_inline_nodes_with_font_state(
        &children[label_end..],
        default_name,
        spacing_enabled,
        font,
    ));
    output
}

/// Unlike Lk, Mt owns a sequence of addresses, not an address and a label.
pub(super) fn lower_mail_addresses(
    children: &[Node],
    default_name: Option<&str>,
    spacing_enabled: bool,
    font: &mut FontState,
) -> Vec<Inline> {
    let mut builder = InlineBuilder::with_spacing(spacing_enabled);
    builder.font = *font;
    let saved = builder.font.push_scope(Font::Emphasis);
    for child in children {
        if child.flags.delimiter_close || child.flags.delimiter_open {
            append_inline_node(&mut builder, child, default_name);
            continue;
        }
        let address = visible_text(child.text.as_deref().unwrap_or_default());
        let address_nodes =
            lower_inline_node(child, default_name, spacing_enabled, &mut builder.font);
        if !address.is_empty() {
            builder.append(lower_external_link(
                address,
                address_nodes,
                Vec::new(),
                true,
            ));
        }
    }
    builder.font.pop_scope(saved);
    *font = builder.font;
    builder.finish()
}

/// Build an mdoc external link without allowing punctuation to hide its target.
///
/// `Lk` accepts ordinary trailing sentence punctuation as an argument.
/// It is not a descriptive label: a source spelling such as `.Lk URL .` must
/// render `URL.` rather than an otherwise invisible link whose only child is
/// `.`. The same policy is shared with the source fallback below.
fn lower_external_link(
    address: String,
    address_nodes: Vec<Inline>,
    label: Vec<Inline>,
    email: bool,
) -> Vec<Inline> {
    let punctuation_only = is_source_closing_punctuation(&plain_text(&label));
    if punctuation_only {
        let children = address_nodes;
        let target = external_link_target(address, email);
        let mut output = vec![Inline::Link {
            target,
            title: None,
            children,
        }];
        output.extend(label);
        return output;
    }
    let children = if label.is_empty() {
        address_nodes
    } else {
        label
    };
    vec![Inline::Link {
        target: external_link_target(address, email),
        title: None,
        children,
    }]
}

fn is_source_closing_punctuation(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            matches!(
                character,
                '.' | ',' | ':' | ';' | '!' | '?' | ')' | ']' | '}'
            )
        })
}

/// Lower the portable semantic forms of mdoc `Bx` from its authored arguments.
///
///
/// libmandoc appends a generated `BSD` node with `Ns` and intentionally leaves
/// lifecycle arguments as compact `-develBSD` text. The mdoc contract instead
/// gives the lifecycle forms descriptive meanings, while an ordinary version
/// and optional release render as `versionBSD release`. The raw AST flags make
/// this distinction explicit without reparsing source text or depending on a
/// particular formatter's generated nodes.
pub(super) fn append_bsd_reference(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let authored = node
        .children
        .iter()
        .filter(|child| {
            child.kind == NodeKind::Text && !child.flags.generated && !child.flags.no_print
        })
        .filter(|child| {
            child
                .text
                .as_deref()
                .is_some_and(|text| !visible_text(text).is_empty())
        })
        .collect::<Vec<_>>();
    let Some(first) = authored.first() else {
        builder.append_text("BSD");
        return;
    };
    if authored.len() > 2 {
        append_inline_nodes(builder, inline_children(node), name);
        return;
    }
    let first_text = visible_text(first.text.as_deref().unwrap_or_default());
    if authored.len() == 1 {
        let lifecycle = match first_text.as_str() {
            "-alpha" => Some("BSD (currently in alpha test)"),
            "-beta" => Some("BSD (currently in beta test)"),
            "-devel" => Some("BSD (currently under development)"),
            _ => None,
        };
        if let Some(lifecycle) = lifecycle {
            builder.append_text(lifecycle);
            return;
        }
    }
    // Execute authored operands and generated spelling in the caller's one
    // formatter stream. Native `Bx` inserts `BSD` with an `Ns` join, but its
    // generated sibling cannot see a caller-owned zero-advance state after an
    // atomic reconstruction. Keeping the suffix here preserves release-word
    // spacing and lets it overstrike a pending glyph.
    append_inline_node(builder, first, name);
    builder.tighten_next_boundary();
    builder.append_text("BSD");
    if let Some(second) = authored.get(1) {
        builder.append_text(" ");
        append_inline_node(builder, second, name);
    }
}

fn external_link_target(address: String, email: bool) -> mant_ir::LinkTarget {
    if email {
        mant_ir::LinkTarget::Email { address }
    } else {
        mant_ir::LinkTarget::External { uri: address }
    }
}

/// Lower GNU man-ext `.UR` and `.MT` blocks as one inline phrase.
///
/// The macros are structural in libmandoc's tree because their label occupies
/// a body, but they do not start a paragraph in man(7). A descriptive label
/// keeps the target visible after the link so text search and citation views
/// retain both pieces of source information.
pub(in crate::mandoc) fn lower_man_link(
    node: &Node,
    default_name: Option<&str>,
    spacing_enabled: bool,
) -> Vec<Inline> {
    let target = plain_text(&lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Head),
        default_name,
        spacing_enabled,
    ));
    if target.is_empty() {
        return lower_inline_nodes_with_spacing(
            first_part_children(node, NodeKind::Body),
            default_name,
            spacing_enabled,
        );
    }

    let label = lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Body),
        default_name,
        spacing_enabled,
    );
    let has_label = !label.is_empty();
    let children = if has_label { label } else { text_node(&target) };
    let link_target = if node.macro_name.as_deref() == Some("MT") {
        mant_ir::LinkTarget::Email {
            address: target.clone(),
        }
    } else {
        mant_ir::LinkTarget::External {
            uri: target.clone(),
        }
    };
    let mut output = vec![Inline::Link {
        target: link_target,
        title: None,
        children,
    }];
    if has_label {
        output.push(Inline::Text {
            value: format!(" ⟨{target}⟩"),
        });
    }
    output.extend(lower_inline_nodes_with_spacing(
        first_part_children(node, NodeKind::Tail),
        default_name,
        spacing_enabled,
    ));
    output
}
