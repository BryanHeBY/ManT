//! Dialect-specific link execution, separate from pure target construction.
use super::{
    Font, Inline, InlineBuilder, Node, NodeKind, append_inline_nodes, append_isolated_inline_node,
    first_part_children, inline_children,
};
use crate::definitions::NativeOperandRole;
use libmandoc_rs::{MacroToken::Man, ManMacro};

pub(in crate::mandoc) mod identity;
pub(in crate::mandoc::inline) mod presentation;

mod annotation;
use super::ownership::markers;
use annotation::{append_external_link, source_link_target};
use identity::link_identity_text;

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
    let absent_target = address.is_empty();
    // Resolve eligibility against the authored URI before replacing the
    // destination with a private annotation-pool key.
    let literal_uri = first
        .text
        .as_deref()
        .is_some_and(|source| contiguous_uri_spelling(source, &address));
    let owner = builder
        .zero_advance
        .new_annotation(source_link_target(address, false))
        .key;
    let address = if absent_target {
        String::new()
    } else {
        markers::target_key(owner)
    };
    let label_end = children
        .iter()
        .rposition(|child| !child.flags.delimiter_close)
        .map_or(1, |index| index + 1)
        .max(1);
    let label = &children[1..label_end];
    builder.with_native_operand_role(NativeOperandRole::ReferenceText(None), |builder| {
        if label.is_empty() {
            append_link_target_or_text(builder, first, &address, default_name, literal_uri);
        } else {
            // termp_lk_pre() executes the label, colon and URI before
            // authored trailing delimiters. Treat their punctuation as
            // part of the reference when reading an enclosing declaration.
            let capture =
                builder.zero_advance.annotations.as_ref().is_some_and(
                    super::ownership::annotations::AnnotationPool::captures_link_layout,
                );
            presentation::append_owned_part(
                builder,
                owner,
                if capture {
                    presentation::Part::CapturedDescription
                } else {
                    presentation::Part::Description
                },
                &address,
                None,
                |builder| {
                    if capture {
                        builder.zero_advance.mark_described_native_receiver();
                    }
                    builder.with_font_scope(Font::Emphasis, |builder| {
                        append_inline_nodes(builder, label, default_name);
                    });
                },
            );
            append_terminal_link_suffix(builder, owner, first, &address, default_name);
        }
    });
    append_inline_nodes(builder, &children[label_end..], default_name);
}

/// A Roman font selection changes formatter state but writes no terminal
/// cell (CVS term.c::term_word). Recognize only that complete escape here;
/// other escapes may move, overstrike, or otherwise alter writer ownership.
fn contiguous_uri_spelling(source: &str, address: &str) -> bool {
    let mut bytes = source.as_bytes().iter().copied();
    let mut expected = address.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'\\' {
            if bytes.next() != Some(b'f') || bytes.next() != Some(b'R') {
                return false;
            }
        } else if !byte.is_ascii_graphic() || expected.next() != Some(byte) {
            return false;
        }
    }
    expected.next().is_none()
}

#[cfg(test)]
mod contiguous_uri_tests {
    use super::contiguous_uri_spelling;

    #[test]
    fn only_literal_graphics_and_complete_roman_font_escape_qualify() {
        let uri = "https://z.test";
        for source in [uri, r"https://z.test\fR", r"https://z.\fRtest"] {
            assert!(contiguous_uri_spelling(source, uri), "{source}");
        }
        for source in [
            r"https://z.test\f",
            r"https://z.test\zJ",
            r"https://z.test\h'1n'",
            r"https://z.test\fX",
        ] {
            assert!(!contiguous_uri_spelling(source, uri), "{source}");
        }
    }
}

/// CVS `termp_lk_pre()` font-pops after the label, selects NOSPACE, then
/// executes the colon and URI. Annotation never starts another word or
/// settles a pending glyph merely to decide its typed label.
fn append_terminal_link_suffix(
    builder: &mut InlineBuilder,
    owner: super::ownership::annotations::AnnotationKey,
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
        // term_word(":") can write into the live prefix left by HORIZ,
        // before old glyphs already projected during the description. Use
        // native word replay, not a literal append after the occupied tail.
        |builder| super::word_input::append_normalized_word(builder, ":"),
    );
    presentation::append_owned_part(
        builder,
        owner,
        presentation::Part::Uri,
        address,
        None,
        |builder| append_isolated_inline_node(builder, first, default_name),
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
            append_isolated_inline_node(builder, child, default_name);
            continue;
        }
        builder.with_native_operand_role(NativeOperandRole::ReferenceText(None), |builder| {
            let address = link_identity_text(child.text.as_deref().unwrap_or_default());
            if address.is_empty() {
                // Pure href decoding can erase a pending zero-advance glyph.
                // termp_under_pre() still executes the visible operand.
                append_isolated_inline_node(builder, child, default_name);
            } else {
                append_external_link(builder, &address, true, |builder| {
                    append_isolated_inline_node(builder, child, default_name);
                });
            }
        });
    }
    builder.font.pop_scope(saved);
}

fn append_link_target_or_text(
    builder: &mut InlineBuilder,
    node: &Node,
    address: &str,
    default_name: Option<&str>,
    literal_uri: bool,
) {
    if address.is_empty() {
        // A malformed or control-only target must degrade to ordinary source
        // output. It still owns font and zero-width effects; an invalid URI
        // is not permission to erase its visible label or later state.
        append_isolated_inline_node(builder, node, default_name);
    } else {
        append_external_link(builder, address, false, |builder| {
            if literal_uri {
                builder.zero_advance.mark_contiguous_native_receiver();
            }
            append_isolated_inline_node(builder, node, default_name);
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
        // strips fonts/zero-width controls when building the href. Like
        // mdoc__x_pre(), read only the first TEXT and retain attribute
        // whitespace: trimming it could repair an invalid authored target.
        children
            .first()
            .and_then(|child| child.text.as_deref())
            .map(link_identity_text)
            .unwrap_or_default()
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

/// Build one pure destination without copying its complete source spelling.
pub(in crate::mandoc) fn man_link_destination(
    head: &[Node],
    email: bool,
) -> (mant_ir::LinkTarget, bool) {
    let address = man_link_identity_text(head);
    let present = !address.is_empty();
    (source_link_target(address, email), present)
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
        super::word_input::append_normalized_word(builder, "BSD");
        return;
    };
    // Execute positional operands and generated spelling in the caller's one
    // formatter stream. The first authored child is the version even when it
    // contains only controls; every later child is a release/variant word.
    // Visibility must not rewrite that native arity or reorder execution.
    append_isolated_inline_node(builder, first, name);
    builder.tighten_next_boundary();
    super::word_input::append_normalized_word(builder, "BSD");
    for child in &authored[1..] {
        builder.tighten_next_boundary();
        super::word_input::append_normalized_word(builder, "-");
        builder.tighten_next_boundary();
        let checkpoint = builder.begin_output_checkpoint();
        append_isolated_inline_node(builder, child, name);
        if !builder.output_since_has_non_whitespace_glyph(checkpoint) {
            builder.consume_compacted_pending_padding();
        }
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
    let (link_target, has_target) =
        man_link_destination(head, node.macro_token.as_ref() == Some(&Man(ManMacro::Mt)));
    let body = first_part_children(node, NodeKind::Body);
    builder.font.select(Font::Regular); // HEAD pre
    builder.font.select(Font::Regular); // HEAD post
    builder.font.select(Font::Regular); // BODY pre
    let registered = builder.zero_advance.new_annotation(link_target);
    let link_target = registered.target;
    let head_is_label = body.is_empty() && has_target;
    let (_, membership) = builder.append_semantic_scope(
        registered.key,
        true,
        |builder| {
            append_man_link_body(builder, body, default_name, no_fill);
            builder.font.select(Font::Regular);
            builder.prepare_generated_word();
        },
        |children| {
            if !has_target || head_is_label {
                children
            } else {
                vec![Inline::Link {
                    target: link_target.clone(),
                    title: None,
                    children,
                }]
            }
        },
    );
    if has_target && !head_is_label {
        builder.zero_advance.bind_deferred(
            membership,
            &crate::mandoc::inline::ownership::Annotation::Link {
                target: link_target.clone(),
                title: None,
            },
            true,
        );
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
        append_isolated_inline_node(builder, child, name);
    }
}
