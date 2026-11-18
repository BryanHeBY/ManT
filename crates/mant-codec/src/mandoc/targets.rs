//! Conserves validated libmandoc navigation targets across structural lowering.
//!
//! libmandoc deliberately moves `NODE_ID` from a visible macro onto the
//! `Block`, `Head`, or `Body` wrapper that represents its rendered position.
//! Those wrappers are otherwise transparent in `mant-ir`, so target discovery
//! and placement must happen before their structure is discarded.

use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};
use std::collections::HashSet;

use libmandoc_rs::{Node, NodeKind};
use mant_ir::{Block, DefinitionItem, Inline, LayoutHint, SourceSpan};

use super::roff_escape::visible_text;

mod owned;
pub(super) use owned::{OwnedTarget, PendingTargets};

/// Document-wide target facts that must be known before structural lowering.
///
/// libmandoc retains authored tag provenance on the selected owner even if
/// validation rewrites the argument of an argument-less `.Tg`. Computing
/// the namespace once keeps section allocation, anchor normalization and
/// semantic discovery on the same target policy.
#[derive(Debug)]
pub(super) struct NativeTargetPlan {
    /// Authored fragment namespace, including permitted removed-owner recovery.
    explicit: HashSet<String>,
    /// Surviving `NODE_ID` facts; this is not an executed-output namespace.
    validated_owners: HashSet<String>,
}

impl NativeTargetPlan {
    pub(super) fn build(root: &Node) -> Self {
        let mut plan = Self {
            explicit: HashSet::new(),
            validated_owners: HashSet::new(),
        };
        let mut lists = Vec::new();
        plan.collect(root, &mut lists);

        // The list stream also recovers authored targets whose empty owner was
        // removed by native validation. Reserve exactly that recovery set so
        // normalization preserves their original fragment spellings as well.
        for list in lists {
            plan.explicit.extend(
                list.children
                    .iter()
                    .filter_map(|child| list_stream_target(child, &plan.validated_owners)),
            );
        }
        plan
    }

    fn collect<'node>(&mut self, node: &'node Node, lists: &mut Vec<&'node Node>) {
        if let Some(target) = raw_target(node) {
            // A surviving Tg is also its own provenance for older owned
            // snapshots. Relocated IDs require the actual native receipt.
            if node.flags.authored_tag || node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Tg)) {
                self.explicit.insert(target.clone());
            }
            self.validated_owners.insert(target);
        }
        if node.kind == NodeKind::Body && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Bl)) {
            lists.push(node);
        }
        for child in &node.children {
            self.collect(child, lists);
        }
    }

    pub(super) fn explicit(&self) -> &HashSet<String> {
        &self.explicit
    }

    pub(super) fn validated_owners(&self) -> &HashSet<String> {
        &self.validated_owners
    }
}

/// Destination consumed by the list's pending-target stream. A validated
/// argument-less Tg is authoritative; only an actual authored argument may
/// recover a target discarded with an empty native item. A surviving native
/// owner takes precedence over recovery: `mdoc_validate.c::post_tg()` hides
/// the Tg when it moves `NODE_ID` to a following macro. NOPRT alone is not proof
/// that ownership survived, because `post_it()` can delete an empty column
/// item afterwards. Never revive stale unflagged argument-less requests.
pub(super) fn list_stream_target(
    node: &Node,
    validated_owners: &HashSet<String>,
) -> Option<String> {
    if node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Tg)) {
        return None;
    }
    raw_target(node).or_else(|| {
        explicit_target_argument(node).filter(|target| !validated_owners.contains(target))
    })
}

/// Return the first source token used by libmandoc when a target has no tag.
pub(super) fn raw_target(node: &Node) -> Option<String> {
    if node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::Tg)) {
        return node
            .flags
            .deep_link_target
            .then(|| {
                explicit_target_argument(node).or_else(|| node.tag.as_deref().map(visible_text))
            })
            .flatten();
    }
    if !node.flags.deep_link_target {
        return None;
    }
    let target = node
        .tag
        .as_deref()
        .map(visible_text)
        .or_else(|| source_token(node))?;
    let target = target.trim();
    (!target.is_empty()).then(|| target.to_owned())
}

/// Return the first printable source token used by automatic mdoc targets.
pub(super) fn source_token(node: &Node) -> Option<String> {
    let value = visible_text(first_text(node)?);
    value
        .split_whitespace()
        .next()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Return only the destination written as an argument to an explicit `.Tg`.
///
/// For an argument-less request, libmandoc can store a previously active
/// automatic tag on the `.Tg` node itself while placing the destination
/// derived from the following macro on that following node. Callers that need
/// to distinguish authored arguments from derived destinations must therefore
/// not consult `node.tag`. In contrast, a validated `NODE_ID` on the `.Tg`
/// itself is an actual native owner: `raw_target` retains that destination,
/// without claiming that its spelling was written as an explicit argument.
pub(super) fn explicit_target_argument(node: &Node) -> Option<String> {
    if node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::Tg)) {
        return None;
    }
    target_argument(node.children.first()?)
}

fn target_argument(operand: &Node) -> Option<String> {
    if operand.kind != NodeKind::Text {
        return None;
    }
    let spelling = operand.decoder_text()?;
    if spelling.is_empty() || spelling.contains([' ', '\t', '\\']) {
        return None;
    }
    operand.text.as_deref().map(visible_text)
}

/// Collect targets owned by a structural node or one of its direct parts.
///
/// Only direct `Head`/`Body`/`Tail` wrappers belong to this construct. A
/// recursive search would steal a target from an independently lowered child.
pub(super) fn structural_targets(node: &Node) -> Vec<String> {
    structural_target_owners(node)
        .into_iter()
        .map(|target| target.name)
        .collect()
}

pub(super) fn structural_target_owners(node: &Node) -> Vec<OwnedTarget> {
    if !matches!(
        node.macro_token.as_ref(),
        Some(Mdoc(
            MdocMacro::Pp | MdocMacro::Bd | MdocMacro::D1 | MdocMacro::Dl | MdocMacro::Bl
        ))
    ) {
        return Vec::new();
    }
    node_and_part_targets(node)
}

/// Collect targets moved onto an mdoc list item's structural wrappers.
pub(super) fn item_targets(node: &Node) -> Vec<String> {
    item_target_owners(node)
        .into_iter()
        .map(|target| target.name)
        .collect()
}

pub(super) fn item_target_owners(node: &Node) -> Vec<OwnedTarget> {
    if node.macro_token.as_ref() != Some(&Mdoc(MdocMacro::It)) {
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
///
/// The fallback must come from the source node rather than its rendered IR.
/// Renderer spacing and decoration are presentation policy and cannot change
/// the identity selected by libmandoc.
pub(super) fn part_target(node: &Node, kind: NodeKind) -> Option<String> {
    part_target_with_source(node, kind).map(|target| target.name)
}

/// Return a direct structural part's target together with its native owner.
///
/// The wrapper line is semantically significant even when lowering embeds the
/// resulting anchor inside a surrounding paragraph or definition. Keeping it
/// here prevents that container's line from being mistaken for the target's
/// actual landing point by occurrence-aware audits and downstream consumers.
pub(super) fn part_target_with_source(node: &Node, kind: NodeKind) -> Option<OwnedTarget> {
    node.children
        .iter()
        .filter(|child| child.kind == kind)
        .find_map(|child| {
            raw_target(child).map(|target| OwnedTarget::new(target, super::source_span(child)))
        })
}

/// Attach zero-width targets to the first addressable descendant.
///
/// The current v0.13 IR intentionally has no identity field on every block
/// variant. Anchors therefore live in the first inline-bearing descendant. If
/// a structure contains no such descendant, an anchor-only paragraph retains
/// the destination without adding visible text or spacing.
pub(super) fn attach_targets(
    blocks: &mut Vec<Block>,
    targets: impl IntoIterator<Item = String>,
    layout: LayoutHint,
    source: Option<SourceSpan>,
) {
    let mut seen = HashSet::new();
    let mut targets = targets
        .into_iter()
        .filter(|target| seen.insert(target.clone()) && !contains_anchor(blocks, target))
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }
    if prepend_to_first_descendant(blocks, &targets, source) {
        return;
    }
    let children = targets
        .drain(..)
        .map(|target| Inline::anchor_at(target, source))
        .collect();
    let insertion = blocks
        .iter()
        .position(|block| !matches!(block, Block::VerticalSpace { .. }))
        .unwrap_or(blocks.len());
    blocks.insert(
        insertion,
        Block::Paragraph {
            inline_layout: mant_ir::InlineLayout::default(),
            children,
            layout,
            source,
        },
    );
}

/// Attach targets to a definition term, falling back to its description.
///
/// An empty mdoc `.It` has no visible term, but its authored target must still
/// survive as zero-width content. Keeping that anchor in the item prevents it
/// from being reassigned to a neighbouring definition.
pub(super) fn attach_definition_targets(
    item: &mut DefinitionItem,
    targets: impl IntoIterator<Item = String>,
    source: Option<SourceSpan>,
) {
    let mut seen = HashSet::new();
    let targets = targets
        .into_iter()
        .filter(|target| {
            seen.insert(target.clone())
                && !item
                    .terms
                    .iter()
                    .any(|term| inlines_contain_anchor(term, target))
                && !contains_anchor(&item.description, target)
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }
    if let Some(term) = item.terms.first_mut() {
        prepend_inlines(&mut term.content, &targets, source);
    } else if prepend_to_first_descendant(&mut item.description, &targets, source) {
    } else {
        item.terms.push(
            targets
                .into_iter()
                .map(|target| Inline::anchor_at(target, source))
                .collect(),
        );
        item.head_body_relation = mant_ir::HeadBodyRelation::from(true);
    }
}

/// Attach targets after the final addressable descendant of a definition.
pub(super) fn append_definition_targets(
    item: &mut DefinitionItem,
    targets: impl IntoIterator<Item = String>,
    layout: LayoutHint,
    source: Option<SourceSpan>,
) {
    let mut seen = HashSet::new();
    let targets = targets
        .into_iter()
        .filter(|target| {
            seen.insert(target.clone())
                && !item
                    .terms
                    .iter()
                    .any(|term| inlines_contain_anchor(term, target))
                && !contains_anchor(&item.description, target)
        })
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }
    if !item.description.is_empty() {
        append_targets(&mut item.description, targets, layout, source);
    } else if let Some(term) = item.terms.last_mut() {
        term.content.extend(
            targets
                .into_iter()
                .map(|target| Inline::anchor_at(target, source)),
        );
    } else {
        item.terms.push(
            targets
                .into_iter()
                .map(|target| Inline::anchor_at(target, source))
                .collect(),
        );
        item.head_body_relation = mant_ir::HeadBodyRelation::from(true);
    }
}

/// Attach targets after the final addressable descendant in a block sequence.
pub(super) fn append_targets(
    blocks: &mut Vec<Block>,
    targets: impl IntoIterator<Item = String>,
    layout: LayoutHint,
    source: Option<SourceSpan>,
) {
    let mut seen = HashSet::new();
    let targets = targets
        .into_iter()
        .filter(|target| seen.insert(target.clone()) && !contains_anchor(blocks, target))
        .collect::<Vec<_>>();
    if targets.is_empty() {
        return;
    }
    if append_to_last_descendant(blocks, &targets, source) {
        return;
    }
    blocks.push(Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children: targets
            .into_iter()
            .map(|target| Inline::anchor_at(target, source))
            .collect(),
        layout,
        source,
    });
}

/// Collect zero-width anchor identities nested in an inline sequence.
pub(super) fn inline_anchor_ids(nodes: &[Inline], output: &mut Vec<String>) {
    for node in nodes {
        match node {
            Inline::Anchor { id, .. } => output.push(id.to_string()),
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => inline_anchor_ids(children, output),
            Inline::Text { .. }
            | Inline::Code { .. }
            | Inline::Equation { .. }
            | Inline::LineBreak { .. } => {}
        }
    }
}

/// Return the first structural owner location carried by an inline anchor.
pub(super) fn inline_anchor_owner_source(nodes: &[Inline]) -> Option<SourceSpan> {
    nodes.iter().find_map(|node| match node {
        Inline::Anchor { owner_source, .. } => *owner_source,
        Inline::Strong { children }
        | Inline::Emphasis { children }
        | Inline::Link { children, .. } => inline_anchor_owner_source(children),
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Equation { .. }
        | Inline::LineBreak { .. } => None,
    })
}

fn prepend_to_first_descendant(
    blocks: &mut [Block],
    targets: &[String],
    source: Option<SourceSpan>,
) -> bool {
    for block in blocks {
        match block {
            Block::VerticalSpace { .. } => {}
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                prepend_inlines(children, targets, source);
                return true;
            }
            Block::List { items, .. } => {
                let Some(item) = items.first_mut() else {
                    return false;
                };
                if prepend_to_first_descendant(&mut item.blocks, targets, source) {
                    return true;
                }
                return false;
            }
            Block::DefinitionList { items, .. } => {
                let Some(item) = items.first_mut() else {
                    return false;
                };
                if let Some(term) = item.terms.first_mut() {
                    prepend_inlines(&mut term.content, targets, source);
                    return true;
                }
                if prepend_to_first_descendant(&mut item.description, targets, source) {
                    return true;
                }
                return false;
            }
            Block::Table { rows, .. } => {
                let Some(cell) = rows.first_mut().and_then(|row| row.cells.first_mut()) else {
                    return false;
                };
                if prepend_to_first_descendant(&mut cell.blocks, targets, source) {
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

fn append_to_last_descendant(
    blocks: &mut [Block],
    targets: &[String],
    source: Option<SourceSpan>,
) -> bool {
    for block in blocks.iter_mut().rev() {
        match block {
            Block::VerticalSpace { .. } => {}
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                children.extend(
                    targets
                        .iter()
                        .cloned()
                        .map(|target| Inline::anchor_at(target, source)),
                );
                return true;
            }
            Block::List { items, .. } => {
                let Some(item) = items.last_mut() else {
                    return false;
                };
                return append_to_last_descendant(&mut item.blocks, targets, source);
            }
            Block::DefinitionList { items, .. } => {
                let Some(item) = items.last_mut() else {
                    return false;
                };
                if append_to_last_descendant(&mut item.description, targets, source) {
                    return true;
                }
                if let Some(term) = item.terms.last_mut() {
                    term.content.extend(
                        targets
                            .iter()
                            .cloned()
                            .map(|target| Inline::anchor_at(target, source)),
                    );
                    return true;
                }
                return false;
            }
            Block::Table { rows, .. } => {
                let Some(cell) = rows.last_mut().and_then(|row| row.cells.last_mut()) else {
                    return false;
                };
                return append_to_last_descendant(&mut cell.blocks, targets, source);
            }
            Block::Equation { .. } | Block::ThematicBreak { .. } | Block::Unsupported { .. } => {
                return false;
            }
        }
    }
    false
}

fn prepend_inlines(children: &mut Vec<Inline>, targets: &[String], source: Option<SourceSpan>) {
    children.splice(
        0..0,
        targets
            .iter()
            .cloned()
            .map(|target| Inline::anchor_at(target, source)),
    );
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
        Inline::Text { .. }
        | Inline::Code { .. }
        | Inline::Equation { .. }
        | Inline::LineBreak { .. } => false,
    })
}

fn first_text(node: &Node) -> Option<&str> {
    if node.kind == NodeKind::Text && !node.flags.no_print {
        return node.text.as_deref();
    }
    node.children.iter().find_map(first_text)
}

fn node_and_part_targets(node: &Node) -> Vec<OwnedTarget> {
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
            targets.push(OwnedTarget::new(target, super::source_span(owner)));
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use libmandoc_rs::{Node, NodeFlags, NodeKind};
    use mant_ir::{Block, Inline, LayoutHint, ListItem};

    fn node(
        kind: NodeKind,
        macro_name: Option<&str>,
        text: Option<&str>,
        tag: Option<&str>,
        line: u32,
        flags: NodeFlags,
        children: Vec<Node>,
    ) -> Node {
        Node {
            id: 0,
            kind,
            section: libmandoc_rs::NormalizedSection::None,
            scope_end: None,
            reference_quotes_title: false,
            macro_token: macro_name.map(libmandoc_rs::MacroToken::from_name),
            text: text.map(ToOwned::to_owned),
            native_text: None,
            tag: tag.map(ToOwned::to_owned),
            line,
            column: 1,
            flow_epoch: 0,
            table_escape: None,
            table_source_recovery_safe: false,
            table_row_kind: None,
            flags,
            list_kind: None,
            definition_list_style: None,
            display_kind: None,
            font: None,
            author_mode: None,
            enclosure: None,
            compact: false,
            offset: None,
            width: None,
            columns: Vec::new(),
            table_frame: None,
            table_grid: None,
            table_layout: None,
            table_alignment: None,
            table_column_count: None,
            table_cells: Vec::new(),
            table_rule_data_columns: Vec::new(),
            equation: None,
            equation_inline: None,
            children,
        }
    }

    #[test]
    fn explicit_targets_ignore_stale_parser_tags() {
        let text = node(
            NodeKind::Text,
            None,
            Some("--Exact.Target"),
            None,
            7,
            NodeFlags {
                no_print: true,
                ..NodeFlags::default()
            },
            Vec::new(),
        );
        let target = node(
            NodeKind::Element,
            Some("Tg"),
            None,
            Some("stale-automatic-target"),
            7,
            NodeFlags {
                deep_link_target: true,
                ..NodeFlags::default()
            },
            vec![text],
        );

        assert_eq!(
            super::raw_target(&target).as_deref(),
            Some("--Exact.Target")
        );
        assert_eq!(
            super::explicit_target_argument(&target).as_deref(),
            Some("--Exact.Target")
        );

        let argumentless = node(
            NodeKind::Element,
            Some("Tg"),
            None,
            Some("stale-automatic-target"),
            9,
            NodeFlags {
                deep_link_target: true,
                ..NodeFlags::default()
            },
            Vec::new(),
        );
        assert_eq!(
            super::raw_target(&argumentless).as_deref(),
            Some("stale-automatic-target")
        );
        assert_eq!(super::explicit_target_argument(&argumentless), None);
    }

    #[test]
    fn structural_part_targets_use_source_tokens() {
        let source = node(
            NodeKind::Text,
            None,
            Some("--source-target rendered suffix"),
            None,
            11,
            NodeFlags::default(),
            Vec::new(),
        );
        let head = node(
            NodeKind::Head,
            None,
            None,
            None,
            11,
            NodeFlags {
                deep_link_target: true,
                ..NodeFlags::default()
            },
            vec![source],
        );
        let block = node(
            NodeKind::Block,
            Some("Fo"),
            None,
            None,
            11,
            NodeFlags::default(),
            vec![head],
        );

        assert_eq!(
            super::part_target(&block, NodeKind::Head).as_deref(),
            Some("--source-target")
        );
    }

    #[test]
    fn target_attachment_descends_into_nested_lists() {
        let mut blocks = vec![Block::List {
            kind: mant_ir::ListKind::Bullet,
            compact: false,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![Block::Preformatted {
                    inline_layout: mant_ir::InlineLayout::default(),
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
