//! Group only accepted members of one private source occurrence.
use super::{
    HashSet, Inline, LinkTarget,
    markers::{is_annotation_marker, is_target},
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
    let mut anchors = Vec::new();
    for node in lifted {
        if matches!(node, Inline::Anchor { .. } | Inline::LineBreak { .. }) {
            anchors.push(node);
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
                before.append(&mut anchors);
                before.append(&mut children);
            }
            (_, node) => {
                output.append(&mut anchors);
                output.push(node);
            }
        }
    }
    output.append(&mut anchors);
    *nodes = output;
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

pub(super) fn collect_nonempty(nodes: &[Inline], output: &mut HashSet<String>) {
    for node in nodes {
        #[cfg(test)]
        super::super::metrics::note(|counts| counts.normalization += 1);
        match node {
            Inline::Link {
                target: LinkTarget::External { uri },
                children,
                ..
            } if is_target(uri) => {
                if has_scalar(children) {
                    output.insert(uri.clone());
                }
                collect_nonempty(children, output);
            }
            Inline::Link { children, .. }
            | Inline::Strong { children }
            | Inline::Emphasis { children } => collect_nonempty(children, output),
            _ => {}
        }
    }
}

pub(super) fn prune_provisional_identities(nodes: &mut Vec<Inline>, nonempty: &HashSet<String>) {
    let mut output = Vec::with_capacity(nodes.len());
    for mut node in std::mem::take(nodes) {
        #[cfg(test)]
        super::super::metrics::note(|counts| counts.normalization += 1);
        match &mut node {
            Inline::Link { children, .. }
            | Inline::Strong { children }
            | Inline::Emphasis { children } => prune_provisional_identities(children, nonempty),
            _ => {}
        }
        if let Inline::Link {
            target: LinkTarget::External { uri },
            children,
            ..
        } = &mut node
            && nonempty.contains(uri)
            && !has_scalar(children)
        {
            output.append(children);
        } else {
            output.push(node);
        }
    }
    *nodes = output;
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
