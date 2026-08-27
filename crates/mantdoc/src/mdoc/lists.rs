use super::{
    ArgumentPlacement, BTreeMap, BTreeSet, DocumentBuilder, NodeFlags, NodeId, NodeKind,
    NormalizedListKind, Recovery, ScopeFrame, SourceSpan, StructureOutcome,
    clear_leading_explicit_partial_punctuation, coalesce_adjacent_text_children,
    coalesce_text_children, explicit_partial_block_close, inline_target_name,
    insert_generated_system_name, is_explicit_partial_close, mark_manual_target, mark_permalink,
    move_explicit_leading_open_delimiter, node_arguments, split_mdoc_inline_children,
    split_mdoc_inline_tokens, split_mdoc_inline_tokens_with_options,
    structure_nested_implicit_partial_blocks,
};

/// Return the live `It` row when the current source-flow body belongs to a
/// `Bl -column` list.  The parser keeps this relationship in the arena rather
/// than a global mutable row pointer, so nested scopes cannot leak a target to
/// an unrelated list.
pub(super) fn active_column_item(builder: &DocumentBuilder, active_body: NodeId) -> Option<NodeId> {
    if builder.node_kind(active_body) != Some(NodeKind::Body)
        || builder.node_macro_name(active_body) != Some("It")
    {
        return None;
    }
    let item = builder.node_parent(active_body)?;
    if builder.node_kind(item) != Some(NodeKind::Block)
        || builder.node_macro_name(item) != Some("It")
    {
        return None;
    }
    let list_body = builder.node_parent(item)?;
    if builder.node_kind(list_body) == Some(NodeKind::Body)
        && builder.node_macro_name(list_body) == Some("Bl")
        && builder.node_list_kind(list_body) == Some(NormalizedListKind::Column)
    {
        Some(item)
    } else {
        None
    }
}

/// Return the `It` block when the innermost scope crossed by a list closer is
/// an explicit partial block opened from that item's Head.
pub(super) fn item_header_partial_scope(
    builder: &DocumentBuilder,
    scopes: &[ScopeFrame],
    list_index: usize,
) -> Option<NodeId> {
    if builder.node_list_kind(scopes.get(list_index)?.body) != Some(NormalizedListKind::Ordered) {
        return None;
    }
    let partial = scopes.get(list_index + 1)?;
    if !is_explicit_partial_close(partial.close) {
        return None;
    }
    let head = builder.node_parent(partial.open)?;
    if builder.node_kind(head) != Some(NodeKind::Head)
        || builder.node_macro_name(head) != Some("It")
    {
        return None;
    }
    let item = builder.node_parent(head)?;
    (builder.node_kind(item) == Some(NodeKind::Block)
        && builder.node_macro_name(item) == Some("It"))
    .then_some(item)
}

/// Remove the deferred body of an `It` whose header was left open across a
/// malformed list close.  Its visible header and nested partial block remain
/// attached to the list, matching mandoc's finite recovery tree.
pub(super) fn discard_item_body(builder: &mut DocumentBuilder, item: NodeId) {
    let Some(children) = builder.children(item).map(<[NodeId]>::to_vec) else {
        return;
    };
    let retained = children
        .into_iter()
        .filter(|child| {
            !(builder.node_kind(*child) == Some(NodeKind::Body)
                && builder.node_macro_name(*child) == Some("It"))
        })
        .collect::<Vec<_>>();
    let _ = builder.replace_children(item, &retained);
}

/// Build the delayed item findings for the one post-`El` malformed shape
/// where mandoc leaves an ordered list and its header partial scope open.
pub(super) fn broken_item_recoveries(
    builder: &DocumentBuilder,
    list: ScopeFrame,
    item: NodeId,
) -> Vec<Recovery> {
    if builder.node_list_kind(list.body) != Some(NormalizedListKind::Ordered) {
        return Vec::new();
    }
    let Some(head) = builder.children(item).and_then(|children| {
        children.iter().copied().find(|child| {
            builder.node_kind(*child) == Some(NodeKind::Head)
                && builder.node_macro_name(*child) == Some("It")
        })
    }) else {
        return Vec::new();
    };
    let arguments = node_arguments(builder, head).join(" ");
    let location = builder.node_location(item);
    let mut recoveries = vec![Recovery::EmptyListItem {
        list_type: "enum",
        location: location.clone(),
    }];
    if !arguments.is_empty() {
        recoveries.push(Recovery::InvalidArguments {
            message: format!("skipping all arguments: It {arguments}").into(),
            location,
        });
    }
    recoveries
}

/// Move direct list content preceding the first item into the surrounding
/// flow, immediately before the list block.  mdoc performs this recovery when
/// that first `.It` interrupts an active nested scope.
pub(super) fn move_initial_list_content_out(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    list: ScopeFrame,
) -> Vec<Recovery> {
    let Some(list_children) = builder.children(list.body).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    if list_children
        .iter()
        .any(|child| builder.node_macro_name(*child) == Some("It"))
    {
        return Vec::new();
    }
    if list_children.is_empty() {
        return Vec::new();
    }
    // A trailing `Sm on`/`Sm off` or explicit `Tg` controls the first item's
    // spacing/destination and stays in the list. Other direct content,
    // including an earlier spacing change that belongs to malformed prose,
    // moves before the block.
    let retained_start = list_children
        .iter()
        .rposition(|child| !list_content_stays_with_first_item(builder, *child))
        .map_or(0, |index| index + 1);
    let (moved, retained) = list_children.split_at(retained_start);
    let moved = moved.to_vec();
    let retained = retained.to_vec();
    if moved.is_empty() {
        return Vec::new();
    }
    if !builder.replace_children(list.body, &retained) {
        return Vec::new();
    }

    if list.resume_flow == root {
        let Some(index) = root_children.iter().position(|child| *child == list.open) else {
            return Vec::new();
        };
        root_children.splice(index..index, moved.iter().copied());
    } else {
        let Some(parent_children) = builder.children(list.resume_flow).map(<[NodeId]>::to_vec)
        else {
            return Vec::new();
        };
        let Some(index) = parent_children.iter().position(|child| *child == list.open) else {
            return Vec::new();
        };
        let mut reordered = parent_children;
        reordered.splice(index..index, moved.iter().copied());
        if !builder.replace_children(list.resume_flow, &reordered) {
            return Vec::new();
        }
    }
    list_content_recoveries(builder, &moved)
}

/// Trailing item controls belong to the following item flow rather than to
/// the malformed prefix of a list. This mirrors mandoc's `post_bl()` recovery
/// ordering.
pub(super) fn list_content_stays_with_first_item(builder: &DocumentBuilder, child: NodeId) -> bool {
    match builder.node_macro_name(child) {
        Some("Tg") => true,
        Some("Sm") => builder.children(child).is_some_and(|children| {
            children.len() == 1 && matches!(builder.node_text(children[0]), Some("on" | "off"))
        }),
        _ => false,
    }
}

/// Collect the delayed warnings for direct list content that mdoc moves back
/// into surrounding flow when the first `.It` breaks an open nested block.
pub(super) fn list_content_recoveries(
    builder: &DocumentBuilder,
    children: &[NodeId],
) -> Vec<Recovery> {
    children
        .iter()
        .copied()
        .filter_map(|child| {
            let content = if builder.node_kind(child) == Some(NodeKind::Text) {
                Some("text".to_owned())
            } else {
                builder.node_macro_name(child).map(str::to_owned)
            }?;
            Some(Recovery::ContentOutsideList {
                content: content.into_boxed_str(),
                location: builder.node_location(child),
            })
        })
        .collect()
}

/// Whether the current source-flow parent is the body of a `Bl -column`
/// list, before that input has established an explicit or implicit item row.
pub(super) fn active_column_list(builder: &DocumentBuilder, active_body: NodeId) -> bool {
    builder.node_kind(active_body) == Some(NodeKind::Body)
        && builder.node_macro_name(active_body) == Some("Bl")
        && builder.node_list_kind(active_body) == Some(NormalizedListKind::Column)
}

/// Attach a consecutive tbl row to the synthetic column-list item created for
/// the preceding tbl row.  The empty head distinguishes this form from a
/// normal `.It` header; limiting the body's children to tables prevents a
/// later ordinary source line from being swallowed into the same row.
pub(super) fn append_implicit_column_table_row(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    table: NodeId,
) -> bool {
    let Some(item) = builder
        .children(list_body)
        .and_then(<[NodeId]>::last)
        .copied()
    else {
        return false;
    };
    if builder.node_kind(item) != Some(NodeKind::Block)
        || builder.node_macro_name(item) != Some("It")
    {
        return false;
    }
    let Some(children) = builder.children(item) else {
        return false;
    };
    let Some((head, body)) = children
        .split_first()
        .and_then(|(head, rest)| rest.first().map(|body| (*head, *body)))
    else {
        return false;
    };
    if builder.node_kind(head) != Some(NodeKind::Head)
        || builder.node_macro_name(head) != Some("It")
        || builder
            .children(head)
            .is_none_or(|children| !children.is_empty())
        || builder.node_kind(body) != Some(NodeKind::Body)
        || builder.node_macro_name(body) != Some("It")
    {
        return false;
    }
    let Some(body_children) = builder.children(body) else {
        return false;
    };
    if body_children.is_empty()
        || !body_children
            .iter()
            .all(|child| builder.node_kind(*child) == Some(NodeKind::Table))
    {
        return false;
    }
    builder.append_existing_child(body, table)
}

/// Materialize the first tbl row in a `Bl -column` body as the implicit `It`
/// that mandoc exposes in its owned tree.  The table already carries its
/// source location and presentation flags, while the synthetic item, head,
/// and body inherit only structural location information.
pub(super) fn structure_implicit_column_table_item(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    table: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> bool {
    let location = builder.node_location(table);
    if builder.node_count().saturating_add(3) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return false;
    }
    let Some(item) = builder.push(list_body, NodeKind::Block) else {
        return false;
    };
    let Some(head) = builder.push(item, NodeKind::Head) else {
        return false;
    };
    let Some(body) = builder.push(item, NodeKind::Body) else {
        return false;
    };
    if !builder.macro_name(item, "It")
        || !builder.macro_name(head, "It")
        || !builder.macro_name(body, "It")
        || !builder.set_node_location(item, location.clone())
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(body, location)
        || !builder.replace_children(item, &[head, body])
        || !builder.replace_children(body, &[table])
    {
        return false;
    }
    true
}

/// Inline mdoc forms accepted as a row when a `Bl -column` source omits
/// `.It`. Structural controls retain their ordinary dispatch rather than
/// becoming accidental cells.
pub(super) fn is_implicit_column_row_macro(name: Option<&str>) -> bool {
    matches!(
        name,
        Some("Cm" | "Dv" | "Em" | "Er" | "Ev" | "Fl" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va")
    )
}

/// Materialize one `Bl -column` row whose source omitted the usual `.It`.
///
/// The first cell retains an authored mdoc element or literal text node.  The
/// remaining cells begin at in-line `Ta` controls or literal tab boundaries,
/// exactly like the explicit-item splitter above.  This deliberately runs
/// before the global Em/Sy fallback tag pass, allowing the implicit `It` to
/// own the destination while the inline element keeps its permalink.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) fn structure_implicit_column_item(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    scopes: &mut Vec<ScopeFrame>,
) -> bool {
    let location = builder.node_location(node);
    let source_flags = builder.node_flags(node).unwrap_or_default();
    let inline_ta_text_count = (builder.node_kind(node) != Some(NodeKind::Text))
        .then(|| builder.children(node))
        .flatten()
        .map_or(0, |children| {
            children
                .iter()
                .filter_map(|child| builder.node_text(*child))
                .map(inline_column_ta_count)
                .sum::<usize>()
        });
    let element_cells = (builder.node_kind(node) != Some(NodeKind::Text))
        .then(|| builder.children(node))
        .flatten()
        .map(|children| {
            1 + children
                .iter()
                .filter(|child| builder.node_text(**child) == Some("Ta"))
                .count()
                + inline_ta_text_count
        });
    let text_cells = builder
        .node_text(node)
        .filter(|text| text.contains('\t'))
        .map(|text| text.split('\t').count());
    let Some(cell_count) = element_cells.or(text_cells) else {
        return false;
    };
    // Block + Head + one Body per cell. Literal text additionally needs one
    // new node for every cell after the first.
    let additional_nodes = 2_usize
        .saturating_add(cell_count)
        .saturating_add(text_cells.unwrap_or(1).saturating_sub(1))
        .saturating_add(inline_ta_text_count);
    if builder.node_count().saturating_add(additional_nodes) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return false;
    }

    let Some(item) = builder.push(list_body, NodeKind::Block) else {
        return false;
    };
    let Some(head) = builder.push(item, NodeKind::Head) else {
        return false;
    };
    let Some(first_body) = builder.push(item, NodeKind::Body) else {
        return false;
    };
    if !builder.macro_name(item, "It")
        || !builder.macro_name(head, "It")
        || !builder.macro_name(first_body, "It")
        || !builder.set_node_location(item, location.clone())
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(first_body, location.clone())
        || !builder.replace_children(item, &[head, first_body])
    {
        return false;
    }
    let mut item_flags = source_flags;
    item_flags.deep_link_target = false;
    item_flags.permalink = false;
    let _ = builder.set_node_flags(item, item_flags);
    let mut child_flags = source_flags;
    child_flags.line_start = false;
    child_flags.deep_link_target = false;
    child_flags.permalink = false;
    let _ = builder.set_node_flags(node, child_flags);

    if builder.node_kind(node) != Some(NodeKind::Text)
        && let Some(tokens) = builder.children(node).map(<[NodeId]>::to_vec)
    {
        let mut cells = vec![Vec::new()];
        let mut body_locations = vec![location];
        for token in tokens {
            if builder.node_text(token) == Some("Ta") {
                cells.push(Vec::new());
                body_locations.push(builder.node_location(token));
            } else if let Some((prefix, suffix, separator_end)) = builder
                .node_text(token)
                .and_then(split_inline_column_ta_argument)
                .map(|(prefix, suffix, separator_end)| {
                    (prefix.to_owned(), suffix.to_owned(), separator_end)
                })
            {
                let token_location = builder.node_location(token);
                let prefix_length = prefix.len();
                let _ = builder.set_node_text(token, prefix);
                cells
                    .last_mut()
                    .expect("implicit column row has one cell")
                    .push(token);
                cells.push(Vec::new());
                let separator_location = token_location.as_ref().and_then(|span| {
                    let start = span
                        .start
                        .checked_add(u32::try_from(prefix_length + 1).ok()?)?;
                    SourceSpan::new(span.source, start, span.end).ok()
                });
                body_locations.push(separator_location);
                let Some(tail) = builder.push(node, NodeKind::Text) else {
                    return false;
                };
                let tail_location = token_location.and_then(|span| {
                    let start = span.start.checked_add(u32::try_from(separator_end).ok()?)?;
                    SourceSpan::new(span.source, start, span.end).ok()
                });
                if !builder.text(tail, suffix)
                    || !builder.set_node_location(tail, tail_location)
                    || !builder.set_node_flags(tail, NodeFlags::default())
                {
                    return false;
                }
                cells
                    .last_mut()
                    .expect("implicit column row has one cell")
                    .push(tail);
            } else {
                cells
                    .last_mut()
                    .expect("implicit column row has one cell")
                    .push(token);
            }
        }
        let first = cells.remove(0);
        if !builder.replace_children(node, &first) || !builder.replace_children(first_body, &[node])
        {
            return false;
        }
        let mut bodies = vec![first_body];
        for (tokens, cell_location) in cells.into_iter().zip(body_locations.into_iter().skip(1)) {
            let Some(body) = builder.push(item, NodeKind::Body) else {
                return false;
            };
            if !builder.macro_name(body, "It")
                || !builder.set_node_location(body, cell_location)
                || !builder.set_node_flags(body, NodeFlags::default())
            {
                return false;
            }
            let events = split_mdoc_inline_tokens(
                builder,
                body,
                &tokens,
                spacing_enabled,
                max_nodes,
                outcome,
            );
            let _ = builder.replace_children(body, &events);
            structure_nested_implicit_partial_blocks(
                builder,
                body,
                max_nodes,
                outcome,
                spacing_enabled,
            );
            structure_column_cell_explicit_partials(
                builder,
                body,
                max_nodes,
                outcome,
                spacing_enabled,
                scopes,
            );
            bodies.push(body);
        }
        let mut children = vec![head];
        children.extend(bodies);
        let _ = builder.replace_children(item, &children);
        if matches!(builder.node_macro_name(node), Some("Em" | "Sy"))
            && let Some((tag, explicit)) = inline_target_name(builder, node)
        {
            mark_manual_target(builder, item, &tag);
            mark_permalink(builder, node, explicit.then_some(tag.as_str()));
        }
        return true;
    }

    let Some(text) = builder.node_text(node).map(str::to_owned) else {
        return false;
    };
    let Some(text_location) = location else {
        return false;
    };
    let mut bodies = vec![first_body];
    let mut offset = 0_usize;
    for (index, value) in text.split('\t').enumerate() {
        let start = text_location
            .start
            .saturating_add(u32::try_from(offset).unwrap_or(u32::MAX));
        let end = start.saturating_add(u32::try_from(value.len()).unwrap_or(u32::MAX));
        if index == 0 {
            let _ = builder.set_node_text(node, value);
            let _ = builder
                .set_node_location(node, SourceSpan::new(text_location.source, start, end).ok());
            let _ = builder.replace_children(first_body, &[node]);
        } else {
            let Some(body) = builder.push(item, NodeKind::Body) else {
                return false;
            };
            let Some(cell) = builder.push(body, NodeKind::Text) else {
                return false;
            };
            if !builder.macro_name(body, "It")
                || !builder.set_node_location(body, Some(text_location.clone()))
                || !builder.text(cell, value)
                || !builder
                    .set_node_location(cell, SourceSpan::new(text_location.source, start, end).ok())
                || !builder.set_node_flags(cell, NodeFlags::default())
            {
                return false;
            }
            bodies.push(body);
        }
        offset = offset.saturating_add(value.len()).saturating_add(1);
    }
    let mut children = vec![head];
    children.extend(bodies);
    let _ = builder.replace_children(item, &children);
    true
}

/// Count unescaped, whole-word `Ta` spellings that mdoc's inline parser has
/// already coalesced into one argument phrase. A standalone token is handled
/// by the main splitter, so this only counts embedded forms such as `c Ta d`.
pub(super) fn inline_column_ta_count(text: &str) -> usize {
    usize::from(split_inline_column_ta_argument(text).is_some())
}

/// Split the first embedded ` Ta ` phrase separator while preserving the
/// exact byte offset used to source-locate the following cell.
pub(super) fn split_inline_column_ta_argument(text: &str) -> Option<(&str, &str, usize)> {
    let (prefix, suffix) = text.split_once(" Ta ")?;
    (!prefix.is_empty() && !suffix.is_empty()).then_some((prefix, suffix, prefix.len() + 4))
}

pub(super) fn make_block(
    builder: &mut DocumentBuilder,
    block: NodeId,
    macro_name: &str,
    placement: ArgumentPlacement,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<(NodeId, NodeId)> {
    if builder.node_kind(block) != Some(NodeKind::Element) {
        return None;
    }
    if builder.node_count().saturating_add(2) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(block);
        }
        return None;
    }
    if matches!(placement, ArgumentPlacement::Body) {
        coalesce_text_children(builder, block);
    }
    let arguments = builder.children(block)?.to_vec();
    let location = builder.node_location(block);
    let head = builder.push(block, NodeKind::Head)?;
    let body = builder.push(block, NodeKind::Body)?;
    if !builder.set_node_kind(block, NodeKind::Block)
        || !builder.macro_name(block, macro_name)
        || !builder.macro_name(head, macro_name)
        || !builder.macro_name(body, macro_name)
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(body, location)
        || !builder.replace_children(block, &[head, body])
    {
        return None;
    }
    match placement {
        ArgumentPlacement::Head => {
            let _ = builder.replace_children(head, &arguments);
        }
        ArgumentPlacement::Body | ArgumentPlacement::BodyTokens => {
            let _ = builder.replace_children(body, &arguments);
        }
        ArgumentPlacement::Drop => {}
    }
    Some((head, body))
}

/// Rebuild one `Bl -column` item as the legacy sequence of `It` bodies.
///
/// Column-list arguments are phrases rather than a normal `It` head.  `Ta`
/// is an in-line request separating phrases, and a tab ends the current
/// phrase even though generic argument lexing otherwise treats it like a
/// space.  The scanner records that delimiter privately so the public arena
/// can remain source-agnostic after this package pass finishes.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Ordered column-cell recovery mirrors libmandoc's stateful parser without exposing scanner provenance publicly.
pub(super) fn split_column_item_cells(
    builder: &mut DocumentBuilder,
    item: NodeId,
    head: NodeId,
    first_body: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    scopes: &mut Vec<ScopeFrame>,
) -> Option<Vec<NodeId>> {
    let tokens = builder.children(head)?.to_vec();
    let item_location = builder.node_location(item);
    let additional_text_nodes = tokens
        .iter()
        .filter_map(|token| builder.node_text(*token))
        .map(|text| memchr::memchr_iter(b'\t', text.as_bytes()).count())
        .sum::<usize>();
    let mut cells = vec![Vec::new()];
    let mut suppress_first_tab_column_system_name = vec![false];
    let mut leading_tab_padding = vec![false];
    let mut terminal_tab_cell_padding = vec![false];
    let mut body_locations = vec![item_location.clone()];
    let token_count = tokens.len();
    for (token_index, token) in tokens.into_iter().enumerate() {
        if builder.node_text(token) == Some("Ta") {
            cells.push(Vec::new());
            suppress_first_tab_column_system_name.push(false);
            leading_tab_padding.push(false);
            terminal_tab_cell_padding.push(false);
            body_locations.push(builder.node_location(token));
            continue;
        }
        let tab_segments = split_column_tab_token(builder, item, token)?;
        for (index, segment) in tab_segments.into_iter().enumerate() {
            if index > 0 {
                let has_leading_space = builder
                    .node_text(segment)
                    .is_some_and(|text| text.starts_with(' '));
                cells.push(Vec::new());
                suppress_first_tab_column_system_name.push(!has_leading_space);
                leading_tab_padding.push(has_leading_space);
                terminal_tab_cell_padding.push(false);
                // A phrase begun by an in-token tab uses the original `.It`
                // position just like one begun by an ordinary tab separator.
                body_locations.push(item_location.clone());
            }
            cells
                .last_mut()
                .expect("column items always have a first cell")
                .push(segment);
        }
        if builder.node_separator_contains_tab(token) {
            let has_leading_tab_padding = builder.node_separator_after(token) == Some(b'\t')
                && builder.node_separator_width(token) > 1;
            cells.push(Vec::new());
            suppress_first_tab_column_system_name.push(
                builder.node_separator_after(token) == Some(b'\t')
                    && builder.node_separator_width(token) == 1,
            );
            leading_tab_padding.push(has_leading_tab_padding);
            terminal_tab_cell_padding.push(token_index + 1 == token_count);
            // A phrase begun by a tab uses the original `.It` position;
            // `Ta`, in contrast, has its own in-line source position.
            body_locations.push(item_location.clone());
        }
    }

    let additional_bodies = cells.len().saturating_sub(1);
    let additional_tab_padding_nodes = leading_tab_padding.iter().filter(|value| **value).count();
    let additional_terminal_tab_nodes = terminal_tab_cell_padding
        .iter()
        .filter(|value| **value)
        .count();
    if builder
        .node_count()
        .saturating_add(additional_bodies)
        .saturating_add(additional_text_nodes)
        .saturating_add(additional_tab_padding_nodes)
        .saturating_add(additional_terminal_tab_nodes)
        > max_nodes
    {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = item_location;
        }
        return None;
    }

    let mut bodies = vec![first_body];
    for location in body_locations.into_iter().skip(1) {
        let body = builder.push(item, NodeKind::Body)?;
        if !builder.macro_name(body, "It")
            || !builder.set_node_location(body, location)
            || !builder.set_node_flags(body, NodeFlags::default())
        {
            return None;
        }
        bodies.push(body);
    }
    let mut item_children = Vec::with_capacity(bodies.len().saturating_add(1));
    item_children.push(head);
    item_children.extend(bodies.iter().copied());
    if !builder.replace_children(head, &[]) || !builder.replace_children(item, &item_children) {
        return None;
    }

    for (
        (((body, tokens), suppress_first_tab_column_system_name), leading_tab_padding),
        terminal_tab_cell_padding,
    ) in bodies
        .iter()
        .copied()
        .zip(cells)
        .zip(suppress_first_tab_column_system_name)
        .zip(leading_tab_padding)
        .zip(terminal_tab_cell_padding)
    {
        let mut inline_tokens = Vec::with_capacity(
            tokens.len()
                + usize::from(leading_tab_padding)
                + usize::from(terminal_tab_cell_padding),
        );
        if leading_tab_padding {
            let padding = builder.push(body, NodeKind::Text)?;
            let location = tokens
                .first()
                .and_then(|token| builder.node_location(*token))
                .and_then(|span| {
                    let start = span.start.checked_sub(1)?;
                    SourceSpan::new(span.source, start, span.start).ok()
                });
            if !builder.text(padding, String::new())
                || !builder.set_node_location(padding, location)
                || !builder.set_node_flags(padding, NodeFlags::default())
            {
                return None;
            }
            inline_tokens.push(padding);
        }
        if terminal_tab_cell_padding {
            let padding = builder.push(body, NodeKind::Text)?;
            if !builder.text(padding, r"\&".to_owned())
                || !builder.set_node_location(padding, item_location.clone())
                || !builder.set_node_flags(padding, NodeFlags::default())
            {
                return None;
            }
            inline_tokens.push(padding);
        }
        inline_tokens.extend(tokens);
        let events = split_mdoc_inline_tokens_with_options(
            builder,
            body,
            &inline_tokens,
            spacing_enabled,
            max_nodes,
            outcome,
            suppress_first_tab_column_system_name,
        );
        let _ = builder.replace_children(body, &events);
        for event in &events {
            let Some(macro_name) = builder.node_macro_name(*event).map(str::to_owned) else {
                continue;
            };
            if !insert_generated_system_name(builder, *event, &macro_name, max_nodes)
                && outcome.node_limit_location.is_none()
            {
                outcome.node_limit_location = builder.node_location(*event);
            }
        }
        // Column cells own the same parsed inline stream as ordinary list
        // item bodies. In particular an `Aq` nested before `Ta` is an
        // implicit partial Block, not a flat Element merely because the
        // source reached it through the column-cell splitter.
        structure_nested_implicit_partial_blocks(
            builder,
            body,
            max_nodes,
            outcome,
            spacing_enabled,
        );
        structure_column_cell_explicit_partials(
            builder,
            body,
            max_nodes,
            outcome,
            spacing_enabled,
            scopes,
        );
    }
    Some(bodies)
}

/// Detach an inline `Ta` and its following phrase before an ordinary mdoc
/// macro consumes the entire source line.  The prefix remains owned by that
/// macro; the suffix is moved into the next Body of the active column row.
pub(super) fn take_inline_column_ta_tail(
    builder: &mut DocumentBuilder,
    node: NodeId,
    active_body: NodeId,
) -> Option<(Vec<NodeId>, Option<SourceSpan>)> {
    active_column_item(builder, active_body)?;
    if builder.node_macro_name(node) == Some("It") {
        return None;
    }
    let tokens = builder.children(node)?.to_vec();
    let separator = tokens
        .iter()
        .position(|token| builder.node_text(*token) == Some("Ta"))?;
    let tail = tokens.get(separator + 1..)?.to_vec();
    let location = builder.node_location(tokens[separator]);
    if !builder.replace_children(node, &tokens[..separator]) {
        return None;
    }
    Some((tail, location))
}

/// Append one cell introduced by a physical or inline `Ta` to the active
/// `Bl -column` row.  The separator is syntax only; its source position
/// becomes the new Body's location, matching libmandoc's row projection.
#[allow(clippy::too_many_arguments)]
pub(super) fn append_column_ta_cell(
    builder: &mut DocumentBuilder,
    active_body: NodeId,
    location: Option<SourceSpan>,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    scopes: &mut Vec<ScopeFrame>,
) -> Option<NodeId> {
    let item = active_column_item(builder, active_body)?;
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return None;
    }
    let body = builder.push(item, NodeKind::Body)?;
    if !builder.macro_name(body, "It")
        || !builder.set_node_location(body, location)
        || !builder.set_node_flags(body, NodeFlags::default())
    {
        return None;
    }
    // The scanner retains ordinary macro arguments as separate text nodes;
    // a cell introduced by an in-line `Ta` is nevertheless one mdoc phrase.
    // Reuse the normal body coalescer before its inline pass so `after tab`
    // remains one public Text node, as it does through a regular item body.
    if !builder.replace_children(body, tokens) {
        return None;
    }
    coalesce_text_children(builder, body);
    let cell_tokens = builder.children(body)?.to_vec();
    let events = split_mdoc_inline_tokens(
        builder,
        body,
        &cell_tokens,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    if !builder.replace_children(body, &events) {
        return None;
    }
    for event in &events {
        let Some(macro_name) = builder.node_macro_name(*event).map(str::to_owned) else {
            continue;
        };
        if !insert_generated_system_name(builder, *event, &macro_name, max_nodes)
            && outcome.node_limit_location.is_none()
        {
            outcome.node_limit_location = builder.node_location(*event);
        }
    }
    structure_nested_implicit_partial_blocks(builder, body, max_nodes, outcome, spacing_enabled);
    structure_column_cell_explicit_partials(
        builder,
        body,
        max_nodes,
        outcome,
        spacing_enabled,
        scopes,
    );
    Some(body)
}

/// Account for one late `Ta` cell against a row whose initial short prefix
/// was intentionally held pending. Once the declared count is reached, the
/// row no longer needs a deferred wrong-cell finding.
pub(super) fn extend_pending_short_column_item(
    pending_short_column_items: &mut BTreeMap<NodeId, (usize, usize)>,
    item: NodeId,
) {
    let complete = if let Some((columns, cells)) = pending_short_column_items.get_mut(&item) {
        *cells = cells.saturating_add(1);
        *cells >= *columns
    } else {
        false
    };
    if complete {
        pending_short_column_items.remove(&item);
    }
}

/// Split literal tab bytes retained inside one column argument into individual
/// source phrases.  A quoted phrase still treats its literal tabs as cell
/// boundaries; this differs from generic argument lexing, which correctly
/// keeps the quoted token intact for every other mdoc macro family.
pub(super) fn split_column_tab_token(
    builder: &mut DocumentBuilder,
    item: NodeId,
    token: NodeId,
) -> Option<Vec<NodeId>> {
    let text = builder.node_text(token)?.to_owned();
    if !text.contains('\t') {
        return Some(vec![token]);
    }
    let flags = builder.node_flags(token).unwrap_or_default();
    let location = builder.node_location(token);
    let quoted = builder.node_argument_quoted(token);
    let mut segments = text.split('\t');
    let first = segments
        .next()
        .expect("contains a tab but always has a prefix");
    if !builder.text(token, first.to_owned()) {
        return None;
    }
    let mut retained = vec![token];
    let mut text_offset = first.len().saturating_add(1);
    for segment in segments {
        let child = builder.push(item, NodeKind::Text)?;
        if !builder.text(child, segment.to_owned()) || !builder.set_node_flags(child, flags) {
            return None;
        }
        if let Some(span) = location.as_ref() {
            let source_offset = text_offset.saturating_add(usize::from(quoted));
            let start = span
                .start
                .saturating_add(u32::try_from(source_offset).unwrap_or(u32::MAX));
            let end = start.saturating_add(u32::try_from(segment.len()).unwrap_or(u32::MAX));
            let location = SourceSpan::new(span.source, start, end).ok()?;
            if !builder.set_node_location(child, Some(location)) {
                return None;
            }
        }
        retained.push(child);
        text_offset = text_offset.saturating_add(segment.len()).saturating_add(1);
    }
    Some(retained)
}

/// Count column phrases from the scanner representation before the package
/// pass turns them into `It` Bodies.  A tab can be embedded in a quoted
/// argument or occur later in an otherwise space-prefixed separator run, and
/// both spellings are semantic cell boundaries in mdoc.
pub(super) fn column_item_cell_count(builder: &DocumentBuilder, item: NodeId) -> usize {
    let Some(tokens) = builder.children(item) else {
        return 1;
    };
    let mut cells = 1_usize;
    for token in tokens {
        if builder.node_text(*token) == Some("Ta") {
            cells = cells.saturating_add(1);
            continue;
        }
        cells = cells.saturating_add(builder.node_embedded_tab_count(*token) as usize);
        if builder.node_separator_contains_tab(*token) {
            cells = cells.saturating_add(1);
        }
    }
    cells
}

/// Complete the preceding zero-argument column item at its next structural
/// boundary.  libmandoc keeps such an item when its first Body acquires input
/// from the following physical line, but removes it when another item or the
/// list closer arrives first.
pub(super) fn finalize_last_empty_column_item(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    pending_empty_column_items: &mut BTreeSet<NodeId>,
    outcome: &mut StructureOutcome,
) {
    let Some(item) = builder
        .children(list_body)
        .and_then(|children| children.last())
        .copied()
        .filter(|item| builder.node_macro_name(*item) == Some("It"))
    else {
        return;
    };
    if !pending_empty_column_items.remove(&item) {
        return;
    }
    let bodies = builder
        .children(item)
        .map(|children| {
            children
                .iter()
                .copied()
                .filter(|child| builder.node_kind(*child) == Some(NodeKind::Body))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if bodies
        .first()
        .is_none_or(|body| builder.children(*body).is_none_or(<[NodeId]>::is_empty))
    {
        if bodies.len() == 1 {
            let mut retained = builder
                .children(list_body)
                .map(<[NodeId]>::to_vec)
                .unwrap_or_default();
            retained.pop();
            let _ = builder.replace_children(list_body, &retained);
            outcome.recoveries.push(Recovery::EmptyMacro {
                macro_name: "It",
                location: builder.node_location(item),
            });
        }
        return;
    }
    outcome.recoveries.push(Recovery::ColumnItemUsesNextLine {
        location: builder.node_location(item),
    });
}

/// Whether an item Head is syntax only for this list selector.  The selector
/// is retained separately from `NormalizedListKind`, whose `Plain` projection
/// deliberately merges several mdoc list families with different validators.
pub(super) fn fixed_head_list_type(list_type: &str) -> bool {
    matches!(list_type, "bullet" | "dash" | "enum" | "hyphen" | "item")
}

/// Validate the immediately preceding fixed-head item when the next item or
/// the list close gives it a complete Body. This ordering is observable: an
/// empty item warning precedes its ignored Head arguments, while an earlier
/// non-empty row reports its ignored arguments before a later empty row.
pub(super) fn finalize_last_fixed_head_list_item(
    builder: &DocumentBuilder,
    list_body: NodeId,
    list_type: &'static str,
    deferred_argument_items: &BTreeSet<NodeId>,
    outcome: &mut StructureOutcome,
) {
    let Some(item) = builder
        .children(list_body)
        .and_then(|children| children.last())
        .copied()
        .filter(|item| builder.node_macro_name(*item) == Some("It"))
    else {
        return;
    };
    let Some((head, body)) = builder.children(item).and_then(|children| {
        let head = children.iter().copied().find(|child| {
            builder.node_kind(*child) == Some(NodeKind::Head)
                && builder.node_macro_name(*child) == Some("It")
        })?;
        let body = children.iter().copied().find(|child| {
            builder.node_kind(*child) == Some(NodeKind::Body)
                && builder.node_macro_name(*child) == Some("It")
        })?;
        Some((head, body))
    }) else {
        return;
    };
    let location = builder.node_location(item);
    if list_type != "item" && builder.children(body).is_none_or(<[NodeId]>::is_empty) {
        outcome.recoveries.push(Recovery::EmptyListItem {
            list_type,
            location: location.clone(),
        });
    }
    let arguments = fixed_head_item_arguments(builder, head);
    if !deferred_argument_items.contains(&item) && !arguments.is_empty() {
        outcome.recoveries.push(Recovery::InvalidArguments {
            message: format!("skipping all arguments: It {arguments}").into(),
            location,
        });
    }
}

/// Summarize a marker-style item's ignored Head as mandoc's validator does:
/// ordinary prose remains one phrase, while a callable macro contributes its
/// own selector but none of its private argument subtree.
pub(super) fn fixed_head_item_arguments(builder: &DocumentBuilder, head: NodeId) -> String {
    builder
        .children(head)
        .into_iter()
        .flatten()
        .filter_map(|child| {
            builder
                .node_text(*child)
                .or_else(|| builder.node_macro_name(*child))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Report all still-short rows in one completed column list.  A physical
/// `.Ta` can add a cell after an `.It` has already been structured, so the
/// row is deliberately not diagnosed until its next item or list boundary.
pub(super) fn finalize_short_column_items(
    builder: &DocumentBuilder,
    list_body: NodeId,
    pending_short_column_items: &mut BTreeMap<NodeId, (usize, usize)>,
    outcome: &mut StructureOutcome,
) {
    let pending = pending_short_column_items
        .iter()
        .filter_map(|(item, (columns, cells))| {
            (builder.node_parent(*item) == Some(list_body)).then_some((*item, *columns, *cells))
        })
        .collect::<Vec<_>>();
    for (item, columns, cells) in pending {
        pending_short_column_items.remove(&item);
        outcome.recoveries.push(Recovery::WrongNumberOfColumnCells {
            columns,
            cells,
            location: builder.node_location(item),
        });
    }
}

/// Reify explicit partial openers embedded in a column cell and retain their
/// cross-line close state. The ordinary top-level dispatcher cannot see these
/// as standalone source nodes: they began life as `.It` arguments, but a
/// following physical `.Bc`/… still closes the same mdoc scope.
pub(super) fn structure_column_cell_explicit_partials(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
    scopes: &mut Vec<ScopeFrame>,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for node in children {
        let Some(name) = builder.node_macro_name(node).map(str::to_owned) else {
            continue;
        };
        let Some(close) = explicit_partial_block_close(&name) else {
            continue;
        };
        let Some((head, body)) = make_block(
            builder,
            node,
            &name,
            ArgumentPlacement::BodyTokens,
            max_nodes,
            outcome,
        ) else {
            continue;
        };
        let children =
            split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
        let _ = builder.replace_children(body, &children);
        clear_leading_explicit_partial_punctuation(builder, body);
        move_explicit_leading_open_delimiter(builder, node, head, body);
        coalesce_adjacent_text_children(builder, body);
        scopes.push(ScopeFrame {
            close,
            open: node,
            body,
            tail_on_close: false,
            transparent_target_taken: false,
            suppress_implicit_ancestor_break: false,
            resume_active: parent,
            resume_flow: parent,
        });
    }
}

/// Allocate a source-less partial block nested inside a validated parent.
/// `.It Xo` carries its opener as an `It` argument rather than a scanner
/// event, so it needs the same public Block/Head/Body shape without first
/// converting an independent source node via [`make_block`].
pub(super) fn make_synthetic_block(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    macro_name: &str,
    location: Option<SourceSpan>,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<(NodeId, NodeId, NodeId)> {
    if builder.node_count().saturating_add(3) > max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = location;
        }
        return None;
    }
    let block = builder.push(parent, NodeKind::Block)?;
    let head = builder.push(block, NodeKind::Head)?;
    let body = builder.push(block, NodeKind::Body)?;
    if !builder.macro_name(block, macro_name)
        || !builder.macro_name(head, macro_name)
        || !builder.macro_name(body, macro_name)
        || !builder.set_node_location(block, location.clone())
        || !builder.set_node_location(head, location.clone())
        || !builder.set_node_location(body, location)
        || !builder.replace_children(block, &[head, body])
    {
        return None;
    }
    Some((block, head, body))
}
