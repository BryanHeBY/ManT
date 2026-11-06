//! Execute nested list structures in the caller's formatter stream.

use super::super::{Font, InlineBuilder, Node, NodeKind, append_inline_nodes};
use libmandoc_rs::{MacroToken::Mdoc, MdocMacro};

pub(super) fn append_column_list(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    // Bl BLOCK pre owns an ordinary term_newln(); its HEAD is not printed
    // (mdoc_term.c::termp_bl_pre,1129-1136). Keep this execution in the
    // caller's state instead of treating a structural HEAD child as text.
    builder.no_fill_source_line();
    let in_head = builder.has_definition_head();
    let saved = builder.enter_nested_list_scope(true);
    let Some(body) = node
        .children
        .iter()
        .find(|child| child.kind == NodeKind::Body)
    else {
        builder.exit_nested_list_scope(saved, false);
        return;
    };
    builder.begin_executed_node(body);
    let mut first_item = true;
    for item in &body.children {
        if item.kind != NodeKind::Block || item.macro_token.as_ref() != Some(&Mdoc(MdocMacro::It)) {
            append_inline_nodes(builder, std::slice::from_ref(item), name);
            continue;
        }
        builder.begin_executed_node(item);
        builder.no_fill_source_line();
        // print_bvspace() stops at the enclosing non-item It ancestor. Only
        // the first column item adds this row; later column items suppress
        // inter-item vspace (mdoc_term.c:583-628).
        if first_item && !node.compact && in_head {
            builder.native_vertical_space(1);
        }
        first_item = false;
        if let Some(head) = item
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Head)
        {
            builder.begin_executed_node(head);
            builder.enter_nested_column_part(true);
            builder.execution.finish_executed_node(head);
        }
        let parts: Vec<_> = item
            .children
            .iter()
            .filter(|part| part.kind == NodeKind::Body)
            .collect();
        for (index, part) in parts.iter().enumerate() {
            builder.begin_executed_node(part);
            builder.enter_nested_column_part(index + 1 == parts.len());
            let posts = builder.scope_posts.clone();
            posts.enter_body(part.id, builder.font.checkpoint());
            append_inline_nodes(builder, &part.children, name);
            if let Some(font) = posts.exit_body(part.id) {
                builder.font.pop_scope(font);
            }
            builder.finish_nested_column_part();
            builder.execution.finish_executed_node(part);
        }
        builder.execution.finish_executed_node(item);
    }
    builder.execution.finish_executed_node(body);
    // Bl BLOCK post calls term_newln() before resetting the default stops.
    builder.no_fill_source_line();
    super::super::display_tabs::exit_post(&mut builder.execution, node);
    builder.exit_nested_list_scope(saved, !first_item);
}

fn nested_list_distance(text: &str) -> crate::mandoc::layout::Distance {
    // a2width() treats an unscaled token as a printed width sample, not
    // as an implicit EN distance (mdoc_term.c:566-577). This reads a layout
    // fact; it never executes these sample escapes in the live formatter.
    crate::mandoc::layout::mdoc_distance(text).unwrap_or_default()
}

struct NestedListGeometry {
    kind: libmandoc_rs::NormalizedListKind,
    style: Option<libmandoc_rs::DefinitionListStyle>,
    head_width_units: usize,
    body_width_units: usize,
    offset_units: i32,
    flush_head: bool,
}

impl NestedListGeometry {
    fn from_node(node: &Node) -> Self {
        use crate::mandoc::layout::Distance;
        use libmandoc_rs::{DefinitionListStyle, NormalizedListKind};

        let kind = node.list_kind.unwrap_or(NormalizedListKind::Definition);
        let style = node.definition_list_style;
        let sized = matches!(
            kind,
            NormalizedListKind::Bullet | NormalizedListKind::Dash | NormalizedListKind::Ordered
        ) || matches!(
            style,
            Some(DefinitionListStyle::Tag | DefinitionListStyle::Hang)
        );
        let default_width = if matches!(
            style,
            Some(DefinitionListStyle::Tag | DefinitionListStyle::Hang)
        ) {
            8
        } else if sized {
            2
        } else {
            0
        };
        let width = node
            .width
            .as_deref()
            .map_or(Distance::cells(default_width), |width| {
                nested_list_distance(width).add(Distance::cells(2)).0
            });
        let head_width_units = width.nonnegative_basic_units();
        Self {
            kind,
            style,
            head_width_units,
            body_width_units: if sized { head_width_units } else { 0 },
            offset_units: node
                .offset
                .as_deref()
                .map_or(0, |offset| nested_list_distance(offset).basic_units()),
            flush_head: kind != NormalizedListKind::Plain
                && !matches!(
                    style,
                    Some(DefinitionListStyle::Inset | DefinitionListStyle::Diagnostic)
                ),
        }
    }
}

fn append_nested_list_head(
    builder: &mut InlineBuilder,
    head: &Node,
    geometry: &NestedListGeometry,
    ordinal: usize,
    name: Option<&str>,
) {
    use libmandoc_rs::{DefinitionListStyle, NormalizedListKind};

    let font = builder.font.checkpoint();
    match geometry.kind {
        NormalizedListKind::Bullet
        | NormalizedListKind::Dash
        | NormalizedListKind::Ordered
        | NormalizedListKind::Plain => {
            builder.append_native_list_marker(
                geometry.kind,
                u64::try_from(ordinal).unwrap_or(u64::MAX),
            );
        }
        _ if geometry.style == Some(DefinitionListStyle::Diagnostic) => {
            builder.with_font_scope(Font::Strong, |builder| {
                append_inline_nodes(builder, &head.children, name);
            });
        }
        _ => append_inline_nodes(builder, &head.children, name),
    }
    builder.font.pop_scope(font);
}

pub(super) fn append_nested_list(builder: &mut InlineBuilder, node: &Node, name: Option<&str>) {
    use libmandoc_rs::DefinitionListStyle;

    // Bl BLOCK pre, It BLOCK pre, each HEAD/BODY post, and Bl BLOCK post
    // are native execution phases even when their output belongs to an
    // outer definition term (mdoc_term.c:583-963,1129-1151). Keep the same
    // builder, field buffer, and source-entry dispatcher for all children.
    builder.no_fill_source_line();
    let saved = builder.enter_nested_list_scope(false);
    let geometry = NestedListGeometry::from_node(node);
    let Some(body) = node
        .children
        .iter()
        .find(|part| part.kind == NodeKind::Body)
    else {
        builder.exit_nested_list_scope(saved, false);
        return;
    };
    builder.begin_executed_node(body);
    let mut item_count = 0usize;
    let mut previous_body_empty = false;
    for item in &body.children {
        if item.kind != NodeKind::Block || item.macro_token.as_ref() != Some(&Mdoc(MdocMacro::It)) {
            append_inline_nodes(builder, std::slice::from_ref(item), name);
            continue;
        }
        builder.begin_executed_node(item);
        builder.no_fill_source_line();
        // First nested It reaches the enclosing non-item It ancestor. On
        // later items only bodyless diag suppresses inter-item vspace;
        // compact and negative .sp debt use the shared vertical executor.
        if !(node.compact
            || item_count > 0
                && geometry.style == Some(DefinitionListStyle::Diagnostic)
                && previous_body_empty)
        {
            builder.native_vertical_space(1);
        }
        item_count = item_count.saturating_add(1);
        let head = item
            .children
            .iter()
            .find(|part| part.kind == NodeKind::Head);
        let parts: Vec<_> = item
            .children
            .iter()
            .filter(|part| part.kind == NodeKind::Body)
            .collect();
        let body_empty = parts.iter().all(|part| part.children.is_empty());
        // blk_full() allocates direct DELIMO before HEAD. Execute those
        // siblings before HEAD pre selects NOSPACE, margin or diag font.
        let prefix_end = item
            .children
            .iter()
            .position(|part| part.kind == NodeKind::Head)
            .unwrap_or_default();
        append_inline_nodes(builder, &item.children[..prefix_end], name);
        if let Some(head) = head {
            builder.begin_executed_node(head);
            builder.enter_nested_list_head(
                geometry.kind,
                geometry.style,
                geometry.head_width_units,
                geometry.offset_units,
                body_empty,
            );
            append_nested_list_head(builder, head, &geometry, item_count, name);
            builder.finish_nested_list_head(geometry.flush_head);
            builder.execution.finish_executed_node(head);
            builder.restore_nested_list_geometry(saved);
        }
        for part in parts {
            builder.begin_executed_node(part);
            let fixed_cells = match geometry.style {
                Some(DefinitionListStyle::Inset)
                    if head.is_some_and(|head| !head.children.is_empty()) =>
                {
                    1
                }
                Some(DefinitionListStyle::Diagnostic) => 2,
                _ => 0,
            };
            builder.enter_nested_list_body(
                saved,
                geometry.body_width_units,
                geometry.offset_units,
                fixed_cells,
            );
            let posts = builder.scope_posts.clone();
            posts.enter_body(part.id, builder.font.checkpoint());
            append_inline_nodes(builder, &part.children, name);
            if let Some(font) = posts.exit_body(part.id) {
                builder.font.pop_scope(font);
            }
            builder.no_fill_source_line();
            builder.execution.finish_executed_node(part);
            builder.restore_nested_list_geometry(saved);
        }
        builder.execution.finish_executed_node(item);
        previous_body_empty = body_empty;
    }
    builder.execution.finish_executed_node(body);
    builder.no_fill_source_line();
    super::super::display_tabs::exit_post(&mut builder.execution, node);
    builder.exit_nested_list_scope(saved, item_count > 0);
}
