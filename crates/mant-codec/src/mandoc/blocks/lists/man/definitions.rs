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
        ip_run,
        has_predecessor,
    } = state;
    // pre_IP/pre_TP execute print_bvspace before HEAD and BODY words can
    // clear skipvsp. TQ requests no distance but retains its own tag role.
    let requested = if node.macro_name.as_deref() == Some("TQ") {
        0
    } else {
        *paragraph_distance
    };
    let spacing_before =
        crate::mandoc::layout::execute_man_paragraph_spacing(formatter, requested, has_predecessor);
    let LoweredManItem {
        mut item,
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
    if node.macro_name.as_deref() == Some("IP")
        && first_part_children(node, NodeKind::Head).is_empty()
    {
        context
            .native_heads
            .borrow_mut()
            .hanging
            .body(&mut item, std::ptr::from_ref(node) as usize);
    }
    // print_man_node() finishes the BLOCK after HEAD/BODY execution and
    // post_IP/post_TP; its Roman replacement updates fontlast as well.
    formatter.font.man_text_boundary();
    let macro_name = node.macro_name.as_deref();
    let independent_mark = matches!(macro_name, Some("IP" | "TP" | "TQ"))
        && ip_run.is_none()
        && record_mark_role(node, &item, context);
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
    // A TQ carrying an ambiguous punctuation mark is its own native DT/DD
    // pair. Joining it to a prior empty TP would attach its BODY to the
    // earlier declaration and invalidate its exact head-role evidence.
    let merge = if macro_name == Some("TQ") && !independent_mark {
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
            ip_run,
        },
    );
    if macro_name == Some("TQ")
        && !independent_mark
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
    ip_run: Option<IpRun>,
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
        ip_run,
    } = emission;
    if let Some(run) = ip_run {
        list_state.reset();
        append_ip_marked_list(output, item, indent_columns, spacing_before, source, run);
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
    let head = first_part_children(node, NodeKind::Head);
    let leading_head_distance = leading_paragraph_distance(head);
    if let Some(distance) = leading_head_distance {
        *paragraph_distance = distance;
    }
    update_man_definition_width(node, context, definition_hanging_width);
    let head_field_columns =
        u16::try_from(definition_hanging_width.position_columns().max(0)).unwrap_or(u16::MAX);
    let item = definition_item(
        node,
        context,
        indent_columns,
        paragraph_distance,
        crate::mandoc::layout::DefinitionGeometry {
            native_head_field_units: Some(definition_hanging_width.nonnegative_basic_units()),
            body: *definition_hanging_width,
            placement: crate::mandoc::layout::TermPlacement::Fit,
            gap: 1,
            head_field_columns,
            relation_override: None,
        },
        super::super::DefinitionFlow {
            spacing_enabled,
            // The detached body still follows the native tag row. This is
            // source-flow evidence even when the body has not emitted IR yet.
            paragraph_predecessor: true,
            shares_pending_term_row: true,
            head: super::super::DefinitionHeadFlow::Detached {
                // man_term.c::pre_IP/pre_TP configures NOBREAK before
                // the real HEAD post term_flushln. Its receipt determines
                // whether a BODY request can still close the tag row.
                author_break_effect: crate::mandoc::inline::AuthorBreakEffect::Field {
                    gap_cells: 1,
                    body_width_columns: head_field_columns,
                    field_width_columns: head_field_columns,
                    flags: crate::mandoc::inline::FieldFlags::man_head(
                        node.macro_name.as_deref() != Some("IP"),
                    ),
                },
            },
        },
        formatter,
    );
    let max_width = usize::try_from(
        item.layout
            .body_indent_columns
            .saturating_sub(i32::from(item.layout.min_term_gap_columns)),
    )
    .unwrap_or(0);
    LoweredManItem { item, max_width }
}

pub(in crate::mandoc::blocks) struct ManDefinitionState<'a> {
    pub(in crate::mandoc::blocks) paragraph_distance: &'a mut u16,
    pub(in crate::mandoc::blocks) output: &'a mut Vec<Block>,
    pub(in crate::mandoc::blocks) definition_hanging_width: &'a mut crate::mandoc::layout::Distance,
    pub(in crate::mandoc::blocks) list_state: &'a mut ManListState,
    pub(in crate::mandoc::blocks) ip_run: Option<IpRun>,
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
                if !terms_fit_inline(&item.terms, max_term_width) {
                    item.layout.head_body_relation = mant_ir::HeadBodyRelation::Separate;
                }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum IpMark {
    Star,
    Dash,
    NamedBullet,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc::blocks) struct IpRun {
    kind: ListKind,
    continues: bool,
}

/// CVS `man_html.c::list_continues()` inspects the first raw HEAD child of
/// adjacent IP blocks before deciding whether to open a UL. A single IP, a
/// mixed pair, or a styled mark remains a definition with its authored term.
pub(in crate::mandoc::blocks) fn adjacent_ip_run(nodes: &[Node], index: usize) -> Option<IpRun> {
    fn mark(node: &Node) -> Option<IpMark> {
        if node.kind != NodeKind::Block || node.macro_name.as_deref() != Some("IP") {
            return None;
        }
        match first_part_children(node, NodeKind::Head)
            .first()
            .and_then(|child| child.text.as_deref())?
        {
            "*" => Some(IpMark::Star),
            r"\-" => Some(IpMark::Dash),
            r"\(bu" | r"\[bu]" => Some(IpMark::NamedBullet),
            _ => None,
        }
    }
    let current = mark(nodes.get(index)?)?;
    let continues = nodes[..index]
        .iter()
        .rev()
        .find(|node| crate::mandoc::adjacency::is_logical_sibling(node))
        .and_then(mark)
        == Some(current);
    let followed =
        crate::mandoc::adjacency::next(&nodes[index + 1..]).and_then(mark) == Some(current);
    (continues || followed).then_some(IpRun {
        kind: if current == IpMark::Dash {
            ListKind::Dash
        } else {
            ListKind::Bullet
        },
        continues,
    })
}

/// Convert only a source-proven adjacent IP run to a list. A new run remains
/// separate even when the rendered glyph matches a prior run's list kind.
fn append_ip_marked_list(
    output: &mut Vec<Block>,
    item: DefinitionItem,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: u16,
    source: Option<mant_ir::SourceSpan>,
    run: IpRun,
) {
    let list_item = super::ordered::spaced_man_list_item(item, 2, source, paragraph_distance);
    if run.continues
        && let Some(Block::List {
            kind,
            compact,
            items,
            ..
        }) = output
            .last_mut()
            .filter(|block| block_indent(block) == Some(indent_columns.relative_columns()))
        && *kind == run.kind
    {
        *compact = *compact && paragraph_distance == 0;
        items.push(list_item);
        return;
    }

    output.push(Block::List {
        kind: run.kind,
        compact: paragraph_distance == 0,
        items: vec![list_item],
        layout: layout_with_spacing(indent_columns, 0),
        source,
    });
}

/// Preserve literal tags without turning typographical marks into discovered
/// values or terms. Explicit bold/code marking is positive key-name evidence;
/// section names and the role of a containing option are not.
fn record_mark_role(node: &Node, item: &DefinitionItem, context: &LoweringContext<'_>) -> bool {
    use crate::definitions::NativeHeadRole;
    use mant_ir::Inline;

    fn styled(inlines: &[Inline], in_style: bool) -> bool {
        inlines.iter().all(|inline| match inline {
            Inline::Anchor { .. } | Inline::Code { .. } | Inline::Equation { .. } => true,
            Inline::Text { value } => value.trim().is_empty() || in_style,
            Inline::Strong { children } => styled(children, true),
            Inline::Emphasis { children } | Inline::Link { children, .. } => {
                styled(children, in_style)
            }
            Inline::LineBreak { .. } => false,
        })
    }
    let [term] = item.terms.as_slice() else {
        return false;
    };
    let text = plain_text(term);
    // A named roff bullet is an authored DT in a singleton IP/TP, but still
    // only a presentation mark unless its source explicitly styles the term.
    // Require both its complete visible spelling and native escape evidence:
    // a literal Unicode bullet need not have the same source role.
    if text.trim() == "•"
        && super::super::definition::visible_definition_head(node)
            .iter()
            .any(contains_named_bullet_escape)
    {
        let role = if styled(term, false) {
            NativeHeadRole::LiteralTerm
        } else {
            NativeHeadRole::Presentation
        };
        context.native_heads.borrow_mut().record(item, role);
        return true;
    }
    let mut chars = text.trim().chars();
    let Some(mark) = chars.next() else {
        return false;
    };
    if chars.next().is_some() || !mark.is_ascii() || (mark.is_ascii_alphanumeric() && mark != 'o') {
        return false;
    }
    // An explicitly styled dash in a TP/TQ head is a complete shell operand
    // (for example, `set -`). An IP head uses the same visible DT for a
    // literal punctuation key, so retain that separate source role.
    // man_html.c::man_IP_pre() renders both spellings as authored tags.
    if mark == '-' && styled(term, false) && node.macro_name.as_deref() != Some("IP") {
        context
            .native_heads
            .borrow_mut()
            .record(item, NativeHeadRole::Operand);
        return true;
    }
    let role = if styled(term, false) {
        NativeHeadRole::LiteralTerm
    } else {
        NativeHeadRole::Presentation
    };
    context.native_heads.borrow_mut().record(item, role);
    true
}

fn contains_named_bullet_escape(node: &Node) -> bool {
    node.text
        .as_ref()
        .is_some_and(|text| text.contains(r"\(bu") || text.contains(r"\[bu]"))
        || node.children.iter().any(contains_named_bullet_escape)
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionItem};

    #[test]
    fn bold_single_dash_tp_retains_operand_identity() {
        // Exact source checked with pinned CVS tree/HTML/UTF-8. The TP HEAD
        // has a B child spelling `\-`; man_html.c::man_IP_pre prints that DT
        // and man_term.c::pre_B keeps the authored strong style.
        let source = b".TH SET 1\n.SH OPTIONS\n.TP\n.B \\-\nSignal the end of options.\n";
        let document = crate::mandoc::parse_plain_manual(std::path::Path::new("set.1"), source)
            .expect("lower styled dash operand");
        let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
            panic!("styled dash must retain its definition");
        };
        let entry = items[0].entry.as_ref().expect("styled dash operand");
        assert_eq!(entry.names, ["-"]);
        assert_eq!(
            entry.kind,
            mant_ir::EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Operand,
            }
        );
    }

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
    fn singleton_ip_bullet_stays_an_authored_definition_term() {
        // CVS man_html.c::list_continues() needs an adjacent compatible IP
        // before man_IP_pre() opens a UL; these exact inputs all yield DL.
        for (mark, literal_term) in [
            (r"\(bu", false),
            (r"\[bu]", false),
            (r"\fB\[bu]\fP", true),
            (r"\ \(bu", false),
        ] {
            let source = format!(".TH MARK 1\n.SH DESCRIPTION\n.IP \"{mark}\" 4\nBODY\n");
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("mark.1"),
                source.as_bytes(),
            )
            .unwrap();
            let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
                panic!("singleton {mark:?} must keep its authored tag");
            };
            assert_eq!(items.len(), 1);
            assert!(super::plain_text(&items[0].terms[0]).contains('•'));
            assert_eq!(items[0].entry.is_some(), literal_term, "{mark:?}");
        }
    }

    #[test]
    fn adjacent_ip_runs_follow_native_head_marker_identity() {
        // CVS man_html.c::list_continues() recognizes raw adjacent IP HEADs:
        // two escaped dashes form Bl-dash, star and named-bullet runs form
        // separate Bl-bullet lists, and mixed marks stay independent DLs.
        for (source, expected) in [
            (
                ".TH MARK 1\n.SH DESCRIPTION\n.IP \\- 2\nfirst\n.IP \\- 2\nsecond\n",
                vec![("dash", 2)],
            ),
            (
                ".TH MARK 1\n.SH DESCRIPTION\n.IP \\(bu 2\nFIRST\n.IP \\[bu] 2\nSECOND\n",
                vec![("bullet", 2)],
            ),
            (
                ".TH MARK 1\n.SH DESCRIPTION\n.IP * 2\nONE\n.IP * 2\nTWO\n.IP \\(bu 2\nTHREE\n.IP \\(bu 2\nFOUR\n",
                vec![("bullet", 2), ("bullet", 2)],
            ),
        ] {
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("ip-runs.1"),
                source.as_bytes(),
            )
            .expect("lower adjacent IP run");
            let actual = document.sections[0]
                .blocks
                .iter()
                .map(|block| match block {
                    Block::List { kind, items, .. } => (
                        match kind {
                            mant_ir::ListKind::Bullet => "bullet",
                            mant_ir::ListKind::Dash => "dash",
                            _ => "other",
                        },
                        items.len(),
                    ),
                    _ => ("definition", 0),
                })
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "{source}");
        }
    }

    #[test]
    fn mixed_ip_marks_and_single_tp_tq_bullets_keep_definition_owners() {
        // Pinned CVS man_html.c::man_IP_pre() opens two DLs for mixed IP
        // marks and one DL for a lone TP or TQ, even when a DT draws as a
        // bullet. man_macro.c::blk_imp opens the next-line HEAD for both.
        for source in [
            ".TH MARK 1\n.SH DESCRIPTION\n.IP \\(bu 2\nfirst\n.IP \\- 2\nsecond\n",
            ".TH MARK 1\n.SH DESCRIPTION\n.TP\n\\(bu\nBODY\n",
            ".TH MARK 1\n.SH OPTIONS\n.TQ\n\\(bu\nBODY\n",
        ] {
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("ip-mixed.1"),
                source.as_bytes(),
            )
            .expect("lower mixed IP or lone TP");
            assert!(
                document.sections[0]
                    .blocks
                    .iter()
                    .all(|block| matches!(block, Block::DefinitionList { .. }))
            );
            for block in &document.sections[0].blocks {
                if let Block::DefinitionList { items, .. } = block {
                    assert!(items.iter().all(|item| item.entry.is_none()), "{source}");
                }
            }
        }
    }

    #[test]
    fn singleton_presentation_bullet_does_not_hide_following_option_identity() {
        // Exact pinned CVS HTML: separate DLs for the bullet and --flag;
        // man_html.c::man_IP_pre() keeps the later head as its own DT.
        let source = b".TH MARK 1\n.SH OPTIONS\n.IP \\(bu 4\nBODY\n.TP\n.B --flag\nreal option\n";
        let document =
            crate::mandoc::parse_plain_manual(std::path::Path::new("bullet-option.1"), source)
                .expect("lower presentation bullet and option");
        let entries = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::DefinitionList { items, .. } => Some(items),
                _ => None,
            })
            .flat_map(|items| items.iter())
            .map(|item| item.entry.as_ref())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_none());
        assert!(
            matches!(entries[1], Some(entry) if entry.names.iter().any(|name| name == "--flag"))
        );
    }

    #[test]
    fn tq_presentation_bullet_does_not_become_an_option_alias() {
        // Exact source checked with pinned CVS tree/HTML/UTF-8. The TP HEAD
        // carries --flag, while the following TQ HEAD is a separate visible
        // bullet DT; man_html.c::man_IP_pre never treats it as a name.
        let source = b".TH MARK 1\n.SH OPTIONS\n.TP\n.B --flag\n.TQ\n\\(bu\nBODY\n";
        let document =
            crate::mandoc::parse_plain_manual(std::path::Path::new("tq-combined.1"), source)
                .expect("lower option followed by TQ bullet");
        let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
            panic!("TP and TQ retain their authored definition rows");
        };
        assert_eq!(items.len(), 2);
        assert!(items[0].description.is_empty(), "TP has its own empty DD");
        assert!(items[1].entry.is_none(), "TQ bullet is presentation");
        assert!(items[1].description.iter().any(|block| matches!(
            block,
            Block::Paragraph { children, .. } if super::plain_text(children) == "BODY"
        )));
        let names = document.sections[0]
            .blocks
            .iter()
            .filter_map(|block| match block {
                Block::DefinitionList { items, .. } => Some(items),
                _ => None,
            })
            .flat_map(|items| items.iter())
            .filter_map(|item| item.entry.as_ref())
            .flat_map(|entry| entry.names.iter())
            .collect::<Vec<_>>();
        assert_eq!(names, ["--flag"]);
    }

    #[test]
    fn tq_styled_punctuation_keeps_its_own_term_and_body() {
        // Both exact inputs were checked with pinned CVS tree/HTML/UTF-8:
        // man_html.c::man_IP_pre emits two DT/DD pairs, with an empty first
        // DD. Operand versus Term is ManT's source-aware reading category.
        for (head, name, kind) in [
            (
                r".B \-",
                "-",
                mant_ir::EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Operand,
                },
            ),
            (r".B #", "#", mant_ir::EntryKind::Term),
        ] {
            let source = format!(".TH MARK 1\n.SH OPTIONS\n.TP\n.B --foo\n.TQ\n{head}\nBODY\n");
            let document = crate::mandoc::parse_plain_manual(
                std::path::Path::new("tq-styled.1"),
                source.as_bytes(),
            )
            .expect("lower styled TQ head");
            let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
                panic!("expected separate TP and TQ rows");
            };
            assert_eq!(items.len(), 2, "{source}");
            assert!(items[0].description.is_empty(), "{source}");
            let entry = items[1].entry.as_ref().expect("styled TQ key");
            assert_eq!(entry.names, [name], "{source}");
            assert_eq!(entry.kind, kind, "{source}");
            assert!(items[1].description.iter().any(|block| matches!(
                block,
                Block::Paragraph { children, .. } if super::plain_text(children) == "BODY"
            )));
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
