//! Lower ordinary mdoc list items over the active formatter stream.

use super::super::super::{BlockLowerer, FormatterRowBoundary};
use super::{
    Block, ListItem, ListKind, LoweringContext, MdocListItem, Node, NodeKind, NormalizedListKind,
    ScopeFlow, attach_item_targets, first_part_children, layout, source_span,
};

fn plain_list_kind(node: &Node) -> ListKind {
    match node.list_kind {
        Some(NormalizedListKind::Ordered) => ListKind::Ordered { start: Some(1) },
        Some(NormalizedListKind::Dash) => ListKind::Dash,
        Some(NormalizedListKind::Plain) => ListKind::Plain,
        _ => ListKind::Bullet,
    }
}

pub(super) fn lower_mdoc_plain_list(
    node: &Node,
    items: Vec<MdocListItem<'_>>,
    context: &LoweringContext<'_>,
    list_indent: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Block {
    use crate::mandoc::layout::Distance;
    let kind = plain_list_kind(node);
    let width = node.width.as_deref().map_or(
        Distance::cells(if kind == ListKind::Plain { 0 } else { 2 }),
        |width| {
            context
                .measured_mdoc_distance(node, width, Distance::default())
                .add(Distance::cells(2))
                .0
        },
    );
    let offset = node
        .offset
        .as_deref()
        .map_or(Distance::default(), |offset| {
            context.measured_mdoc_distance(node, offset, Distance::default())
        });
    // An empty Bl executes only its own conditional newline. With no It
    // HEAD/BODY there is no field configuration or projection owner to enter.
    let saved = (!items.is_empty()).then(|| {
        formatter.execution.enter_native_field_scope();
        formatter.with_output_builder(&mut Vec::new(), |builder| {
            builder.enter_nested_list_scope(false)
        })
    });
    let block = Block::List {
        kind,
        compact: node.compact,
        items: items
            .into_iter()
            .enumerate()
            .map(|(index, item)| {
                let marker_width = match kind {
                    ListKind::Plain => 0,
                    ListKind::Bullet | ListKind::Dash => 2,
                    ListKind::Ordered { .. } => {
                        mant_ir::geometry::coordinate(mant_ir::geometry::text_width(&format!(
                            "{}. ",
                            kind.ordinal(index).unwrap_or(u64::MAX)
                        )))
                    }
                };
                let body_origin = context.offset_indent(item.node, list_indent, width);
                let measured_columns = body_origin.offset_from(list_indent);
                let body_columns = measured_columns.max(marker_width);
                // Visible cell rounding must not feed back into source state:
                // nested fractional offsets continue from this exact origin.
                let body_origin = context
                    .offset_indent(
                        item.node,
                        body_origin,
                        Distance::cells(body_columns.saturating_sub(measured_columns)),
                    )
                    .content_origin();
                context.lower_inline_with_spacing(
                    item.leading_controls,
                    formatter.spacing_enabled(),
                    formatter,
                );
                let mut blocks = lower_plain_item_body(
                    item.node,
                    context,
                    paragraph_distance,
                    formatter,
                    &ScopeFlow::body_post_row_end(
                        body_origin,
                        formatter.spacing_enabled(),
                        item_body_predecessor(
                            kind == ListKind::Plain,
                            index,
                            paragraph_predecessor,
                        ),
                    ),
                    PlainItemGeometry {
                        kind,
                        width_units: width.nonnegative_basic_units(),
                        offset_units: offset.basic_units(),
                        ordinal: kind.ordinal(index).unwrap_or(u64::MAX),
                    },
                    saved.expect("an actual It owns its field scope"),
                );
                // BODY post already executed its term_newln() in the scope
                // driver. Its following generic It flag clear precedes Bl
                // post and the next source word, independently of IR drain.
                formatter.execution.clear_native_list_part_flags();
                attach_item_targets(&mut blocks, &item, layout(body_origin));
                mant_ir::geometry::rebase_roots(&mut blocks, body_columns, marker_width);
                ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: source_span(item.node),
                    entry: None,
                    blocks,
                }
            })
            .collect(),
        layout: layout(list_indent),
        source: source_span(node),
    };
    if let Some(saved) = saved {
        formatter.with_output_builder(&mut Vec::new(), |builder| {
            builder.exit_nested_list_scope(saved, true);
        });
        formatter.execution.exit_native_field_scope();
    }
    block
}

#[derive(Clone, Copy)]
struct PlainItemGeometry {
    kind: ListKind,
    width_units: usize,
    offset_units: i32,
    ordinal: u64,
}

/// `blk_full()` may leave authored delimiters before HEAD. Execute them in
/// this item's normal block driver before the generated marker and skipped
/// HEAD, retaining the same pending glyphs, source rows and output owner.
fn lower_plain_item_body(
    node: &Node,
    context: &LoweringContext<'_>,
    paragraph_distance: &mut u16,
    formatter: &mut crate::mandoc::formatter::FormatterState,
    flow: &ScopeFlow<'_>,
    geometry: PlainItemGeometry,
    saved: crate::mandoc::inline::NestedListScope,
) -> Vec<Block> {
    let mut lowerer = BlockLowerer::new(
        context,
        flow.indent_columns,
        paragraph_distance,
        flow.spacing_enabled,
        Vec::new(),
        std::mem::take(formatter),
    );
    lowerer.paragraph_predecessor = flow.paragraph_predecessor;
    if let Some(head) = node
        .children
        .iter()
        .position(|part| part.kind == NodeKind::Head)
    {
        lowerer.push_nodes(&node.children[..head]);
    }
    // mdoc_term.c::termp_it_pre/post: HEAD consumes its own real marker
    // field before BODY changes offset. Only geometry is restored; its
    // word, device-row and flag effects survive the change of IR owner.
    lowerer.state.with_active_output_builder(|builder| {
        if let Some(head) = node
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Head)
        {
            builder.begin_executed_node(head);
            let native_kind = match geometry.kind {
                ListKind::Plain => NormalizedListKind::Plain,
                ListKind::Dash => NormalizedListKind::Dash,
                ListKind::Ordered { .. } => NormalizedListKind::Ordered,
                ListKind::Bullet => NormalizedListKind::Bullet,
            };
            builder.enter_nested_list_head(
                native_kind,
                None,
                geometry.width_units,
                geometry.offset_units,
                first_part_children(node, NodeKind::Body).is_empty(),
            );
            // The marker is represented by ListKind. Execute it in this
            // active vector, retaining any earlier delimiter/pending glyph,
            // then retire only its already-consumed projection.
            if geometry.kind == ListKind::Plain {
                builder.finish_nested_list_head(false);
            } else {
                builder.declare_structural_list_marker_origin();
                builder.execute_structural_marker(|builder| {
                    builder.append_native_list_marker(native_kind, geometry.ordinal);
                    builder.finish_nested_list_head(true);
                });
            }
            builder.execution.finish_executed_node(head);
            builder.restore_nested_list_geometry(saved);
        }
        if let Some(body) = node
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Body)
        {
            builder.begin_executed_node(body);
        }
        builder.begin_structural_list_body(saved, geometry.width_units, geometry.offset_units);
        builder
            .execution
            .set_output_owner_origin(flow.indent_columns.physical_basic_units());
    });
    lowerer.push_nodes(first_part_children(node, NodeKind::Body));
    // This return is the actual original It BODY post, not a generic
    // output-owner settle. It must consume the same live input even when
    // an outer definition keeps the formatter session alive afterwards.
    lowerer.finish_into(formatter, FormatterRowBoundary::MdocItemBody)
}

/// mandoc `print_bvspace` climbs first-child wrappers, except that a non-plain
/// It is itself a paragraph boundary even before its first body child.
/// Plain (-item) items instead inherit a preceding sibling/outer scope.
fn item_body_predecessor(plain: bool, index: usize, inherited: bool) -> bool {
    !plain || index > 0 || inherited
}
