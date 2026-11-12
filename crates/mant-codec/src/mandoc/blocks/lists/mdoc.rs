//! Lowers mdoc(7) `.Bl` and `.It` list structures.

use super::{
    Block, DefinitionFlow, DefinitionHeadFlow, DefinitionItem, DefinitionListStyle, Inline,
    ListItem, ListKind, LoweringContext, Node, NodeKind, NormalizedListKind, RunInHeadStyle,
    ScopeFlow, definition_item, first_part_children, layout, ordinal_sequence, source_span,
    targets,
};
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

mod column;
mod definition;
mod items;
mod plain;

use definition::lower_mdoc_definition_list;
use items::{
    MdocListItem, MdocListItems, append_list_targets, attach_item_targets, is_mdoc_list_item,
    mdoc_list_items,
};
use plain::lower_mdoc_plain_list;

/// Only normalized `It` blocks execute `termp_it_pre`'s paragraph distance.
/// Navigation-only IR items inserted below do not establish this source fact.
pub(in crate::mandoc::blocks) fn has_native_mdoc_list_items(node: &Node) -> bool {
    first_part_children(node, NodeKind::Body)
        .iter()
        .any(is_mdoc_list_item)
}

pub(in crate::mandoc::blocks) fn lower_mdoc_list(
    node: &Node,
    context: &LoweringContext<'_>,
    indent_columns: crate::mandoc::layout::SourceIndent,
    paragraph_distance: &mut u16,
    initial_spacing: bool,
    paragraph_predecessor: bool,
    formatter: &mut crate::mandoc::formatter::FormatterState,
) -> Block {
    let MdocListItems {
        items,
        trailing_targets,
        trailing_controls,
    } = mdoc_list_items(node, context);
    formatter.set_spacing_enabled(initial_spacing);
    let is_definition = matches!(
        node.list_kind,
        Some(NormalizedListKind::Definition | NormalizedListKind::Column)
    ) || (node.list_kind.is_none()
        && items
            .iter()
            .any(|item| !first_part_children(item.node, NodeKind::Head).is_empty()));
    let offset =
        node.offset
            .as_deref()
            .map_or(crate::mandoc::layout::Distance::default(), |offset| {
                context.measured_mdoc_distance(
                    node,
                    offset,
                    crate::mandoc::layout::Distance::default(),
                )
            });
    let list_indent = context.offset_indent(node, indent_columns, offset);
    let mut block = if node.list_kind == Some(NormalizedListKind::Column) {
        column::lower_mdoc_column_list(
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
        // List marker geometry belongs to this source scope. Rebase its
        // inherited offset correction into the list's normal parent rather
        // than shifting only the label and leaving the visible marker at
        // the old origin (termp_it_pre HEAD/BODY offset, mdoc_term.c:874).
        // The independent ti remains live until the actual BODY endline.
        let shifted_indent = context.offset_indent(
            node,
            list_indent,
            crate::mandoc::layout::Distance::from_basic_units(
                formatter.row_presentation.offset_shift,
            ),
        );
        formatter.row_presentation.offset_shift = 0;
        lower_mdoc_plain_list(
            node,
            items,
            context,
            shifted_indent,
            paragraph_distance,
            paragraph_predecessor,
            formatter,
        )
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
    context.lower_inline_with_spacing(trailing_controls, formatter.spacing_enabled(), formatter);
    crate::mandoc::inline::display_tabs::exit_post(&mut formatter.execution, node);
    block
}
