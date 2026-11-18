//! Collect item/control/target ownership without executing the collected stream.

use super::{
    Block, DefinitionItem, ListItem, LoweringContext, Mdoc, MdocMacro, Node, NodeKind, column,
    first_part_children, targets,
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
        let native = targets::item_target_owners(self.node);
        // Prefer the actual It wrapper when native validation moved ownership
        // there. Otherwise the pending Tg remains the owner, including native
        // argument-less targets and authored recovery of removed empty rows.
        let mut targets = self
            .leading_targets
            .iter()
            .filter(|target| !native.iter().any(|owner| owner.node_id == target.node_id))
            .cloned()
            .collect::<Vec<_>>();
        targets.extend(native);
        targets
    }
}

pub(super) fn attach_item_targets(
    blocks: &mut Vec<Block>,
    item: &MdocListItem<'_>,
    layout: mant_ir::LayoutHint,
) {
    for target in item.targets().into_iter().rev() {
        targets::attach_owned_target(blocks, target, layout);
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
        if let Some(target) = targets::list_stream_target(child, &context.native_target_owners) {
            pending_targets.push(target);
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
    target: targets::OwnedTarget,
    layout: mant_ir::LayoutHint,
) {
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
            targets::append_owned_target(
                &mut items.last_mut().expect("list item inserted").blocks,
                target,
                layout,
            );
        }
        Block::DefinitionList { items, .. } => {
            if let Some(item) = items.last_mut() {
                targets::append_definition_target(item, target, layout);
            } else {
                items.push(DefinitionItem {
                    head_body_relation: mant_ir::HeadBodyRelation::from(true),
                    source: None,
                    entry: None,
                    terms: vec![vec![target.into_anchor()].into()],
                    description: Vec::new(),
                    layout: mant_ir::DefinitionLayout {
                        body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                        spacing_before_lines: None,
                        ..Default::default()
                    },
                });
            }
        }
        Block::Table { rows, .. } => column::append_target(rows, target, layout),
        _ => unreachable!("mdoc list lowering returns a list-like block"),
    }
}
