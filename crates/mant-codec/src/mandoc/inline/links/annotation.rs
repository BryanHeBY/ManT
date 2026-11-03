//! Place typed link annotations around their accepted source-owned output.

use super::{Inline, InlineBuilder};

/// Wrap newly executed visible content without interrupting the caller's
/// formatter state. A link is an IR annotation around output, not an atomic
/// source fragment: following `Ns`, `\\z`, font, and word-boundary events still
/// observe the same builder state.
pub(super) fn append_external_link(
    builder: &mut InlineBuilder,
    address: &str,
    email: bool,
    append: impl FnOnce(&mut InlineBuilder),
) {
    let checkpoint = builder.begin_output_checkpoint();
    let pending = builder.zero_advance.pending_output_prefix();
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
        pending.count
    } else {
        0
    };
    builder.wrap_output_since(checkpoint, |children| {
        let (mut output, children) = split_boundary_prefix(children);
        previous_glyph = previous_glyph
            .saturating_sub(output.iter().map(|node| pending.measure.count(node)).sum());
        // CVS term_word()/encode1() may print an earlier operand's cached
        // glyph while this address executes. Its source ownership survives
        // that delay: annotation cannot move it into the address hit range.
        let (prior, children) =
            split_visible_prefix(children, &mut previous_glyph, pending.measure);
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
pub(super) fn split_visible_prefix(
    nodes: Vec<Inline>,
    remaining: &mut usize,
    measure: super::super::PrefixMeasure,
) -> (Vec<Inline>, Vec<Inline>) {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    for node in nodes {
        if *remaining == 0 {
            suffix.push(node);
            continue;
        }
        match node {
            Inline::Text { value } => {
                split_text_prefix(&value, remaining, false, &mut prefix, &mut suffix, measure);
            }
            Inline::Code { value } => {
                split_text_prefix(&value, remaining, true, &mut prefix, &mut suffix, measure);
            }
            Inline::Strong { children } => {
                let (before, after) = split_visible_prefix(children, remaining, measure);
                if !before.is_empty() {
                    prefix.push(Inline::Strong { children: before });
                }
                if !after.is_empty() {
                    suffix.push(Inline::Strong { children: after });
                }
            }
            Inline::Emphasis { children } => {
                let (before, after) = split_visible_prefix(children, remaining, measure);
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
                let (before, after) = split_visible_prefix(children, remaining, measure);
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
    measure: super::super::PrefixMeasure,
) {
    let split = value
        .char_indices()
        .find_map(|(index, ch)| {
            if measure.includes(ch) {
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
pub(super) fn split_boundary_prefix(children: Vec<Inline>) -> (Vec<Inline>, Vec<Inline>) {
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

pub(super) fn external_link_target(address: String, email: bool) -> mant_ir::LinkTarget {
    if email {
        mant_ir::LinkTarget::Email { address }
    } else {
        mant_ir::LinkTarget::External { uri: address }
    }
}

#[cfg(test)]
mod tests {
    use super::{Inline, split_boundary_prefix};

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
}
