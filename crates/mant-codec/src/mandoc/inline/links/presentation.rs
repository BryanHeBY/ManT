//! Private Lk ownership over final accepted native output, never execution.

use std::collections::{HashMap, HashSet};

use super::{
    Inline, InlineBuilder, external_link_target, split_boundary_prefix, split_visible_prefix,
};

const OWNER_PREFIX: &str = "\0mant:lk-presentation:";

#[cfg(test)]
std::thread_local! {
    static PRESENTATION_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_visit() {
    PRESENTATION_VISITS.with(|visits| visits.set(visits.get().saturating_add(1)));
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(super) enum Part {
    Description,
    Colon,
    Suffix,
    Uri,
}

impl Part {
    const fn spelling(self) -> &'static str {
        match self {
            Self::Description => "d",
            Self::Colon => "c",
            Self::Suffix => "s",
            Self::Uri => "u",
        }
    }
}

pub(super) fn scope_marker(owner: u32, part: Part) -> String {
    format!("\0mant:output-scope:lk:{owner}:{}", part.spelling())
}

fn placeholder(owner: u32, part: Part, address: &str, children: Vec<Inline>) -> Inline {
    // This NUL-prefixed target is an internal, invalid public-IR value. It
    // never passes URI admission or executes as an href: finalization below
    // removes it after native acceptance and before semantic recognition.
    Inline::Link {
        target: external_link_target(
            format!("{OWNER_PREFIX}{owner}:{}:{address}", part.spelling()),
            false,
        ),
        // Authored Lk targets cannot supply this matching private title.
        // The pair distinguishes ownership metadata from an invalid authored
        // address which merely resembles the NUL-prefixed target namespace.
        title: Some(scope_marker(owner, part)),
        children,
    }
}

/// Execute once in the caller's state, then bind its new output by a stable
/// local marker. `encode1()` may release the preceding operand's delayed glyph:
/// the zero-advance receipt retains that operand's owner and original style.
pub(super) fn append_owned_part(
    builder: &mut InlineBuilder,
    owner: u32,
    part: Part,
    address: &str,
    preceding: Option<Part>,
    append: impl FnOnce(&mut InlineBuilder),
) {
    let marker = scope_marker(owner, part);
    builder.begin_output_scope(&marker);
    let pending = builder.zero_advance.pending_visible_characters();
    builder.zero_advance.begin_output_owner();
    append(builder);
    let mut previous = if builder.zero_advance.end_output_owner() {
        pending
    } else {
        0
    };
    let mut first_fragment = true;
    builder.wrap_output_scope(&marker, |children| {
        let (mut output, children) = if first_fragment {
            first_fragment = false;
            split_boundary_prefix(children)
        } else {
            (Vec::new(), children)
        };
        let (prior, children) = split_visible_prefix(children, &mut previous);
        if let Some(preceding) = preceding {
            if !prior.is_empty() {
                output.push(placeholder(owner, preceding, address, prior));
            }
        } else {
            output.extend(prior);
        }
        output.push(placeholder(owner, part, address, children));
        output
    });
}

/// A label glyph released by the colon remains a description. Keep that
/// prefix outside the suffix owner; the colon and URI still execute in source
/// order and their accepted native children remain the sole public text.
pub(super) fn wrap_suffix(owner: u32, address: &str, children: Vec<Inline>) -> Vec<Inline> {
    let mut prefix = Vec::new();
    let mut suffix = Vec::new();
    for node in children {
        if suffix.is_empty()
            && (matches!(node, Inline::Anchor { .. } | Inline::LineBreak { .. })
                || metadata(&node).is_some_and(|(_, part, _)| part == Part::Description))
        {
            prefix.push(node);
        } else {
            suffix.push(node);
        }
    }
    prefix.push(placeholder(owner, Part::Suffix, address, suffix));
    prefix
}

fn metadata(node: &Inline) -> Option<(u32, Part, &str)> {
    let Inline::Link {
        target: mant_ir::LinkTarget::External { uri },
        title: Some(title),
        ..
    } = node
    else {
        return None;
    };
    let mut fields = uri.strip_prefix(OWNER_PREFIX)?.splitn(3, ':');
    let owner = fields.next()?.parse().ok()?;
    let part = match fields.next()? {
        "d" => Part::Description,
        "c" => Part::Colon,
        "s" => Part::Suffix,
        "u" => Part::Uri,
        _ => return None,
    };
    (title == &scope_marker(owner, part)).then_some((owner, part, fields.next()?))
}

/// The URI owner carries the same authored destination fact as a typed Link,
/// even when `term_fill()` rejects every native glyph. Preserve that receipt
/// while trimming rejected text; `mdoc_lk_pre()` still emits its authored href.
pub(in crate::mandoc::inline) fn retains_authored_identity(node: &Inline) -> bool {
    metadata(node).is_some_and(|(_, part, address)| part == Part::Uri && !address.is_empty())
}

/// `wrap_output_from()` can split a semantic owner at a real native commit.
/// Retain the existing authored-link seam only for the two potential anchors;
/// a colon/suffix is presentation rather than another link occurrence.
pub(in crate::mandoc::inline) fn same_link_owner(left: &Inline, right: &Inline) -> bool {
    matches!((metadata(left), metadata(right)),
        (Some((left_owner, left_part, left_address)), Some((right_owner, right_part, right_address)))
            if left_owner == right_owner
                && left_part == right_part
                && matches!(left_part, Part::Description | Part::Uri)
                && left_address == right_address)
}

pub(in crate::mandoc::inline) fn is_private_owner(node: &Inline) -> bool {
    metadata(node).is_some()
}

/// Link activation needs an actual readable cell, rather than merely a native
/// graph scalar. Zero-width scalars remain accepted content; they cannot by
/// themselves provide a readable label instead of the authored address.
fn text_is_readable(value: &str) -> bool {
    let mut scalar = [0; 4];
    value.chars().any(|character| {
        !character.is_whitespace()
            && mant_ir::geometry::text_width(character.encode_utf8(&mut scalar)) > 0
    })
}

fn has_readable_text(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| {
        #[cfg(test)]
        note_visit();
        match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                text_is_readable(value)
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => has_readable_text(children),
            Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
        }
    })
}

#[derive(Default)]
struct AcceptedLabel {
    readable: bool,
}

/// Final native field receipts already chose these children. One collection
/// and one rewrite visit the local drained output, not one scan per Lk, and
/// every private placeholder disappears before public IR can be returned.
pub(in crate::mandoc::inline) fn finalize_accepted_links(nodes: &mut Vec<Inline>) {
    let mut labels = HashMap::new();
    collect_labels(nodes, &mut labels, None);
    if !labels.is_empty() {
        resolve_parts(nodes, &labels, &mut HashSet::new());
    }
}

fn collect_labels(
    nodes: &[Inline],
    labels: &mut HashMap<u32, AcceptedLabel>,
    description: Option<u32>,
) {
    for node in nodes {
        #[cfg(test)]
        note_visit();
        let description = if let Some((owner, part, _)) = metadata(node) {
            labels.entry(owner).or_default();
            (part == Part::Description).then_some(owner)
        } else {
            description
        };
        match node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => collect_labels(children, labels, description),
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                if let Some(owner) = description {
                    labels.entry(owner).or_default().readable |= text_is_readable(value);
                }
            }
            Inline::Anchor { .. } | Inline::LineBreak { .. } => {}
        }
    }
}

fn resolve_parts(
    nodes: &mut Vec<Inline>,
    labels: &HashMap<u32, AcceptedLabel>,
    bound: &mut HashSet<(u32, Part)>,
) {
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        #[cfg(test)]
        note_visit();
        let part = metadata(&node).map(|(owner, part, address)| (owner, part, address.to_owned()));
        match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => resolve_parts(children, labels, bound),
            _ => {}
        }
        if let Some((owner, part, address)) = part {
            let Inline::Link { children, .. } = node else {
                unreachable!();
            };
            let label = labels.get(&owner).is_some_and(|label| label.readable);
            match part {
                Part::Description if label && !address.is_empty() => {
                    bind_accepted_link(&mut output, children, address, owner, part, bound);
                }
                Part::Uri if !label && !address.is_empty() => {
                    bind_accepted_link(&mut output, children, address, owner, part, bound);
                }
                _ => output.extend(children),
            }
        } else {
            output.push(node);
        }
    }
    *nodes = output;
}

fn bind_accepted_link(
    output: &mut Vec<Inline>,
    children: Vec<Inline>,
    address: String,
    owner: u32,
    part: Part,
    bound: &mut HashSet<(u32, Part)>,
) {
    if has_readable_text(&children) {
        let (prefix, children) = if bound.insert((owner, part)) {
            split_boundary_prefix(children)
        } else {
            (Vec::new(), children)
        };
        output.extend(prefix);
        output.push(Inline::Link {
            target: external_link_target(address, false),
            title: None,
            children,
        });
    } else {
        // Rejection removes the clickable range, not the authored identity.
        // Resolve it once for this source owner, as an empty typed Link; no
        // rejected glyph, physical row, or alternate label is reconstructed.
        if bound.insert((owner, part)) {
            output.push(Inline::Link {
                target: external_link_target(address, false),
                title: None,
                children,
            });
        } else {
            output.extend(children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Inline {
        Inline::Text {
            value: value.to_owned(),
        }
    }

    #[test]
    fn accepted_annotation_visits_each_local_owner_a_bounded_number_of_times() {
        // IR-only complexity contract: public styles are transparent to the
        // already accepted children. This does not claim a terminal gold for
        // arbitrarily deep roff or increase the producer's nesting budget.
        for owners in [1_u32, 32, 256] {
            for depth in [0, 16, 64] {
                let mut nodes = Vec::new();
                for owner in 0..owners {
                    let mut description = vec![text("label")];
                    for _ in 0..depth {
                        description = vec![Inline::Emphasis {
                            children: description,
                        }];
                    }
                    nodes.push(placeholder(
                        owner,
                        Part::Description,
                        "https://example.com",
                        description,
                    ));
                    nodes.push(placeholder(
                        owner,
                        Part::Suffix,
                        "https://example.com",
                        vec![
                            text(": "),
                            placeholder(
                                owner,
                                Part::Uri,
                                "https://example.com",
                                vec![text("https://example.com")],
                            ),
                        ],
                    ));
                }
                PRESENTATION_VISITS.with(|visits| visits.set(0));
                finalize_accepted_links(&mut nodes);
                let work = PRESENTATION_VISITS.with(std::cell::Cell::get);
                let original_nodes = usize::try_from(owners).unwrap() * (depth + 6);
                assert!(
                    work <= original_nodes * 4,
                    "{owners} owners, depth {depth}: {work}"
                );
                assert_eq!(
                    nodes
                        .iter()
                        .filter(|node| matches!(node, Inline::Link { .. }))
                        .count(),
                    usize::try_from(owners).unwrap()
                );
                assert!(!format!("{nodes:?}").contains("mant:lk-presentation"));
            }
        }
    }

    #[test]
    fn private_ownership_requires_the_matching_non_authored_scope_title() {
        let mut node = placeholder(17, Part::Uri, "https://example.com", vec![text("URI")]);
        assert!(metadata(&node).is_some());
        let Inline::Link { title, .. } = &mut node else {
            panic!("private ownership container");
        };
        *title = None;
        assert!(
            metadata(&node).is_none(),
            "a target alone cannot forge an owner"
        );
        let Inline::Link { title, .. } = &mut node else {
            unreachable!();
        };
        *title = Some(scope_marker(18, Part::Uri));
        assert!(
            metadata(&node).is_none(),
            "another owner cannot lend its proof"
        );
        // This is an intentionally invalid public-IR target for a private
        // metadata unit check, not an admitted public value or a roff oracle.
        // Public validators reject its NUL independently of this detection.
    }
}
