//! Collect item/control/target ownership without executing the collected stream.

use super::{
    Block, DefinitionItem, Inline, ListItem, LoweringContext, Mdoc, MdocMacro, Node, NodeKind,
    column, first_part_children, source_span, targets,
};

pub(super) fn is_mdoc_list_item(node: &Node) -> bool {
    node.kind == NodeKind::Block && node.macro_token.as_ref() == Some(&Mdoc(MdocMacro::It))
}

pub(super) struct MdocListItem<'a> {
    pub(super) node: &'a Node,
    pub(super) leading_controls: &'a [Node],
    pub(super) leading_targets: Vec<targets::OwnedTarget>,
}

impl MdocListItem<'_> {
    pub(super) fn targets(&self) -> Vec<targets::OwnedTarget> {
        let native = targets::item_targets(self.node);
        // Prefer the actual It wrapper when native validation moved ownership
        // there. Otherwise the pending Tg remains the owner, including native
        // argument-less targets and authored recovery of removed empty rows.
        let mut targets = self
            .leading_targets
            .iter()
            .filter(|target| !native.contains(&target.name))
            .cloned()
            .collect::<Vec<_>>();
        targets.extend(
            native
                .into_iter()
                .map(|target| targets::OwnedTarget::new(target, source_span(self.node))),
        );
        targets
    }
}

pub(super) fn attach_item_targets(
    blocks: &mut Vec<Block>,
    item: &MdocListItem<'_>,
    layout: mant_ir::LayoutHint,
) {
    for targets::OwnedTarget {
        name: target,
        owner_source: source,
    } in item.targets().into_iter().rev()
    {
        targets::attach_targets(blocks, [target], layout, source);
    }
}

pub(super) struct MdocListItems<'a> {
    pub(super) items: Vec<MdocListItem<'a>>,
    pub(super) trailing_targets: Vec<targets::OwnedTarget>,
    pub(super) trailing_controls: &'a [Node],
}

/// Retain control slices between items without executing formatter state.
///
/// libmandoc keeps state-only `.Sm` requests as siblings of `.It` blocks.
/// Filtering the body directly to items therefore erased precisely the state
/// needed to render compact forms such as `Odevice`, `:S/old/new/`, and
/// `@newuser name:uid`. Consumers execute these slices between item bodies,
/// including trailing controls, exactly once in source order. This also
/// preserves font requests, not just the spacing settings known to a scanner.
pub(super) fn mdoc_list_items<'node>(
    node: &'node Node,
    context: &LoweringContext<'_>,
) -> MdocListItems<'node> {
    let body = first_part_children(node, NodeKind::Body);
    let mut controls_start = 0;
    let mut items = Vec::new();
    let mut pending_targets: Vec<targets::OwnedTarget> = Vec::new();
    for (index, child) in body.iter().enumerate() {
        if let Some(target) = targets::list_stream_target(child, &context.native_target_owners)
            && !pending_targets.iter().any(|pending| pending.name == target)
        {
            pending_targets.push(targets::OwnedTarget::new(target, source_span(child)));
        }
        if is_mdoc_list_item(child) {
            items.push(MdocListItem {
                node: child,
                leading_controls: &body[controls_start..index],
                leading_targets: std::mem::take(&mut pending_targets),
            });
            controls_start = index + 1;
        }
    }
    MdocListItems {
        items,
        trailing_targets: pending_targets,
        trailing_controls: &body[controls_start..],
    }
}

pub(super) fn append_list_targets(
    block: &mut Block,
    targets: Vec<String>,
    layout: mant_ir::LayoutHint,
    source: Option<mant_ir::SourceSpan>,
) {
    if targets.is_empty() {
        return;
    }
    match block {
        Block::List { items, .. } => {
            if items.is_empty() {
                items.push(ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                    blocks: Vec::new(),
                });
            }
            targets::append_targets(
                &mut items.last_mut().expect("list item inserted").blocks,
                targets,
                layout,
                source,
            );
        }
        Block::DefinitionList { items, .. } => {
            if let Some(item) = items.last_mut() {
                targets::append_definition_targets(item, targets, layout, source);
            } else {
                items.push(DefinitionItem {
                    head_body_relation: mant_ir::HeadBodyRelation::from(true),
                    source: None,
                    entry: None,
                    terms: (vec![
                        targets
                            .into_iter()
                            .map(|target| Inline::anchor_at(target, source))
                            .collect::<Vec<_>>(),
                    ])
                    .into_iter()
                    .map(Into::into)
                    .collect(),
                    description: Vec::new(),
                    layout: mant_ir::DefinitionLayout {
                        body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                        spacing_before_lines: None,
                        ..Default::default()
                    },
                });
            }
        }
        Block::Table { rows, .. } => column::append_targets(rows, targets, layout, source),
        _ => unreachable!("mdoc list lowering returns a list-like block"),
    }
}
