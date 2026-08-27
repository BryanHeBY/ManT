use super::{
    DocumentBuilder, NodeId, NodeKind, Recovery, StructureOutcome, is_implicit_partial_block_macro,
    is_mdoc_closing_delimiter, is_mdoc_middle_delimiter, mark_opening_delimiter,
    transfer_line_start,
};

/// Recombine a semantic line argument from scanner-level lexical tokens.
///
/// The scanner intentionally tokenizes every control line so that the roff
/// executor can apply expansion safely.  Some mdoc macros (`Dd`, `Nd`) take
/// one complete line argument in libmandoc's public AST. Reusing the first
/// temporary child keeps its source position and bounded arena allocation.
pub(super) fn coalesce_text_children(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(&first) = children.first() else {
        return;
    };
    if children.len() == 1 {
        return;
    }
    let value = children
        .iter()
        .filter_map(|child| builder.node_text(*child))
        .collect::<Vec<_>>()
        .join(" ");
    if builder.text(first, value) {
        let _ = builder.replace_children(node, &[first]);
    }
}

/// Merge adjacent direct text children while preserving macro boundaries.
/// Scanner tokens are deliberately word-sized; partial mdoc blocks expose a
/// complete text run as one owned-AST text node between any callable elements.
pub(super) fn coalesce_adjacent_text_children(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut merged = Vec::with_capacity(children.len());
    let mut text_run = None::<NodeId>;
    for child in children {
        if let Some(text) = builder.node_text(child).map(str::to_owned) {
            if let Some(first) = text_run {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            } else {
                merged.push(child);
                text_run = Some(child);
            }
        } else {
            merged.push(child);
            text_run = None;
        }
    }
    let _ = builder.replace_children(node, &merged);
}

/// When an implicit partial contains another implicit partial, a crossed
/// explicit closer belongs to that innermost construct.  The scanner exposes
/// the closer as a direct child of the outer argument run, so repair the
/// ownership after recursive block construction and before phrase coalescing.
pub(super) fn relocate_crossed_closer_to_nested_implicit_body(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    closer_body: NodeId,
) -> Option<NodeId> {
    let children = builder.children(parent)?.to_vec();
    let closer_index = children.iter().position(|child| *child == closer_body)?;
    let previous = *children.get(closer_index.checked_sub(1)?)?;
    if builder.node_kind(previous) != Some(NodeKind::Block)
        || !builder
            .node_macro_name(previous)
            .is_some_and(is_implicit_partial_block_macro)
    {
        return None;
    }
    let nested_body = builder.children(previous)?.iter().copied().find(|child| {
        builder.node_kind(*child) == Some(NodeKind::Body)
            && builder.node_macro_name(*child) == builder.node_macro_name(previous)
    })?;
    let mut nested_children = builder.children(nested_body)?.to_vec();
    nested_children.push(closer_body);
    let mut cursor = closer_index + 1;
    while let Some(child) = children.get(cursor) {
        if builder.node_kind(*child) != Some(NodeKind::Text) {
            break;
        }
        nested_children.push(*child);
        cursor += 1;
    }
    let mut retained = children[..closer_index].to_vec();
    retained.extend_from_slice(&children[cursor..]);
    if builder.replace_children(nested_body, &nested_children)
        && builder.replace_children(parent, &retained)
    {
        Some(nested_body)
    } else {
        None
    }
}

/// Merge the direct text run that resumes after a structural recovery marker.
/// The words before the marker retain their scanner-visible boundaries; mdoc
/// uses that distinction for an implicit partial block interrupted by an
/// explicit closer (`.Op first Dc resumed words`).
pub(super) fn coalesce_text_children_after(
    builder: &mut DocumentBuilder,
    node: NodeId,
    marker: NodeId,
) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(marker_index) = children.iter().position(|child| *child == marker) else {
        return;
    };
    let mut merged = children[..=marker_index].to_vec();
    let mut text_run = None::<NodeId>;
    for child in &children[marker_index + 1..] {
        if let Some(text) = builder.node_text(*child).map(str::to_owned) {
            if let Some(first) = text_run {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            } else {
                merged.push(*child);
                text_run = Some(*child);
            }
        } else {
            merged.push(*child);
            text_run = None;
        }
    }
    let _ = builder.replace_children(node, &merged);
}

/// Merge ordinary implicit-partial body prose without crossing an authored
/// mdoc delimiter.  Those delimiters are independently observable nodes even
/// when they occur within a body (`.Op a | z`), while ordinary word runs keep
/// their phrase representation (`.Op now optional`).
pub(super) fn coalesce_implicit_partial_body_text(builder: &mut DocumentBuilder, node: NodeId) {
    // SYNOPSIS keeps the scanner's individual argument nodes.  This matters
    // for partial blocks emitted after an explicit closer as well as ordinary
    // source-order implicit blocks (`.Pq one line` remains two text nodes).
    if builder
        .node_flags(node)
        .is_some_and(|flags| flags.synopsis_pretty)
    {
        return;
    }
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut merged = Vec::with_capacity(children.len());
    let mut text_run = None::<NodeId>;
    for child in children {
        let delimiter = builder.node_text(child).is_some_and(|text| {
            matches!(text, "(" | "[")
                || is_mdoc_middle_delimiter(text)
                || is_mdoc_closing_delimiter(text)
        });
        if delimiter {
            merged.push(child);
            text_run = None;
        } else if let Some(text) = builder.node_text(child).map(str::to_owned) {
            if let Some(first) = text_run {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            } else {
                merged.push(child);
                text_run = Some(child);
            }
        } else {
            merged.push(child);
            text_run = None;
        }
    }
    let _ = builder.replace_children(node, &merged);
}

/// Reconstruct mdoc's `ARGS_PHRASE` partition for one-line displays.
///
/// `D1` and `Dl` preserve their first doubled separator as a public
/// text-node boundary. The legacy `ARGS_PPHRASE` mode then treats all
/// remaining ordinary arguments as one phrase (including later doubled
/// separators). Inline macro elements deliberately terminate a phrase run
/// rather than being folded through as text.
pub(super) fn coalesce_mdoc_display_phrases(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let mut rebuilt = Vec::with_capacity(children.len());
    let mut phrase = None::<NodeId>;
    let mut previous_text = None::<NodeId>;
    let mut phrase_boundary_seen = false;
    for child in children {
        if let Some(text) = builder.node_text(child).map(str::to_owned) {
            // Display phrases coalesce ordinary words, but the inline splitter
            // has already classified mdoc delimiters.  Folding `(` and `)`
            // into a text phrase erases their no-space flags and turns
            // `.Dl name ( ) command` into `name ( ) command` instead of the
            // legacy `name () command`.
            if matches!(text.as_str(), "(" | "[")
                || is_mdoc_middle_delimiter(&text)
                || is_mdoc_closing_delimiter(&text)
            {
                rebuilt.push(child);
                phrase = None;
                previous_text = None;
                continue;
            }
            let phrase_break = previous_text.is_none_or(|previous| {
                !phrase_boundary_seen && builder.node_separator_width(previous) >= 2
            });
            if phrase_break {
                if let Some(previous) = previous_text
                    && !phrase_boundary_seen
                    && builder.node_separator_width(previous) >= 2
                {
                    // The mdoc argument parser attaches the source location
                    // of the separating whitespace to the second phrase.
                    // Scanner tokens begin at the following word, so repair
                    // that private provenance before freezing the public AST.
                    if let (Some(previous_location), Some(mut location)) = (
                        builder.node_location(previous),
                        builder.node_location(child),
                    ) {
                        let width = builder.node_text(previous).map_or(0, |value| {
                            u32::try_from(value.len())
                                .expect("source text length fits public u32 spans")
                        });
                        location.start = previous_location.start.saturating_add(width);
                        let _ = builder.set_node_location(child, Some(location));
                    }
                    phrase_boundary_seen = true;
                }
                rebuilt.push(child);
                phrase = Some(child);
            } else if let Some(first) = phrase {
                let Some(existing) = builder.node_text(first) else {
                    continue;
                };
                let value = format!("{existing} {text}");
                let _ = builder.text(first, value);
            }
            previous_text = Some(child);
        } else {
            rebuilt.push(child);
            phrase = None;
            previous_text = None;
        }
    }
    let _ = builder.replace_children(node, &rebuilt);
}

/// mdoc's `Fl` is variadic, but each ordinary argument owns a distinct output
/// element (and therefore a distinct rendered dash). Opening delimiters and a
/// vertical-bar argument stay in outer flow rather than becoming flags.
#[allow(clippy::too_many_lines)] // `Fl` preserves several delimiter and recovery edge cases in one pass.
pub(super) fn expand_fl_elements(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    nodes: Vec<NodeId>,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let mut expanded = Vec::with_capacity(nodes.len());
    for node in nodes {
        if builder.node_macro_name(node) != Some("Fl") {
            expanded.push(node);
            continue;
        }
        let arguments = builder
            .children(node)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        if arguments
            .first()
            .is_some_and(|argument| builder.node_text(*argument) == Some("Es"))
        {
            let enclosure = arguments[0];
            let enclosure_arguments = &arguments[1..];
            if !builder.replace_children(node, &[])
                || !builder.clear_node_text(enclosure)
                || !builder.set_node_kind(enclosure, NodeKind::Element)
                || !builder.macro_name(enclosure, "Es")
                || !builder.replace_children(enclosure, enclosure_arguments)
            {
                expanded.push(node);
                continue;
            }
            outcome.recoveries.push(Recovery::Obsolete {
                macro_name: "Es",
                location: builder.node_location(enclosure),
            });
            expanded.push(node);
            expanded.push(enclosure);
            continue;
        }
        let argument_count = arguments
            .iter()
            .filter(|argument| {
                builder
                    .node_text(**argument)
                    .is_none_or(|text| text != "|" && !matches!(text, "(" | "["))
            })
            .count();
        let leading_separator = arguments
            .first()
            .is_some_and(|argument| builder.node_text(*argument) == Some("|"));
        let flag_count = argument_count.saturating_add(usize::from(leading_separator));
        let has_opening_delimiter = arguments.iter().any(|argument| {
            builder
                .node_text(*argument)
                .is_some_and(|text| matches!(text, "(" | "["))
        });
        if flag_count <= 1 && !has_opening_delimiter {
            expanded.push(node);
            continue;
        }
        let additional = flag_count.saturating_sub(1);
        if builder.node_count().saturating_add(additional) > max_nodes {
            if outcome.node_limit_location.is_none() {
                outcome.node_limit_location = builder.node_location(node);
            }
            expanded.push(node);
            continue;
        }

        let location = builder.node_location(node);
        let mut inherited_flags = builder.node_flags(node).unwrap_or_default();
        let mut first = !leading_separator;
        if leading_separator {
            let _ = builder.replace_children(node, &[]);
            expanded.push(node);
        }
        for argument in arguments {
            if builder.node_text(argument) == Some("|") {
                expanded.push(argument);
                continue;
            }
            if builder
                .node_text(argument)
                .is_some_and(|text| matches!(text, "(" | "["))
            {
                let delimiter_text = builder.node_text(argument).map(str::to_owned);
                mark_opening_delimiter(builder, argument, delimiter_text.as_deref());
                if expanded.is_empty() {
                    transfer_line_start(builder, node, argument);
                    if let Some(mut flags) = builder.node_flags(node) {
                        flags.line_start = false;
                        let _ = builder.set_node_flags(node, flags);
                    }
                    inherited_flags.line_start = false;
                }
                expanded.push(argument);
                continue;
            }
            let flag = if first {
                first = false;
                node
            } else {
                let Some(flag) = builder.push(allocation_parent, NodeKind::Element) else {
                    continue;
                };
                inherited_flags.line_start = false;
                let _ = builder.macro_name(flag, "Fl");
                let _ = builder.set_node_location(flag, location.clone());
                let _ = builder.set_node_flags(flag, inherited_flags);
                flag
            };
            let _ = builder.replace_children(flag, &[argument]);
            expanded.push(flag);
        }
    }
    expanded
}

/// An mdoc list-term spelling of `Fl Fl long` is a single long-option
/// element.  The first `Fl` is a semantic prefix, not a separately rendered
/// empty flag; `mdoc_validate.c` therefore retains the second element and
/// gives its word the escaped leading dash.  Keep this deliberately local to
/// already split list heads so ordinary adjacent inline macros retain their
/// own source structure.
pub(super) fn collapse_long_option_prefixes(
    builder: &mut DocumentBuilder,
    nodes: &[NodeId],
) -> Vec<NodeId> {
    let mut collapsed = Vec::with_capacity(nodes.len());
    let mut index = 0;
    while index < nodes.len() {
        let Some(next) = nodes.get(index.saturating_add(1)).copied() else {
            collapsed.push(nodes[index]);
            break;
        };
        let current = nodes[index];
        let is_empty_prefix = builder.node_macro_name(current) == Some("Fl")
            && builder.children(current).is_some_and(<[NodeId]>::is_empty);
        let Some(text) = (builder.node_macro_name(next) == Some("Fl"))
            .then(|| {
                builder
                    .children(next)
                    .and_then(|children| children.first().copied())
            })
            .flatten()
            .and_then(|child| builder.node_text(child).map(str::to_owned))
        else {
            collapsed.push(current);
            index = index.saturating_add(1);
            continue;
        };
        if !is_empty_prefix || text.is_empty() {
            collapsed.push(current);
            index = index.saturating_add(1);
            continue;
        }
        if let Some(child) = builder
            .children(next)
            .and_then(|children| children.first().copied())
        {
            let _ = builder.text(child, format!("\\-{text}"));
            collapsed.push(next);
            index = index.saturating_add(2);
        } else {
            collapsed.push(current);
            index = index.saturating_add(1);
        }
    }
    collapsed
}
