//! Lowers mdoc(7) `.Bl` and `.It` list structures.

use super::{
    AstTableAlignment, AstTableCell, Block, DefinitionItem, Inline, ListItem, ListKind,
    LoweringContext, Node, NodeKind, NormalizedListKind, TableRow, definition_item, display_indent,
    first_part_children, horizontal_distance_columns, layout, lower_blocks_with_spacing,
    ordinal_sequence, part_child_groups, plain_text, source_span, spacing_after_node,
    spacing_after_nodes, targets, terms_fit_inline,
};

pub(in crate::mandoc::blocks) fn lower_mdoc_list(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
    initial_spacing: bool,
) -> Block {
    let MdocListItems {
        items,
        trailing_targets,
    } = mdoc_list_items(node, initial_spacing, context.default_name);
    let is_definition = matches!(
        node.list_kind,
        Some(NormalizedListKind::Definition | NormalizedListKind::Column)
    ) || (node.list_kind.is_none()
        && items
            .iter()
            .any(|item| !first_part_children(item.node, NodeKind::Head).is_empty()));
    let list_indent = indent_columns + display_indent(node);
    let mut block = if node.list_kind == Some(NormalizedListKind::Column) {
        lower_mdoc_column_list(
            node,
            items,
            context,
            indent_columns,
            list_indent,
            paragraph_distance,
        )
    } else if is_definition {
        lower_mdoc_definition_list(
            node,
            items,
            context,
            indent_columns,
            list_indent,
            paragraph_distance,
        )
    } else {
        Block::List {
            kind: match node.list_kind {
                Some(NormalizedListKind::Ordered) => ListKind::Ordered,
                Some(NormalizedListKind::Plain) => ListKind::Plain,
                _ => ListKind::Bullet,
            },
            start: (node.list_kind == Some(NormalizedListKind::Ordered)).then_some(1),
            compact: node.compact,
            items: items
                .into_iter()
                .map(|item| {
                    let mut blocks = lower_blocks_with_spacing(
                        first_part_children(item.node, NodeKind::Body),
                        context,
                        list_indent,
                        paragraph_distance,
                        spacing_after_nodes(
                            first_part_children(item.node, NodeKind::Head),
                            item.spacing_enabled,
                            context.default_name,
                        ),
                    );
                    let targets = item
                        .leading_targets
                        .into_iter()
                        .chain(targets::item_targets(item.node));
                    targets::attach_targets(
                        &mut blocks,
                        targets,
                        layout(list_indent),
                        source_span(item.node),
                    );
                    ListItem { blocks }
                })
                .collect(),
            layout: layout(indent_columns),
            source: source_span(node),
        }
    };
    append_list_targets(
        &mut block,
        trailing_targets,
        layout(list_indent),
        source_span(node),
    );
    block
}

fn lower_mdoc_definition_list(
    node: &Node,
    items: Vec<MdocListItem<'_>>,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    list_indent: u16,
    paragraph_distance: &mut u16,
) -> Block {
    let max_term_width = node
        .width
        .as_deref()
        .and_then(horizontal_distance_columns)
        .unwrap_or(6);
    let lowered_items = items
        .into_iter()
        .map(|item| {
            let mut lowered = definition_item(
                item.node,
                context,
                list_indent,
                paragraph_distance,
                max_term_width,
                item.spacing_enabled,
            );
            let targets = item
                .leading_targets
                .into_iter()
                .chain(targets::item_targets(item.node));
            targets::attach_definition_targets(&mut lowered, targets);
            lowered
        })
        .collect::<Vec<_>>();
    if node.list_kind == Some(NormalizedListKind::Definition)
        && let Some(first) = ordinal_sequence(&lowered_items)
    {
        return Block::List {
            kind: ListKind::Ordered,
            start: Some(first.value()),
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
        items: coalesce_pending_definition_terms(lowered_items, max_term_width),
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
    list_indent: u16,
    source: Option<mant_ir::SourceSpan>,
) -> ListItem {
    let DefinitionItem {
        terms,
        mut description,
        ..
    } = item;
    let mut anchors = Vec::new();
    for term in &terms {
        targets::inline_anchor_ids(term, &mut anchors);
    }
    targets::attach_targets(&mut description, anchors, layout(list_indent), source);
    ListItem {
        blocks: description,
    }
}

/// Attach consecutive description-less definition heads to the next item.
///
/// Both man(7) `.TQ` and mdoc(7) commonly express several equivalent input
/// forms as a run of heads followed by one shared body.  The man lowering
/// path already folds pending `.TQ` heads in [`append_definition`]; mdoc lists
/// arrive as one complete collection, so perform the same structural
/// normalization before the source-specific list representation leaves this
/// module.  A trailing run stays intact because no shared description proves
/// that the terms belong to one definition.
fn coalesce_pending_definition_terms(
    items: Vec<DefinitionItem>,
    max_term_width: usize,
) -> Vec<DefinitionItem> {
    let mut output = Vec::with_capacity(items.len());
    let mut pending = Vec::new();

    for mut item in items {
        if item.description.is_empty() {
            pending.push(item);
            continue;
        }
        if is_option_definition(&item) && pending.iter().all(is_option_definition) {
            let pending_terms = pending
                .drain(..)
                .flat_map(|pending: DefinitionItem| pending.terms);
            item.terms.splice(0..0, pending_terms);
            item.inline_term = terms_fit_inline(&item.terms, max_term_width);
        } else {
            output.append(&mut pending);
        }
        output.push(item);
    }

    output.extend(pending);
    output
}

fn is_option_definition(item: &DefinitionItem) -> bool {
    item.terms.iter().any(|term| {
        let text = plain_text(term);
        let Some(token) = text.split_whitespace().next() else {
            return false;
        };
        let name =
            token.trim_matches(|character: char| matches!(character, '[' | ']' | '(' | ')' | ','));
        name.starts_with('-') && name != "-"
    })
}

struct MdocListItem<'a> {
    node: &'a Node,
    spacing_enabled: bool,
    leading_targets: Vec<String>,
}

struct MdocListItems<'a> {
    items: Vec<MdocListItem<'a>>,
    trailing_targets: Vec<String>,
}

/// Pair each mdoc list item with the formatter spacing state active at its
/// source position.
///
/// libmandoc keeps state-only `.Sm` requests as siblings of `.It` blocks.
/// Filtering the body directly to items therefore erased precisely the state
/// needed to render compact forms such as `Odevice`, `:S/old/new/`, and
/// `@newuser name:uid`. Walking the structural stream once also lets an
/// intentionally unbalanced transition inside an item affect later items.
fn mdoc_list_items<'a>(
    node: &'a Node,
    initial_spacing: bool,
    default_name: Option<&str>,
) -> MdocListItems<'a> {
    let mut spacing_enabled = initial_spacing;
    let mut items = Vec::new();
    let mut pending_targets = Vec::new();
    for child in first_part_children(node, NodeKind::Body) {
        if let Some(target) = targets::explicit_target_argument(child)
            && !pending_targets.contains(&target)
        {
            pending_targets.push(target);
        }
        if child.macro_name.as_deref() == Some("It") {
            items.push(MdocListItem {
                node: child,
                spacing_enabled,
                leading_targets: std::mem::take(&mut pending_targets),
            });
        }
        spacing_enabled = spacing_after_node(child, spacing_enabled, default_name);
    }
    MdocListItems {
        items,
        trailing_targets: pending_targets,
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
    indent_columns: u16,
    cell_indent: u16,
    paragraph_distance: &mut u16,
) -> Block {
    let rows = items
        .into_iter()
        .map(|item| {
            let body_spacing = spacing_after_nodes(
                first_part_children(item.node, NodeKind::Head),
                item.spacing_enabled,
                context.default_name,
            );
            let mut cells = part_child_groups(item.node, NodeKind::Body)
                .map(|body| AstTableCell {
                    blocks: lower_blocks_with_spacing(
                        body,
                        context,
                        cell_indent,
                        paragraph_distance,
                        body_spacing,
                    ),
                    column_span: 1,
                    row_span: 1,
                    alignment: Some(AstTableAlignment::Left),
                })
                .collect::<Vec<_>>();
            if let Some(cell) = cells.first_mut() {
                let targets = item
                    .leading_targets
                    .into_iter()
                    .chain(targets::item_targets(item.node));
                targets::attach_targets(
                    &mut cell.blocks,
                    targets,
                    layout(cell_indent),
                    source_span(item.node),
                );
            } else {
                let mut blocks = Vec::new();
                let targets = item
                    .leading_targets
                    .into_iter()
                    .chain(targets::item_targets(item.node));
                targets::attach_targets(
                    &mut blocks,
                    targets,
                    layout(cell_indent),
                    source_span(item.node),
                );
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
                items.push(ListItem { blocks: Vec::new() });
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
                    identity: None,
                    terms: vec![targets.into_iter().map(Inline::anchor).collect()],
                    description: Vec::new(),
                    inline_term: true,
                    spacing_before_lines: None,
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
