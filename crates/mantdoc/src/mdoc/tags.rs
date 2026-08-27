use super::{
    BTreeMap, BTreeSet, DocumentBuilder, NodeId, NodeKind, NormalizedListKind,
    is_implicit_partial_block_macro,
};

pub(super) fn mark_section_targets(builder: &mut DocumentBuilder, heads: &[NodeId]) {
    let mut fallback_sections = std::collections::BTreeMap::<String, NodeId>::new();
    let mut duplicate_fallback_sections = std::collections::BTreeSet::<String>::new();
    for head in heads {
        let Some(heading) = visible_head_text(builder, *head) else {
            continue;
        };
        let tag = deroff_section_heading(&heading);
        let candidate = tag.trim_start_matches('-');
        if !candidate
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        {
            continue;
        }
        if tag.is_empty() {
            continue;
        }
        // libmandoc's parser retains an internal discretionary-hyphen marker
        // in section text.  The public AST deliberately drops that marker,
        // while `tag_put()` still observes it and stores the visible spelling
        // as an explicit tag.  Preserve that observable result without
        // leaking the private marker into native tree text.
        if tag == heading {
            if heading.contains('-') {
                mark_target(builder, *head, Some(&tag));
            } else {
                mark_target(builder, *head, None);
            }
        } else if !duplicate_fallback_sections.contains(&tag) {
            if let Some(previous) = fallback_sections.remove(&tag) {
                clear_target(builder, previous);
                duplicate_fallback_sections.insert(tag);
            } else {
                mark_target(builder, *head, Some(&tag));
                fallback_sections.insert(tag, *head);
            }
        }
    }
}

/// Extract the section-heading spelling used by libmandoc's `deroff()` plus
/// the space-to-underscore conversion in `post_section()`.  This is purpose-
/// built for title tags: public AST text continues to retain authored escapes.
pub(super) fn deroff_section_heading(heading: &str) -> String {
    let heading = heading
        .strip_prefix("\\&")
        .or_else(|| heading.strip_prefix("\\ "))
        .unwrap_or(heading);
    // The scanner retains `\\t` spelling in public mdoc text, while
    // libmandoc's deroffed heading observes its tabulation whitespace.
    let heading = heading.replace("\\t", " ");
    let heading = heading.trim_start_matches(char::is_whitespace);
    heading
        .trim_end_matches(char::is_whitespace)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
}

/// `post_em` gives an emphasis macro a fallback automatic tag after its
/// ordinary delimiter validation.  As in libmandoc's `tag_put`, a fallback
/// name is useful only when it occurs exactly once: a second occurrence
/// removes the first target and leaves neither one addressable.
pub(super) fn mark_emphasis_targets(builder: &mut DocumentBuilder, elements: &[NodeId]) {
    let mut fallback = std::collections::BTreeMap::<String, Vec<(NodeId, bool)>>::new();
    // Strong/manual targets are constructed before this fallback pass.  They
    // occupy the global `tag_put()` namespace, so a later Em/Sy fallback of
    // the same spelling must be ignored rather than reintroducing an ID.
    let occupied = elements
        .iter()
        .filter(|element| {
            builder
                .node_flags(**element)
                .is_some_and(|flags| flags.permalink)
        })
        .filter_map(|element| inline_target_name(builder, *element).map(|(name, _)| name))
        .collect::<std::collections::BTreeSet<_>>();
    for element in elements {
        // A definition-list head may already have consumed this element as a
        // strong destination.  It must not re-enter the weaker, unique-only
        // fallback namespace.
        if builder
            .node_flags(*element)
            .is_some_and(|flags| flags.permalink)
        {
            continue;
        }
        let Some(text) = builder
            .children(*element)
            .and_then(|children| children.first())
            .and_then(|child| builder.node_text(*child))
            .map(str::to_owned)
        else {
            continue;
        };
        let end = text
            .bytes()
            .position(|byte| matches!(byte, b' ' | b'\t' | b'\\'))
            .unwrap_or(text.len());
        let Some(name) = text.get(..end).filter(|name| !name.is_empty()) else {
            continue;
        };
        if occupied.contains(name) {
            continue;
        }
        fallback
            .entry(name.to_owned())
            .or_default()
            .push((*element, end != text.len()));
    }
    // Apply only unique fallback names.  Deferring the mutation matters when
    // `tag_move_id()` would otherwise have transferred the first candidate to
    // a paragraph: a later duplicate must leave no stale paragraph target.
    for (name, candidates) in fallback {
        let [(element, explicit)] = candidates.as_slice() else {
            continue;
        };
        mark_target(builder, *element, explicit.then_some(name.as_str()));
        move_inline_target_to_preceding_paragraph(builder, *element, &name);
    }
}

/// `post_tag()` makes the leading command-like macro of a semantic list item
/// a strong destination. `tag_postprocess()` then transfers that ID to the
/// `It` head (the rendered `<dt>` or marker term) while retaining the inline
/// macro's permalink. The source parser has already split the head, so
/// restricting this to its first event and events immediately following a
/// literal `|` reproduces the upstream eligibility rule without guessing
/// across prose.
pub(super) fn mark_definition_item_head_targets(
    builder: &mut DocumentBuilder,
    list_body: NodeId,
    head: NodeId,
    children: &[NodeId],
) {
    if !matches!(
        builder.node_list_kind(list_body),
        Some(
            NormalizedListKind::Definition
                | NormalizedListKind::Bullet
                | NormalizedListKind::Ordered
        )
    ) {
        return;
    }
    for (index, candidate) in children.iter().copied().enumerate() {
        let eligible = index == 0
            || children
                .get(index.saturating_sub(1))
                .and_then(|previous| builder.node_text(*previous))
                == Some("|");
        if !eligible {
            continue;
        }
        let Some(element) = leading_definition_item_target(builder, candidate) else {
            continue;
        };
        let Some((tag, explicit)) = inline_target_name(builder, element) else {
            continue;
        };
        mark_target(builder, element, explicit.then_some(tag.as_str()));
        move_inline_target_to_item_head(builder, element, head, &tag);
    }
}

/// Return the command-like macro that owns an eligible definition-list term.
/// Implicit partial blocks are presentation wrappers, but mdoc's tag pass
/// descends through their Body before selecting the leading tag macro: for
/// example `.It Bq Er ENOENT` assigns the `ENOENT` destination to the `It`
/// Head and leaves `Er` as its permalink.  Do not search past the first Body
/// event; later prose or macros are not term-leading candidates.
pub(super) fn leading_definition_item_target(
    builder: &DocumentBuilder,
    node: NodeId,
) -> Option<NodeId> {
    if builder
        .node_macro_name(node)
        .is_some_and(is_definition_item_target_macro)
    {
        return Some(node);
    }
    if !builder
        .node_macro_name(node)
        .is_some_and(is_implicit_partial_block_macro)
    {
        return None;
    }
    let body = builder.children(node)?.iter().copied().find(|child| {
        builder.node_kind(*child) == Some(NodeKind::Body)
            && builder.node_macro_name(*child) == builder.node_macro_name(node)
    })?;
    let first = builder.children(body)?.first().copied()?;
    leading_nested_definition_item_target(builder, first)
}

/// The error-name macro does not itself make a bare definition-list term a
/// destination (`.It Er one`).  It does when it is the first semantic child
/// of an enclosure wrapper (`.It Bq Er ENOENT`), which is the narrow upstream
/// `tag_postprocess()` shape exercised by the mdoc regression suite.
pub(super) fn leading_nested_definition_item_target(
    builder: &DocumentBuilder,
    node: NodeId,
) -> Option<NodeId> {
    if builder.node_macro_name(node) == Some("Er")
        || builder
            .node_macro_name(node)
            .is_some_and(is_definition_item_target_macro)
    {
        return Some(node);
    }
    if !builder
        .node_macro_name(node)
        .is_some_and(is_implicit_partial_block_macro)
    {
        return None;
    }
    let body = builder.children(node)?.iter().copied().find(|child| {
        builder.node_kind(*child) == Some(NodeKind::Body)
            && builder.node_macro_name(*child) == builder.node_macro_name(node)
    })?;
    let first = builder.children(body)?.first().copied()?;
    leading_nested_definition_item_target(builder, first)
}

pub(super) fn is_definition_item_target_macro(name: &str) -> bool {
    matches!(
        name,
        "Cm" | "Dv" | "Em" | "Ev" | "Fl" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va"
    )
}

/// Complete the same `post_tag()` rule for the cross-line `.It Xo` form.
/// The first command-like child of Xo's body is logically the first item-head
/// macro, even though the public AST correctly nests it below an Xo block.
pub(super) fn mark_definition_item_xo_head_targets(builder: &mut DocumentBuilder) {
    let mut pending = vec![DocumentBuilder::root()];
    while let Some(list) = pending.pop() {
        if builder.node_kind(list) == Some(NodeKind::Block)
            && builder.node_macro_name(list) == Some("Bl")
            && builder
                .children(list)
                .and_then(|children| {
                    children.iter().copied().find(|child| {
                        builder.node_kind(*child) == Some(NodeKind::Body)
                            && builder.node_macro_name(*child) == Some("Bl")
                    })
                })
                .is_some_and(|body| {
                    builder.node_list_kind(body) == Some(NormalizedListKind::Definition)
                })
        {
            let Some(list_body) = builder.children(list).and_then(|children| {
                children.iter().copied().find(|child| {
                    builder.node_kind(*child) == Some(NodeKind::Body)
                        && builder.node_macro_name(*child) == Some("Bl")
                })
            }) else {
                continue;
            };
            let items = builder
                .children(list_body)
                .map(<[NodeId]>::to_vec)
                .unwrap_or_default();
            for item in items {
                if builder.node_kind(item) != Some(NodeKind::Block)
                    || builder.node_macro_name(item) != Some("It")
                {
                    continue;
                }
                let Some(item_head) = builder.children(item).and_then(|children| {
                    children.iter().copied().find(|child| {
                        builder.node_kind(*child) == Some(NodeKind::Head)
                            && builder.node_macro_name(*child) == Some("It")
                    })
                }) else {
                    continue;
                };
                let Some(xo) = builder.children(item_head).and_then(|children| {
                    (children.len() == 1)
                        .then(|| children.first().copied())
                        .flatten()
                        .filter(|child| {
                            builder.node_kind(*child) == Some(NodeKind::Block)
                                && builder.node_macro_name(*child) == Some("Xo")
                        })
                }) else {
                    continue;
                };
                let Some(xo_body) = builder.children(xo).and_then(|children| {
                    children.iter().copied().find(|child| {
                        builder.node_kind(*child) == Some(NodeKind::Body)
                            && builder.node_macro_name(*child) == Some("Xo")
                    })
                }) else {
                    continue;
                };
                let Some(element) = builder
                    .children(xo_body)
                    .and_then(|children| children.first())
                    .copied()
                else {
                    continue;
                };
                if !matches!(
                    builder.node_macro_name(element),
                    Some(
                        "Cm" | "Dv" | "Em" | "Ev" | "Fl" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va"
                    )
                ) {
                    continue;
                }
                let Some((tag, explicit)) = inline_target_name(builder, element) else {
                    continue;
                };
                mark_target(builder, element, explicit.then_some(tag.as_str()));
                move_inline_target_to_item_head(builder, element, item_head, &tag);
            }
        }
        if let Some(children) = builder.children(list) {
            pending.extend(children.iter().rev().copied());
        }
    }
}

/// Return the public destination spelling of a taggable inline macro and
/// whether it differs from its literal first child.  This mirrors the narrow
/// prefix treatment in libmandoc's `tag_put()` (`-`, `\\&`, `\\-`, `\\e`).
pub(super) fn inline_target_name(
    builder: &DocumentBuilder,
    element: NodeId,
) -> Option<(String, bool)> {
    let source = builder
        .children(element)?
        .first()
        .and_then(|child| builder.node_text(*child))?;
    let mut candidate = source;
    if let Some(rest) = candidate.strip_prefix('-') {
        candidate = rest;
    } else {
        for prefix in ["\\&", "\\-", "\\e"] {
            if let Some(rest) = candidate.strip_prefix(prefix) {
                candidate = rest;
                break;
            }
        }
    }
    let end = candidate
        .bytes()
        .position(|byte| matches!(byte, b' ' | b'\t' | b'\\'))
        .unwrap_or(candidate.len());
    let tag = candidate.get(..end).filter(|tag| !tag.is_empty())?;
    Some((
        tag.to_owned(),
        candidate.len() != source.len() || end != candidate.len(),
    ))
}

/// Move a strong inline destination into its definition-list term unless a
/// previous strong destination already owns that term.  In the latter case,
/// both inline macro targets remain observable, exactly as `tag_move_id()`
/// does after `tag_put()` has resolved priorities.
pub(super) fn move_inline_target_to_item_head(
    builder: &mut DocumentBuilder,
    element: NodeId,
    head: NodeId,
    tag: &str,
) {
    if builder
        .node_flags(head)
        .is_some_and(|flags| flags.deep_link_target)
    {
        return;
    }
    mark_manual_target(builder, head, tag);
    if let Some(mut flags) = builder.node_flags(element) {
        flags.deep_link_target = false;
        let _ = builder.set_node_flags(element, flags);
    }
}

/// `tag_move_id()` walks backward across ordinary inline siblings after a
/// successful Em/Sy fallback tag.  A preceding paragraph owns the stable
/// destination, while the inline element keeps only its permalink.  Stop at
/// the same major block boundary used by the upstream postprocessor.
pub(super) fn move_inline_target_to_preceding_paragraph(
    builder: &mut DocumentBuilder,
    element: NodeId,
    tag: &str,
) {
    let mut current = element;
    loop {
        let Some(parent) = builder.node_parent(current) else {
            return;
        };
        let Some(siblings) = builder.children(parent) else {
            return;
        };
        let Some(index) = siblings.iter().position(|sibling| *sibling == current) else {
            return;
        };
        current = if index == 0 {
            parent
        } else {
            siblings[index - 1]
        };
        match builder.node_macro_name(current) {
            Some("Pp") => {
                let occupied = builder
                    .node_flags(current)
                    .is_some_and(|flags| flags.deep_link_target);
                let punctuation_fallback = builder
                    .node_tag(current)
                    .filter(|previous| matches!(*previous, "." | "!" | "?"))
                    .map(str::to_owned);
                if occupied && punctuation_fallback.is_none() {
                    return;
                }
                // `tag_move_id()` lets a later Em/Sy fallback replace an
                // earlier punctuation-only fallback on the same paragraph.
                // The punctuation macro keeps its permalink, while the
                // meaningful spelling owns the destination.
                if let Some(previous) = punctuation_fallback.as_deref() {
                    restore_punctuation_fallback_target(builder, current, previous);
                }
                mark_manual_target(builder, current, tag);
                if let Some(mut flags) = builder.node_flags(element) {
                    flags.deep_link_target = false;
                    let _ = builder.set_node_flags(element, flags);
                }
                return;
            }
            Some("Sh" | "Ss" | "Bd" | "Bl" | "D1" | "Dl" | "Rs") => return,
            _ => {}
        }
    }
}

/// Restore a punctuation fallback that was provisionally moved onto a
/// paragraph and has just been superseded by a later, meaningful fallback.
pub(super) fn restore_punctuation_fallback_target(
    builder: &mut DocumentBuilder,
    paragraph: NodeId,
    tag: &str,
) {
    let Some(parent) = builder.node_parent(paragraph) else {
        return;
    };
    let Some(siblings) = builder.children(parent) else {
        return;
    };
    let Some(index) = siblings.iter().position(|sibling| *sibling == paragraph) else {
        return;
    };
    for candidate in &siblings[index + 1..] {
        if matches!(
            builder.node_macro_name(*candidate),
            Some("Pp" | "Sh" | "Ss")
        ) {
            return;
        }
        if !matches!(builder.node_macro_name(*candidate), Some("Em" | "Sy"))
            || !builder
                .node_flags(*candidate)
                .is_some_and(|flags| flags.permalink)
        {
            continue;
        }
        let Some((candidate_tag, explicit)) = inline_target_name(builder, *candidate) else {
            continue;
        };
        if candidate_tag != tag {
            continue;
        }
        mark_target(builder, *candidate, explicit.then_some(tag));
        return;
    }
}

/// `post_em()` is shared by Em and Sy in libmandoc's validation table.  Run
/// after source-order restructuring so elements originating in a nested body
/// or after an explicit closer participate in the same fallback namespace.
pub(super) fn emphasis_fallback_elements(builder: &DocumentBuilder) -> Vec<NodeId> {
    let mut elements = Vec::new();
    let mut pending = vec![DocumentBuilder::root()];
    while let Some(node) = pending.pop() {
        if builder.node_kind(node) == Some(NodeKind::Element)
            && matches!(builder.node_macro_name(node), Some("Em" | "Sy"))
        {
            elements.push(node);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().copied());
        }
    }
    elements
}

pub(super) fn visible_head_text(builder: &DocumentBuilder, head: NodeId) -> Option<String> {
    let mut values = Vec::new();
    let mut pending = builder
        .children(head)?
        .iter()
        .rev()
        .copied()
        .collect::<Vec<_>>();
    while let Some(node) = pending.pop() {
        if let Some(text) = builder.node_text(node) {
            values.push(text);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().copied());
        }
    }
    (!values.is_empty()).then(|| values.join(" "))
}

/// Mirror `tag_put(NULL, …)` for the automatic function-name destination.
/// The public AST retains formatting escapes, but the legacy tag contract ends
/// at the first whitespace or escape after skipping only its three permitted
/// leading zero-width spellings.
pub(super) fn automatic_mdoc_function_tag(value: &str) -> Option<&str> {
    let value = value.strip_prefix('-').unwrap_or(value);
    let value = value
        .strip_prefix("\\&")
        .or_else(|| value.strip_prefix("\\-"))
        .or_else(|| value.strip_prefix("\\e"))
        .unwrap_or(value);
    let length = value.find([' ', '\t', '\\']).unwrap_or(value.len());
    (length > 0).then_some(&value[..length])
}

/// Commit automatic function tags only when their spelling appears once in
/// the document.  The target bit was already set at source-order time; this
/// pass supplies the global duplicate suppression performed by `tag_put()`.
pub(super) fn mark_unique_function_targets(
    builder: &mut DocumentBuilder,
    targets: &[(NodeId, String, bool)],
    occurrences: &[String],
) {
    let mut counts = BTreeMap::<&str, usize>::new();
    for tag in occurrences {
        *counts.entry(tag).or_default() += 1;
    }
    let mut retained_duplicates = BTreeSet::<&str>::new();
    for (node, tag, exposes_tag) in targets {
        if *exposes_tag && counts.get(tag.as_str()) == Some(&1) {
            // `tag_put(NULL, …)` records the target bit without allocating a
            // redundant tag when the public first word is already the exact
            // destination spelling.  A separate tag is only observable when
            // normalization shortened or otherwise transformed that word.
            let public_first_word = builder
                .children(*node)
                .and_then(|children| children.first())
                .and_then(|child| builder.node_text(*child));
            if public_first_word != Some(tag.as_str()) {
                let _ = builder.set_node_tag(*node, tag.as_str());
            }
        } else if !retained_duplicates.insert(tag) {
            // `tag_put()` keeps the first declaration's destination bit for
            // a repeated automatic function spelling, then suppresses every
            // later candidate.  The spelling remains tagless in both cases
            // because it is not globally unique.
            clear_target(builder, *node);
        }
    }
}

pub(super) fn mark_target(builder: &mut DocumentBuilder, head: NodeId, tag: Option<&str>) {
    let Some(mut flags) = builder.node_flags(head) else {
        return;
    };
    flags.deep_link_target = true;
    flags.permalink = true;
    let _ = builder.set_node_flags(head, flags);
    if let Some(tag) = tag {
        let _ = builder.set_node_tag(head, tag);
    }
}

pub(super) fn mark_destination(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.deep_link_target = true;
    let _ = builder.set_node_flags(node, flags);
}

pub(super) fn mark_permalink(builder: &mut DocumentBuilder, node: NodeId, tag: Option<&str>) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.permalink = true;
    let _ = builder.set_node_flags(node, flags);
    if let Some(tag) = tag {
        let _ = builder.set_node_tag(node, tag);
    }
}

/// Move a same-line display destination to its first visible leaf.
///
/// A one-line D1/Dl body already owns the authored text at the time `.Tg`
/// is validated, unlike a multi-line Bd whose first visible line arrives in
/// a later source event.
pub(super) fn mark_first_visible_permalink(builder: &mut DocumentBuilder, root: NodeId, tag: &str) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        if builder.node_kind(node) == Some(NodeKind::Text)
            && builder
                .node_flags(node)
                .is_some_and(|flags| !flags.no_print)
        {
            mark_permalink(builder, node, Some(tag));
            return;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().copied());
        }
    }
}

/// Attach a manual `.Tg` destination without also making the syntax node its
/// own permalink.  `tag_postprocess()` moves the latter to following text for
/// `.Pp` targets.
pub(super) fn mark_manual_target(builder: &mut DocumentBuilder, node: NodeId, tag: &str) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.deep_link_target = true;
    let _ = builder.set_node_flags(node, flags);
    let _ = builder.set_node_tag(node, tag);
}

pub(super) fn clear_target(builder: &mut DocumentBuilder, head: NodeId) {
    let Some(mut flags) = builder.node_flags(head) else {
        return;
    };
    flags.deep_link_target = false;
    flags.permalink = false;
    let _ = builder.set_node_flags(head, flags);
    let _ = builder.clear_node_tag(head);
}

pub(super) fn default_volume(section: &str) -> Option<String> {
    let section = section.strip_suffix('p').unwrap_or(section);
    Some(
        match section {
            "1" => "General Commands Manual",
            "2" => "System Calls Manual",
            "3" => "Library Functions Manual",
            "4" => "Kernel Interfaces Manual",
            "5" => "File Formats Manual",
            "6" => "Games Manual",
            "7" => "Miscellaneous Information Manual",
            "8" => "System Manager's Manual",
            "9" => "Kernel Developer's Manual",
            _ => return None,
        }
        .to_owned(),
    )
}
