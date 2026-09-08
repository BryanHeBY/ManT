//! Man tagged paragraphs: independent owners and explicit TQ head continuation.
use super::super::{
    Block, DefinitionItem, DefinitionLocation, ListKind, LoweringContext, ManListState, Node,
    NodeKind, append_ordered, block_indent, block_layout_mut, definition_item, first_part_children,
    horizontal_distance_columns, layout_with_spacing, list_item_from_definition, ordinal_marker,
    paragraph_distance_lines, plain_text, prepend_definition_heads, source_span, terms_fit_inline,
};

fn is_bullet_glyph(text: &str) -> bool {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        // `o` is the ASCII bullet convention in man pages; otherwise any single
        // non-alphanumeric mark (`*`, `•`, `-`, `+`, …) is a bullet.
        (Some(glyph), None) => glyph == 'o' || !glyph.is_alphanumeric(),
        _ => false,
    }
}

pub(in crate::mandoc::blocks) fn lower_man_definition(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    state: ManDefinitionState<'_>,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) {
    let ManDefinitionState {
        paragraph_distance,
        output,
        definition_hanging_width,
        list_state,
    } = state;
    let LoweredManItem {
        mut item,
        spacing_before,
        max_width,
    } = lower_man_item(
        node,
        context,
        indent_columns,
        paragraph_distance,
        definition_hanging_width,
        spacing_enabled,
        formatter,
    );
    let macro_name = node.macro_name.as_deref();
    let bullet = (macro_name == Some("IP") && is_ip_bullet_item(&item))
        || (macro_name == Some("TP") && is_explicit_tp_bullet(node, &item));
    let ordinal = matches!(macro_name, Some("IP" | "TP"))
        .then(|| {
            ordinal_marker(
                &item,
                macro_name == Some("IP") && context.man_ip_uses_incrementing_register(node.line),
            )
        })
        .flatten();
    // Only TQ explicitly adds another tag to the immediately preceding empty
    // definition. Paragraph distance and empty independent IP/TP items never
    // authorize borrowing the next item's description.
    let merge = if macro_name == Some("TQ") {
        last_definition_location(output, indent_columns)
            .map_or(DefinitionMerge::None, DefinitionMerge::From)
    } else {
        DefinitionMerge::None
    };
    let continuation_sources = item
        .description
        .iter()
        .filter_map(crate::block::block_source)
        .map(|s| (s.line, s.column))
        .collect::<Vec<_>>();
    if node.macro_name.as_deref() == Some("IP")
        && item.terms.is_empty()
        && append_ip_continuation(output, &mut item, indent_columns, spacing_before)
    {
        context
            .native_heads
            .borrow_mut()
            .continuations
            .extend(continuation_sources);
        return;
    }
    emit_man_definition(
        output,
        list_state,
        ManDefinitionEmission {
            item,
            source: source_span(node),
            indent_columns,
            spacing_before,
            max_width,
            ordinal,
            merge,
            bullet,
        },
    );
    if macro_name == Some("TQ")
        && let Some(Block::DefinitionList { items, .. }) = output.last()
        && let Some(item) = items.last()
    {
        context
            .native_heads
            .borrow_mut()
            .groups
            .continued(item, std::ptr::from_ref(node) as usize);
    }
}

struct ManDefinitionEmission {
    item: DefinitionItem,
    source: Option<mant_ir::SourceSpan>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    spacing_before: u16,
    max_width: usize,
    ordinal: Option<super::ordered::ManOrdinalMarker>,
    merge: DefinitionMerge,
    bullet: bool,
}

fn emit_man_definition(
    output: &mut Vec<Block>,
    list_state: &mut ManListState,
    emission: ManDefinitionEmission,
) {
    let ManDefinitionEmission {
        item,
        source,
        indent_columns,
        spacing_before,
        max_width,
        ordinal,
        merge,
        bullet,
    } = emission;
    if bullet {
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
            return;
        }
        append_definition(
            output,
            item,
            indent_columns,
            spacing_before,
            source,
            max_width,
            merge,
        );
        *list_state = ManListState::None;
    }
}

struct LoweredManItem {
    item: DefinitionItem,
    spacing_before: u16,
    max_width: usize,
}

fn lower_man_item(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    definition_hanging_width: &mut usize,
    spacing_enabled: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
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
    let leading_head_distance = leading_paragraph_distance(head);
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
        formatter,
    );
    LoweredManItem {
        item,
        spacing_before,
        max_width,
    }
}

pub(in crate::mandoc::blocks) struct ManDefinitionState<'a> {
    pub(in crate::mandoc::blocks) paragraph_distance: &'a mut u16,
    pub(in crate::mandoc::blocks) output: &'a mut Vec<Block>,
    pub(in crate::mandoc::blocks) definition_hanging_width: &'a mut usize,
    pub(in crate::mandoc::blocks) list_state: &'a mut ManListState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DefinitionMerge {
    None,
    From(DefinitionLocation),
}

fn last_definition_location(
    output: &[Block],
    indent_columns: crate::mandoc::layout::SourceIndent,
) -> Option<DefinitionLocation> {
    let block = output.len().checked_sub(1)?;
    let Block::DefinitionList { items, .. } = output
        .last()
        .filter(|candidate| block_indent(candidate) == Some(indent_columns.relative_columns()))?
    else {
        return None;
    };
    Some(DefinitionLocation {
        block,
        item: items.len().checked_sub(1)?,
    })
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
    output: &mut Vec<Block>,
    item: &mut DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
) -> bool {
    if item.description.is_empty() {
        return false;
    }
    // An explicit empty IP continues the preceding tagged paragraph. Fold
    // already completed indented runs first: an intervening RS/RE is not a
    // different declaration, and its later IP notes must follow its contents.
    // The same idempotent topology pass runs at final normalization.
    let Some(start) = output.iter().rposition(|block| match block_indent(block) {
        Some(indent) => indent <= indent_columns.relative_columns(),
        None => !matches!(block, Block::VerticalSpace { .. }),
    }) else {
        return false;
    };
    if !matches!(&output[start], Block::DefinitionList { .. })
        || block_indent(&output[start]) != Some(indent_columns.relative_columns())
    {
        return false;
    }
    if start + 1 < output.len() {
        // Only the new suffix is considered; repeated IP continuations never
        // rescan all earlier definitions or an already-normalized description.
        let mut suffix = output.split_off(start);
        crate::definitions::normalize_definition_nesting(&mut suffix);
        output.append(&mut suffix);
    }
    let Some(Block::DefinitionList { items, compact, .. }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns.relative_columns()))
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

fn append_definition(
    output: &mut Vec<Block>,
    mut item: DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
    max_term_width: usize,
    merge: DefinitionMerge,
) -> DefinitionLocation {
    let block_index = output.len().saturating_sub(1);
    if let Some(Block::DefinitionList { items, compact, .. }) = output
        .last_mut()
        .filter(|block| block_indent(block) == Some(indent_columns.relative_columns()))
    {
        {
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
                prepend_definition_heads(&mut item, items.drain(first_pending..));
                // Explicit TQ tags retain their source order and one owner;
                // this does not assert semantic name equivalence.
                item.layout.inline_term = terms_fit_inline(&item.terms, max_term_width);
            }
        }
        item.layout.spacing_before_lines = Some(if items.is_empty() {
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
        item.layout.spacing_before_lines = Some(0);
        let spacing_before_lines = if output.is_empty() {
            0
        } else {
            paragraph_distance
        };
        output.push(Block::DefinitionList {
            declaration_groups: Vec::new(),
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
    indent_columns: crate::mandoc::layout::SourceIndent,
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
        .filter(|block| block_indent(block) == Some(indent_columns.relative_columns()))
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
        compact: paragraph_distance == 0,
        items: vec![list_item],
        layout: layout_with_spacing(indent_columns, spacing_before_lines),
        source,
    });
}

pub(in crate::mandoc::blocks) fn is_ip_bullet_item(item: &DefinitionItem) -> bool {
    let [term] = item.terms.as_slice() else {
        return false;
    };
    is_bullet_glyph(plain_text(term).trim())
}

/// TP can define literal operators, so do not apply IP's broad marker
/// convention. Require the complete tag to be a bullet and native source
/// evidence for the named roff bullet escape; no section-name heuristic.
fn is_explicit_tp_bullet(node: &Node, item: &DefinitionItem) -> bool {
    fn contains_bullet_escape(node: &Node) -> bool {
        node.text
            .as_ref()
            .is_some_and(|text| text.contains(r"\(bu") || text.contains(r"\[bu]"))
            || node.children.iter().any(contains_bullet_escape)
    }
    matches!(item.terms.as_slice(), [term] if plain_text(term).trim() == "•")
        && super::super::definition::visible_definition_head(node)
            .iter()
            .any(contains_bullet_escape)
}
