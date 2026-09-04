//! Lowers man and mdoc list and definition structures.

use libmandoc_rs::{Node, NodeKind, NormalizedListKind};
use mant_ir::{
    Block, DefinitionItem, Inline, ListItem, ListKind, TableAlignment as AstTableAlignment,
    TableCell as AstTableCell, TableRow,
};

use super::super::{
    LoweringContext, first_part_children,
    inline::{
        InlineBuilder, lower_inline_nodes_with_spacing, plain_text, spacing_after_node,
        spacing_after_nodes, terms_fit_inline,
    },
    layout::{
        block_indent, display_indent, horizontal_distance_columns, layout, layout_with_spacing,
        paragraph_distance_lines,
    },
    part_child_groups, source_span, targets,
};
use super::{
    ends_with_line_continuation, is_inline_equation, is_inline_equation_quote_artifact,
    lower_blocks_with_spacing,
    man_lists::{
        DefinitionLocation, MAN_DEFINITION_BODY_INDENT, ManListState, append_ordered,
        list_item_from_definition, ordinal_marker, ordinal_sequence,
    },
};
use crate::block::block_layout_mut;

fn is_bullet_glyph(text: &str) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        // `o` is the ASCII bullet convention in man pages; otherwise any single
        // non-alphanumeric mark (`*`, `•`, `-`, `+`, …) is a bullet.
        (Some(glyph), None) => glyph == 'o' || !glyph.is_alphanumeric(),
        _ => false,
    }
}

pub(super) fn lower_man_definition(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    state: ManDefinitionState<'_>,
    spacing_enabled: bool,
) {
    let ManDefinitionState {
        paragraph_distance,
        output,
        definition_hanging_width,
        alias_state,
        list_state,
    } = state;
    let LoweredManItem {
        mut item,
        spacing_before,
        leading_head_distance,
        leading_body_distance,
        max_width,
    } = lower_man_item(
        node,
        context,
        indent_columns,
        paragraph_distance,
        definition_hanging_width,
        spacing_enabled,
    );
    let macro_name = node.macro_name.as_deref();
    let ordinal = matches!(macro_name, Some("IP" | "TP"))
        .then(|| {
            ordinal_marker(
                &item,
                macro_name == Some("IP") && context.man_ip_uses_incrementing_register(node.line),
            )
        })
        .flatten();
    let description_empty = item.description.is_empty();
    let opens_compact_group = macro_name == Some("TP")
        && description_empty
        && (leading_head_distance == Some(0) || leading_body_distance == Some(0));
    let closes_compact_group = macro_name == Some("TP")
        && !description_empty
        && leading_body_distance.is_some_and(|distance| distance != 0);
    let explicit_continuation = description_empty
        && (macro_name == Some("TQ")
            || visible_definition_head(node)
                .last()
                .is_some_and(ends_with_line_continuation));
    let previous_location = last_definition_location(output, indent_columns);
    let merge = definition_merge(
        macro_name,
        closes_compact_group,
        *alias_state,
        previous_location,
    );
    warn_unproven_alias_boundary(
        node,
        context,
        output,
        indent_columns,
        macro_name,
        description_empty,
        merge,
    );
    if node.macro_name.as_deref() == Some("IP")
        && item.terms.is_empty()
        && append_ip_continuation(output, &mut item, indent_columns, spacing_before)
    {
        return;
    }
    emit_man_definition(
        output,
        alias_state,
        list_state,
        ManDefinitionEmission {
            item,
            macro_name,
            source: source_span(node),
            indent_columns,
            spacing_before,
            max_width,
            ordinal,
            description_empty,
            opens_compact_group,
            explicit_continuation,
            previous_location,
            merge,
            leading_head_distance,
            leading_body_distance,
        },
    );
}

struct ManDefinitionEmission<'a> {
    item: DefinitionItem,
    macro_name: Option<&'a str>,
    source: Option<mant_ir::SourceSpan>,
    indent_columns: u16,
    spacing_before: u16,
    max_width: usize,
    ordinal: Option<super::man_lists::ManOrdinalMarker>,
    description_empty: bool,
    opens_compact_group: bool,
    explicit_continuation: bool,
    previous_location: Option<DefinitionLocation>,
    merge: DefinitionMerge,
    leading_head_distance: Option<u16>,
    leading_body_distance: Option<u16>,
}

fn emit_man_definition(
    output: &mut Vec<Block>,
    alias_state: &mut ManAliasState,
    list_state: &mut ManListState,
    emission: ManDefinitionEmission<'_>,
) {
    let ManDefinitionEmission {
        item,
        macro_name,
        source,
        indent_columns,
        spacing_before,
        max_width,
        ordinal,
        description_empty,
        opens_compact_group,
        explicit_continuation,
        previous_location,
        merge,
        leading_head_distance,
        leading_body_distance,
    } = emission;
    if macro_name == Some("IP") && is_ip_bullet_item(&item) {
        *list_state = ManListState::None;
        append_ip_bullet(output, item, indent_columns, spacing_before, source);
    } else {
        if let Some(marker) = ordinal {
            append_ordered(
                output,
                item,
                indent_columns,
                spacing_before,
                source,
                marker,
                list_state,
            );
            *alias_state = ManAliasState::None;
            return;
        }
        let location = append_definition(
            output,
            item,
            indent_columns,
            spacing_before,
            source,
            max_width,
            merge,
        );
        transition_alias_state(
            alias_state,
            AliasTransition {
                macro_name,
                description_empty,
                opens_compact_group,
                explicit_continuation,
                previous_location,
                merge,
                location,
                spacing_before,
                leading_head_distance,
                leading_body_distance,
            },
        );
        *list_state = ManListState::None;
    }
}

struct LoweredManItem {
    item: DefinitionItem,
    spacing_before: u16,
    leading_head_distance: Option<u16>,
    leading_body_distance: Option<u16>,
    max_width: usize,
}

fn lower_man_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
    definition_hanging_width: &mut usize,
    spacing_enabled: bool,
) -> LoweredManItem {
    // Capture the distance before lowering the body: a `.PD` request that
    // follows this item can live inside libmandoc's block scope and updates
    // spacing for the *next* item, not the current one.
    let spacing_before = if node.macro_name.as_deref() == Some("TQ") {
        0
    } else {
        *paragraph_distance
    };
    let head = first_part_children(node, NodeKind::Head);
    let body = first_part_children(node, NodeKind::Body);
    let leading_head_distance = leading_paragraph_distance(head);
    let leading_body_distance = leading_paragraph_distance(body);
    if let Some(distance) = leading_head_distance {
        *paragraph_distance = distance;
    }
    update_man_definition_width(node, definition_hanging_width);
    let max_width = definition_hanging_width.saturating_sub(1);
    let item = definition_item(
        node,
        context,
        indent_columns,
        paragraph_distance,
        max_width,
        spacing_enabled,
    );
    LoweredManItem {
        item,
        spacing_before,
        leading_head_distance,
        leading_body_distance,
        max_width,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ManAliasState {
    None,
    ExplicitContinuation(DefinitionLocation),
    CompactRun(DefinitionLocation),
}

impl ManAliasState {
    const fn compact_start(self) -> Option<DefinitionLocation> {
        match self {
            Self::CompactRun(location) => Some(location),
            Self::None | Self::ExplicitContinuation(_) => None,
        }
    }

    const fn explicit_start(self) -> Option<DefinitionLocation> {
        match self {
            Self::ExplicitContinuation(location) => Some(location),
            Self::None | Self::CompactRun(_) => None,
        }
    }
}

pub(super) struct ManDefinitionState<'a> {
    pub(super) paragraph_distance: &'a mut u16,
    pub(super) output: &'a mut Vec<Block>,
    pub(super) definition_hanging_width: &'a mut usize,
    pub(super) alias_state: &'a mut ManAliasState,
    pub(super) list_state: &'a mut ManListState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DefinitionMerge {
    None,
    From(DefinitionLocation),
}

#[derive(Clone, Copy)]
struct AliasTransition<'a> {
    macro_name: Option<&'a str>,
    description_empty: bool,
    opens_compact_group: bool,
    explicit_continuation: bool,
    previous_location: Option<DefinitionLocation>,
    merge: DefinitionMerge,
    location: DefinitionLocation,
    spacing_before: u16,
    leading_head_distance: Option<u16>,
    leading_body_distance: Option<u16>,
}

fn transition_alias_state(alias_state: &mut ManAliasState, transition: AliasTransition<'_>) {
    let AliasTransition {
        macro_name,
        description_empty,
        opens_compact_group,
        explicit_continuation,
        previous_location,
        merge,
        location,
        spacing_before,
        leading_head_distance,
        leading_body_distance,
    } = transition;
    *alias_state = if opens_compact_group
        || (macro_name == Some("IP")
            && description_empty
            && (spacing_before == 0
                || leading_head_distance == Some(0)
                || leading_body_distance == Some(0)))
    {
        match *alias_state {
            ManAliasState::CompactRun(start) => ManAliasState::CompactRun(start),
            ManAliasState::None | ManAliasState::ExplicitContinuation(_) => {
                ManAliasState::CompactRun(location)
            }
        }
    } else if explicit_continuation {
        match *alias_state {
            ManAliasState::CompactRun(start) => ManAliasState::CompactRun(start),
            ManAliasState::ExplicitContinuation(start) => {
                ManAliasState::ExplicitContinuation(start)
            }
            ManAliasState::None if macro_name == Some("TQ") => previous_location.map_or(
                ManAliasState::ExplicitContinuation(location),
                ManAliasState::ExplicitContinuation,
            ),
            ManAliasState::None => ManAliasState::ExplicitContinuation(location),
        }
    } else if matches!(merge, DefinitionMerge::None) && description_empty {
        *alias_state
    } else {
        ManAliasState::None
    };
}

fn definition_merge(
    macro_name: Option<&str>,
    closes_compact_group: bool,
    alias_state: ManAliasState,
    previous_location: Option<DefinitionLocation>,
) -> DefinitionMerge {
    let start = if macro_name == Some("TQ") {
        alias_state
            .explicit_start()
            .or_else(|| alias_state.compact_start())
            .or(previous_location)
    } else if closes_compact_group {
        alias_state.compact_start()
    } else if let Some(start) = alias_state.explicit_start() {
        Some(start)
    } else if macro_name == Some("IP") {
        alias_state.compact_start()
    } else {
        None
    };
    start.map_or(DefinitionMerge::None, DefinitionMerge::From)
}

fn warn_unproven_alias_boundary(
    node: &Node,
    context: &LoweringContext<'_>,
    output: &[Block],
    indent_columns: u16,
    macro_name: Option<&str>,
    description_empty: bool,
    merge: DefinitionMerge,
) {
    if !description_empty
        && matches!(macro_name, Some("IP" | "TQ"))
        && pending_definition_start(output, indent_columns).is_some_and(
            |pending| !matches!(merge, DefinitionMerge::From(start) if start == pending),
        )
    {
        context.warn_definition_alias_boundary(node);
    }
}

fn last_definition_location(output: &[Block], indent_columns: u16) -> Option<DefinitionLocation> {
    let block = output.len().checked_sub(1)?;
    let Block::DefinitionList { items, .. } = output
        .last()
        .filter(|candidate| block_indent(candidate) == Some(indent_columns))?
    else {
        return None;
    };
    Some(DefinitionLocation {
        block,
        item: items.len().checked_sub(1)?,
    })
}

fn pending_definition_start(output: &[Block], indent_columns: u16) -> Option<DefinitionLocation> {
    let block = output.len().checked_sub(1)?;
    let Block::DefinitionList { items, .. } = output
        .last()
        .filter(|candidate| block_indent(candidate) == Some(indent_columns))?
    else {
        return None;
    };
    let item = items
        .iter()
        .rposition(|previous| !previous.description.is_empty())
        .map_or(0, |index| index + 1);
    (item < items.len()).then_some(DefinitionLocation { block, item })
}

fn leading_paragraph_distance(nodes: &[Node]) -> Option<u16> {
    let mut distance = None;
    for node in nodes {
        if node.macro_name.as_deref() == Some("PD") {
            if let Some(value) = paragraph_distance_lines(node) {
                distance = Some(value);
            }
        } else if !node.flags.no_print && node.kind != NodeKind::Comment {
            break;
        }
    }
    distance
}

/// Attach an unlabelled `.IP` body to the preceding labelled item.
///
/// man(7) uses a headless `.IP` to begin another indented paragraph under the
/// current tag. It is a continuation only when the immediately preceding
/// item already has both a term and a description; otherwise the anonymous
/// block remains explicit so malformed or intentionally unlabelled input is
/// never discarded.
fn append_ip_continuation(
    output: &mut [Block],
    item: &mut DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
) -> bool {
    if item.description.is_empty() {
        return false;
    }
    let Some(Block::DefinitionList { items, compact, .. }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns))
    else {
        return false;
    };
    let Some(previous) = items
        .last_mut()
        .filter(|previous| !previous.terms.is_empty() && !previous.description.is_empty())
    else {
        return false;
    };
    if let Some(layout) = item.description.first_mut().and_then(block_layout_mut) {
        layout.spacing_before_lines = layout.spacing_before_lines.max(paragraph_distance);
    }
    previous.description.append(&mut item.description);
    *compact = *compact && paragraph_distance == 0;
    true
}

pub(super) fn lower_mdoc_list(
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
        if let Some(target) = targets::explicit_target(child)
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

fn definition_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: u16,
    paragraph_distance: &mut u16,
    max_term_width: usize,
    spacing_enabled: bool,
) -> DefinitionItem {
    let head = visible_definition_head(node);
    let body = first_part_children(node, NodeKind::Body);
    let (displaced_equations, body) = displaced_definition_equations(head, body);
    let mut term_builder = InlineBuilder::with_spacing(spacing_enabled);
    term_builder.append(lower_inline_nodes_with_spacing(
        head,
        context.default_name,
        spacing_enabled,
    ));
    for equation in displaced_equations {
        term_builder.append(lower_inline_nodes_with_spacing(
            std::slice::from_ref(equation),
            context.default_name,
            spacing_enabled,
        ));
    }
    let mut term = term_builder.finish();
    if let Some(id) = definition_head_anchor(node, &term) {
        term.insert(0, Inline::anchor(id));
    }
    let terms = split_definition_terms(term);
    DefinitionItem {
        identity: None,
        inline_term: terms_fit_inline(&terms, max_term_width),
        terms,
        description: lower_blocks_with_spacing(
            body,
            context,
            indent_columns.saturating_add(MAN_DEFINITION_BODY_INDENT),
            paragraph_distance,
            spacing_after_nodes(head, spacing_enabled, context.default_name),
        ),
        spacing_before_lines: None,
    }
}

/// Recover inline eqn arguments that libmandoc moved from a man macro head to
/// the beginning of its owning definition body.
fn displaced_definition_equations<'a>(
    head: &[Node],
    body: &'a [Node],
) -> (Vec<&'a Node>, &'a [Node]) {
    let Some(head_line) = head.iter().map(maximum_node_line).max() else {
        return (Vec::new(), body);
    };
    let mut equations = Vec::new();
    let mut consumed = 0;
    while let Some(candidate) = body
        .get(consumed)
        .filter(|candidate| candidate.line == head_line)
    {
        if is_inline_equation(candidate) {
            equations.push(candidate);
            consumed += 1;
            continue;
        }
        if consumed > 0 && is_inline_equation_quote_artifact(body, consumed) {
            consumed += 1;
            continue;
        }
        break;
    }
    if equations.is_empty() {
        (equations, body)
    } else {
        (equations, &body[consumed..])
    }
}

fn maximum_node_line(node: &Node) -> u32 {
    node.children
        .iter()
        .map(maximum_node_line)
        .fold(node.line, u32::max)
}

/// Split alternatives embedded in one extended mdoc definition head.
///
/// libmandoc retains `.Pp` inside `It Xo ... Xc` as an inline child. In that
/// position it separates equivalent term spellings rather than starting a
/// new description paragraph. The IR already models such aliases as several
/// terms on one definition item, so preserve that structure explicitly.
fn split_definition_terms(term: Vec<Inline>) -> Vec<Vec<Inline>> {
    let mut terms = Vec::new();
    let mut current = Vec::new();
    for node in term {
        if node == Inline::LineBreak {
            if !current.is_empty() {
                terms.push(std::mem::take(&mut current));
            }
        } else {
            current.push(node);
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

/// Preserve libmandoc's tag on a man(7) `.TP`/`.IP` head. Unlike mdoc `Fl`
/// tags, this identity lives on the structural head rather than a visible
/// inline child, so it has to be copied before lowering discards that wrapper.
fn definition_head_anchor(node: &Node, term: &[Inline]) -> Option<String> {
    let fallback = plain_text(term);
    targets::part_target(
        node,
        NodeKind::Head,
        fallback
            .trim_start_matches('-')
            .split_whitespace()
            .next()
            .unwrap_or_default(),
    )
}

/// Return only document content from a definition macro's mixed-purpose head.
///
/// This follows mandoc's own HTML and terminal renderers: `.IP` prints its
/// first head node and treats later arguments as layout, while `.TP`/`.TQ`
/// print only nodes beginning on the following input line. The distinction is
/// structural; inspecting strings such as `96u` would incorrectly remove a
/// numeric term while still leaking non-numeric width expressions.
fn visible_definition_head(node: &Node) -> &[Node] {
    let head = first_part_children(node, NodeKind::Head);
    match node.macro_name.as_deref() {
        Some("IP") => head.first().map_or(&[], std::slice::from_ref),
        Some("TP" | "TQ") => head
            .iter()
            .position(|child| child.flags.line_start)
            .map_or(&[], |visible_start| &head[visible_start..]),
        _ => head,
    }
}

fn append_definition(
    output: &mut Vec<Block>,
    mut item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
    max_term_width: usize,
    merge: DefinitionMerge,
) -> DefinitionLocation {
    let block_index = output.len().saturating_sub(1);
    if let Some(Block::DefinitionList { items, compact, .. }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns))
    {
        if !item.description.is_empty() {
            let first_pending = match merge {
                DefinitionMerge::From(location)
                    if location.block == block_index
                        && location.item < items.len()
                        && items[location.item..]
                            .iter()
                            .all(|pending| pending.description.is_empty()) =>
                {
                    Some(location.item)
                }
                DefinitionMerge::None | DefinitionMerge::From(_) => None,
            };
            if let Some(first_pending) = first_pending {
                let pending_terms = items
                    .drain(first_pending..)
                    .flat_map(|pending| pending.terms);
                item.terms.splice(0..0, pending_terms);
                // Source-proven `.TQ`, `\c`, and bounded compact aliases are
                // collected as pending terms. Recompute their combined layout.
                item.inline_term = terms_fit_inline(&item.terms, max_term_width);
            }
        }
        item.spacing_before_lines = Some(if items.is_empty() {
            0
        } else {
            paragraph_distance
        });
        *compact = *compact && paragraph_distance == 0;
        let item_index = items.len();
        items.push(item);
        DefinitionLocation {
            block: block_index,
            item: item_index,
        }
    } else {
        item.spacing_before_lines = Some(0);
        let spacing_before_lines = if output.is_empty() {
            0
        } else {
            paragraph_distance
        };
        output.push(Block::DefinitionList {
            items: vec![item],
            compact: paragraph_distance == 0,
            layout: layout_with_spacing(indent_columns, spacing_before_lines),
            source,
        });
        DefinitionLocation {
            block: output.len() - 1,
            item: 0,
        }
    }
}

fn update_man_definition_width(node: &Node, current_width: &mut usize) {
    let head = first_part_children(node, NodeKind::Head);
    let argument = match node.macro_name.as_deref() {
        Some("TP" | "TQ") => head
            .iter()
            .find(|child| !child.flags.line_start)
            .and_then(first_node_text),
        Some("IP") => head.get(1).and_then(first_node_text),
        _ => None,
    };
    if let Some(width) = argument.and_then(horizontal_distance_columns) {
        *current_width = width;
    }
}

fn first_node_text(node: &Node) -> Option<&str> {
    node.text
        .as_deref()
        .or_else(|| node.children.iter().find_map(first_node_text))
}

/// Append a man(7) `.IP` bullet while the source macro is still known.
///
/// Inferring this later from the serialized term text is unsafe: a legitimate
/// `.TP *` glossary entry looks identical after lowering. Keeping the decision
/// at this boundary preserves real `.IP o`/`.IP \(bu` lists without erasing
/// punctuation-only definition terms.
fn append_ip_bullet(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: u16,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
) {
    let list_item = list_item_from_definition(item, indent_columns, source);
    if let Some(Block::List {
        kind: ListKind::Bullet,
        compact,
        items,
        ..
    }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns))
    {
        *compact = *compact && paragraph_distance == 0;
        items.push(list_item);
        return;
    }

    let spacing_before_lines = if output.is_empty() {
        0
    } else {
        paragraph_distance
    };
    output.push(Block::List {
        kind: ListKind::Bullet,
        start: None,
        compact: paragraph_distance == 0,
        items: vec![list_item],
        layout: layout_with_spacing(indent_columns, spacing_before_lines),
        source,
    });
}

fn is_ip_bullet_item(item: &DefinitionItem) -> bool {
    let [term] = item.terms.as_slice() else {
        return false;
    };
    is_bullet_glyph(plain_text(term).trim())
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem, Inline, LayoutHint};

    fn text(value: &str) -> Vec<Inline> {
        vec![Inline::Text {
            value: value.to_owned(),
        }]
    }

    fn definition(term: &str, description: &str) -> DefinitionItem {
        DefinitionItem {
            identity: None,
            inline_term: false,
            terms: vec![text(term)],
            description: vec![Block::Paragraph {
                children: text(description),
                layout: LayoutHint::default(),
                source: None,
            }],
            spacing_before_lines: None,
        }
    }

    #[test]
    fn only_single_glyph_definition_terms_are_ip_bullets() {
        assert!(super::is_ip_bullet_item(&definition("*", "multiply")));
        assert!(super::is_ip_bullet_item(&definition("o", "item")));
        assert!(!super::is_ip_bullet_item(&definition("&&", "logical and")));
        assert!(!super::is_ip_bullet_item(&definition(
            "-a, --all",
            "show all"
        )));
    }

    #[test]
    fn short_terms_hang_inline_but_long_ones_do_not() {
        // Matches man(1): a tag that fits the default hanging indent shares the
        // first description line; wider tags take their own line.
        assert!(super::terms_fit_inline(&[text("space")], 6));
        assert!(super::terms_fit_inline(&[text("* / %")], 6));
        assert!(!super::terms_fit_inline(&[text("--listed-incremental")], 6));
        assert!(!super::terms_fit_inline(&[], 6));
    }

    #[test]
    fn extended_definition_terms_split_only_at_semantic_line_breaks() {
        let terms = super::split_definition_terms(vec![
            Inline::Text {
                value: "first".to_owned(),
            },
            Inline::LineBreak,
            Inline::Strong {
                children: text("second"),
            },
        ]);

        assert_eq!(
            terms,
            [
                text("first"),
                vec![Inline::Strong {
                    children: text("second")
                }]
            ]
        );
    }
}
