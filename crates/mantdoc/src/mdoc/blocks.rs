use super::{
    DocumentBuilder, NodeFlags, NodeId, NodeKind, NormalizedListKind, Recovery, ScopeFrame,
    SourceSpan, StructureOutcome, is_implicit_partial_block_macro, mark_destination,
    mark_manual_target, mark_no_print, node_arguments, open_name, split_mdoc_inline_tokens,
};

/// Complete the Tail created for an explicit Eo block from its Ec control
/// line.  Ec itself is structural syntax and must not remain in the public
/// tree; its arguments retain their original source positions under Tail.
pub(super) fn complete_explicit_tail(
    builder: &mut DocumentBuilder,
    tail: NodeId,
    closer: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let children = builder
        .children(closer)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let events = split_mdoc_inline_tokens(
        builder,
        closer,
        &children,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    // An Ec tail owns the delimiter/text prefix only.  A callable macro after
    // that prefix begins normal source-order flow again (for example
    // `.Ec >> "Sy" bold` puts `>>` in Tail and `Sy bold` after the Eo block).
    let split_at = events
        .iter()
        .position(|event| builder.node_macro_name(*event).is_some())
        .unwrap_or(events.len());
    let _ = builder.set_node_location(tail, builder.node_location(closer));
    if let Some(flags) = builder.node_flags(closer) {
        let _ = builder.set_node_flags(tail, flags);
    }
    let _ = builder.replace_children(tail, &events[..split_at]);
    events[split_at..].to_vec()
}

/// Recover an unmatched `.Ec` exactly as mdoc's line-break fallback: the
/// closing control becomes a visible `br` element and its parsed arguments
/// resume ordinary sibling flow.  Other unmatched closers remain validation
/// syntax and do not have this Eo-specific AST fallback.
#[allow(clippy::too_many_arguments)] // Recovery must retain root attachment, flow parent, and bounded splitter state.
pub(super) fn recover_unmatched_ec(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let children = builder
        .children(node)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let siblings = split_mdoc_inline_tokens(
        builder,
        node,
        &children,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let _ = builder.replace_children(node, &[]);
    let _ = builder.macro_name(node, "br");
    append_to_parent(builder, root, root_children, parent, node);
    for sibling in siblings {
        append_to_parent(builder, root, root_children, parent, sibling);
    }
}

pub(super) fn append_to_parent(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    node: NodeId,
) {
    if parent == root {
        root_children.push(node);
    } else {
        let _ = builder.append_existing_child(parent, node);
    }
}

/// Remove a preceding paragraph-layout control when the next block's
/// validator declares it redundant.  The root's children are still staged in
/// a local vector, while nested parents already own provisional arena edges.
pub(super) fn discard_previous_paragraph_control(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
) -> Option<NodeId> {
    let previous = if parent == root {
        root_children.last().copied()
    } else {
        builder
            .children(parent)
            .and_then(|children| children.last().copied())
    }?;
    if !matches!(builder.node_macro_name(previous), Some("Pp" | "br")) {
        return None;
    }
    if parent == root {
        root_children.pop();
    } else {
        let children = builder.children(parent)?.to_vec();
        let (last, retained) = children.split_last()?;
        debug_assert_eq!(*last, previous);
        let _ = builder.replace_children(parent, retained);
    }
    Some(previous)
}

/// Materialize the closer-owned Body node that mdoc leaves inside an explicit
/// partial scope when a full block is closed through that scope.  The surviving
/// partial frame retains the surrounding flow until its authored closer.
pub(super) fn append_broken_full_block_body(
    builder: &mut DocumentBuilder,
    active_body: NodeId,
    close: &str,
    frame: ScopeFrame,
    closer: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<NodeId> {
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(closer);
        }
        return None;
    }
    let Some(body) = builder.push(active_body, NodeKind::Body) else {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(closer);
        }
        return None;
    };
    let _ = builder.macro_name(body, open_name(close));
    let _ = builder.copy_node_layout(frame.body, body);
    let _ = builder.set_node_flags(body, builder.node_flags(closer).unwrap_or_default());
    if let Some(location) = builder.node_location(closer) {
        let _ = builder.location(body, location);
    }
    Some(body)
}

/// Collect the nearest-to-farthest implicit partial blocks that contain an
/// explicit partial opener.  Their source request ends before the explicit
/// scope does, so a later physical closer leaves closer-owned empty Bodies in
/// the explicit Body and reports each interrupted implicit block.
pub(super) fn implicit_partial_ancestor_blocks(
    builder: &DocumentBuilder,
    node: NodeId,
) -> Vec<NodeId> {
    let mut blocks = Vec::new();
    let mut cursor = builder.node_parent(node);
    while let Some(parent) = cursor {
        if builder.node_kind(parent) == Some(NodeKind::Body)
            && let Some(name) = builder.node_macro_name(parent)
            && is_implicit_partial_block_macro(name)
            && let Some(block) = builder.node_parent(parent)
            && builder.node_kind(block) == Some(NodeKind::Block)
            && builder.node_macro_name(block) == Some(name)
        {
            blocks.push(block);
        }
        cursor = builder.node_parent(parent);
    }
    blocks
}

/// Insert the public empty Body retained by a crossed implicit partial block.
/// Unlike a full scope there is no close-token-to-name mapping: the block
/// itself supplies the observable macro identity and source location.
pub(super) fn append_broken_implicit_block_body(
    builder: &mut DocumentBuilder,
    active_body: NodeId,
    block: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<NodeId> {
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(block);
        }
        return None;
    }
    let name = builder.node_macro_name(block)?.to_owned();
    let location = builder.node_location(block);
    let body = builder.push(active_body, NodeKind::Body)?;
    if !builder.macro_name(body, name.as_str())
        || !builder.set_node_location(body, location)
        || !builder.set_node_flags(body, NodeFlags::default())
    {
        return None;
    }
    Some(body)
}

/// Detach the physical-line text that arrived in an explicit partial Body
/// before its later closer.  A crossed implicit ancestor inserts its empty
/// Bodies before this continuation, and the continuation is no longer a new
/// public flow event at that point.
pub(super) fn take_trailing_line_start_text_children(
    builder: &mut DocumentBuilder,
    parent: NodeId,
) -> Vec<NodeId> {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let split = children
        .iter()
        .rposition(|child| {
            builder.node_kind(*child) != Some(NodeKind::Text)
                || builder
                    .node_flags(*child)
                    .is_none_or(|flags| !flags.line_start)
        })
        .map_or(0, |index| index + 1);
    if split == children.len() {
        return Vec::new();
    }
    let trailing = children[split..].to_vec();
    let _ = builder.replace_children(parent, &children[..split]);
    trailing
}

/// Return the first root child that is neither retained prologue metadata nor
/// a comment.  mdoc validates only this one node when checking that a manual
/// begins with a section header.
pub(super) fn first_mdoc_content_node(
    builder: &DocumentBuilder,
    root_children: &[NodeId],
) -> Option<NodeId> {
    root_children.iter().copied().find(|node| {
        builder.node_kind(*node) != Some(NodeKind::Comment)
            && !matches!(builder.node_macro_name(*node), Some("Dd" | "Dt" | "Os"))
    })
}

/// Finalize `blk_part_imp()`'s trailing `.Ns` rule after every structural pass
/// has established ownership. A final no-space Element leaves an implicit
/// block Body and becomes a direct block sibling before any closing tail.
pub(super) fn normalize_trailing_no_space_in_implicit_blocks(
    builder: &mut DocumentBuilder,
    root: NodeId,
) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        let children = builder
            .children(node)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        pending.extend(children.iter().rev().copied());

        if builder.node_kind(node) != Some(NodeKind::Block)
            || !builder
                .node_macro_name(node)
                .is_some_and(is_implicit_partial_block_macro)
        {
            continue;
        }
        let Some((_, body)) = children.iter().copied().enumerate().find(|(_, child)| {
            builder.node_kind(*child) == Some(NodeKind::Body)
                && builder.node_macro_name(*child) == builder.node_macro_name(node)
        }) else {
            continue;
        };
        let Some(mut body_children) = builder.children(body).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(last) = body_children.last().copied() else {
            continue;
        };
        if builder.node_macro_name(last) != Some("Ns") {
            continue;
        }

        let Some(parent) = builder.node_parent(node) else {
            continue;
        };
        let Some(mut parent_children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(block_index) = parent_children.iter().position(|child| *child == node) else {
            continue;
        };
        body_children.pop();
        let _ = builder.replace_children(body, &body_children);
        parent_children.insert(block_index + 1, last);
        let _ = builder.replace_children(parent, &parent_children);
    }
}

/// Mirror `post_bl_block()` for the paragraph controls at the tail of list
/// items.  A non-final item in a non-compact, non-column list drops a trailing
/// `Pp`/`br` before the next item.  A final item's trailing control is instead
/// relinked directly after the completed list, where ordinary sibling
/// validation can subsequently compare it with following paragraph flow.
pub(super) fn normalize_list_trailing_paragraph_controls(
    builder: &mut DocumentBuilder,
    root: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let mut pending = vec![(root, false)];
    while let Some((node, visited)) = pending.pop() {
        if !visited {
            pending.push((node, true));
            if let Some(children) = builder.children(node) {
                pending.extend(children.iter().rev().copied().map(|child| (child, false)));
            }
            continue;
        }
        if builder.node_kind(node) != Some(NodeKind::Block)
            || builder.node_macro_name(node) != Some("Bl")
        {
            continue;
        }
        let Some(body) = builder.children(node).and_then(|children| {
            children.iter().copied().find(|child| {
                builder.node_kind(*child) == Some(NodeKind::Body)
                    && builder.node_macro_name(*child) == Some("Bl")
            })
        }) else {
            continue;
        };
        let list_children = builder
            .children(body)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        let compact = builder.node_compact(body).unwrap_or(false);
        let column = builder.node_list_kind(body) == Some(NormalizedListKind::Column);
        let mut moved = Vec::new();

        for (item_index, item) in list_children.iter().copied().enumerate() {
            if builder.node_kind(item) != Some(NodeKind::Block)
                || builder.node_macro_name(item) != Some("It")
            {
                continue;
            }
            let Some(item_body) = builder.children(item).and_then(|children| {
                children.iter().copied().find(|child| {
                    builder.node_kind(*child) == Some(NodeKind::Body)
                        && builder.node_macro_name(*child) == Some("It")
                })
            }) else {
                continue;
            };
            let final_item = item_index + 1 == list_children.len();
            let mut children = builder
                .children(item_body)
                .map(<[NodeId]>::to_vec)
                .unwrap_or_default();
            while let Some(control) = children
                .last()
                .copied()
                .filter(|control| matches!(builder.node_macro_name(*control), Some("Pp" | "br")))
            {
                let macro_name = match builder.node_macro_name(control) {
                    Some("Pp") => "Pp",
                    Some("br") => "br",
                    _ => unreachable!("the list-tail predicate checked the macro name"),
                };
                if final_item {
                    children.pop();
                    recoveries.push(Recovery::ParagraphMovedOutOfList {
                        macro_name,
                        location: builder.node_location(control),
                    });
                    moved.push(control);
                    continue;
                }
                if compact || column {
                    break;
                }
                children.pop();
                recoveries.push(Recovery::ParagraphBoundary {
                    macro_name,
                    placement: "before",
                    blocker: "It",
                    location: builder.node_location(control),
                });
            }
            let _ = builder.replace_children(item_body, &children);
        }

        if moved.is_empty() {
            continue;
        }
        let Some(parent) = builder.node_parent(node) else {
            continue;
        };
        let Some(mut siblings) = builder.children(parent).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(list_index) = siblings.iter().position(|sibling| *sibling == node) else {
            continue;
        };
        // Controls were popped from item tails in reverse source order.
        // Restore their authored order when placing them after the list.
        moved.reverse();
        siblings.splice((list_index + 1)..=list_index, moved);
        let _ = builder.replace_children(parent, &siblings);
    }
}

/// Stable source-order key for the two paragraph-layout postprocessors.  A
/// stable sort deliberately leaves a list relocation before the generic
/// adjacent-control finding at the same source control, matching mandoc's
/// `post_bl_block()` then roff-validation order.
pub(super) fn paragraph_layout_recovery_offset(recovery: &Recovery) -> u32 {
    match recovery {
        Recovery::ParagraphBoundary { location, .. }
        | Recovery::ParagraphMovedOutOfList { location, .. } => location
            .as_ref()
            .map_or(u32::MAX, |location| location.start),
        _ => u32::MAX,
    }
}

/// Mirror the roff-level paragraph controls that validate while an mdoc
/// document is being built.  These checks deliberately precede section
/// post-validation: upstream first resolves adjacent `br`/`sp`/`Pp` requests
/// in a completed local body, then lets `post_section()` inspect the resulting
/// first and last child.
///
/// The traversal is iterative and post-order so controls inside list items or
/// display bodies are normalized before their enclosing macro gets a chance
/// to apply its own boundary rule.  Only direct sibling relationships matter;
/// transparent nodes remain ordinary siblings and never manufacture a false
/// paragraph predecessor.
#[allow(clippy::too_many_lines)] // Post-order mdoc control recovery requires one source-order pass.
pub(super) fn normalize_inline_paragraph_controls(
    builder: &mut DocumentBuilder,
    root: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let mut pending = vec![(root, false)];
    while let Some((parent, visited)) = pending.pop() {
        if !visited {
            pending.push((parent, true));
            if let Some(children) = builder.children(parent) {
                pending.extend(children.iter().rev().copied().map(|child| (child, false)));
            }
            continue;
        }

        let mut retained = builder
            .children(parent)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        let mut index = 0;
        while index < retained.len() {
            let node = retained[index];
            let macro_name = builder.node_macro_name(node);
            let previous_control = preceding_paragraph_control(builder, &retained, index);

            match macro_name {
                Some("br") => {
                    if let Some((previous_index, previous, previous_name @ ("br" | "sp" | "Pp"))) =
                        previous_control
                    {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: "br",
                            placement: "after",
                            blocker: previous_name,
                            location: builder.node_location(node),
                        });
                        preserve_transparent_tag_after_deleted_current_control(
                            builder,
                            &retained,
                            previous_index,
                            index,
                            previous,
                            previous_name,
                        );
                        retained.remove(index);
                        continue;
                    }
                }
                Some("sp") => match previous_control {
                    Some((previous_index, previous, "br")) => {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: "br",
                            placement: "before",
                            blocker: "sp",
                            location: builder.node_location(previous),
                        });
                        preserve_transparent_tag_after_deleted_previous_control(
                            builder,
                            &retained,
                            previous_index,
                            index,
                            "sp",
                        );
                        retained.remove(previous_index);
                        index = index.saturating_sub(1);
                        continue;
                    }
                    Some((previous_index, previous, "Pp")) => {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: "sp",
                            placement: "after",
                            blocker: "Pp",
                            location: builder.node_location(node),
                        });
                        preserve_transparent_tag_after_deleted_current_control(
                            builder,
                            &retained,
                            previous_index,
                            index,
                            previous,
                            "Pp",
                        );
                        retained.remove(index);
                        continue;
                    }
                    _ => {}
                },
                Some("Pp") => {
                    if let Some((previous_index, previous, previous_name @ ("br" | "Pp"))) =
                        previous_control
                    {
                        recoveries.push(Recovery::ParagraphBoundary {
                            macro_name: previous_name,
                            placement: "before",
                            blocker: "Pp",
                            location: builder.node_location(previous),
                        });
                        retained.remove(previous_index);
                        index = index.saturating_sub(1);
                        continue;
                    }
                }
                _ => {}
            }
            index += 1;
        }
        normalize_transparent_layout_tag_destinations(builder, &retained);
        let _ = builder.replace_children(parent, &retained);
    }
}

/// `post_tg()` keeps an explicit tag as its own destination when the local
/// paragraph-control run has no surviving `.Pp` owner.  Tags already hidden
/// by a paragraph owner are left untouched; this only covers the direct
/// `br/sp Tg br/sp` and blank-line forms that roff validation treats as
/// transparent layout separators.
pub(super) fn normalize_transparent_layout_tag_destinations(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
) {
    for (index, node) in children.iter().copied().enumerate() {
        if builder.node_macro_name(node) != Some("Tg")
            || builder
                .node_flags(node)
                .is_none_or(|flags| flags.no_print || flags.deep_link_target)
            || node_arguments(builder, node)
                .first()
                .is_none_or(String::is_empty)
        {
            continue;
        }
        let has_layout_neighbour = index
            .checked_sub(1)
            .and_then(|previous| children.get(previous))
            .copied()
            .into_iter()
            .chain(children.get(index + 1).copied())
            .any(|neighbour| {
                matches!(builder.node_macro_name(neighbour), Some("br" | "sp" | "Pp"))
            });
        if has_layout_neighbour {
            mark_destination(builder, node);
        }
    }
}

/// Preserve the narrow `post_tg()` destination relation when roff validation
/// removes a control that originally followed one or more transparent tags.
/// A surviving preceding `.Pp` owns the tag and hides the tag syntax; other
/// layout controls are not destination owners, so the tag remains a direct
/// destination without publishing an explicit tag string.
pub(super) fn preserve_transparent_tag_after_deleted_current_control(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
    previous_index: usize,
    current_index: usize,
    previous: NodeId,
    previous_name: &str,
) {
    let tags = transparent_tag_arguments(builder, children, previous_index, current_index);
    if previous_name == "Pp" {
        for (tag_node, tag) in tags {
            mark_manual_target(builder, previous, &tag);
            mark_no_print(builder, tag_node);
        }
    } else {
        for (tag_node, _) in tags {
            mark_destination(builder, tag_node);
        }
    }
}

/// When `.sp` deletes a preceding `.br`, the following control cannot own a
/// `.Tg` target; retain that destination on the transparent tag instead.
pub(super) fn preserve_transparent_tag_after_deleted_previous_control(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
    previous_index: usize,
    current_index: usize,
    current_name: &str,
) {
    if current_name != "sp" {
        return;
    }
    for (tag_node, _) in transparent_tag_arguments(builder, children, previous_index, current_index)
    {
        mark_destination(builder, tag_node);
    }
}

/// Return valid explicit `.Tg` spellings strictly between two source siblings.
/// The caller has already established that all intervening siblings are
/// transparent tags, so ordinary text or another macro deliberately ends the
/// search instead of accidentally moving a destination across visible flow.
pub(super) fn transparent_tag_arguments(
    builder: &DocumentBuilder,
    children: &[NodeId],
    previous_index: usize,
    current_index: usize,
) -> Vec<(NodeId, String)> {
    children[previous_index.saturating_add(1)..current_index]
        .iter()
        .copied()
        .map_while(|node| (builder.node_macro_name(node) == Some("Tg")).then_some(node))
        .filter_map(|node| {
            node_arguments(builder, node)
                .first()
                .cloned()
                .filter(|tag| !tag.is_empty())
                .map(|tag| (node, tag))
        })
        .collect()
}

/// Find the preceding layout control recognized by `roff_node_prev()`.  A
/// manual tag is transparent to this particular source-order query: it owns a
/// destination but must not break `br Tg br` or `Pp Tg sp` validation.
pub(super) fn preceding_paragraph_control(
    builder: &DocumentBuilder,
    children: &[NodeId],
    index: usize,
) -> Option<(usize, NodeId, &'static str)> {
    for previous_index in (0..index).rev() {
        let previous = children[previous_index];
        match builder.node_macro_name(previous) {
            Some("br") => return Some((previous_index, previous, "br")),
            Some("sp") => return Some((previous_index, previous, "sp")),
            Some("Pp") => return Some((previous_index, previous, "Pp")),
            Some("Tg") => {}
            _ => return None,
        }
    }
    None
}

/// Apply the narrow `post_section()` / `post_prevpar()` paragraph checks that
/// libmandoc runs while post-validating `Sh` and `Ss` trees. This is a
/// post-order walk on the final arena topology: validating a nested section
/// before its parent deliberately preserves the legacy diagnostic order.
pub(super) fn normalize_section_paragraph_boundaries(
    builder: &mut DocumentBuilder,
    root: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let mut pending = vec![(root, false)];
    while let Some((node, visited)) = pending.pop() {
        if !visited {
            pending.push((node, true));
            if let Some(children) = builder.children(node) {
                pending.extend(children.iter().rev().copied().map(|child| (child, false)));
            }
            continue;
        }

        let section = matches!(
            (builder.node_kind(node), builder.node_macro_name(node)),
            (Some(NodeKind::Block | NodeKind::Body), Some("Sh" | "Ss"))
        );
        if !section {
            continue;
        }

        if builder.node_kind(node) == Some(NodeKind::Body) {
            normalize_section_body_paragraph_boundaries(builder, node, recoveries);
        } else {
            normalize_section_preceding_paragraph_boundary(builder, node, recoveries);
        }
    }
}

/// Match `post_section()` on a completed section Body. The initial request
/// accepts `Pp`, `br`, and `sp`, while only `Pp` and `br` are redundant at its
/// trailing edge.
pub(super) fn normalize_section_body_paragraph_boundaries(
    builder: &mut DocumentBuilder,
    body: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let blocker = match builder.node_macro_name(body) {
        Some("Sh") => "Sh",
        Some("Ss") => "Ss",
        _ => return,
    };
    let mut children = builder
        .children(body)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let original = children.clone();

    if let Some(first) = children.first().copied()
        && let Some(macro_name) = paragraph_control_name(builder, first, true)
        // A paragraph control can be syntactically redundant at a section
        // boundary yet still own an explicit or automatic destination.  In
        // that case libmandoc retains it as the tag anchor; dropping it would
        // silently discard the destination transferred from Tg/Fn/Fo/Em/Sy.
        && !builder
            .node_flags(first)
            .is_some_and(|flags| flags.deep_link_target || flags.permalink)
    {
        children.remove(0);
        recoveries.push(Recovery::ParagraphBoundary {
            macro_name,
            placement: "after",
            blocker,
            location: builder.node_location(first),
        });
    }
    if let Some(last) = children.last().copied()
        && let Some(macro_name) = paragraph_control_name(builder, last, false)
    {
        children.pop();
        recoveries.push(Recovery::ParagraphBoundary {
            macro_name,
            placement: "at the end of",
            blocker,
            location: builder.node_location(last),
        });
    }
    if children != original {
        let _ = builder.replace_children(body, &children);
    }
}

/// Match `post_prevpar()` when a completed section Block has a direct
/// preceding `Pp` or `br` sibling.
pub(super) fn normalize_section_preceding_paragraph_boundary(
    builder: &mut DocumentBuilder,
    block: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(parent) = builder.node_parent(block) else {
        return;
    };
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(index) = children.iter().position(|child| *child == block) else {
        return;
    };
    let Some(previous) = index
        .checked_sub(1)
        .and_then(|index| children.get(index))
        .copied()
    else {
        return;
    };
    let Some(macro_name) = paragraph_control_name(builder, previous, false) else {
        return;
    };
    let blocker = match builder.node_macro_name(block) {
        Some("Sh") => "Sh",
        Some("Ss") => "Ss",
        _ => return,
    };

    let mut retained = children;
    retained.remove(index - 1);
    let _ = builder.replace_children(parent, &retained);
    recoveries.push(Recovery::ParagraphBoundary {
        macro_name,
        placement: "before",
        blocker,
        location: builder.node_location(previous),
    });
}

/// Return an mdoc paragraph-layout control accepted at a section boundary.
/// The `sp` request is accepted only immediately after a section starts.
pub(super) fn paragraph_control_name(
    builder: &DocumentBuilder,
    node: NodeId,
    allow_space: bool,
) -> Option<&'static str> {
    match builder.node_macro_name(node) {
        Some("Pp") => Some("Pp"),
        Some("br") => Some("br"),
        Some("sp") if allow_space => Some("sp"),
        _ => None,
    }
}

/// Use the upstream visible spelling for a root node with no macro name.
pub(super) fn node_kind_name(kind: Option<NodeKind>) -> &'static str {
    match kind {
        Some(NodeKind::Text) => "text",
        Some(NodeKind::Table) => "TS",
        Some(NodeKind::Equation) => "EQ",
        Some(NodeKind::Comment) => "comment",
        Some(
            NodeKind::Root | NodeKind::Block | NodeKind::Head | NodeKind::Body | NodeKind::Tail,
        ) => "block",
        Some(NodeKind::Element) | None => "unknown",
    }
}

/// Remove one already-published semantic node before the root topology is
/// frozen. Root and nested parents use their respective in-progress edges.
pub(super) fn discard_node_from_parent(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    node: NodeId,
) {
    let Some(parent) = builder.node_parent(node) else {
        return;
    };
    if parent == root {
        root_children.retain(|child| *child != node);
    } else if let Some(mut children) = builder.children(parent).map(<[NodeId]>::to_vec) {
        children.retain(|child| *child != node);
        let _ = builder.replace_children(parent, &children);
    }
}

/// Remove an empty full block after its closer validates it away. The parser
/// has not frozen root children yet, so root and nested parents use their
/// respective in-progress edge lists.
pub(super) fn discard_empty_block(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    block: NodeId,
) {
    if parent == root {
        root_children.retain(|child| *child != block);
        return;
    }
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let retained = children
        .into_iter()
        .filter(|child| *child != block)
        .collect::<Vec<_>>();
    let _ = builder.replace_children(parent, &retained);
}

pub(super) fn argument_location(
    builder: &DocumentBuilder,
    node: NodeId,
    index: usize,
) -> Option<SourceSpan> {
    builder
        .children(node)
        .and_then(|children| children.get(index))
        .and_then(|argument| builder.node_location(*argument))
}
