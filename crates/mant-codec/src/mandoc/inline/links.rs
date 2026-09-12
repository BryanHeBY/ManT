//! Dialect-specific link execution, separate from pure target construction.
use super::font::{execute_hidden_text, execute_suppressed_text_controls};
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, RoffInlineEvent, append_inline_node,
    append_inline_nodes, decode, first_part_children, inline_children,
    lower_inline_nodes_with_spacing, plain_text, text_node, visible_text,
};

/// Execute mdoc `.Lk` in the caller's formatter stream, then wrap the visible
/// label in a typed link. The address is identity data, so it must not force
/// a private builder that loses pending `\\z` state at the macro boundary.
pub(super) fn append_link(builder: &mut InlineBuilder, node: &Node, default_name: Option<&str>) {
    let children = inline_children(node);
    let Some(first) = children.first() else {
        return;
    };
    // Identity extraction is pure. The formatter executes the label before
    // the URI, even when compact presentation hides the latter's glyphs.
    let address = link_identity_text(first.text.as_deref().unwrap_or_default());
    let label_end = children
        .iter()
        .rposition(|child| !child.flags.delimiter_close)
        .map_or(1, |index| index + 1)
        .max(1);
    let label = &children[1..label_end];
    if label.is_empty() {
        append_link_target_or_text(builder, first, address, default_name);
    } else if source_closing_punctuation(label) {
        append_link_target_or_text(builder, first, address, default_name);
        append_inline_nodes(builder, label, default_name);
    } else {
        // A descriptive Lk label replaces the rendered URI, but remains in
        // the same output stream as its surrounding source siblings. CVS
        // `termp_lk_pre()` presents the label first, then its colon and URI,
        // regardless of source operand order. Preserve that execution order:
        // a hidden URI's controls must be applied after label controls.
        let checkpoint = builder.output_checkpoint();
        builder.with_font_scope(Font::Emphasis, |builder| {
            append_inline_nodes(builder, label, default_name);
        });
        // CVS `termp_lk_pre()` writes a generated colon between the
        // descriptive label and URI. Compact Mant output intentionally hides
        // that punctuation and repeated URI, but it must retain the colon's
        // zero-advance overwrite effect before later source siblings run.
        builder.zero_advance.consume_hidden_generated_glyph();
        if builder.output_since_is_printable(&checkpoint) {
            if !address.is_empty() {
                wrap_external_link_output(builder, &checkpoint, address);
            }
            // The URI is hidden by compact presentation, not absent from
            // native execution. Decode it completely after the label and
            // generated colon; controls and `\\z` operands must not leak into
            // later source siblings.
            execute_hidden_node(builder, first);
        } else {
            // A syntactically present label can disappear after zero-width
            // projection. Do not leave an empty link: rollback only its
            // output, retain its executed state, and render the URI fallback
            // exactly once.
            builder.discard_output_since(checkpoint);
            append_link_target_or_text(builder, first, address, default_name);
        }
    }
    append_inline_nodes(builder, &children[label_end..], default_name);
}

/// Unlike Lk, Mt owns a sequence of addresses, not an address and a label.
pub(super) fn append_mail_addresses(
    builder: &mut InlineBuilder,
    node: &Node,
    default_name: Option<&str>,
) {
    let children = inline_children(node);
    let saved = builder.font.push_scope(Font::Emphasis);
    for child in children {
        if child.flags.delimiter_close || child.flags.delimiter_open {
            append_inline_node(builder, child, default_name);
            continue;
        }
        let address = link_identity_text(child.text.as_deref().unwrap_or_default());
        if address.is_empty() {
            // A control-only mail operand is not an address, but it still
            // changes the formatter state consumed by the following address
            // and sibling nodes.
            execute_hidden_node(builder, child);
        } else {
            append_external_link(builder, address, true, |builder| {
                append_inline_node(builder, child, default_name);
            });
        }
    }
    builder.font.pop_scope(saved);
}

/// Semantic wrappers may replace an authored operand's visible spelling, but
/// they never erase its execution effects. Keep that state transition in the
/// caller's one formatter stream so hidden URI/mail controls, empty labels,
/// and later siblings observe the same font and `\\z` state as CVS mandoc.
fn execute_hidden_node(builder: &mut InlineBuilder, node: &Node) {
    if let Some(source) = node.text.as_deref() {
        execute_hidden_text(source, &mut builder.font, &mut builder.zero_advance);
    }
}

fn append_link_target_or_text(
    builder: &mut InlineBuilder,
    node: &Node,
    address: String,
    default_name: Option<&str>,
) {
    if address.is_empty() {
        // A malformed or control-only target must degrade to ordinary source
        // output. It still owns font and zero-width effects; an invalid URI
        // is not permission to erase its visible label or later state.
        append_inline_node(builder, node, default_name);
    } else {
        append_external_link(builder, address, false, |builder| {
            append_inline_node(builder, node, default_name);
        });
    }
}

/// Wrap newly executed visible content without interrupting the caller's
/// formatter state. A link is an IR annotation around output, not an atomic
/// source fragment: following `Ns`, `\\z`, font, and word-boundary events still
/// observe the same builder state.
fn append_external_link(
    builder: &mut InlineBuilder,
    address: String,
    email: bool,
    append: impl FnOnce(&mut InlineBuilder),
) {
    builder.append_scope(append, |children| {
        let (prefix, children) = split_boundary_prefix(children);
        let mut output = prefix;
        output.push(Inline::Link {
            target: external_link_target(address, email),
            title: None,
            children,
        });
        output
    });
}

/// Attach link identity to an already-executed descriptive label.  The label
/// must not be lowered a second time merely because its final visible form is
/// only known after `\\z` and generated colon projection have run.
fn wrap_external_link_output(
    builder: &mut InlineBuilder,
    checkpoint: &super::flow::OutputCheckpoint,
    address: String,
) {
    builder.wrap_output_since(checkpoint, |children| {
        let (prefix, children) = split_boundary_prefix(children);
        let mut output = prefix;
        output.push(Inline::Link {
            target: external_link_target(address, false),
            title: None,
            children,
        });
        output
    });
}

/// `InlineBuilder` owns inter-word padding. When a new link begins a word,
/// that padding is appended before its first child; retain it as surrounding
/// prose rather than accidentally making it part of the clickable label.
fn split_boundary_prefix(mut children: Vec<Inline>) -> (Vec<Inline>, Vec<Inline>) {
    let Some(Inline::Text { value }) = children.first_mut() else {
        return (Vec::new(), children);
    };
    let prefix_len = value
        .char_indices()
        .take_while(|(_, character)| character.is_whitespace())
        .last()
        .map_or(0, |(index, character)| index + character.len_utf8());
    if prefix_len == 0 {
        return (Vec::new(), children);
    }
    let suffix = value.split_off(prefix_len);
    let prefix = std::mem::replace(value, suffix);
    if matches!(children.first(), Some(Inline::Text { value }) if value.is_empty()) {
        children.remove(0);
    }
    (vec![Inline::Text { value: prefix }], children)
}

fn source_closing_punctuation(nodes: &[Node]) -> bool {
    let mut text = String::new();
    for node in nodes {
        let Some(value) = node.text.as_deref() else {
            return false;
        };
        text.push_str(&visible_text(value));
    }
    !text.is_empty()
        && text.chars().all(|character| {
            matches!(
                character,
                '.' | ',' | ':' | ';' | '!' | '?' | ')' | ']' | '}'
            )
        })
}

/// Extract a link destination from the source spelling without borrowing its
/// rendered glyph stream.  A `\z` glyph participates in terminal overstrike
/// layout but is not part of mdoc's destination: CVS `mdoc_html.c` builds the
/// `Lk`/`Mt` href from the logical operand while terminal rendering consumes
/// the zero-advance glyph in the surrounding output flow.  Keeping this
/// separate from [`visible_text`] prevents a later sibling from changing a
/// typed destination and lets the label retain its own formatter state.
fn link_identity_text(source: &str) -> String {
    let mut identity = String::new();
    let mut discard_next_glyph = false;
    for event in decode(source) {
        match event {
            RoffInlineEvent::Text(value) => {
                for character in value.chars() {
                    if discard_next_glyph {
                        discard_next_glyph = false;
                    } else {
                        identity.push(character);
                    }
                }
            }
            RoffInlineEvent::Glyph(value) | RoffInlineEvent::FallbackGlyph(value) => {
                if discard_next_glyph {
                    discard_next_glyph = false;
                } else {
                    identity.push_str(&value);
                }
            }
            RoffInlineEvent::ZeroAdvance => discard_next_glyph = true,
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::EmptyDestination
            | RoffInlineEvent::LineBreak
            | RoffInlineEvent::Presentation { .. } => {}
        }
    }
    identity
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
        .collect::<Vec<_>>();
    let visible = authored
        .iter()
        .enumerate()
        .filter_map(|(index, child)| {
            child
                .text
                .as_deref()
                .is_some_and(|text| !visible_text(text).is_empty())
                .then_some(index)
        })
        .collect::<Vec<_>>();
    let Some(&first_index) = visible.first() else {
        // `.Bx \\fB` has no authored glyph, but CVS still executes the font
        // escape before the validator-generated BSD text.
        execute_bsd_suppressed_controls(builder, &authored);
        builder.append_text("BSD");
        return;
    };
    if visible.len() > 2 {
        append_inline_nodes(builder, inline_children(node), name);
        return;
    }
    let first = authored[first_index];
    let first_text = visible_text(first.text.as_deref().unwrap_or_default());
    if visible.len() == 1 {
        let lifecycle = match first_text.as_str() {
            "-alpha" => Some("BSD (currently in alpha test)"),
            "-beta" => Some("BSD (currently in beta test)"),
            "-devel" => Some("BSD (currently under development)"),
            _ => None,
        };
        if let Some(lifecycle) = lifecycle {
            // The lifecycle spelling is intentionally replaced, but all its
            // controls remain executed source state for generated text and
            // following siblings.
            execute_bsd_suppressed_controls(builder, &authored);
            builder.append_text(lifecycle);
            return;
        }
    }
    // Execute authored operands and generated spelling in the caller's one
    // formatter stream. Native `Bx` inserts `BSD` with an `Ns` join, but its
    // generated sibling cannot see a caller-owned zero-advance state after an
    // atomic reconstruction. Keeping the suffix here preserves release-word
    // spacing and lets it overstrike a pending glyph.
    for child in &authored[..first_index] {
        execute_bsd_suppressed_controls(builder, std::slice::from_ref(child));
    }
    append_inline_node(builder, first, name);
    builder.tighten_next_boundary();
    builder.append_text("BSD");
    if let Some(&second_index) = visible.get(1) {
        for child in &authored[first_index + 1..second_index] {
            execute_bsd_suppressed_controls(builder, std::slice::from_ref(child));
        }
        let second = authored[second_index];
        builder.append_text(" ");
        append_inline_node(builder, second, name);
    }
    let final_index = visible.last().copied().unwrap_or(first_index);
    for child in &authored[final_index + 1..] {
        execute_bsd_suppressed_controls(builder, std::slice::from_ref(child));
    }
}

fn execute_bsd_suppressed_controls(builder: &mut InlineBuilder, nodes: &[&Node]) {
    for node in nodes {
        if let Some(source) = node.text.as_deref() {
            execute_suppressed_text_controls(source, &mut builder.font, &mut builder.zero_advance);
        }
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
