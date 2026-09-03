//! Conserves validated libmandoc navigation targets across structural lowering.
//!
//! libmandoc deliberately moves `NODE_ID` from a visible macro onto the
//! `Block`, `Head`, or `Body` wrapper that represents its rendered position.
//! Those wrappers are otherwise transparent in `mant-ir`, so target discovery
//! and placement must happen before their structure is discarded.

use std::collections::HashSet;

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Block, Inline, LayoutHint, SourceSpan};

use super::roff_escape::visible_text;

/// Return the validated target owned by one exact AST node.
pub(super) fn node_target(node: &Node, fallback: Option<&str>) -> Option<String> {
    if !node.flags.deep_link_target {
        return None;
    }
    let target = node
        .tag
        .as_deref()
        .map(visible_text)
        .or_else(|| fallback.map(visible_text))?;
    let target = target.trim();
    (!target.is_empty()).then(|| target.to_owned())
}

/// Return the first source token used by libmandoc when a target has no tag.
pub(super) fn raw_target(node: &Node) -> Option<String> {
    node_target(node, first_text(node))
}

/// Collect targets owned by a structural node or one of its direct parts.
///
/// Only direct `Head`/`Body`/`Tail` wrappers belong to this construct. A
/// recursive search would steal a target from an independently lowered child.
pub(super) fn structural_targets(node: &Node) -> Vec<String> {
    if !matches!(
        node.macro_name.as_deref(),
        Some("Pp" | "Bd" | "D1" | "Dl" | "Bl")
    ) {
        return Vec::new();
    }
    node_and_part_targets(node)
}

/// Collect targets moved onto an mdoc list item's structural wrappers.
pub(super) fn item_targets(node: &Node) -> Vec<String> {
    if node.macro_name.as_deref() != Some("It") {
        return Vec::new();
    }
    node_and_part_targets(node)
}

/// Return a target moved onto a section block or its head.
pub(super) fn section_target(node: &Node) -> Option<String> {
    std::iter::once(node)
        .chain(
            node.children
                .iter()
                .filter(|child| child.kind == NodeKind::Head),
        )
        .find_map(raw_target)
}

/// Return a target owned by one direct structural part.
pub(super) fn part_target(node: &Node, kind: NodeKind, fallback: &str) -> Option<String> {
    node.children
        .iter()
        .filter(|child| child.kind == kind)
        .find_map(|owner| node_target(owner, Some(fallback)))
}

/// Attach zero-width targets to the first addressable descendant.
///
/// The current v0.11 IR intentionally has no identity field on every block
/// variant. Anchors therefore live in the first inline-bearing descendant. If
/// a structure contains no such descendant, an anchor-only paragraph retains
/// the destination without adding visible text or spacing.
pub(super) fn attach_targets(
    blocks: &mut Vec<Block>,
    targets: impl IntoIterator<Item = String>,
    layout: LayoutHint,
    source: Option<SourceSpan>,
) {
    let mut targets = targets
        .into_iter()
        .filter(|target| !contains_anchor(blocks, target))
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }
    if prepend_to_first_descendant(blocks, &targets) {
        return;
    }
    let children = targets.drain(..).map(Inline::anchor).collect();
    let insertion = blocks
        .iter()
        .position(|block| !matches!(block, Block::VerticalSpace { .. }))
        .unwrap_or(blocks.len());
    blocks.insert(
        insertion,
        Block::Paragraph {
            children,
            layout,
            source,
        },
    );
}

fn prepend_to_first_descendant(blocks: &mut [Block], targets: &[String]) -> bool {
    for block in blocks {
        match block {
            Block::VerticalSpace { .. } => {}
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                prepend_inlines(children, targets);
                return true;
            }
            Block::List { items, .. } => {
                let Some(item) = items.first_mut() else {
                    return false;
                };
                if prepend_to_first_descendant(&mut item.blocks, targets) {
                    return true;
                }
                return false;
            }
            Block::DefinitionList { items, .. } => {
                let Some(item) = items.first_mut() else {
                    return false;
                };
                if let Some(term) = item.terms.first_mut() {
                    prepend_inlines(term, targets);
                    return true;
                }
                if prepend_to_first_descendant(&mut item.description, targets) {
                    return true;
                }
                return false;
            }
            Block::Table { rows, .. } => {
                let Some(cell) = rows.first_mut().and_then(|row| row.cells.first_mut()) else {
                    return false;
                };
                if prepend_to_first_descendant(&mut cell.blocks, targets) {
                    return true;
                }
                return false;
            }
            Block::Equation { .. } | Block::ThematicBreak { .. } | Block::Unsupported { .. } => {
                return false;
            }
        }
    }
    false
}

fn prepend_inlines(children: &mut Vec<Inline>, targets: &[String]) {
    children.splice(0..0, targets.iter().cloned().map(Inline::anchor));
}

fn contains_anchor(blocks: &[Block], target: &str) -> bool {
    blocks.iter().any(|block| match block {
        Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
            inlines_contain_anchor(children, target)
        }
        Block::List { items, .. } => items
            .iter()
            .any(|item| contains_anchor(&item.blocks, target)),
        Block::DefinitionList { items, .. } => items.iter().any(|item| {
            item.terms
                .iter()
                .any(|term| inlines_contain_anchor(term, target))
                || contains_anchor(&item.description, target)
        }),
        Block::Table { rows, .. } => rows.iter().any(|row| {
            row.cells
                .iter()
                .any(|cell| contains_anchor(&cell.blocks, target))
        }),
        Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => false,
    })
}

fn inlines_contain_anchor(nodes: &[Inline], target: &str) -> bool {
    nodes.iter().any(|node| match node {
        Inline::Anchor { id, .. } => id == target,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => inlines_contain_anchor(children, target),
        Inline::Text { .. } | Inline::Code { .. } | Inline::LineBreak => false,
    })
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Text && !node.flags.no_print {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

fn node_and_part_targets(node: &Node) -> Vec<String> {
    let mut targets = Vec::new();
    let mut seen = HashSet::new();
    for owner in std::iter::once(node).chain(
        node.children
            .iter()
            .filter(|child| matches!(child.kind, NodeKind::Head | NodeKind::Body | NodeKind::Tail)),
    ) {
        if let Some(target) = raw_target(owner)
            && seen.insert(target.clone())
        {
            targets.push(target);
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, Inline, LayoutHint, ListItem};

    #[test]
    fn target_attachment_descends_into_nested_lists() {
        let mut blocks = vec![Block::List {
            kind: mant_ir::ListKind::Bullet,
            start: None,
            compact: false,
            items: vec![ListItem {
                blocks: vec![Block::Preformatted {
                    children: vec![Inline::Text {
                        value: "body".to_owned(),
                    }],
                    language: None,
                    layout: LayoutHint::default(),
                    source: None,
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        super::attach_targets(
            &mut blocks,
            ["nested-target".to_owned()],
            LayoutHint::default(),
            None,
        );

        let Block::List { items, .. } = &blocks[0] else {
            panic!("list");
        };
        let Block::Preformatted { children, .. } = &items[0].blocks[0] else {
            panic!("preformatted child");
        };
        assert!(matches!(
            children.first(),
            Some(Inline::Anchor { id, .. }) if id == "nested-target"
        ));
    }

    #[test]
    fn empty_structures_receive_only_zero_width_content() {
        let mut blocks = Vec::new();
        super::attach_targets(
            &mut blocks,
            ["empty-target".to_owned()],
            LayoutHint::default(),
            None,
        );

        assert!(matches!(
            blocks.as_slice(),
            [Block::Paragraph { children, .. }]
                if matches!(children.as_slice(), [Inline::Anchor { id, .. }] if id == "empty-target")
        ));
    }
}
