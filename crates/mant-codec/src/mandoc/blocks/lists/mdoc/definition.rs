//! Lower definition-list heads and bodies without changing their execution scopes.

use super::{
    Block, DefinitionFlow, DefinitionHeadFlow, DefinitionItem, DefinitionListStyle, ListItem,
    ListKind, LoweringContext, MdocListItem, Node, NodeKind, RunInHeadStyle, definition_item,
    first_part_children, layout, ordinal_sequence, source_span, targets,
};

pub(super) fn lower_mdoc_definition_list(
    node: &Node,
    items: Vec<MdocListItem<'_>>,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    list_indent: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Block {
    use crate::mandoc::layout::{DefinitionGeometry, Distance, TermPlacement};
    let width = node.width.as_deref().map_or(Distance::cells(8), |width| {
        context
            .measured_mdoc_distance(node, width, Distance::cells(6))
            .add(Distance::cells(2))
            .0
    });
    let head_field_columns = u16::try_from(width.position_columns().max(0)).unwrap_or(u16::MAX);
    let geometry = match node.definition_list_style {
        Some(DefinitionListStyle::Hang) => DefinitionGeometry {
            body: width,
            placement: TermPlacement::RunIn,
            gap: 1,
            native_head_field_units: Some(width.nonnegative_basic_units()),
            head_field_columns,
            relation_override: None,
        },
        Some(DefinitionListStyle::Inset) => DefinitionGeometry {
            body: Distance::default(),
            placement: TermPlacement::RunIn,
            gap: 1,
            native_head_field_units: None,
            head_field_columns: 0,
            relation_override: None,
        },
        Some(DefinitionListStyle::Diagnostic) => DefinitionGeometry {
            body: Distance::default(),
            placement: TermPlacement::RunIn,
            gap: 2,
            native_head_field_units: None,
            head_field_columns: 0,
            relation_override: None,
        },
        Some(DefinitionListStyle::Overhang) => DefinitionGeometry {
            body: Distance::default(),
            placement: TermPlacement::Stacked,
            gap: 0,
            native_head_field_units: None,
            head_field_columns: 0,
            relation_override: None,
        },
        _ => DefinitionGeometry {
            body: width,
            placement: TermPlacement::Fit,
            gap: 2,
            native_head_field_units: Some(width.nonnegative_basic_units()),
            head_field_columns,
            relation_override: None,
        },
    };
    let lowered_items = items
        .into_iter()
        .map(|item| {
            lower_mdoc_definition_item(
                &item,
                context,
                list_indent,
                paragraph_distance,
                geometry,
                node.definition_list_style,
                formatter,
            )
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
                .enumerate()
                .map(|(index, item)| {
                    let ordinal = ListKind::Ordered {
                        start: Some(first.value()),
                    }
                    .ordinal(index)
                    .unwrap_or(u64::MAX);
                    let marker_width = mant_ir::geometry::coordinate(
                        mant_ir::geometry::text_width(&format!("{ordinal}. ")),
                    );
                    mdoc_list_item_from_definition(item, marker_width, source_span(node))
                })
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

fn lower_mdoc_definition_item(
    item: &MdocListItem<'_>,
    context: &LoweringContext<'_>,
    list_indent: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    geometry: crate::mandoc::layout::DefinitionGeometry,
    style: Option<DefinitionListStyle>,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> DefinitionItem {
    context.lower_inline_with_spacing(
        item.leading_controls,
        formatter.spacing_enabled(),
        formatter,
    );
    let shares_pending_term_row = style != Some(DefinitionListStyle::Overhang);
    let head = first_part_children(item.node, NodeKind::Head);
    let mut lowered = definition_item(
        item.node,
        context,
        list_indent,
        paragraph_distance,
        geometry,
        DefinitionFlow {
            spacing_enabled: formatter.spacing_enabled(),
            paragraph_predecessor: true,
            shares_pending_term_row,
            head: match style {
                Some(DefinitionListStyle::Inset) => DefinitionHeadFlow::RunIn {
                    cells: u8::from(!head.is_empty()),
                    style: RunInHeadStyle::Plain,
                    // mdoc_term.c:795-797: inset sets no pad/break flags.
                    flags: crate::mandoc::inline::FieldFlags::inset(),
                },
                Some(DefinitionListStyle::Diagnostic) => DefinitionHeadFlow::RunIn {
                    cells: 2,
                    style: RunInHeadStyle::Strong,
                    // mdoc_term.c:827-831: NOBREAK|BRIND, no HANG.
                    flags: crate::mandoc::inline::FieldFlags::diag(),
                },
                Some(DefinitionListStyle::Tag) => DefinitionHeadFlow::Detached {
                    author_break_effect: crate::mandoc::inline::AuthorBreakEffect::Field {
                        gap_cells: 2,
                        body_width_columns: geometry.body_columns(),
                        field_width_columns: geometry.head_field_columns,
                        // mdoc_term.c:805-814: NOBREAK|BRTRSP|BRIND, plus
                        // HANG exactly when the item has no BODY.
                        flags: crate::mandoc::inline::FieldFlags::tag(
                            first_part_children(item.node, NodeKind::Body).is_empty(),
                        ),
                    },
                },
                Some(DefinitionListStyle::Hang) => DefinitionHeadFlow::Detached {
                    author_break_effect: crate::mandoc::inline::AuthorBreakEffect::Field {
                        gap_cells: 1,
                        body_width_columns: geometry.body_columns(),
                        field_width_columns: geometry.head_field_columns,
                        // mdoc_term.c:800-804: NOBREAK|BRIND|HANG.
                        flags: crate::mandoc::inline::FieldFlags::hang(),
                    },
                },
                _ => DefinitionHeadFlow::Detached {
                    author_break_effect: crate::mandoc::inline::AuthorBreakEffect::Line,
                },
            },
        },
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
}

/// Drop source-visible ordinal terms after a complete mdoc tag list has proved
/// ordered-list semantics, while retaining any navigation targets attached to
/// those terms at the same item position.
fn mdoc_list_item_from_definition(
    item: DefinitionItem,
    marker_width: i32,
    source: Option<mant_ir::SourceSpan>,
) -> ListItem {
    let DefinitionItem {
        source: item_source,
        terms,
        mut description,
        layout: definition_layout,
        ..
    } = item;
    mant_ir::geometry::rebase_roots(
        &mut description,
        definition_layout.body_indent_columns,
        marker_width,
    );
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
        mant_ir::LayoutHint::default(),
        owner_source,
    );
    ListItem {
        layout: mant_ir::ListItemLayout::default(),
        source: item_source,
        entry: None,
        blocks: description,
    }
}
