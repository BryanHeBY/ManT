//! Group only accepted members of one private source occurrence.
use crate::mandoc::inline::has_rendered_formatter_glyph;

use super::{
    HashSet, Inline, LinkTarget,
    markers::{COVERED_ANNOTATION, is_annotation_marker, is_target},
};

/// Join by source key, not destination equality. This forward pass moves
/// accepted children once and does not rescan or remove a growing prefix.
pub(super) fn coalesce(nodes: &mut Vec<Inline>) {
    let mut lifted = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        #[cfg(test)]
        super::super::metrics::note(|counts| counts.normalization += 1);
        match &mut node {
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => coalesce(children),
            _ => {}
        }
        if let Inline::Link {
            target: LinkTarget::External { uri },
            children,
            ..
        } = &mut node
            && is_target(uri)
        {
            let mut flat = Vec::with_capacity(children.len());
            for child in std::mem::take(children) {
                match child {
                    Inline::Link {
                        target: LinkTarget::External { uri: inner },
                        children,
                        ..
                    } if inner == *uri => flat.extend(children),
                    node => flat.push(node),
                }
            }
            *children = flat;
        }
        match node {
            Inline::Strong { children } => lift_style(children, true, &mut lifted),
            Inline::Emphasis { children } => lift_style(children, false, &mut lifted),
            node => lifted.push(node),
        }
    }
    let mut output: Vec<Inline> = Vec::with_capacity(lifted.len());
    let mut between = Vec::new();
    for node in lifted {
        if matches!(node, Inline::Anchor { .. } | Inline::LineBreak { .. }) || is_empty_style(&node)
        {
            between.push(node);
            continue;
        }
        match (output.last_mut(), node) {
            (
                Some(Inline::Link {
                    target: LinkTarget::External { uri: previous },
                    children: before,
                    ..
                }),
                Inline::Link {
                    target: LinkTarget::External { uri },
                    mut children,
                    ..
                },
            ) if is_target(previous) && *previous == uri => {
                before.append(&mut between);
                before.append(&mut children);
            }
            (_, node) => {
                output.append(&mut between);
                output.push(node);
            }
        }
    }
    output.append(&mut between);
    *nodes = output;
}

fn is_empty_style(node: &Inline) -> bool {
    // A covered glyph can leave a style around empty text. Only that
    // paint-free shape can join the fragments: a space, anchor, break, or
    // separately owned link inside a shell still belongs at its position.
    match node {
        Inline::Strong { children } | Inline::Emphasis { children } => {
            children.iter().all(|child| match child {
                Inline::Text { value } => value.is_empty(),
                child => is_empty_style(child),
            })
        }
        _ => false,
    }
}

fn lift_style(children: Vec<Inline>, strong: bool, output: &mut Vec<Inline>) {
    let wrap = |children| {
        if strong {
            Inline::Strong { children }
        } else {
            Inline::Emphasis { children }
        }
    };
    let mut ordinary = Vec::new();
    for node in children {
        match node {
            Inline::Link {
                target: LinkTarget::External { uri },
                title,
                children,
            } if is_target(&uri) => {
                if !ordinary.is_empty() {
                    output.push(wrap(std::mem::take(&mut ordinary)));
                }
                output.push(Inline::Link {
                    target: LinkTarget::External { uri },
                    title,
                    children: vec![wrap(children)],
                });
            }
            node => ordinary.push(node),
        }
    }
    if !ordinary.is_empty() {
        output.push(wrap(ordinary));
    }
}

pub(super) fn compact_styles(nodes: &mut Vec<Inline>) {
    let mut output: Vec<Inline> = Vec::with_capacity(nodes.len());
    for node in std::mem::take(nodes) {
        match (output.last_mut(), node) {
            (Some(Inline::Text { value: before }), Inline::Text { value })
            | (Some(Inline::Code { value: before }), Inline::Code { value }) => {
                before.push_str(&value);
            }
            (Some(Inline::Strong { children: before }), Inline::Strong { children })
            | (Some(Inline::Emphasis { children: before }), Inline::Emphasis { children }) => {
                before.extend(children);
            }
            (_, node) => output.push(node),
        }
    }
    *nodes = output;
}

fn covered_without_glyphs(
    uri: &str,
    title: Option<&str>,
    children: &[Inline],
    drop_if_covered: &impl Fn(&str) -> bool,
) -> bool {
    title == Some(COVERED_ANNOTATION)
        && drop_if_covered(uri)
        && !has_rendered_formatter_glyph(children)
}

pub(super) fn collect_nonempty(
    nodes: &[Inline],
    output: &mut HashSet<String>,
    drop_if_covered: &impl Fn(&str) -> bool,
) {
    for node in nodes {
        #[cfg(test)]
        super::super::metrics::note(|counts| counts.normalization += 1);
        match node {
            Inline::Link {
                target: LinkTarget::External { uri },
                children,
                title,
            } if is_target(uri) => {
                if has_scalar(children)
                    && !covered_without_glyphs(uri, title.as_deref(), children, drop_if_covered)
                {
                    output.insert(uri.clone());
                }
                collect_nonempty(children, output, drop_if_covered);
            }
            Inline::Link { children, .. }
            | Inline::Strong { children }
            | Inline::Emphasis { children } => collect_nonempty(children, output, drop_if_covered),
            _ => {}
        }
    }
}

/// Find actual accepted paint for each tty link occurrence, across final
/// inline roots. This comparison is by the private source key, not address,
/// so independently authored links sharing a destination remain independent.
pub(super) fn source_glyph_labels(
    blocks: &[mant_ir::Block],
    sections: &[mant_ir::Section],
    is_glyph_link: &impl Fn(&str) -> bool,
) -> HashSet<String> {
    use mant_ir::visit::{self, Visit};
    struct Labels<'a, F> {
        is_glyph_link: &'a F,
        keys: HashSet<String>,
    }
    impl<'ir, F: Fn(&str) -> bool> Visit<'ir> for Labels<'_, F> {
        fn visit_inline(&mut self, node: &'ir Inline) {
            if let Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            } = node
                && (self.is_glyph_link)(uri)
                && has_rendered_formatter_glyph(children)
            {
                self.keys.insert(uri.clone());
            }
            visit::walk_inline(self, node);
        }
    }
    let mut labels = Labels {
        is_glyph_link,
        keys: HashSet::new(),
    };
    for block in blocks {
        labels.visit_block(block);
    }
    for section in sections {
        labels.visit_section(section);
    }
    labels.keys
}

pub(super) fn prune_provisional_identities(
    nodes: &mut Vec<Inline>,
    has_nonempty: &impl Fn(&str) -> bool,
    drop_if_covered: &impl Fn(&str) -> bool,
) {
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        #[cfg(test)]
        super::super::metrics::note(|counts| counts.normalization += 1);
        match &mut node {
            Inline::Link { children, .. }
            | Inline::Strong { children }
            | Inline::Emphasis { children } => {
                prune_provisional_identities(children, has_nonempty, drop_if_covered);
            }
            _ => {}
        }
        if let Inline::Link {
            target: LinkTarget::External { uri },
            title,
            children,
        } = &mut node
            && ((has_nonempty(uri) && !has_scalar(children))
                || covered_without_glyphs(uri, title.as_deref(), children, drop_if_covered))
        {
            output.append(children);
        } else {
            output.push(node);
        }
    }
    *nodes = output;
}

#[cfg(test)]
#[test]
fn covered_members_leave_a_separate_empty_destination_intact() {
    let uri = super::markers::target_key(0);
    let fragment = |title, children| Inline::Link {
        target: LinkTarget::External { uri: uri.clone() },
        title,
        children,
    };
    let rejected = fragment(None, Vec::new());
    let covered = fragment(
        Some(COVERED_ANNOTATION.to_owned()),
        vec![Inline::anchor("witness"), Inline::line_break()],
    );
    let mut nodes = vec![rejected.clone(), covered];
    let drop_if_covered = |key: &str| key == uri;
    let mut nonempty = HashSet::new();
    collect_nonempty(&nodes, &mut nonempty, &drop_if_covered);
    assert!(nonempty.is_empty());
    prune_provisional_identities(&mut nodes, &|uri| nonempty.contains(uri), &drop_if_covered);
    assert_eq!(
        nodes,
        [rejected, Inline::anchor("witness"), Inline::line_break()]
    );
}

#[cfg(test)]
#[test]
fn a_style_shell_with_an_author_anchor_or_visible_space_keeps_its_boundary() {
    let fragment = |glyph: &str| Inline::Link {
        target: LinkTarget::External {
            uri: super::markers::target_key(0),
        },
        title: None,
        children: vec![Inline::Text {
            value: glyph.to_owned(),
        }],
    };
    for member in [
        Inline::anchor("authored"),
        Inline::Text { value: " ".into() },
    ] {
        let mut nodes = vec![
            fragment("J"),
            Inline::Emphasis {
                children: vec![member],
            },
            fragment("K"),
        ];
        let expected = nodes.clone();
        coalesce(&mut nodes);
        assert_eq!(nodes, expected);
    }
}

#[cfg(test)]
pub(super) fn target_bytes(target: &LinkTarget) -> usize {
    match target {
        LinkTarget::External { uri } => uri.len(),
        LinkTarget::Email { address } => address.len(),
        LinkTarget::Section { id } => id.len(),
        LinkTarget::Manual {
            name,
            manual_section,
        } => name.len() + manual_section.as_ref().map_or(0, String::len),
        LinkTarget::Document { name, fragment } => {
            name.len() + fragment.as_ref().map_or(0, String::len)
        }
    }
}

pub(super) fn has_scalar(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| {
        #[cfg(test)]
        super::super::metrics::note(|counts| counts.normalization += 1);
        match node {
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                !value.is_empty()
            }
            Inline::LineBreak { .. } => true,
            Inline::Anchor { .. } => false,
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => has_scalar(children),
        }
    })
}

pub(super) fn has_annotation(nodes: &[Inline]) -> bool {
    nodes.iter().any(|node| {
        #[cfg(test)]
        super::super::metrics::note(|counts| counts.normalization += 1);
        match node {
            Inline::Anchor { id, .. } => is_annotation_marker(id.as_str()),
            Inline::Link {
                target: LinkTarget::External { uri },
                ..
            } if is_target(uri) => true,
            Inline::Link { children, .. }
            | Inline::Strong { children }
            | Inline::Emphasis { children } => has_annotation(children),
            _ => false,
        }
    })
}
