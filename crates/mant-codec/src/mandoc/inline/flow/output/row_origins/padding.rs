//! Preserve the source boundary of late native device padding.

use super::Inline;

/// Keep device padding distinct until leading annotation boundaries have
/// returned. No authored scalar is identified as padding by its appearance.
pub(super) enum ProjectedNode {
    Content { node: Inline, boundary: bool },
    Position(Inline),
    FieldPadding(usize),
}

impl ProjectedNode {
    pub(super) fn content(node: Inline) -> Self {
        let boundary = match &node {
            Inline::Anchor { .. } => false,
            Inline::Text { value } | Inline::Code { value } => !value.is_empty(),
            _ => true,
        };
        Self::Content { node, boundary }
    }

    pub(super) fn is_source(&self) -> bool {
        matches!(self, Self::Content { .. })
    }

    pub(super) fn materialize(self) -> Inline {
        match self {
            Self::Content { node, .. } | Self::Position(node) => node,
            Self::FieldPadding(cells) => Inline::Text {
                value: " ".repeat(cells),
            },
        }
    }
}

/// A field's first device gap precedes the annotation, like `term_field`'s
/// advance before its first letter (term.c:389-427). Once real content or a
/// hard row has occurred, retain the existing owner: moving an internal gap
/// outside a Link would require splitting that Link's identity.
pub(super) fn project_annotation(
    mut node: Inline,
    children: Vec<ProjectedNode>,
    output: &mut Vec<ProjectedNode>,
) {
    let mut projected = Vec::with_capacity(children.len());
    let mut boundary = false;
    for child in children {
        #[cfg(test)]
        super::ANNOTATION_NODES_PROJECTED.with(|nodes| nodes.set(nodes.get().saturating_add(1)));
        match child {
            ProjectedNode::FieldPadding(_) if !boundary => output.push(child),
            ProjectedNode::Content {
                boundary: content, ..
            } => {
                boundary |= content;
                projected.push(child.materialize());
            }
            ProjectedNode::Position(_) | ProjectedNode::FieldPadding(_) => {
                projected.push(child.materialize());
            }
        }
    }
    let (Inline::Strong { children }
    | Inline::Emphasis { children }
    | Inline::Link { children, .. }) = &mut node
    else {
        unreachable!("only annotations have projected children")
    };
    *children = projected;
    output.push(ProjectedNode::Content { node, boundary });
}
