//! Man tagged paragraphs: independent owners and explicit TQ head continuation.
use super::super::{
    Block, DefinitionItem, ListKind, LoweringContext, ManListState, Node, NodeKind, append_ordered,
    block_indent, definition_item, first_part_children, layout_with_spacing, ordinal_marker,
    paragraph_distance_lines, plain_text, prepend_definition_heads, source_span, terms_fit_inline,
};

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
        has_predecessor,
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
    let spacing_before =
        crate::mandoc::layout::man_paragraph_spacing(spacing_before, has_predecessor);
    let macro_name = node.macro_name.as_deref();
    let bullet = matches!(macro_name, Some("IP" | "TP")) && is_explicit_bullet(node, &item);
    if macro_name == Some("IP") && !bullet {
        record_ambiguous_ip_mark(&item, context);
    }
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
        .filter_map(mant_ir::geometry::block_source)
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
        list_state.reset();
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
        list_state.reset();
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
    definition_hanging_width: &mut crate::mandoc::layout::Distance,
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
    update_man_definition_width(node, context, definition_hanging_width);
    let item = definition_item(
        node,
        context,
        indent_columns,
        paragraph_distance,
        crate::mandoc::layout::DefinitionGeometry {
            body: *definition_hanging_width,
            placement: if body_breaks_pending_head(first_part_children(node, NodeKind::Body)) {
                crate::mandoc::layout::TermPlacement::Stacked
            } else {
                crate::mandoc::layout::TermPlacement::Fit
            },
            gap: 1,
        },
        super::super::DefinitionFlow {
            spacing_enabled,
            paragraph_predecessor: false,
        },
        formatter,
    );
    let max_width = usize::try_from(
        item.layout
            .body_indent_columns
            .saturating_sub(i32::from(item.layout.min_term_gap_columns)),
    )
    .unwrap_or(0);
    LoweredManItem {
        item,
        spacing_before,
        max_width,
    }
}

/// The native formatter enters a TP/IP body with the head still pending on
/// its line. A detached body's empty inline builder cannot represent that
/// state: an initial br/fi/nf must therefore constrain head placement, not
/// manufacture an empty paragraph or an additional vertical-space request.
fn body_breaks_pending_head(nodes: &[Node]) -> bool {
    for node in nodes {
        if node.kind == NodeKind::Comment {
            continue;
        }
        let name = node.macro_name.as_deref();
        // roff_term dispatches fi/nf to br; in, ti, sp and ce/rj also end the
        // pending line before their separate layout/captured-text effects.
        // man_term's pre_literal does the same for EX/EE.
        if matches!(
            name,
            Some("br" | "fi" | "nf" | "in" | "ti" | "sp" | "ce" | "rj" | "EX" | "EE")
        ) {
            return true;
        }
        if node.flags.no_print
            || name == Some("Tg")
            || crate::mandoc::controls::operand_control(name).is_some()
        {
            // Classification only: normal lowering still executes font and
            // layout controls and retains targets exactly once.
            continue;
        }
        if node.kind == NodeKind::Text
            && node.text.as_deref().is_some_and(|text| {
                !text.is_empty()
                    && crate::mandoc::roff_escape::decode(text)
                        .iter()
                        .all(|event| {
                            matches!(
                                crate::mandoc::roff_escape::inline_event_effect(event),
                                crate::mandoc::roff_escape::InlineEventEffect::StateOnly
                                    | crate::mandoc::roff_escape::InlineEventEffect::RowMarker
                            )
                        })
            })
        {
            continue;
        }
        // Printable content or an independently handled structural scope
        // consumes the initial head/body boundary. Never search past it.
        return false;
    }
    false
}

pub(in crate::mandoc::blocks) struct ManDefinitionState<'a> {
    pub(in crate::mandoc::blocks) paragraph_distance: &'a mut u16,
    pub(in crate::mandoc::blocks) output: &'a mut Vec<Block>,
    pub(in crate::mandoc::blocks) definition_hanging_width: &'a mut crate::mandoc::layout::Distance,
    pub(in crate::mandoc::blocks) list_state: &'a mut ManListState,
    pub(in crate::mandoc::blocks) has_predecessor: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DefinitionLocation {
    block: usize,
    item: usize,
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
    if let Some(first) = item.description.first_mut() {
        // The headless IP paragraph and its first body request are independent
        // source boundaries. Preserve both, including when the body begins
        // with an explicit VerticalSpace rather than a layout-bearing block.
        crate::mandoc::layout::set_block_spacing(first, paragraph_distance);
    }
    mant_ir::geometry::rebase_roots(
        &mut item.description,
        item.layout.body_indent_columns,
        previous.layout.body_indent_columns,
    );
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
                // Adding earlier TQ heads can tighten width fitting, but
                // cannot reopen the final head's explicitly closed line.
                item.layout.inline_term &= terms_fit_inline(&item.terms, max_term_width);
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
        output.push(Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![item],
            compact: paragraph_distance == 0,
            layout: layout_with_spacing(indent_columns, paragraph_distance),
            source,
        });
        DefinitionLocation {
            block: output.len() - 1,
            item: 0,
        }
    }
}

fn update_man_definition_width(
    node: &Node,
    context: &LoweringContext<'_>,
    current_width: &mut crate::mandoc::layout::Distance,
) {
    let head = first_part_children(node, NodeKind::Head);
    let argument = match node.macro_name.as_deref() {
        Some("TP" | "TQ") => head
            .iter()
            .find(|child| !child.flags.line_start)
            .and_then(first_node_text),
        Some("IP") => head.get(1).and_then(first_node_text),
        _ => None,
    };
    if let Some(argument) = argument {
        *current_width = context.distance_or(node, argument, *current_width);
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
/// at this boundary preserves explicit named-bullet lists without erasing
/// punctuation-only definition terms or literal key names.
fn append_ip_bullet(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
) {
    let list_item = super::ordered::spaced_man_list_item(item, 2, source, paragraph_distance);
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

    output.push(Block::List {
        kind: ListKind::Bullet,
        compact: paragraph_distance == 0,
        items: vec![list_item],
        layout: layout_with_spacing(indent_columns, 0),
        source,
    });
}

/// IP and TP both print authored tags; neither authorizes replacing arbitrary
/// glyphs with bullets. Require a complete named bullet, not a section-name,
/// styling, or adjacent-item heuristic: even `*` and `o` can name editor keys.
fn is_explicit_bullet(node: &Node, item: &DefinitionItem) -> bool {
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

/// Preserve literal tags without turning typographical marks into discovered
/// values or terms. Explicit bold/code marking is positive key-name evidence;
/// section names and the role of a containing option are not.
fn record_ambiguous_ip_mark(item: &DefinitionItem, context: &LoweringContext<'_>) {
    use crate::definitions::NativeHeadRole;
    use mant_ir::Inline;

    fn styled(inlines: &[Inline], in_style: bool) -> bool {
        inlines.iter().all(|inline| match inline {
            Inline::Anchor { .. } | Inline::Code { .. } => true,
            Inline::Text { value } => value.trim().is_empty() || in_style,
            Inline::Strong { children } => styled(children, true),
            Inline::Emphasis { children } | Inline::Link { children, .. } => {
                styled(children, in_style)
            }
            Inline::LineBreak => false,
        })
    }
    let [term] = item.terms.as_slice() else {
        return;
    };
    let text = plain_text(term);
    let mut chars = text.trim().chars();
    let Some(mark) = chars.next() else { return };
    if chars.next().is_some() || !mark.is_ascii() || (mark.is_ascii_alphanumeric() && mark != 'o') {
        return;
    }
    context.native_heads.borrow_mut().record(
        item,
        if styled(term, false) {
            NativeHeadRole::LiteralTerm
        } else {
            NativeHeadRole::Presentation
        },
    );
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem};

    #[test]
    fn ip_literal_marks_and_styled_keys_are_not_inferred_bullets() {
        for (mark, expected) in [
            ("*", "*"),
            ("o", "o"),
            ("#", "#"),
            ("=", "="),
            (r"\e", "\\"),
            ("^", "^"),
            ("$", "$"),
            ("+", "+"),
            ("-", "-"),
            (r"\fB*\fP", "*"),
            ("•", "•"),
        ] {
            let source = format!(".TH MARK 1\n.SH DESCRIPTION\n.IP \"{mark}\" 4\nBODY\n");
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("mark.1"),
                source.as_bytes(),
            )
            .unwrap();
            let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
                panic!("literal {mark:?} must retain its authored tag");
            };
            assert_eq!(super::plain_text(&items[0].terms[0]), expected);
        }
    }

    #[test]
    fn ip_named_bullets_retain_explicit_source_evidence() {
        for mark in [r"\(bu", r"\[bu]", r"\fB\[bu]\fP", r"\ \(bu"] {
            let source = format!(".TH MARK 1\n.SH DESCRIPTION\n.IP \"{mark}\" 4\nBODY\n");
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("mark.1"),
                source.as_bytes(),
            )
            .unwrap();
            assert!(
                matches!(&document.sections[0].blocks[0], Block::List { kind: mant_ir::ListKind::Bullet, items, .. } if items.len() == 1),
                "{mark:?}"
            );
        }
    }

    #[test]
    fn native_tq_merges_only_an_immediately_pending_empty_definition() {
        for (first_body, separator, expected_terms) in [
            ("", ".PD 0\n", vec![2]),
            ("FIRST BODY\n", "", vec![1, 1]),
            ("", ".PP\nBOUNDARY\n", vec![1, 1]),
        ] {
            let source = format!(
                ".TH STATE 1\n.SH DESCRIPTION\n.TP\n.B FIRST\n{first_body}{separator}.TQ\n.B SECOND\nSECOND BODY\n"
            );
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("tq-state.1"),
                source.as_bytes(),
            )
            .unwrap();
            let items: Vec<&DefinitionItem> = document.sections[0]
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::DefinitionList { items, .. } => Some(items),
                    _ => None,
                })
                .flatten()
                .collect();
            assert_eq!(
                items
                    .iter()
                    .map(|item| item.terms.len())
                    .collect::<Vec<_>>(),
                expected_terms,
                "{source}\n{document:?}"
            );
        }
    }
}
