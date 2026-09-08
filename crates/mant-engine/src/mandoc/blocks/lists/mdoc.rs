//! Lowers mdoc(7) `.Bl` and `.It` list structures.

use super::{
    AstTableAlignment, AstTableCell, Block, DefinitionItem, DefinitionListStyle, Inline, ListItem,
    ListKind, LoweringContext, Node, NodeKind, NormalizedListKind, TableRow, definition_item,
    first_part_children, horizontal_distance_columns, layout, lower_blocks_with_spacing,
    ordinal_sequence, part_child_groups, source_span, targets,
};

pub(in crate::mandoc::blocks) fn lower_mdoc_list(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    initial_spacing: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Block {
    let MdocListItems {
        items,
        trailing_targets,
        trailing_controls,
    } = mdoc_list_items(node);
    formatter.spacing = initial_spacing;
    let is_definition = matches!(
        node.list_kind,
        Some(NormalizedListKind::Definition | NormalizedListKind::Column)
    ) || (node.list_kind.is_none()
        && items
            .iter()
            .any(|item| !first_part_children(item.node, NodeKind::Head).is_empty()));
    let offset = node
        .offset
        .as_ref()
        .map_or(0, |_| context.display_offset(node));
    let list_indent = context.nested_indent(node, indent_columns, offset);
    let mut block = if node.list_kind == Some(NormalizedListKind::Column) {
        lower_mdoc_column_list(
            node,
            items,
            context,
            list_indent,
            list_indent,
            paragraph_distance,
            formatter,
        )
    } else if is_definition {
        lower_mdoc_definition_list(
            node,
            items,
            context,
            list_indent,
            list_indent,
            paragraph_distance,
            formatter,
        )
    } else {
        Block::List {
            kind: match node.list_kind {
                Some(NormalizedListKind::Ordered) => ListKind::Ordered { start: Some(1) },
                Some(NormalizedListKind::Plain) => ListKind::Plain,
                _ => ListKind::Bullet,
            },
            compact: node.compact,
            items: items
                .into_iter()
                .map(|item| {
                    context.lower_inline_with_spacing(
                        item.leading_controls,
                        formatter.spacing,
                        formatter,
                    );
                    context.lower_inline_with_spacing(
                        first_part_children(item.node, NodeKind::Head),
                        formatter.spacing,
                        formatter,
                    );
                    let mut blocks = lower_blocks_with_spacing(
                        first_part_children(item.node, NodeKind::Body),
                        context,
                        list_indent.content_origin(),
                        paragraph_distance,
                        formatter.spacing,
                        formatter,
                    );
                    attach_item_targets(&mut blocks, &item, layout(list_indent.content_origin()));
                    ListItem {
                        source: source_span(item.node),
                        entry: None,
                        blocks,
                    }
                })
                .collect(),
            layout: layout(list_indent),
            source: source_span(node),
        }
    };
    for targets::OwnedTarget {
        name: target,
        owner_source: source,
    } in trailing_targets
    {
        append_list_targets(
            &mut block,
            vec![target],
            layout(list_indent.content_origin()),
            source,
        );
    }
    context.lower_inline_with_spacing(trailing_controls, formatter.spacing, formatter);
    block
}

fn lower_mdoc_definition_list(
    node: &Node,
    items: Vec<MdocListItem<'_>>,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    list_indent: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Block {
    let max_term_width = node
        .width
        .as_deref()
        .and_then(horizontal_distance_columns)
        .unwrap_or(6);
    let lowered_items = items
        .into_iter()
        .map(|item| {
            context.lower_inline_with_spacing(item.leading_controls, formatter.spacing, formatter);
            let mut lowered = definition_item(
                item.node,
                context,
                list_indent,
                paragraph_distance,
                max_term_width,
                formatter.spacing,
                formatter,
            );
            for targets::OwnedTarget {
                name: target,
                owner_source: source,
            } in item.targets().into_iter().rev()
            {
                targets::attach_definition_targets(&mut lowered, [target], source);
            }
            lowered
        })
        .collect::<Vec<_>>();
    if node.definition_list_style == Some(DefinitionListStyle::Tag)
        && let Some(first) = ordinal_sequence(&lowered_items)
    {
        return Block::List {
            kind: ListKind::Ordered {
                start: Some(first.value()),
            },
            compact: node.compact,
            items: lowered_items
                .into_iter()
                .map(|item| mdoc_list_item_from_definition(item, list_indent, source_span(node)))
                .collect(),
            layout: layout(indent_columns),
            source: source_span(node),
        };
    }
    Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: lowered_items,
        compact: node.compact,
        layout: layout(indent_columns),
        source: source_span(node),
    }
}

/// Drop source-visible ordinal terms after a complete mdoc tag list has proved
/// ordered-list semantics, while retaining any navigation targets attached to
/// those terms at the same item position.
fn mdoc_list_item_from_definition(
    item: DefinitionItem,
    list_indent: crate::mandoc::layout::SourceIndent,
    source: Option<mant_ir::SourceSpan>,
) -> ListItem {
    let DefinitionItem {
        source: item_source,
        terms,
        mut description,
        ..
    } = item;
    let owner_source = terms
        .iter()
        .find_map(|term| targets::inline_anchor_owner_source(term))
        .or(source);
    let mut anchors = Vec::new();
    for term in &terms {
        targets::inline_anchor_ids(term, &mut anchors);
    }
    targets::attach_targets(
        &mut description,
        anchors,
        layout(list_indent.content_origin()),
        owner_source,
    );
    ListItem {
        source: item_source,
        entry: None,
        blocks: description,
    }
}

struct MdocListItem<'a> {
    node: &'a Node,
    leading_controls: &'a [Node],
    leading_targets: Vec<targets::OwnedTarget>,
}

impl MdocListItem<'_> {
    fn targets(&self) -> Vec<targets::OwnedTarget> {
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

fn attach_item_targets(
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

struct MdocListItems<'a> {
    items: Vec<MdocListItem<'a>>,
    trailing_targets: Vec<targets::OwnedTarget>,
    trailing_controls: &'a [Node],
}

/// Retain control slices between items without executing formatter state.
///
/// libmandoc keeps state-only `.Sm` requests as siblings of `.It` blocks.
/// Filtering the body directly to items therefore erased precisely the state
/// needed to render compact forms such as `Odevice`, `:S/old/new/`, and
/// `@newuser name:uid`. Consumers execute these slices between item bodies,
/// including trailing controls, exactly once in source order. This also
/// preserves font requests, not just the spacing settings known to a scanner.
fn mdoc_list_items(node: &Node) -> MdocListItems<'_> {
    let body = first_part_children(node, NodeKind::Body);
    let mut controls_start = 0;
    let mut items = Vec::new();
    let mut pending_targets: Vec<targets::OwnedTarget> = Vec::new();
    for (index, child) in body.iter().enumerate() {
        if let Some(target) = targets::list_stream_target(child)
            && !pending_targets.iter().any(|pending| pending.name == target)
        {
            pending_targets.push(targets::OwnedTarget::new(target, source_span(child)));
        }
        if child.macro_name.as_deref() == Some("It") {
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

/// Preserve every body sibling of an mdoc `Bl -column` item as one table cell.
///
/// libmandoc represents `Ta` separators by creating several `Body` siblings
/// below the same `It` block. The usual term/body helper intentionally returns
/// only one structural part, so treating a column list as a definition list
/// silently discarded every cell after the first.
fn lower_mdoc_column_list(
    node: &Node,
    items: Vec<MdocListItem<'_>>,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    cell_indent: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Block {
    let rows = items
        .into_iter()
        .map(|item| {
            context.lower_inline_with_spacing(item.leading_controls, formatter.spacing, formatter);
            context.lower_inline_with_spacing(
                first_part_children(item.node, NodeKind::Head),
                formatter.spacing,
                formatter,
            );
            let mut cells = part_child_groups(item.node, NodeKind::Body)
                .map(|body| AstTableCell {
                    blocks: lower_blocks_with_spacing(
                        body,
                        context,
                        cell_indent.content_origin(),
                        paragraph_distance,
                        formatter.spacing,
                        formatter,
                    ),
                    column_span: 1,
                    row_span: 1,
                    alignment: Some(AstTableAlignment::Left),
                })
                .collect::<Vec<_>>();
            if let Some(cell) = cells.first_mut() {
                attach_item_targets(
                    &mut cell.blocks,
                    &item,
                    layout(cell_indent.content_origin()),
                );
            } else {
                let mut blocks = Vec::new();
                attach_item_targets(&mut blocks, &item, layout(cell_indent.content_origin()));
                if !blocks.is_empty() {
                    cells.push(AstTableCell {
                        blocks,
                        column_span: 1,
                        row_span: 1,
                        alignment: Some(AstTableAlignment::Left),
                    });
                }
            }
            TableRow { cells }
        })
        .filter(|row| !row.cells.is_empty())
        .collect();
    Block::Table {
        rows,
        layout: layout(indent_columns),
        source: source_span(node),
    }
}

fn append_list_targets(
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
                    source: None,
                    entry: None,
                    terms: vec![
                        targets
                            .into_iter()
                            .map(|target| Inline::anchor_at(target, source))
                            .collect(),
                    ],
                    description: Vec::new(),
                    layout: mant_ir::DefinitionLayout {
                        inline_term: true,
                        spacing_before_lines: None,
                    },
                });
            }
        }
        Block::Table { rows, .. } => {
            if rows.is_empty() {
                rows.push(TableRow {
                    cells: vec![AstTableCell {
                        blocks: Vec::new(),
                        column_span: 1,
                        row_span: 1,
                        alignment: Some(AstTableAlignment::Left),
                    }],
                });
            }
            let cell = rows
                .last_mut()
                .and_then(|row| row.cells.last_mut())
                .expect("table cell inserted");
            targets::append_targets(&mut cell.blocks, targets, layout, source);
        }
        _ => unreachable!("mdoc list lowering returns a list-like block"),
    }
}
