//! Dialect-specific link execution, separate from pure target construction.
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, RoffInlineEvent, append_inline_node,
    append_inline_nodes, decode, first_part_children, inline_children,
};

pub(in crate::mandoc::inline) mod presentation;

/// Execute mdoc `.Lk` in the caller's formatter stream, then wrap the visible
/// label in a typed link. The address is identity data, so it must not force
/// a private builder that loses pending `\\z` state at the macro boundary.
pub(super) fn append_link(builder: &mut InlineBuilder, node: &Node, default_name: Option<&str>) {
    let children = inline_children(node);
    let Some(first) = children.first() else {
        return;
    };
    // Identity extraction is pure. The formatter executes the label before
    // the generated colon and the visible URI word.
    let address = link_identity_text(first.text.as_deref().unwrap_or_default());
    let label_end = children
        .iter()
        .rposition(|child| !child.flags.delimiter_close)
        .map_or(1, |index| index + 1)
        .max(1);
    let label = &children[1..label_end];
    if label.is_empty() {
        append_link_target_or_text(builder, first, &address, default_name);
    } else {
        // A descriptive Lk label replaces the typed anchor, but the label,
        // generated colon, and URI all execute in the same output stream as
        // the surrounding source siblings. CVS `termp_lk_pre()` presents the
        // label (underlined), a plain generated `:`, and the URI word,
        // regardless of source operand order.
        presentation::append_owned_part(
            builder,
            node.id,
            presentation::Part::Description,
            &address,
            None,
            |builder| {
                builder.with_font_scope(Font::Emphasis, |builder| {
                    append_inline_nodes(builder, label, default_name);
                });
            },
        );
        // CVS termp_lk_pre() always executes this suffix for a syntactically
        // present description. The native receipt accepts or rejects its
        // owned text; all projections retain that accepted result.
        append_terminal_link_suffix(builder, node.id, first, &address, default_name);
    }
    append_inline_nodes(builder, &children[label_end..], default_name);
}

/// CVS `termp_lk_pre()` font-pops after the label, selects NOSPACE, then
/// executes the colon and URI. Annotation never starts another word or
/// settles a pending glyph merely to decide its typed label.
fn append_terminal_link_suffix(
    builder: &mut InlineBuilder,
    owner: u32,
    first: &Node,
    address: &str,
    default_name: Option<&str>,
) {
    let marker = format!(
        "{}:all",
        presentation::scope_marker(owner, presentation::Part::Suffix)
    );
    builder.begin_output_scope(&marker);
    builder.tighten_next_boundary();
    presentation::append_owned_part(
        builder,
        owner,
        presentation::Part::Colon,
        address,
        Some(presentation::Part::Description),
        |builder| builder.append_text(":"),
    );
    presentation::append_owned_part(
        builder,
        owner,
        presentation::Part::Uri,
        address,
        None,
        |builder| append_inline_node(builder, first, default_name),
    );
    // The outer owner keeps separators with their colon/URI interval until
    // the native receipt has decided its accepted range.
    builder.wrap_output_scope(&marker, |children| {
        presentation::wrap_suffix(owner, address, children)
    });
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
            // Pure href decoding can erase a pending zero-advance glyph,
            // but the visible operand still runs as ordinary Mt text.
            // `termp_under_pre()` visits every child; `encode1()` writes
            // the glyph before BACKBEFORE consumes the next word separator.
            append_inline_node(builder, child, default_name);
        } else {
            append_external_link(builder, &address, true, |builder| {
                append_inline_node(builder, child, default_name);
            });
        }
    }
    builder.font.pop_scope(saved);
}

fn append_link_target_or_text(
    builder: &mut InlineBuilder,
    node: &Node,
    address: &str,
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

/// `mdoc_html.c::mdoc__x_pre()` (1553-1581): a `%U` field always carries its
/// decoded argument as an external target; `%R` tests the original first
/// operand for an exact `RFC ` prefix and an all-digit remainder before
/// HTML encoding. The visible word
/// stays the authored argument; only the typed target is enriched.
pub(super) fn append_reference_field_link(
    builder: &mut InlineBuilder,
    children: &[Node],
    default_name: Option<&str>,
    rfc_editor: bool,
) {
    let address = if rfc_editor {
        // The RFC decision precedes print_encode(): visible font or
        // zero-width escapes do not qualify otherwise similar spelling.
        children
            .first()
            .and_then(|child| child.text.as_deref())
            .and_then(rfc_editor_url)
            .unwrap_or_default()
    } else {
        // Identity conversion is pure; the source still executes once in
        // append_inline_nodes below. html.c::print_encode(norecurse=1)
        // strips fonts/zero-width controls when building the href.
        children
            .iter()
            .map(|child| link_identity_text(child.text.as_deref().unwrap_or_default()))
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned()
    };
    if address.is_empty() {
        append_inline_nodes(builder, children, default_name);
    } else {
        append_external_link(builder, &address, false, |builder| {
            append_inline_nodes(builder, children, default_name);
        });
    }
}

/// The upstream HTML expansion rule: `RFC ` plus an all-digit remainder.
/// `mdoc_html.c::mdoc__x_pre()` also accepts an empty remainder: its digit
/// loop runs zero times and still reaches the terminating NUL.
fn rfc_editor_url(argument: &str) -> Option<String> {
    let digits = argument.strip_prefix("RFC ")?;
    digits
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| format!("https://www.rfc-editor.org/rfc/rfc{digits}.html"))
}

/// Wrap newly executed visible content without interrupting the caller's
/// formatter state. A link is an IR annotation around output, not an atomic
/// source fragment: following `Ns`, `\\z`, font, and word-boundary events still
/// observe the same builder state.
fn append_external_link(
    builder: &mut InlineBuilder,
    address: &str,
    email: bool,
    append: impl FnOnce(&mut InlineBuilder),
) {
    let checkpoint = builder.begin_output_checkpoint();
    let pending_visible = builder.zero_advance.pending_visible_characters();
    let previous_owner = builder.zero_advance.pending_native_owner();
    builder.zero_advance.begin_output_owner();
    append(builder);
    let previous_glyph_emitted = builder.zero_advance.end_output_owner();
    builder.zero_advance.bind_pending_link(
        previous_owner,
        external_link_target(address.to_owned(), email),
        true, // This operand always emits its own typed Link below.
    );
    let mut previous_glyph = if previous_glyph_emitted {
        pending_visible
    } else {
        0
    };
    builder.wrap_output_since(checkpoint, |children| {
        let (mut output, children) = split_boundary_prefix(children);
        // CVS term_word()/encode1() may print an earlier operand's cached
        // glyph while this address executes. Its source ownership survives
        // that delay: annotation cannot move it into the address hit range.
        let (prior, children) = split_visible_prefix(children, &mut previous_glyph);
        output.extend(prior);
        output.push(Inline::Link {
            target: external_link_target(address.to_owned(), email),
            title: None,
            children,
        });
        output
    });
}

/// Separate an already-receipted preceding glyph while retaining its styles
/// and zero-width markers. This changes output ownership only; it executes no
/// formatter boundary and does not settle another pending source glyph.
fn split_visible_prefix(nodes: Vec<Inline>, remaining: &mut usize) -> (Vec<Inline>, Vec<Inline>) {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    for node in nodes {
        if *remaining == 0 {
            suffix.push(node);
            continue;
        }
        match node {
            Inline::Text { value } => {
                split_text_prefix(&value, remaining, false, &mut prefix, &mut suffix);
            }
            Inline::Code { value } => {
                split_text_prefix(&value, remaining, true, &mut prefix, &mut suffix);
            }
            Inline::Strong { children } => {
                let (before, after) = split_visible_prefix(children, remaining);
                if !before.is_empty() {
                    prefix.push(Inline::Strong { children: before });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Strong { children: after });
                }
            }
            Inline::Emphasis { children } => {
                let (before, after) = split_visible_prefix(children, remaining);
                if !before.is_empty() {
                    prefix.push(Inline::Emphasis { children: before });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Emphasis { children: after });
                }
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                let (before, after) = split_visible_prefix(children, remaining);
                if !before.is_empty() {
                    prefix.push(Inline::Link {
                        target: target.clone(),
                        title: title.clone(),
                        children: before,
                    });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Link {
                        target,
                        title,
                        children: after,
                    });
                }
            }
            Inline::Equation { .. } => {
                // A pending zero-advance glyph is never a structured equation.
                // Preserve the node intact if an equation precedes the range.
                prefix.push(node);
            }
            Inline::Anchor { .. } | Inline::LineBreak { .. } => prefix.push(node),
        }
    }
    (prefix, suffix)
}

fn split_text_prefix(
    value: &str,
    remaining: &mut usize,
    code: bool,
    prefix: &mut Vec<Inline>,
    suffix: &mut Vec<Inline>,
) {
    let split = value
        .char_indices()
        .find_map(|(index, ch)| {
            if !ch.is_whitespace() {
                *remaining -= 1;
            }
            (*remaining == 0).then_some(index + ch.len_utf8())
        })
        .unwrap_or(value.len());
    let inline = |value: String| {
        if code {
            Inline::Code { value }
        } else {
            Inline::Text { value }
        }
    };
    if split > 0 {
        prefix.push(inline(value[..split].to_owned()));
    }
    if split < value.len() {
        suffix.push(inline(value[split..].to_owned()));
    }
}

/// `InlineBuilder` owns inter-word padding. When a new link begins a word,
/// that padding is appended before its first child. Executed row boundaries
/// and private word anchors can precede it through style wrappers too: keep
/// the full layout prefix outside the clickable label without swallowing it.
fn split_boundary_prefix(children: Vec<Inline>) -> (Vec<Inline>, Vec<Inline>) {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    for node in children {
        if !suffix.is_empty() {
            suffix.push(node);
            continue;
        }
        match node {
            Inline::Text { value } => {
                split_boundary_text(value, false, &mut prefix, &mut suffix);
            }
            Inline::Code { value } => {
                split_boundary_text(value, true, &mut prefix, &mut suffix);
            }
            Inline::Anchor { .. } | Inline::LineBreak { .. } => prefix.push(node),
            Inline::Strong { children } => {
                let (before, after) = split_boundary_prefix(children);
                if !before.is_empty() {
                    prefix.push(Inline::Strong { children: before });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Strong { children: after });
                }
            }
            Inline::Emphasis { children } => {
                let (before, after) = split_boundary_prefix(children);
                if !before.is_empty() {
                    prefix.push(Inline::Emphasis { children: before });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Emphasis { children: after });
                }
            }
            Inline::Link {
                target,
                title,
                children,
            } => {
                let (before, after) = split_boundary_prefix(children);
                prefix.extend(before);
                // A boundary has no clickable label. The typed identity,
                // including an empty private URI receipt, belongs to the
                // remaining source owner and is never cloned onto the row.
                suffix.push(Inline::Link {
                    target,
                    title,
                    children: after,
                });
            }
            other @ Inline::Equation { .. } => suffix.push(other),
        }
    }
    (prefix, suffix)
}

fn split_boundary_text(
    mut value: String,
    code: bool,
    prefix: &mut Vec<Inline>,
    suffix: &mut Vec<Inline>,
) {
    let prefix_len = value
        .char_indices()
        .take_while(|(_, character)| character.is_whitespace())
        .last()
        .map_or(0, |(index, character)| index + character.len_utf8());
    let tail = value.split_off(prefix_len);
    let inline = |value| {
        if code {
            Inline::Code { value }
        } else {
            Inline::Text { value }
        }
    };
    if !value.is_empty() {
        prefix.push(inline(value));
    }
    if !tail.is_empty() {
        suffix.push(inline(tail));
    }
}

/// Extract a link destination from the source spelling without borrowing its
/// rendered glyph stream.  A `\z` glyph participates in terminal overstrike
/// layout but is not part of mdoc's destination: CVS `mdoc_html.c` builds the
/// `Lk`/`Mt` href from the logical operand while terminal rendering consumes
/// the zero-advance glyph in the surrounding output flow.  Keeping this
/// separate from [`visible_text`](super::visible_text) prevents a later sibling from changing a
/// typed destination and lets the label retain its own formatter state.
fn link_identity_text(source: &str) -> String {
    let mut identity = String::new();
    let mut skip_next_glyph = false;
    for event in decode(source) {
        match event {
            RoffInlineEvent::Text(value) => {
                for character in value.chars() {
                    if skip_next_glyph {
                        skip_next_glyph = false;
                    } else {
                        identity.push(character);
                    }
                }
            }
            RoffInlineEvent::Glyph(value) => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else {
                    identity.push_str(&value);
                }
            }
            RoffInlineEvent::BreakableHyphen => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else {
                    identity.push('-');
                }
            }
            RoffInlineEvent::FallbackGlyph(_) => {
                // Unknown and out-of-range glyphs have no HTML codepoint.
                // They remain readable in terminal prose but are omitted by
                // CVS `print_encode(..., norecurse=1)` when building hrefs.
                skip_next_glyph = false;
            }
            RoffInlineEvent::DeviceName => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else {
                    identity.push_str("html");
                }
            }
            RoffInlineEvent::Overstrike { source, .. } => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else if let Some(character) = source.chars().next_back() {
                    identity.push(character);
                }
            }
            RoffInlineEvent::ZeroAdvance => skip_next_glyph = true,
            // CVS HTML_SKIPCHAR survives font changes, but every other
            // formatter escape consumes the pending skip before a later
            // address glyph. In particular, `\\z\\c` must not delete the
            // first character following `\\c` from a typed target.
            RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => {}
            RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::EmptyDestination
            | RoffInlineEvent::LineBreak
            | RoffInlineEvent::NoSpace
            | RoffInlineEvent::Presentation { .. } => skip_next_glyph = false,
        }
    }
    identity
}

/// `man_html.c::man_UR_pre()` supplies the HEAD's first TEXT directly to
/// `print_otag("ch")`; `html.c::print_encode(norecurse=1)` decodes its href
/// independently of the terminal word emitted by `post_UR()`. Both document
/// and recovered inline links use this pure identity path.
pub(in crate::mandoc) fn man_link_identity_text(head: &[Node]) -> String {
    link_identity_text(
        head.first()
            .and_then(|node| node.text.as_deref())
            .unwrap_or_default(),
    )
}

/// Execute the spelling normalized by CVS `mdoc_validate.c::post_bx()`.
///
/// Authored operands retain their controls and wording, including lifecycle
/// names. Generated `BSD`, `Ns`, and an optional release separator join those
/// words; no alternate expansion replaces the accepted source content.
pub(super) fn append_bsd_reference(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    let authored = node
        .children
        .iter()
        .filter(|child| {
            child.kind == NodeKind::Text && !child.flags.generated && !child.flags.no_print
        })
        .collect::<Vec<_>>();
    let Some(first) = authored.first().copied() else {
        builder.append_generated_word("BSD");
        return;
    };
    // Execute positional operands and generated spelling in the caller's one
    // formatter stream. The first authored child is the version even when it
    // contains only controls; every later child is a release/variant word.
    // Visibility must not rewrite that native arity or reorder execution.
    append_inline_node(builder, first, name);
    builder.tighten_next_boundary();
    builder.append_generated_word("BSD");
    for child in &authored[1..] {
        builder.tighten_next_boundary();
        builder.append_generated_word("-");
        builder.tighten_next_boundary();
        let checkpoint = builder.begin_output_checkpoint();
        append_inline_node(builder, child, name);
        if !builder.output_since_has_non_whitespace_glyph(checkpoint) {
            builder.consume_compacted_pending_padding();
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

/// Lower GNU man-ext `.UR` and `.MT` inside a recovered table-cell fragment.
///
/// `source_fragment::inline_request()` admits only a closed inline language
/// here. Ordinary document links use the block driver, which can execute IP,
/// PP, nested links, and fill-mode changes in BODY.
pub(in crate::mandoc) fn append_man_link(
    builder: &mut InlineBuilder,
    node: &Node,
    default_name: Option<&str>,
    no_fill: bool,
) {
    // man_term.c::print_man_node() enters BLOCK, HEAD, and BODY separately.
    // Each entry replaces the current font even when it is already Roman:
    // term_fontrepl() also updates the independent previous-font register.
    builder.font.select(Font::Regular);
    let head = first_part_children(node, NodeKind::Head);
    let target = man_link_identity_text(head);
    let body = first_part_children(node, NodeKind::Body);
    builder.font.select(Font::Regular); // HEAD pre
    builder.font.select(Font::Regular); // HEAD post
    builder.font.select(Font::Regular); // BODY pre
    let link_target = if node.macro_name.as_deref() == Some("MT") {
        mant_ir::LinkTarget::Email {
            address: target.clone(),
        }
    } else {
        mant_ir::LinkTarget::External {
            uri: target.clone(),
        }
    };
    let checkpoint = builder.begin_output_checkpoint();
    let pending_visible = builder.zero_advance.pending_visible_characters();
    builder.zero_advance.begin_output_owner();
    append_man_link_body(builder, body, default_name, no_fill);
    // BODY post resets font before BLOCK post executes its first word. That
    // word's separator may settle a label-final glyph or pending hard break;
    // the typed destination never decides whether this native post executes.
    builder.font.select(Font::Regular);
    builder.prepare_generated_word();
    let mut previous_glyph = if builder.zero_advance.end_output_owner() {
        pending_visible
    } else {
        0
    };
    let head_is_label = body.is_empty() && !target.is_empty();
    if !target.is_empty() && !head_is_label {
        builder.wrap_output_since(checkpoint, |children| {
            let (mut output, children) = split_boundary_prefix(children);
            let (prior, children) = split_visible_prefix(children, &mut previous_glyph);
            output.extend(prior);
            output.push(Inline::Link {
                target: link_target.clone(),
                title: None,
                children,
            });
            output
        });
    }
    // post_UR always executes <, the original HEAD word, and >, including an
    // empty logical href. man_UR_pre instead chooses HEAD as its label only
    // if BODY has no child; control-only BODY must not trigger that fallback.
    builder.append_prepared_text("<");
    builder.tighten_next_boundary();
    if head_is_label {
        builder.append_scope(
            |builder| append_inline_nodes(builder, head, default_name),
            |children| {
                vec![Inline::Link {
                    target: link_target.clone(),
                    title: None,
                    children,
                }]
            },
        );
    } else {
        append_inline_nodes(builder, head, default_name);
    }
    builder.tighten_next_boundary();
    builder.append_text(">");
    append_man_link_body(
        builder,
        first_part_children(node, NodeKind::Tail),
        default_name,
        no_fill,
    );
    builder.font.select(Font::Regular);
}

fn append_man_link_body(
    builder: &mut InlineBuilder,
    nodes: &[Node],
    name: Option<&str>,
    no_fill: bool,
) {
    for (index, child) in nodes.iter().enumerate() {
        if no_fill
            && index > 0
            && child.flags.line_start
            && !builder.final_source_continuation_or(false)
        {
            // man_term.c::print_man_node() applies NODE_LINE before visiting
            // each BODY child, including lines inside UR/MT.
            builder.hard_break();
        }
        append_inline_node(builder, child, name);
    }
}

#[cfg(test)]
mod tests {
    use super::{Inline, link_identity_text, split_boundary_prefix};
    use crate::mandoc::roff_escape::visible_text;

    #[test]
    fn semantic_boundary_prefixes_remain_outside_link_labels() {
        // The exact X\p/ta/Lk é名 source ran pristine first. These wrappers
        // only annotate that already-executed native boundary; they cannot
        // turn the previous physical row into a link-label scalar.
        let children = vec![
            Inline::anchor("\0mant:field-word:0-0"),
            Inline::Emphasis {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::External {
                        uri: "https://example.com".into(),
                    },
                    title: None,
                    children: vec![
                        Inline::line_break(),
                        Inline::Code {
                            value: "é名".into(),
                        },
                    ],
                }],
            },
        ];
        let (before, after) = split_boundary_prefix(children);
        assert_eq!(mant_ir::inline_plain_text(&before), "\n");
        assert_eq!(mant_ir::inline_plain_text(&after), "é名");
        assert!(matches!(after.as_slice(), [Inline::Emphasis { children }]
            if matches!(children.as_slice(), [Inline::Link { children, .. }]
                if matches!(children.as_slice(), [Inline::Code { value }] if value == "é名"))));
        assert!(
            matches!(before.as_slice(), [Inline::Anchor { .. }, Inline::Emphasis { children }]
            if matches!(children.as_slice(), [Inline::LineBreak { .. }]))
        );
    }

    #[test]
    fn overstrike_terminal_projection_and_html_identity_remain_independent() {
        // CVS term.c trims the trailing backspace/blank pair and leaves C in
        // its one-cell projection. html.c deliberately uses the final source
        // byte for the href instead; neither representation can substitute
        // for the other.
        assert_eq!(visible_text(r"\o'BC '"), "C");
        assert_eq!(link_identity_text(r"\o'BC '"), " ");

        // The fixed-CVS overstrike loop exposes the spelling after a nested
        // reverse-solidus while HTML chooses the same final source byte.
        assert_eq!(visible_text(r"\o'BC\fI'"), "I");
        assert_eq!(link_identity_text(r"\o'BC\fI'"), "I");

        // The delimiter belonging to a nested escape is part of the outer
        // source argument.  Only the final quote closes `\o`, so both the
        // terminal and identity projections see the complete operand.
        for source in [r"\o'BC\N'8''", r"\o'BC\h'1n''", r"\o'BC\C'x''"] {
            assert_eq!(visible_text(source), "'", "{source}");
            assert_eq!(link_identity_text(source), "'", "{source}");
        }
    }

    #[test]
    fn numbered_link_identity_uses_the_same_terminal_codepoint_contract() {
        assert_eq!(link_identity_text(r"prefix\N'0'suffix"), "prefix�suffix");
        assert_eq!(
            link_identity_text(r"prefix\N'160'suffix"),
            "prefix\u{a0}suffix"
        );
    }
}
