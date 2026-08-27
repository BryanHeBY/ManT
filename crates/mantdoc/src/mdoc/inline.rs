use super::{
    ArgumentPlacement, DocumentBuilder, NodeId, NodeKind, Recovery, ScopeFrame, StructureOutcome,
    append_to_parent, close_explicit_partial_scope, coalesce_adjacent_text_children,
    coalesce_implicit_partial_body_text, coalesce_text_children,
    column_system_name_starts_next_element, expand_fl_elements, generated_system_name,
    insert_generated_system_names, is_explicit_partial_close, is_tag_style_delimiter_restart_macro,
    make_block, mark_explicit_partial_close_tail_line_start, mark_sentence_end,
    mark_synopsis_pretty,
};

/// Split a callable mdoc macro's scanner tokens into the source-order inline
/// macro events that mandoc's `in_line()` parser constructs.  The scanner
/// already owns one `Text` arena record per lexical token, so macro-name
/// tokens can be reclassified in place: no new AST node allocation is needed
/// and every argument keeps its original source location.
#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered in_line state transitions without hiding macro-boundary cases.
pub(super) fn split_inline_macro_events(
    builder: &mut DocumentBuilder,
    node: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let Some(name) = builder.node_macro_name(node) else {
        return vec![node];
    };
    if !is_inline_mdoc_macro(name) && name != "Vt" {
        return vec![node];
    }
    let Some(tokens) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return vec![node];
    };

    let mut events = vec![node];
    let mut remaining_arguments = mdoc_inline_argument_limit(name);
    let mut current = (remaining_arguments != Some(0)).then_some(node);
    let mut current_children = Vec::new();
    let mut resume_after_delimiter = None::<NodeId>;
    let mut pending_trailing_opening_delimiters = Vec::<NodeId>::new();
    let mut pending_nested_leading_delimiter_sentence_end = None::<NodeId>;
    let mut reopened_after_middle_delimiter = false;
    // A top-level zero-argument request arrives from the scanner with its
    // following lexical tokens provisionally attached below the control
    // node.  They are source-order siblings, not macro arguments (`.Ux .`
    // is the visible regression); detach them before emitting the event
    // stream.  Reclassified inline tokens already have no children.
    if remaining_arguments == Some(0) {
        let _ = builder.replace_children(node, &[]);
    }
    for (token_index, token) in tokens.iter().copied().enumerate() {
        // A nested source macro can publish a leading closing delimiter that
        // ends a sentence only when it is the final token of its physical
        // request. Any next token resumes or supersedes that private macro
        // state and therefore clears the pending sentence boundary.
        pending_nested_leading_delimiter_sentence_end = None;
        if let Some(current_node) = current
            && let Some(current_name) = builder.node_macro_name(current_node)
            && let Some(close) = explicit_partial_block_close(current_name)
        {
            // A callable explicit partial block owns its raw source stream
            // through its paired closer. Its later structural pass then
            // parses that body, including nested inline macros, as a unit.
            current_children.push(token);
            if builder.node_text(token) == Some(close) {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                current = None;
                remaining_arguments = None;
                current_children.clear();
            }
            continue;
        }
        if let Some(current_node) = current
            && builder
                .node_macro_name(current_node)
                .is_some_and(is_implicit_partial_block_macro)
        {
            // An implicit partial block extends to the rest of its source
            // line. Nested macros are parsed when its Body is constructed,
            // matching the upstream block parser's first-call handoff.
            current_children.push(token);
            continue;
        }
        if let Some(current_node) = current
            && matches!(builder.node_macro_name(current_node), Some("Ar" | "Pa"))
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // `in_line()` publishes an opening delimiter between consecutive
            // Ar and Pa elements. A leading delimiter simply moves before the
            // first element; a later one ends the current element and starts
            // the next.
            mark_opening_delimiter(builder, token, Some("("));
            if current_children.is_empty() {
                events.retain(|event| *event != current_node);
                events.push(token);
                if let Some(mut flags) = builder.node_flags(current_node) {
                    flags.line_start = false;
                    let _ = builder.set_node_flags(current_node, flags);
                }
                if let Some(mut flags) = builder.node_flags(token) {
                    flags.line_start = true;
                    let _ = builder.set_node_flags(token, flags);
                }
                events.push(current_node);
                continue;
            }
            finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            events.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, node, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            events.push(reopened);
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            continue;
        }
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Nm")
            && current_children.is_empty()
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // Nm begins a leading-delimiter form by publishing the delimiter
            // outside a temporary empty Element, then reuses that Element for
            // the following literal name.
            let delimiter = builder.node_text(token).map(str::to_owned);
            mark_opening_delimiter(builder, token, delimiter.as_deref());
            events.retain(|event| *event != current_node);
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(current_node) {
                flags.line_start = false;
                let _ = builder.set_node_flags(current_node, flags);
            }
            events.push(token);
            events.push(current_node);
            continue;
        }
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Xr")
            && current_children.is_empty()
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // `in_line_argn()` publishes a leading delimiter before the
            // fixed two-argument Xr element. It owns the request's
            // line-start provenance, leaving the reference itself inline.
            let delimiter = builder.node_text(token).map(str::to_owned);
            mark_opening_delimiter(builder, token, delimiter.as_deref());
            events.retain(|event| *event != current_node);
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(current_node) {
                flags.line_start = false;
                let _ = builder.set_node_flags(current_node, flags);
            }
            events.push(token);
            events.push(current_node);
            continue;
        }
        if let Some(current_node) = current
            && let Some(remaining) = remaining_arguments
            && remaining > 0
            && (builder.node_macro_name(current_node) == Some("Pf")
                || !builder
                    .node_text(token)
                    .is_some_and(is_mdoc_closing_delimiter))
            && !(builder.node_macro_name(current_node) == Some("St")
                && builder.node_text(token).is_some_and(is_mdoc_callable_macro))
        {
            // A finite-argument macro owns its next token literally, except
            // St's callable first argument: `.St Fl called` is an empty St
            // followed by Fl, matching `in_line_argn()`. Pf also owns a
            // leading closing delimiter literally: it is its prefix, rather
            // than outer punctuation.
            let terminal_pf_prefix = builder.node_macro_name(current_node) == Some("Pf")
                && builder
                    .node_text(token)
                    .is_some_and(is_mdoc_closing_delimiter)
                && tokens[token_index + 1..].is_empty();
            current_children.push(token);
            if remaining == 1 {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                if terminal_pf_prefix {
                    mark_sentence_end(builder, token);
                }
                current = None;
                remaining_arguments = None;
                current_children.clear();
            } else {
                remaining_arguments = Some(remaining - 1);
            }
            continue;
        }
        let token_text = builder.node_text(token).map(str::to_owned);
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Lk")
            && !token_text.as_deref().is_some_and(is_mdoc_callable_macro)
        {
            // `in_line()` keeps a link scope open through ordinary source
            // words and punctuation. Only a following callable macro ends
            // the link and starts independent inline flow.
            current_children.push(token);
            continue;
        }
        if let Some(current_node) = current
            && is_tag_style_delimiter_restart_macro(builder.node_macro_name(current_node))
            && token_text
                .as_deref()
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // `in_line_argn()` keeps opening punctuation outside these
            // tag-style macros,
            // then resumes the macro with the next ordinary word. This also
            // applies when the opening punctuation is the first token: the
            // empty source element stays private so structural validation can
            // retain its empty-macro diagnostic if there is no later word.
            let starts_tag_macro = current_children.is_empty();
            finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            mark_opening_delimiter(builder, token, token_text.as_deref());
            if starts_tag_macro {
                transfer_line_start(builder, current_node, token);
                if builder
                    .node_source_position(current_node)
                    .is_some_and(|position| position.column > 2)
                {
                    // A nested source-spelled tag macro may consist solely
                    // of an isolated delimiter. It is kept as an opening
                    // delimiter only if a subsequent callable macro proves
                    // that the flow actually continues.
                    pending_trailing_opening_delimiters.push(token);
                }
            } else {
                // Whether this is an opening delimiter depends on whether
                // later input actually resumes the source macro. Defer the
                // public flag until the complete request has been consumed.
                pending_trailing_opening_delimiters.push(token);
            }
            events.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = Some(current_node);
            reopened_after_middle_delimiter = false;
            continue;
        }
        if let Some(current_node) = current
            && is_tag_style_delimiter_restart_macro(builder.node_macro_name(current_node))
            && current_children.is_empty()
            && token_text.as_deref().is_some_and(is_mdoc_closing_delimiter)
        {
            // A leading closing delimiter is not tag-style macro content.
            // libmandoc publishes it as the line's first literal node without
            // a delimiter-close flag, then lets a later word reopen the same
            // source request. A nested source-spelled tag macro at physical
            // end keeps the punctuation's sentence boundary, but a later
            // token clears it before this splitter returns.
            let nested_source_macro = builder
                .node_source_position(current_node)
                .is_some_and(|position| position.column > 2);
            finish_inline_element(builder, current_node, &[], spacing_enabled);
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(token) {
                flags.delimiter_close = false;
                flags.sentence_end = false;
                let _ = builder.set_node_flags(token, flags);
            }
            if nested_source_macro {
                pending_nested_leading_delimiter_sentence_end = Some(token);
            }
            events.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = Some(current_node);
            reopened_after_middle_delimiter = false;
            continue;
        }
        let inline_name = builder
            .node_text(token)
            .filter(|text| is_mdoc_callable_macro(text))
            .map(str::to_owned);
        if let Some(inline_name) = inline_name {
            let st_source = current.filter(|current_node| {
                builder.node_macro_name(*current_node) == Some("St") && current_children.is_empty()
            });
            resume_after_delimiter = None;
            pending_trailing_opening_delimiters.clear();
            if let Some(current) = current {
                if reopened_after_middle_delimiter && current_children.is_empty() {
                    // A middle delimiter only reopens the preceding macro
                    // for following ordinary words.  A callable macro takes
                    // over the stream directly, so the temporary empty
                    // element is not public (`Ar word | Fl flag`).
                    events.retain(|event| *event != current);
                } else {
                    finish_inline_element(builder, current, &current_children, spacing_enabled);
                }
            }
            let remaining = mdoc_inline_argument_limit(&inline_name);
            if !builder.clear_node_text(token)
                || !builder.set_node_kind(token, NodeKind::Element)
                || !builder.macro_name(token, inline_name)
            {
                return events;
            }
            if let Some(source) = st_source {
                transfer_line_start(builder, source, token);
            }
            events.push(token);
            current = (remaining != Some(0)).then_some(token);
            remaining_arguments = remaining;
            current_children.clear();
            reopened_after_middle_delimiter = false;
        } else if token_text.as_deref().is_some_and(is_mdoc_middle_delimiter) {
            let Some(current_node) = current else {
                events.push(token);
                continue;
            };
            let leading_tag_macro_delimiter =
                is_tag_style_delimiter_restart_macro(builder.node_macro_name(current_node))
                    && current_children.is_empty();
            if is_empty_middle_delimiter_element(builder, current_node, &current_children) {
                events.retain(|event| *event != current_node);
                if let Some(mut flags) = builder.node_flags(token) {
                    // The discarded source macro owns no public node, so its
                    // middle delimiter becomes this input line's first event.
                    flags.line_start = true;
                    let _ = builder.set_node_flags(token, flags);
                }
            } else {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            }
            if leading_tag_macro_delimiter {
                // Leading middle delimiters in tag-style inline macros follow
                // the same private-element rule as leading opening and
                // closing delimiters.
                transfer_line_start(builder, current_node, token);
            }
            events.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, node, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            events.push(reopened);
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            reopened_after_middle_delimiter = true;
        } else if token_text.as_deref().is_some_and(is_mdoc_closing_delimiter) {
            let resume = current.filter(|current| {
                builder.node_macro_name(*current) != Some("Fn")
                    && !(builder.node_macro_name(*current) == Some("Nm")
                        && current_children.is_empty())
            });
            if let Some(current) = current {
                finish_inline_element(builder, current, &current_children, spacing_enabled);
            }
            if let Some(mut flags) = builder.node_flags(token) {
                flags.delimiter_close = true;
                let _ = builder.set_node_flags(token, flags);
            }
            mark_sentence_end(builder, token);
            events.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = resume;
            reopened_after_middle_delimiter = false;
        } else if current.is_some() {
            current_children.push(token);
            reopened_after_middle_delimiter = false;
        } else if let Some(source) = resume_after_delimiter
            && is_tag_style_delimiter_restart_macro(builder.node_macro_name(source))
            && token_text
                .as_deref()
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            // A chain of opening delimiters stays outside the private empty
            // restart element (`Em a Em ( [ Em b`).  Do not reopen until a
            // real word arrives; a following callable macro consumes the
            // pending restart without exposing it at all.
            mark_opening_delimiter(builder, token, token_text.as_deref());
            events.push(token);
            if !pending_trailing_opening_delimiters.is_empty() {
                pending_trailing_opening_delimiters.push(token);
            }
        } else if let Some(source) = resume_after_delimiter.take() {
            let Some(reopened) = reopen_inline_element(builder, node, source, max_nodes, outcome)
            else {
                events.push(token);
                continue;
            };
            events.push(reopened);
            current = Some(reopened);
            current_children.push(token);
            pending_trailing_opening_delimiters.clear();
            reopened_after_middle_delimiter = false;
        } else {
            mark_opening_delimiter(builder, token, token_text.as_deref());
            events.push(token);
        }
    }
    if let Some(current) = current {
        if reopened_after_middle_delimiter && current_children.is_empty() {
            // A middle delimiter opens a provisional continuation only for a
            // following word. At physical end of line that continuation is
            // not public; retaining it would turn `.Em a Em |` into a second
            // spurious empty Em recovery.
            events.retain(|event| *event != current);
        } else {
            finish_inline_element(builder, current, &current_children, spacing_enabled);
        }
    }
    if let Some(delimiter) = pending_nested_leading_delimiter_sentence_end {
        mark_sentence_end(builder, delimiter);
    }
    for delimiter in pending_trailing_opening_delimiters {
        if let Some(mut flags) = builder.node_flags(delimiter) {
            flags.delimiter_open = false;
            let _ = builder.set_node_flags(delimiter, flags);
        }
    }
    clear_nonterminal_inline_delimiter_flags(builder, &events);
    events
}

/// Split a scanner-tokenized argument sequence directly beneath `parent`.
///
/// `Vt` in a SYNOPSIS section is an implicit partial block: its Body owns the
/// literal prefix and any nested callable macros, rather than the outer `Vt`
/// element owning all scanner tokens or the nested macros escaping as siblings.
pub(super) fn split_mdoc_inline_children(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let Some(tokens) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    split_mdoc_inline_tokens(
        builder,
        parent,
        &tokens,
        spacing_enabled,
        max_nodes,
        outcome,
    )
}

/// Classify an already-selected run of scanner tokens as direct text, callable
/// elements, and closing delimiters.  It does not attach the returned nodes;
/// callers can place a block body or a post-closer tail explicitly.
#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered in_line state transitions without hiding macro-boundary cases.
pub(super) fn split_mdoc_inline_tokens(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    split_mdoc_inline_tokens_with_options(
        builder,
        allocation_parent,
        tokens,
        spacing_enabled,
        max_nodes,
        outcome,
        false,
    )
}

/// Split mdoc inline tokens with the one `Bl -column` provenance distinction
/// that is not part of ordinary inline flow.  A system-name spelling at the
/// very start of a tab-created column remains literal text in libmandoc.
#[allow(clippy::too_many_lines)] // Mirrors mdoc's ordered in_line state transitions without hiding macro-boundary cases.
pub(super) fn split_mdoc_inline_tokens_with_options(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    suppress_first_tab_column_system_name: bool,
) -> Vec<NodeId> {
    let mut children = Vec::new();
    let mut current = None::<NodeId>;
    let mut remaining_arguments = None::<usize>;
    let mut current_children = Vec::new();
    let mut resume_after_delimiter = None::<NodeId>;
    let mut reopened_after_middle_delimiter = false;
    for (token_index, &token) in tokens.iter().enumerate() {
        if let Some(current_node) = current
            && matches!(builder.node_macro_name(current_node), Some("Ar" | "Pa"))
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            mark_opening_delimiter(builder, token, Some("("));
            if current_children.is_empty() {
                children.push(token);
                if let Some(mut flags) = builder.node_flags(current_node) {
                    flags.line_start = false;
                    let _ = builder.set_node_flags(current_node, flags);
                }
                if let Some(mut flags) = builder.node_flags(token) {
                    flags.line_start = true;
                    let _ = builder.set_node_flags(token, flags);
                }
                children.push(current_node);
                continue;
            }
            finish_inline_element(builder, current_node, &current_children, spacing_enabled);
            children.push(current_node);
            children.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, allocation_parent, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            continue;
        }
        if let Some(current_node) = current
            && builder
                .node_macro_name(current_node)
                .is_some_and(is_implicit_partial_block_macro)
        {
            // An implicit partial block owns the rest of its parsed argument
            // stream. Defer callable classification until its Body exists,
            // otherwise `.Op Ar argument` escapes as an empty Op followed by
            // a sibling Ar instead of a nested partial block.
            current_children.push(token);
            continue;
        }
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Xr")
            && current_children.is_empty()
            && builder
                .node_text(token)
                .is_some_and(|text| matches!(text, "(" | "["))
        {
            let delimiter = builder.node_text(token).map(str::to_owned);
            mark_opening_delimiter(builder, token, delimiter.as_deref());
            transfer_line_start(builder, current_node, token);
            if let Some(mut flags) = builder.node_flags(current_node) {
                flags.line_start = false;
                let _ = builder.set_node_flags(current_node, flags);
            }
            children.push(token);
            children.push(current_node);
            continue;
        }
        if let Some(current_node) = current
            && let Some(remaining) = remaining_arguments
            && remaining > 0
            && (builder.node_macro_name(current_node) == Some("Pf")
                || !builder
                    .node_text(token)
                    .is_some_and(is_mdoc_closing_delimiter))
            && !(builder.node_macro_name(current_node) == Some("St")
                && builder.node_text(token).is_some_and(is_mdoc_callable_macro))
            && !column_system_name_starts_next_element(builder, current_node, token)
        {
            current_children.push(token);
            if remaining == 1 {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                children.push(current_node);
                current = None;
                remaining_arguments = None;
                current_children.clear();
            } else {
                remaining_arguments = Some(remaining - 1);
            }
            continue;
        }
        let token_text = builder.node_text(token).map(str::to_owned);
        if let Some(current_node) = current
            && builder.node_macro_name(current_node) == Some("Lk")
            && !token_text.as_deref().is_some_and(is_mdoc_callable_macro)
        {
            current_children.push(token);
            continue;
        }
        let inline_name = builder
            .node_text(token)
            .filter(|text| {
                is_mdoc_callable_macro(text)
                    && !(suppress_first_tab_column_system_name
                        && token_index == 0
                        && generated_system_name(text).is_some())
            })
            .map(str::to_owned);
        if let Some(inline_name) = inline_name {
            let st_source = current.filter(|current_node| {
                builder.node_macro_name(*current_node) == Some("St") && current_children.is_empty()
            });
            resume_after_delimiter = None;
            if let Some(current) = current
                && !(reopened_after_middle_delimiter && current_children.is_empty())
            {
                finish_inline_element(builder, current, &current_children, spacing_enabled);
                children.push(current);
            }
            let remaining = mdoc_inline_argument_limit(&inline_name);
            if !builder.clear_node_text(token)
                || !builder.set_node_kind(token, NodeKind::Element)
                || !builder.macro_name(token, inline_name)
            {
                children.push(token);
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            }
            if let Some(source) = st_source {
                transfer_line_start(builder, source, token);
            }
            if builder.node_macro_name(token) == Some("Ns")
                && no_space_macro_requires_warning(builder, token, &tokens[token_index + 1..])
            {
                outcome.recoveries.push(Recovery::NoSpaceMacro {
                    location: builder.node_location(token),
                });
            }
            if remaining == Some(0) {
                children.push(token);
                current = None;
            } else {
                current = Some(token);
            }
            remaining_arguments = remaining;
            current_children.clear();
        } else if token_text.as_deref().is_some_and(is_mdoc_middle_delimiter) {
            let Some(current_node) = current else {
                children.push(token);
                continue;
            };
            if !is_empty_middle_delimiter_element(builder, current_node, &current_children) {
                finish_inline_element(builder, current_node, &current_children, spacing_enabled);
                children.push(current_node);
            } else if let Some(mut flags) = builder.node_flags(token) {
                flags.line_start = true;
                let _ = builder.set_node_flags(token, flags);
            }
            children.push(token);
            let Some(reopened) =
                reopen_inline_element(builder, allocation_parent, current_node, max_nodes, outcome)
            else {
                current = None;
                remaining_arguments = None;
                current_children.clear();
                continue;
            };
            current = Some(reopened);
            remaining_arguments = None;
            current_children.clear();
            reopened_after_middle_delimiter = true;
        } else if token_text.as_deref().is_some_and(is_mdoc_closing_delimiter) {
            let resume = current.filter(|current| builder.node_macro_name(*current) != Some("Fn"));
            if let Some(current) = current {
                finish_inline_element(builder, current, &current_children, spacing_enabled);
                children.push(current);
            }
            if let Some(mut flags) = builder.node_flags(token) {
                flags.delimiter_close = true;
                let _ = builder.set_node_flags(token, flags);
            }
            mark_sentence_end(builder, token);
            children.push(token);
            current = None;
            remaining_arguments = None;
            current_children.clear();
            resume_after_delimiter = resume;
            reopened_after_middle_delimiter = false;
        } else if current.is_some() {
            current_children.push(token);
            reopened_after_middle_delimiter = false;
        } else if let Some(source) = resume_after_delimiter.take() {
            let Some(reopened) =
                reopen_inline_element(builder, allocation_parent, source, max_nodes, outcome)
            else {
                children.push(token);
                continue;
            };
            current = Some(reopened);
            current_children.push(token);
            reopened_after_middle_delimiter = false;
        } else {
            mark_opening_delimiter(builder, token, token_text.as_deref());
            children.push(token);
        }
    }
    if let Some(current) = current {
        finish_inline_element(builder, current, &current_children, spacing_enabled);
        children.push(current);
    }
    clear_nonterminal_inline_delimiter_flags(builder, &children);
    children
}

/// A middle delimiter suppresses an empty default or compatibility element.
/// The delimiter stays in surrounding flow and the following token opens the
/// next element, matching `in_line()`'s empty-first element handling.
pub(super) fn is_empty_middle_delimiter_element(
    builder: &DocumentBuilder,
    node: NodeId,
    children: &[NodeId],
) -> bool {
    matches!(builder.node_macro_name(node), Some("Ar" | "Nm" | "Pa")) && children.is_empty()
}

/// A delimiter only ends a sentence when it is the final inline event on its
/// source request. Reopened macros after `|` or closing punctuation continue
/// the same input line, so their preceding `.`/`!`/`?` remains nonterminal.
pub(super) fn clear_nonterminal_inline_delimiter_flags(
    builder: &mut DocumentBuilder,
    events: &[NodeId],
) {
    for (index, event) in events.iter().copied().enumerate() {
        if events[index + 1..].is_empty()
            || !builder
                .node_text(event)
                .is_some_and(is_mdoc_closing_delimiter)
        {
            continue;
        }
        if let Some(mut flags) = builder.node_flags(event) {
            flags.sentence_end = false;
            let _ = builder.set_node_flags(event, flags);
        }
    }
}

/// Split a physical explicit partial-block invocation at its same-line closer.
/// The closer is structural syntax and therefore does not survive as a public
/// node; the following tokens re-enter the surrounding source-order flow.
pub(super) fn split_explicit_partial_block_tail(
    builder: &mut DocumentBuilder,
    node: NodeId,
    close: &str,
) -> Vec<NodeId> {
    let Some(tokens) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let Some(close_index) = matching_explicit_partial_close_index(builder, &tokens, close) else {
        return Vec::new();
    };
    let tail = tokens[close_index.saturating_add(1)..].to_vec();
    let _ = builder.replace_children(node, &tokens[..close_index]);
    tail
}

/// Find the closer belonging to an explicit partial opener's current source
/// request.  A physical request may contain nested explicit pairs, so its
/// first syntactic closer is not necessarily the closer for `outer_close`:
/// `.Oo Oo No a Oc Oc` has two distinct `Oc` tokens.
pub(super) fn matching_explicit_partial_close_index(
    builder: &DocumentBuilder,
    tokens: &[NodeId],
    outer_close: &str,
) -> Option<usize> {
    let mut expected_closes = vec![outer_close];
    for (index, token) in tokens.iter().copied().enumerate() {
        let Some(text) = builder.node_text(token) else {
            continue;
        };
        if let Some(close) = explicit_partial_block_close(text) {
            expected_closes.push(close);
        } else if expected_closes
            .last()
            .is_some_and(|expected| *expected == text)
        {
            expected_closes.pop();
            if expected_closes.is_empty() {
                return Some(index);
            }
        }
    }
    None
}

/// Turn every complete explicit partial pair nested directly below `parent`
/// into its public Block/Head/Body projection before the inline splitter sees
/// it.  The splitter otherwise classifies a nested `.Oo` as an ordinary
/// element and turns its `Oc` tokens into visible prose.
pub(super) fn structure_matched_explicit_partial_blocks(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let events = structure_matched_explicit_partial_events(
        builder,
        &children,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let _ = builder.replace_children(parent, &events);
}

/// The event-level form of `structure_matched_explicit_partial_blocks` is
/// also used for a same-line tail, which has no single public parent until it
/// is attached after any outer scope close has been processed.
pub(super) fn structure_matched_explicit_partial_events(
    builder: &mut DocumentBuilder,
    children: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let mut events = Vec::with_capacity(children.len());
    let mut cursor = 0;
    while cursor < children.len() {
        let opener = children[cursor];
        let Some(name) = builder.node_text(opener).map(str::to_owned) else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let Some(close) = explicit_partial_block_close(&name) else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let Some(relative_close) =
            matching_explicit_partial_close_index(builder, &children[cursor + 1..], close)
        else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let close_index = cursor + relative_close + 1;
        let nested_tokens = children[cursor + 1..close_index].to_vec();
        let inherits_synopsis = builder
            .node_flags(opener)
            .is_some_and(|flags| flags.synopsis_pretty);
        if !builder.clear_node_text(opener)
            || !builder.set_node_kind(opener, NodeKind::Element)
            || !builder.macro_name(opener, name.as_str())
        {
            events.push(opener);
            cursor += 1;
            continue;
        }
        let Some((head, body)) = make_block(
            builder,
            opener,
            &name,
            ArgumentPlacement::BodyTokens,
            max_nodes,
            outcome,
        ) else {
            events.push(opener);
            cursor += 1;
            continue;
        };
        let _ = builder.replace_children(body, &nested_tokens);
        if inherits_synopsis {
            mark_synopsis_pretty(builder, head);
            mark_synopsis_pretty(builder, body);
        }
        let nested_events = structure_matched_explicit_partial_events(
            builder,
            &nested_tokens,
            spacing_enabled,
            max_nodes,
            outcome,
        );
        let _ = builder.replace_children(body, &nested_events);
        let nested_children =
            split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
        let _ = builder.replace_children(body, &nested_children);
        clear_leading_explicit_partial_punctuation(builder, body);
        move_explicit_leading_open_delimiter(builder, opener, head, body);
        if matches!(name.as_str(), "Bo" | "Po") {
            coalesce_adjacent_text_children(builder, body);
        }
        events.push(opener);
        cursor = close_index + 1;
    }
    events
}

/// Project a retained source-line tail after its enclosing explicit opener
/// has closed.  The caller determines any global explicit closers first;
/// each resulting segment can then safely form nested explicit and implicit
/// blocks without leaking close syntax into the public tree.
pub(super) fn explicit_partial_tail_events(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    tokens: &[NodeId],
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<NodeId> {
    let events = structure_matched_explicit_partial_events(
        builder,
        tokens,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let events = split_mdoc_inline_tokens(
        builder,
        allocation_parent,
        &events,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    for event in &events {
        structure_implicit_partial_block(builder, *event, max_nodes, outcome, spacing_enabled);
    }
    events
}

/// Attach the source-order tail of an explicit partial opener.  Local explicit
/// pairs are projected as blocks within a segment; a closer not owned by such
/// a pair instead restores the preceding cross-line scope before the next
/// segment is attached.
#[allow(clippy::too_many_arguments)] // This is the source-order scope hand-off itself.
pub(super) fn append_explicit_partial_tail(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    scopes: &mut Vec<ScopeFrame>,
    implicitly_closed: &mut Vec<&'static str>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
    allocation_parent: NodeId,
    tail: &[NodeId],
    mark_tail_line_start: bool,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let mut segment_start = 0;
    let mut local_closes = Vec::new();
    let mut mark_next_tail_segment = mark_tail_line_start;
    for (index, token) in tail.iter().copied().enumerate() {
        let Some(text) = builder.node_text(token).map(str::to_owned) else {
            continue;
        };
        if let Some(local_close) = explicit_partial_block_close(&text) {
            local_closes.push(local_close);
            continue;
        }
        if local_closes
            .last()
            .is_some_and(|local_close| *local_close == text)
        {
            local_closes.pop();
            continue;
        }
        if !local_closes.is_empty() || !is_explicit_partial_close(&text) {
            continue;
        }
        let events = explicit_partial_tail_events(
            builder,
            allocation_parent,
            &tail[segment_start..index],
            spacing_enabled,
            max_nodes,
            outcome,
        );
        if mark_next_tail_segment && !events.is_empty() {
            mark_explicit_partial_close_tail_line_start(builder, &events);
            mark_next_tail_segment = false;
        }
        for sibling in events {
            append_to_parent(builder, root, root_children, *active_body, sibling);
        }
        close_explicit_partial_scope(scopes, implicitly_closed, active_body, flow_parent, &text);
        segment_start = index + 1;
    }
    let events = explicit_partial_tail_events(
        builder,
        allocation_parent,
        &tail[segment_start..],
        spacing_enabled,
        max_nodes,
        outcome,
    );
    if mark_next_tail_segment && !events.is_empty() {
        mark_explicit_partial_close_tail_line_start(builder, &events);
    }
    let has_cross_line_explicit_opener = events.iter().any(|event| {
        builder.node_kind(*event) == Some(NodeKind::Element)
            && builder
                .node_macro_name(*event)
                .is_some_and(|name| explicit_partial_block_close(name).is_some())
    });
    for sibling in events {
        append_to_parent(builder, root, root_children, *active_body, sibling);
    }
    if has_cross_line_explicit_opener {
        let tail_scopes = structure_unclosed_explicit_partial_blocks(
            builder,
            *active_body,
            spacing_enabled,
            max_nodes,
            outcome,
        );
        for scope in tail_scopes {
            // The remainder of a closer request is one mdoc phrase even
            // when it opens a new cross-line partial (`.Bc Po po pc`).
            // The generic nested-opener path preserves lexical tokens for
            // other contexts, so apply this tightening only to tail scopes.
            coalesce_adjacent_text_children(builder, scope.body);
            *active_body = scope.body;
            *flow_parent = scope.body;
            scopes.push(scope);
        }
    }
}

/// Convert unclosed explicit partial openers on this source request into a
/// nested scope stack.  Complete pairs were removed first by
/// `structure_matched_explicit_partial_blocks`; the first remaining opener
/// owns the request suffix and resumes its parent only when a later physical
/// closer arrives.
pub(super) fn structure_unclosed_explicit_partial_blocks(
    builder: &mut DocumentBuilder,
    outer_body: NodeId,
    spacing_enabled: bool,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Vec<ScopeFrame> {
    let Some(children) = builder.children(outer_body).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let Some(opener_index) = children.iter().position(|node| {
        builder
            .node_text(*node)
            .is_some_and(|name| explicit_partial_block_close(name).is_some())
            || (builder.node_kind(*node) == Some(NodeKind::Element)
                && builder
                    .node_macro_name(*node)
                    .is_some_and(|name| explicit_partial_block_close(name).is_some()))
    }) else {
        return Vec::new();
    };
    let opener = children[opener_index];
    let name = builder
        .node_text(opener)
        .or_else(|| builder.node_macro_name(opener))
        .expect("the position predicate required text")
        .to_owned();
    let close = explicit_partial_block_close(&name)
        .expect("the position predicate required an explicit partial opener");
    let mut suffix = builder
        .children(opener)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    suffix.extend_from_slice(&children[opener_index + 1..]);
    let inherits_synopsis = builder
        .node_flags(opener)
        .is_some_and(|flags| flags.synopsis_pretty);
    if (builder.node_text(opener).is_some() && !builder.clear_node_text(opener))
        || !builder.set_node_kind(opener, NodeKind::Element)
        || !builder.macro_name(opener, name.as_str())
    {
        return Vec::new();
    }
    let Some((head, body)) = make_block(
        builder,
        opener,
        &name,
        ArgumentPlacement::BodyTokens,
        max_nodes,
        outcome,
    ) else {
        return Vec::new();
    };
    let _ = builder.replace_children(body, &suffix);
    if inherits_synopsis {
        mark_synopsis_pretty(builder, head);
        mark_synopsis_pretty(builder, body);
    }
    structure_matched_explicit_partial_blocks(builder, body, spacing_enabled, max_nodes, outcome);
    let mut nested_scopes = structure_unclosed_explicit_partial_blocks(
        builder,
        body,
        spacing_enabled,
        max_nodes,
        outcome,
    );
    let nested_children =
        split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
    let _ = builder.replace_children(body, &nested_children);
    clear_leading_explicit_partial_punctuation(builder, body);
    move_explicit_leading_open_delimiter(builder, opener, head, body);
    if matches!(name.as_str(), "Bo" | "Bro" | "Do") {
        coalesce_adjacent_text_children(builder, body);
    }
    let mut retained = children[..opener_index].to_vec();
    retained.push(opener);
    let _ = builder.replace_children(outer_body, &retained);
    let mut scopes = vec![ScopeFrame {
        close,
        open: opener,
        body,
        tail_on_close: false,
        transparent_target_taken: false,
        suppress_implicit_ancestor_break: false,
        resume_active: outer_body,
        resume_flow: outer_body,
    }];
    scopes.append(&mut nested_scopes);
    scopes
}

/// An explicit partial opener at the end of an `.It` header extends that
/// header across following physical macro lines.  It is structurally the same
/// `Ao`/`Bo`/… Block as an opener in ordinary body flow, but its close must
/// resume the item's Body rather than the surrounding list Body.
pub(super) fn structure_item_head_explicit_partial(
    builder: &mut DocumentBuilder,
    item_head: NodeId,
    item_body: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<ScopeFrame> {
    let opener = *builder.children(item_head)?.last()?;
    let name = builder.node_macro_name(opener)?.to_owned();
    let close = explicit_partial_block_close(&name)?;
    let (head, body) = make_block(
        builder,
        opener,
        &name,
        ArgumentPlacement::BodyTokens,
        max_nodes,
        outcome,
    )?;
    if let Some(mut flags) = builder.node_flags(item_body) {
        // The item body is opened while its header extension remains active;
        // mandoc preserves the authored `.It` line-start marker on that
        // deferred body, matching the analogous `.It Xo` transition.
        flags.line_start = true;
        let _ = builder.set_node_flags(item_body, flags);
    }
    move_explicit_leading_open_delimiter(builder, opener, head, body);
    if matches!(name.as_str(), "Ao" | "Bo") {
        // Item-header partials bypass the ordinary top-level branches;
        // retain the one-phrase representation used by their legacy blocks.
        coalesce_adjacent_text_children(builder, body);
    }
    Some(ScopeFrame {
        close,
        open: opener,
        body,
        tail_on_close: false,
        transparent_target_taken: false,
        suppress_implicit_ancestor_break: false,
        resume_active: item_body,
        resume_flow: item_body,
    })
}

/// Initial pure-inline subset of `MDOC_CALLABLE | MDOC_PARSED` macros.
/// Partial/full block macros stay with their dedicated structural state
/// machine until their distinct argument and scope rules are implemented.
pub(super) fn is_inline_mdoc_macro(name: &str) -> bool {
    matches!(
        name,
        "Ad" | "An"
            | "Ap"
            | "Ar"
            | "Bsx"
            | "Bx"
            | "Cd"
            | "Cm"
            | "Dx"
            | "Dv"
            | "Em"
            | "Er"
            | "Ev"
            | "Fa"
            | "Fl"
            | "Fn"
            | "Fx"
            | "Ft"
            | "Ic"
            | "In"
            | "Lk"
            | "Li"
            | "Ms"
            | "Mt"
            | "Nm"
            | "No"
            | "Ns"
            | "Ot"
            | "Nx"
            | "Ox"
            | "Pa"
            | "Pf"
            | "St"
            | "Sx"
            | "Sy"
            | "Tn"
            | "Ux"
            | "Va"
            | "Xr"
    )
}

/// Callable partial implicit blocks share the same token grammar as in-line
/// macros, but their final public shape is Block/Head/Body.  Keep the set
/// separate from the pure inline family so the source-order dispatcher can
/// build the block after its enclosing macro has yielded it as a sibling.
pub(super) fn is_implicit_partial_block_macro(name: &str) -> bool {
    matches!(
        name,
        "Aq" | "Bq" | "Brq" | "Dq" | "Op" | "Pq" | "Ql" | "Qq" | "Sq"
    )
}

/// Return the static public spelling for an implicit partial block macro.
/// The source-order dispatcher holds a borrowed spelling, while recoveries
/// deliberately store only vocabulary from this fixed grammar.
pub(super) fn implicit_partial_block_name(name: &str) -> &'static str {
    match name {
        "Aq" => "Aq",
        "Bq" => "Bq",
        "Brq" => "Brq",
        "Dq" => "Dq",
        "Op" => "Op",
        "Pq" => "Pq",
        "Ql" => "Ql",
        "Qq" => "Qq",
        "Sq" => "Sq",
        _ => unreachable!("caller checked the implicit partial block grammar"),
    }
}

pub(crate) fn is_mdoc_callable_macro(name: &str) -> bool {
    is_inline_mdoc_macro(name)
        || is_implicit_partial_block_macro(name)
        || explicit_partial_block_close(name).is_some()
        // A function closer is callable from another parsed macro's argument
        // list (for example `.Nm name Fc tail`), where it must reach the
        // source-order scope machine instead of remaining literal text.
        || matches!(name, "Ec" | "Eo" | "Fc")
}

/// Known mdoc spellings that `lookup()` recognizes but does not allow as a
/// nested invocation from an `MDOC_PARSED` in-line macro. Keep this separate
/// from `is_mdoc_callable_macro`: the latter intentionally contains only the
/// macro families currently reclassified by the native inline parser.
pub(super) fn is_mdoc_noncallable_macro(name: &str) -> bool {
    matches!(
        name,
        "Dd" | "Dt"
            | "Os"
            | "Sh"
            | "Ss"
            | "Pp"
            | "D1"
            | "Dl"
            | "Bd"
            | "Ed"
            | "Bl"
            | "El"
            | "It"
            | "Ex"
            | "Fd"
            | "Nd"
            | "Rv"
            | "%A"
            | "%B"
            | "%D"
            | "%I"
            | "%J"
            | "%N"
            | "%O"
            | "%P"
            | "%R"
            | "%T"
            | "%V"
            | "Bf"
            | "Db"
            | "Ef"
            | "Re"
            | "Rs"
            | "Sm"
            | "Bk"
            | "Ek"
            | "Bt"
            | "Hf"
            | "Ud"
            | "Lb"
            | "Lp"
            | "%C"
            | "%Q"
            | "%U"
            | "Tg"
    )
}

/// `None` is variadic.  Finite counts consume their authored tokens before
/// callable-macro classification, preserving macro-specific argument rules.
pub(super) fn mdoc_inline_argument_limit(name: &str) -> Option<usize> {
    match name {
        "Ap" | "Ns" | "Ux" => Some(0),
        // `in_line_argn()` owns a fixed prefix before ordinary source flow
        // resumes.  Pf shares its one-argument shape but adds separate
        // validation; Xr is the only currently callable two-argument form.
        "Bsx" | "Dx" | "Fx" | "In" | "Nx" | "Ox" | "Pf" | "St" => Some(1),
        "Bx" | "Xr" => Some(2),
        _ => None,
    }
}

/// Explicit partial-block openers and their matching closers.  The initial
/// implementation handles a closer present on the same physical line; the
/// stored pair is deliberately centralized so cross-line scope handling can
/// reuse this taxonomy without widening the public AST contract.
pub(super) fn explicit_partial_block_close(name: &str) -> Option<&'static str> {
    match name {
        "Ao" => Some("Ac"),
        "Bo" => Some("Bc"),
        "Bro" => Some("Brc"),
        "Do" => Some("Dc"),
        "Eo" => Some("Ec"),
        "Oo" => Some("Oc"),
        "Po" => Some("Pc"),
        "Qo" => Some("Qc"),
        "So" => Some("Sc"),
        "Xo" => Some("Xc"),
        _ => None,
    }
}

pub(super) fn is_mdoc_closing_delimiter(value: &str) -> bool {
    matches!(value, "," | "." | ";" | ":" | "!" | "?" | ")" | "]")
}

/// `post_ns()` warns when a no-space request is the first semantic event of
/// its physical request or is immediately followed by closing punctuation.
/// Other positions either join the neighboring words or are inert after a
/// closer, so they retain the same public empty Element without a finding.
pub(super) fn no_space_macro_requires_warning(
    builder: &DocumentBuilder,
    node: NodeId,
    following: &[NodeId],
) -> bool {
    builder
        .node_flags(node)
        .is_some_and(|flags| flags.line_start)
        || following
            .first()
            .and_then(|next| builder.node_text(*next))
            .is_some_and(is_mdoc_closing_delimiter)
}

/// The only mdoc middle delimiter closes the current `in_line()` element but
/// lets the following ordinary word reopen the same macro.  This is distinct
/// from closing punctuation, after which subsequent words resume surrounding
/// source flow。`mandoc` 同时识别包住该分隔符的常见字体复位拼写，并将其保留为可见文本节点。
pub(super) fn is_mdoc_middle_delimiter(value: &str) -> bool {
    matches!(value, "|" | r"\fR|\fP")
}

/// Allocate the next element after mdoc's middle-delimiter scope rewind.  It
/// inherits the request's source position, but cannot claim a physical line
/// start because the separator occurred in the same parsed argument list.
pub(super) fn reopen_inline_element(
    builder: &mut DocumentBuilder,
    allocation_parent: NodeId,
    source: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) -> Option<NodeId> {
    if builder.node_count() >= max_nodes {
        if outcome.node_limit_location.is_none() {
            outcome.node_limit_location = builder.node_location(source);
        }
        return None;
    }
    let macro_name = builder.node_macro_name(source)?.to_owned();
    let location = builder.node_location(source);
    let mut flags = builder.node_flags(source).unwrap_or_default();
    flags.line_start = false;
    // `push` needs a concrete private parent, but the caller is about to
    // replace that parent's children with its source-order event list.  Keep
    // a snapshot so this provisional allocation cannot become an accidental
    // child of the preceding element (notably the empty `Fl` before `|`).
    let previous_children = builder
        .children(allocation_parent)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let reopened = builder.push(allocation_parent, NodeKind::Element)?;
    let configured = builder.macro_name(reopened, macro_name)
        && builder.set_node_location(reopened, location)
        && builder.set_node_flags(reopened, flags);
    let _ = builder.replace_children(allocation_parent, &previous_children);
    if !configured {
        return None;
    }
    Some(reopened)
}

pub(super) fn mark_opening_delimiter(
    builder: &mut DocumentBuilder,
    node: NodeId,
    text: Option<&str>,
) {
    if !text.is_some_and(|value| matches!(value, "(" | "[")) {
        return;
    }
    if let Some(mut flags) = builder.node_flags(node) {
        flags.delimiter_open = true;
        let _ = builder.set_node_flags(node, flags);
    }
}

/// Move the source request's first-event provenance onto a delimiter that
/// became the public leading event after the private macro element vanished.
pub(super) fn transfer_line_start(builder: &mut DocumentBuilder, source: NodeId, target: NodeId) {
    let line_start = builder
        .node_flags(source)
        .is_some_and(|flags| flags.line_start);
    if let Some(mut flags) = builder.node_flags(target) {
        flags.line_start = line_start;
        let _ = builder.set_node_flags(target, flags);
    }
}

/// Return whether an empty tag-style macro survives its source request as a
/// warning.
///
/// A leading delimiter may split a request into a discarded empty first
/// element and a later populated element of the same macro. Any other
/// follower (including another callable macro) leaves the first element
/// genuinely argument-less and therefore warned.
pub(super) fn tag_empty_macro_requires_warning(
    builder: &DocumentBuilder,
    macro_name: &str,
    following: &[NodeId],
) -> bool {
    let delimiter_count = following
        .iter()
        .take_while(|node| {
            builder.node_text(**node).is_some_and(|text| {
                is_mdoc_closing_delimiter(text)
                    || is_mdoc_middle_delimiter(text)
                    || matches!(text, "(" | "[")
            })
        })
        .count();
    delimiter_count == 0
        || !following.get(delimiter_count).is_some_and(|successor| {
            builder.node_macro_name(*successor) == Some(macro_name)
                && builder
                    .children(*successor)
                    .is_some_and(|children| !children.is_empty())
        })
}

pub(super) fn finish_inline_element(
    builder: &mut DocumentBuilder,
    node: NodeId,
    children: &[NodeId],
    spacing_enabled: bool,
) {
    let _ = builder.replace_children(node, children);
    if spacing_enabled && matches!(builder.node_macro_name(node), Some("Em" | "Sy")) {
        // Em 与 Sy use MDOC_JOIN for ordinary word sequences, but libmandoc
        // disables that join while `.Sm off` is in effect. Real inline
        // boundaries have already been split before this point.
        coalesce_text_children(builder, node);
    } else if spacing_enabled
        && builder.node_macro_name(node) == Some("No")
        && builder
            .node_flags(node)
            .is_none_or(|flags| !flags.synopsis_pretty)
    {
        coalesce_text_children(builder, node);
    }
}

/// Parsed macro arguments can contain a second implicit partial block.  The
/// scanner first classifies that inner macro as an element; mdoc then applies
/// the same Block/Head/Body construction recursively without making it a
/// source-line sibling (for example `.Op one Op two`).
pub(super) fn structure_nested_implicit_partial_blocks(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for node in children {
        structure_implicit_partial_block(builder, node, max_nodes, outcome, spacing_enabled);
    }
}

/// Discover explicit partial scopes nested below already-structured implicit
/// blocks.  Such an opener is not a top-level source event, but its physical
/// closer still participates in the main scope stack (for example
/// `.Aq … Bq … Po` followed by `.Pc`).
pub(super) fn structure_nested_implicit_explicit_scopes(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
) -> Vec<ScopeFrame> {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return Vec::new();
    };
    let mut scopes = Vec::new();
    for child in children {
        if builder.node_kind(child) != Some(NodeKind::Block)
            || !builder
                .node_macro_name(child)
                .is_some_and(is_implicit_partial_block_macro)
        {
            continue;
        }
        let Some(body) = builder.children(child).and_then(|parts| {
            parts.iter().copied().find(|part| {
                builder.node_kind(*part) == Some(NodeKind::Body)
                    && builder.node_macro_name(*part) == builder.node_macro_name(child)
            })
        }) else {
            continue;
        };
        scopes.extend(structure_nested_implicit_explicit_scopes(
            builder,
            body,
            max_nodes,
            outcome,
            spacing_enabled,
        ));
        scopes.extend(structure_unclosed_explicit_partial_blocks(
            builder,
            body,
            spacing_enabled,
            max_nodes,
            outcome,
        ));
    }
    scopes
}

/// Apply the implicit-partial Block projection to one already-classified
/// callable macro.  Explicit partial openers can yield a same-line tail that
/// never re-enters the top-level source-order dispatcher; keeping this helper
/// separate lets those tail events receive the same projection as ordinary
/// children.
pub(super) fn structure_implicit_partial_block(
    builder: &mut DocumentBuilder,
    node: NodeId,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
    spacing_enabled: bool,
) {
    let Some(name) = builder.node_macro_name(node).map(str::to_owned) else {
        return;
    };
    if !is_implicit_partial_block_macro(&name) {
        return;
    }
    let inherits_synopsis = builder
        .node_flags(node)
        .is_some_and(|flags| flags.synopsis_pretty);
    let Some((head, body)) = make_block(
        builder,
        node,
        &name,
        ArgumentPlacement::BodyTokens,
        max_nodes,
        outcome,
    ) else {
        return;
    };
    if inherits_synopsis {
        mark_synopsis_pretty(builder, head);
        mark_synopsis_pretty(builder, body);
    }
    let nested = split_mdoc_inline_children(builder, body, spacing_enabled, max_nodes, outcome);
    let mut nested = expand_fl_elements(builder, body, nested, max_nodes, outcome);
    insert_generated_system_names(builder, &nested, max_nodes, outcome);
    let tail = take_implicit_partial_tail(builder, &mut nested);
    let _ = builder.replace_children(body, &nested);
    move_leading_open_delimiters(builder, node, head, body);
    clear_initial_implicit_body_delimiter_flags(builder, body);
    clear_terminal_implicit_body_opening_flags(builder, body);
    mark_implicit_partial_tail_sentence_ends(builder, &tail);
    if spacing_enabled && name != "Op" {
        coalesce_implicit_partial_body_text(builder, body);
    }
    structure_nested_implicit_partial_blocks(builder, body, max_nodes, outcome, spacing_enabled);
    if !tail.is_empty() {
        let mut block_children = vec![head, body];
        block_children.extend(tail);
        let _ = builder.replace_children(node, &block_children);
    }
}

/// A trailing unescaped closing delimiter is not body prose for an implicit
/// mdoc partial block. `blk_part_imp()` publishes it after the Body, where it
/// carries the terminal sentence state. The inline splitter has already
/// classified only real delimiter tokens, so escaped spellings such as `\\&.`
/// remain ordinary body text.
pub(super) fn take_implicit_partial_tail(
    builder: &DocumentBuilder,
    children: &mut Vec<NodeId>,
) -> Vec<NodeId> {
    let is_tail = |node: &NodeId| {
        builder
            .node_text(*node)
            .is_some_and(is_mdoc_closing_delimiter)
            && builder
                .node_flags(*node)
                .is_some_and(|flags| flags.delimiter_close)
    };
    let split = children
        .iter()
        .rposition(|node| !is_tail(node))
        .map_or(0, |index| index + 1);
    if split < children.len() {
        children.split_off(split)
    } else {
        Vec::new()
    }
}

/// `blk_part_imp()` keeps a leading opening delimiter between its empty Head
/// and Body instead of treating it as the first body word.  That placement is
/// observable in the owned AST and applies to constructs such as
/// `.Dq "(" user@host)`.
pub(super) fn move_leading_open_delimiter(
    builder: &mut DocumentBuilder,
    block: NodeId,
    head: NodeId,
    body: NodeId,
) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some((&first, rest)) = children.split_first() else {
        return;
    };
    if !builder
        .node_text(first)
        .is_some_and(|value| matches!(value, "(" | "["))
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.delimiter_open = true;
        let _ = builder.set_node_flags(first, flags);
    }
    let _ = builder.replace_children(body, rest);
    let _ = builder.replace_children(block, &[head, first, body]);
}

/// `blk_part_imp()` publishes every leading opening delimiter between the
/// empty Head and Body.  The single-delimiter form is common, but an input
/// such as `.Op ( (` exposes both authored delimiters as block children.
pub(super) fn move_leading_open_delimiters(
    builder: &mut DocumentBuilder,
    block: NodeId,
    head: NodeId,
    body: NodeId,
) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    let leading_count = children
        .iter()
        .take_while(|node| {
            builder
                .node_text(**node)
                .is_some_and(|value| matches!(value, "(" | "["))
        })
        .count();
    if leading_count == 0 {
        return;
    }
    let (leading, rest) = children.split_at(leading_count);
    for delimiter in leading {
        if let Some(mut flags) = builder.node_flags(*delimiter) {
            flags.delimiter_open = true;
            let _ = builder.set_node_flags(*delimiter, flags);
        }
    }
    let _ = builder.replace_children(body, rest);
    let mut block_children = Vec::with_capacity(leading.len().saturating_add(2));
    block_children.push(head);
    block_children.extend_from_slice(leading);
    block_children.push(body);
    let _ = builder.replace_children(block, &block_children);
}

/// A leading closing delimiter is literal body content when more source words
/// follow it (`.Op . z`).  The inline tokenizer initially marks every closing
/// delimiter; remove that provisional classification only for this body-first
/// case, after terminal tails have already been selected.
pub(super) fn clear_initial_implicit_body_delimiter_flags(
    builder: &mut DocumentBuilder,
    body: NodeId,
) {
    let Some(&first) = builder.children(body).and_then(|children| children.first()) else {
        return;
    };
    if !builder
        .node_text(first)
        .is_some_and(is_mdoc_closing_delimiter)
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.delimiter_close = false;
        flags.sentence_end = false;
        let _ = builder.set_node_flags(first, flags);
    }
}

/// A trailing opening delimiter remains literal body content.  It only gains
/// its delimiter-open flag when later body flow makes it an in-line opener
/// (`.Op a ( z` versus `.Op a (`).
pub(super) fn clear_terminal_implicit_body_opening_flags(
    builder: &mut DocumentBuilder,
    body: NodeId,
) {
    let Some(&last) = builder.children(body).and_then(|children| children.last()) else {
        return;
    };
    if !builder
        .node_text(last)
        .is_some_and(|text| matches!(text, "(" | "["))
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(last) {
        flags.delimiter_open = false;
        let _ = builder.set_node_flags(last, flags);
    }
}

/// Consecutive terminal delimiters are all public tail nodes.  Re-evaluate
/// each node after splitting so `.Op . .` preserves sentence state on both
/// periods rather than only on the final one.
pub(super) fn mark_implicit_partial_tail_sentence_ends(
    builder: &mut DocumentBuilder,
    tail: &[NodeId],
) {
    for delimiter in tail {
        mark_sentence_end(builder, *delimiter);
    }
}

/// `blk_part_exp()` places a leading opening delimiter before the generated
/// Head, unlike `blk_part_imp()` which keeps it after the Head.
pub(super) fn move_explicit_leading_open_delimiter(
    builder: &mut DocumentBuilder,
    block: NodeId,
    head: NodeId,
    body: NodeId,
) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some((&first, rest)) = children.split_first() else {
        return;
    };
    if !builder
        .node_text(first)
        .is_some_and(|value| matches!(value, "(" | "["))
    {
        return;
    }
    if let Some(mut flags) = builder.node_flags(first) {
        flags.delimiter_open = true;
        let _ = builder.set_node_flags(first, flags);
    }
    let _ = builder.replace_children(body, rest);
    let _ = builder.replace_children(block, &[first, head, body]);
}

/// Punctuation in an explicit partial block remains literal while later body
/// content follows it (`.Oo . word` and `.Oo word . next`).  The shared inline
/// splitter cannot see that body-level continuation, so clear its provisional
/// punctuation flags after the Body is selected.
pub(super) fn clear_leading_explicit_partial_punctuation(
    builder: &mut DocumentBuilder,
    body: NodeId,
) {
    let Some(children) = builder.children(body).map(<[NodeId]>::to_vec) else {
        return;
    };
    for (index, node) in children.iter().copied().enumerate() {
        if index > 0
            && children[index + 1..].is_empty()
            && builder
                .node_text(node)
                .is_some_and(|text| matches!(text, "(" | "["))
            && let Some(mut flags) = builder.node_flags(node)
        {
            flags.delimiter_open = false;
            let _ = builder.set_node_flags(node, flags);
        }
        if !builder
            .node_text(node)
            .is_some_and(is_mdoc_closing_delimiter)
            || children[index + 1..].is_empty()
        {
            continue;
        }
        if let Some(mut flags) = builder.node_flags(node) {
            if index == 0 {
                flags.delimiter_close = false;
            }
            flags.sentence_end = false;
            let _ = builder.set_node_flags(node, flags);
        }
    }
}

/// Reproduce `tag_move_href()` for the validated `.Tg` followed by `.Pp`
/// path.  The paragraph owns the destination and its immediately following
/// text owns the display permalink.  mandoc splits that text only when its
/// historical five-byte scan stops on a separating space.
pub(super) fn move_paragraph_permalink(
    builder: &mut DocumentBuilder,
    text_node: NodeId,
    parent: NodeId,
    tag: &str,
    max_nodes: usize,
    outcome: &mut StructureOutcome,
) {
    let Some(text) = builder.node_text(text_node).map(str::to_owned) else {
        return;
    };
    if text.is_empty() || text.starts_with(' ') {
        return;
    }

    let split = paragraph_permalink_split(&text);
    if let Some(split) = split
        && builder.node_count() < max_nodes
    {
        let tail = text[split + 1..].to_owned();
        let Some(mut flags) = builder.node_flags(text_node) else {
            return;
        };
        let location = builder.node_location(text_node);
        if !builder.text(text_node, text[..split].to_owned()) {
            return;
        }
        let Some(tail_node) = builder.push(parent, NodeKind::Text) else {
            return;
        };
        flags.line_start = false;
        let _ = builder.text(tail_node, tail);
        let _ = builder.set_node_flags(tail_node, flags);
        if let Some(mut location) = location {
            // mandoc assigns the synthetic word `n->pos + (cp - n->string)`,
            // i.e. the delimiter's column rather than the first byte after
            // it.  Preserve that observable legacy location exactly.
            let split = u32::try_from(split).unwrap_or(u32::MAX);
            location.start = location.start.saturating_add(split);
            let _ = builder.set_node_location(tail_node, Some(location));
        }
        let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
            return;
        };
        let Some(position) = children.iter().position(|child| *child == text_node) else {
            return;
        };
        let mut reordered = children;
        let Some(created_position) = reordered.iter().position(|child| *child == tail_node) else {
            return;
        };
        reordered.remove(created_position);
        reordered.insert(position.saturating_add(1), tail_node);
        let _ = builder.replace_children(parent, &reordered);
    } else if split.is_some() && outcome.node_limit_location.is_none() {
        outcome.node_limit_location = builder.node_location(text_node);
    }

    let Some(mut flags) = builder.node_flags(text_node) else {
        return;
    };
    flags.permalink = true;
    let _ = builder.set_node_flags(text_node, flags);
    let _ = builder.set_node_tag(text_node, tag);
}

pub(super) fn paragraph_permalink_split(text: &str) -> Option<usize> {
    let mut search_from = 0;
    let mut space = text[search_from..]
        .find(' ')
        .map(|offset| search_from.saturating_add(offset));
    while space.is_some_and(|offset| offset < 5) {
        search_from = space.expect("space was checked").saturating_add(1);
        space = text[search_from..]
            .find(' ')
            .map(|offset| search_from.saturating_add(offset));
    }
    space.filter(|offset| {
        text.as_bytes()
            .get(offset.saturating_add(1))
            .is_some_and(|byte| *byte != b'\0')
    })
}
