use super::context::MdocContext;
use super::post::{NetBsdValidation, PostValidation, PrologueStatus, merge_syntax_recoveries};
use super::state::{StructureEvent, StructureEvents};
use super::{
    ArgumentPlacement, AutomaticFunctionTarget, BTreeMap, BTreeSet, DocumentBuilder, MacroSet,
    NodeId, NodeKind, NormalizedEnclosure, NormalizedListKind, Recovery, ScopeFrame, SourceSpan,
    StructureOutcome, accepts_pending_manual_tag, active_column_item, active_column_list,
    append_broken_full_block_body, append_broken_implicit_block_body, append_column_ta_cell,
    append_explicit_partial_tail, append_implicit_column_table_row, append_to_parent,
    apply_attributes, argument_location, automatic_mdoc_function_tag, broken_item_recoveries,
    clear_generated_synopsis_pretty_children, clear_initial_implicit_body_delimiter_flags,
    clear_leading_explicit_partial_punctuation, clear_quoted_bx_trailing_delimiter_sentence_end,
    clear_terminal_implicit_body_opening_flags, close_explicit_partial_scope, close_name,
    coalesce_adjacent_text_children, coalesce_implicit_partial_body_text,
    coalesce_mdoc_display_phrases, coalesce_text_children, coalesce_text_children_after,
    collapse_long_option_prefixes, column_item_cell_count, complete_explicit_tail,
    discard_empty_block, discard_item_body, discard_node_from_parent,
    discard_previous_paragraph_control, display_attributes, empty_tag_macro_name,
    expand_fl_elements, expand_standard_exit_status, expand_standard_return_value,
    explicit_partial_block_close, explicit_partial_tail_events, extend_pending_short_column_item,
    finalize_last_empty_column_item, finalize_last_fixed_head_list_item,
    finalize_short_column_items, fixed_head_list_type, flush_pending_authors_section,
    flush_pending_name_section, flush_pending_nd_delimiters, font_attributes,
    generated_system_name, implicit_partial_ancestor_blocks, implicit_partial_block_name,
    insert_generated_ar_default, insert_generated_nm_name, insert_generated_nonbreaking_default,
    insert_generated_system_name, insert_generated_system_names, is_bf_option,
    is_explicit_partial_close, is_explicit_partial_scope, is_implicit_column_row_macro,
    is_implicit_partial_block_macro, is_inline_mdoc_macro, is_legacy_roff_font,
    is_mdoc_closing_delimiter, is_mdoc_noncallable_macro, is_reference_field_macro,
    item_header_partial_scope, list_attributes, make_block, make_synthetic_block,
    mark_definition_item_head_targets, mark_destination,
    mark_explicit_partial_close_tail_line_start, mark_first_visible_permalink,
    mark_implicit_partial_tail_sentence_ends, mark_link_terminal_delimiter, mark_manual_target,
    mark_no_print, mark_permalink, mark_synopsis_pretty, mark_target,
    matching_explicit_partial_close_index, mdoc_heading_tab_recoveries, mdoc_inline_argument_limit,
    mdoc_operating_system_flavour, move_explicit_leading_open_delimiter,
    move_initial_list_content_out, move_leading_open_delimiter, move_leading_open_delimiters,
    move_paragraph_permalink, named_mdoc_section, no_space_macro_requires_warning, node_arguments,
    normalize_reference_field_order, open_name, preceding_manual_tag_paragraph,
    push_generated_text_at, record_date, record_name, record_operating_system, record_title,
    recover_unmatched_ec, reference_field_joins_arguments,
    relocate_crossed_closer_to_nested_implicit_body, split_column_item_cells,
    split_explicit_partial_block_tail, split_inline_macro_events, split_mdoc_inline_children,
    split_mdoc_inline_tokens, standard_description, structure_implicit_column_item,
    structure_implicit_column_table_item, structure_item_head_explicit_partial,
    structure_matched_explicit_partial_blocks, structure_nested_implicit_explicit_scopes,
    structure_nested_implicit_partial_blocks, structure_unclosed_explicit_partial_blocks,
    system_macro_name, tag_empty_macro_requires_warning, tag_macro_name,
    take_explicit_partial_close_argument, take_implicit_partial_tail, take_inline_column_ta_tail,
    take_trailing_line_start_text_children, text_offset_location, unexpected_section_manuals,
    validate_an, validate_at, validate_function_argument_commas, validate_function_name,
    validate_library, validate_no_break_trailing_delimiter, validate_prefix_following,
    validate_tag, visible_head_text,
};

/// Restructure the initial M5 mdoc macro families in a bounded arena.
#[allow(clippy::too_many_lines)] // One source-order state machine keeps scope ownership auditable.
pub(crate) fn structure(
    builder: &mut DocumentBuilder,
    max_nodes: usize,
    mut saw_operating_system_request: bool,
) -> StructureOutcome {
    let mut package = MdocContext::default();
    if builder.macro_set() != MacroSet::Mdoc {
        return package.outcome;
    }
    let root = DocumentBuilder::root();
    let Some(flat) = builder.children(root).map(<[NodeId]>::to_vec) else {
        return package.outcome;
    };
    let mut machine = StructureEvents::prepare(builder, flat);
    let outcome = &mut package.outcome;
    let deferred = &mut package.deferred;

    let mut root_children = Vec::new();
    let mut section_parent = root;
    let mut has_section = false;
    // `post_nd()` validates both the completed description phrase and its
    // section after the block has received following physical text.  Retain
    // the current named section separately from the structural Body node so
    // that an `.Nd` can be checked when a later block boundary closes it.
    let mut in_name_section = false;
    let mut pending_nd_delimiter_bodies = Vec::new();
    // `post_sh_name()` validates only direct children of a completed NAME
    // section.  Retain its Body until the next `.Sh` (or EOF) so nested
    // `.Nm`/`.Nd` entries remain intentionally insufficient.
    let mut pending_name_section_body = None;
    let mut pending_authors_body = None;
    // Only named conventional sections participate in the mdoc ordering
    // convention. Custom section titles leave this cursor untouched.
    let mut last_named_section = None::<u8>;
    let mut flow_parent = root;
    let mut active_body = root;
    let mut scopes = Vec::<ScopeFrame>::new();
    // `Bl -column` consumes a variable number of declaration phrases before
    // the next option.  The public normalized list kind intentionally omits
    // that parser-only count, so retain it by the list Body while items are
    // structured below.
    let mut column_counts = BTreeMap::<NodeId, usize>::new();
    // Normalized definition lists coalesce several authored selectors.  Keep
    // this private discriminator for validators with `-diag`-only behavior.
    let mut list_types = BTreeMap::<NodeId, &'static str>::new();
    // A syntactically empty `.It` requires delayed validation because the
    // following physical input line may become its first column body.
    let mut pending_empty_column_items = BTreeSet::<NodeId>::new();
    // A short column row may acquire further cells from a later physical
    // `.Ta` request, so defer its count validation until the next structural
    // boundary rather than diagnosing the provisional prefix immediately.
    let mut pending_short_column_items = BTreeMap::<NodeId, (usize, usize)>::new();
    // A marker-list item beginning with an invalid `Ta` already queues its
    // ignored-argument recovery for the post-validation phase.  Its normal
    // item-boundary validation must not report the same Head a second time.
    let mut deferred_fixed_head_argument_items = BTreeSet::<NodeId>::new();
    let mut target_heads = Vec::new();
    let mut synopsis_bodies = Vec::new();
    // `post_fname()` gives each automatic function tag a source-order
    // priority.  Pp resets it to `TAG_STRONG`; the final tag pass resolves
    // equal and competing names after all destinations are known.
    let mut automatic_function_targets = Vec::<AutomaticFunctionTarget>::new();
    let mut function_tag_priority = 2_u32;
    // mandoc validates Pp after it has completed the containing body, while
    // roff layout requests such as br and sp are validated immediately.
    // Keep the two queues separate to preserve observable finding order.
    // Delimiter spacing is validated after the syntax pass. Keep those
    // findings distinct so a later source-level recovery is reported first.
    // A list closer that crosses a partial scope opened from an item header
    // leaves the list and partial block unclosed.  mandoc reports the
    // resulting item validation only after those EOF closers, so retain these
    // narrow recovery events separately from ordinary source-order findings.
    // List-content relocation is a post-validation action: all item-break
    // errors are observable before the warnings for material moved out of its
    // enclosing list.
    // A callable explicit closer encountered while an implicit partial block
    // is parsed is a syntax-stage finding in libmandoc.  Emit it before the
    // later section/list post-validation findings, despite the implicit
    // block's public AST node being assembled only after those tokens.
    // A validated `.Tg` registers an explicit manual tag for the immediately
    // following mdoc node.  The general tag priority table comes later; this
    // state only covers the source-order relationship needed to preserve the
    // public tree for paragraph anchors.
    let mut pending_manual_tag = None::<(NodeId, String)>;
    // An empty `Fl` can make a following Tg transparent: the preceding
    // paragraph owns the destination while the next inline macro receives
    // only the matching permalink.
    let mut pending_transparent_permalink = None::<String>;
    let mut pending_paragraph_href = None::<String>;
    // Function names can transfer their target to the immediately eligible
    // paragraph-layout event.  This is independent of automatic duplicate
    // resolution, which is handled by the deferred global tag pass.
    let mut pending_function_paragraph = None::<NodeId>;
    let mut enclosure = None::<NormalizedEnclosure>;
    let mut implicitly_closed = Vec::<&'static str>::new();
    let mut in_synopsis = false;
    // `Sh SYNOPSIS` and the private roff `nS` register both enter the same
    // structural context, but libmandoc's generated `.Nm` fallback keeps a
    // distinct presentation bit for the latter execution-driven form.
    let mut synopsis_from_register = false;
    let mut synopsis_name_body = None::<NodeId>;
    // `Bk ... Ek` ends a SYNOPSIS name flow before a following paragraph,
    // whereas an ordinary in-flow Pp remains inside that name block.
    let mut synopsis_keep_boundary = false;
    // `.Sm` changes how the mdoc validator groups otherwise adjacent source
    // words in later partial blocks.  It is stateful: a bare request toggles
    // the current setting, rather than resetting it to the default.
    let mut spacing_enabled = true;
    let mut preserve_leading_comments = true;
    // libmandoc retains the last authored prologue metadata but reports each
    // repeated request, even when the later request appears after the body.
    let mut saw_date_prologue = false;
    let mut saw_title_prologue = false;
    let mut saw_operating_system_prologue = false;
    let mut first_date_prologue = None::<(Box<str>, Option<SourceSpan>)>;
    let mut operating_system_flavour = None::<&'static str>;
    let mut netbsd_operating_system_validation = false;
    let mut saw_netbsd_rcs_id = false;
    let mut saw_openbsd_rcs_id = false;

    while let Some(event) = machine.step() {
        let StructureEvent {
            flat_index,
            node,
            suppressed,
            blank_line_recovery,
        } = event;
        while let Some(state) = machine.next_synopsis_transition(flat_index) {
            if in_synopsis == state {
                continue;
            }
            // A disabled `nS` finishes a surrounding synopsis-name flow, but
            // must not tear down an explicit partial enclosure that remains
            // open across the state request.  Its later closer still owns
            // the resumed non-synopsis text.
            if !state && scopes.is_empty() {
                active_body = section_parent;
                flow_parent = section_parent;
            }
            synopsis_name_body = None;
            synopsis_keep_boundary = false;
            in_synopsis = state;
            synopsis_from_register = state;
        }
        if suppressed {
            continue;
        }
        if let Some(recovery) = blank_line_recovery {
            outcome.recoveries.push(recovery);
        }
        if builder.node_kind(node) == Some(NodeKind::Comment) {
            // Like man(7), mdoc retains only the source preamble comments in
            // the public syntax tree.  Comments encountered after the first
            // parsed document event are validator input, not rendered or
            // consumer-visible content.
            if preserve_leading_comments {
                root_children.push(node);
                if builder
                    .node_text(node)
                    .is_some_and(|text| text.contains("$NetBSD:"))
                {
                    saw_netbsd_rcs_id = true;
                }
                if builder
                    .node_text(node)
                    .is_some_and(|text| text.contains("$OpenBSD:"))
                {
                    saw_openbsd_rcs_id = true;
                }
            }
            continue;
        }
        preserve_leading_comments = false;
        // A tbl range inside `Bl -column` has no mdoc control line that can
        // introduce its row.  mandoc therefore materializes one implicit,
        // empty-headed `It` and keeps consecutive table rows in that body's
        // source order.  Do this before generic dispatch: a Table has no
        // macro name and would otherwise remain a list-body sibling.
        if active_column_list(builder, active_body)
            && builder.node_kind(node) == Some(NodeKind::Table)
            && (append_implicit_column_table_row(builder, active_body, node)
                || structure_implicit_column_table_item(
                    builder,
                    active_body,
                    node,
                    max_nodes,
                    outcome,
                ))
        {
            continue;
        }
        // Bk -words retains its parsed inline words as separately owned AST
        // children.  This is presentation-independent semantic grouping, so
        // derive it from the active explicit scope rather than mutating the
        // document-wide `.Sm` state.
        let inline_spacing_enabled =
            spacing_enabled && !scopes.iter().any(|frame| frame.close == "Ek");
        // Vt has a distinct partial-block form in SYNOPSIS.  Delay its
        // inline splitting until that context has selected its final parent.
        if in_synopsis {
            // Preserve SYNOPSIS state before splitting: No's join rule needs
            // the package context while it still owns raw scanner words.
            mark_synopsis_pretty(builder, node);
        }
        let mut inline_column_ta_tail = take_inline_column_ta_tail(builder, node, active_body);
        let inline_events = if builder.node_macro_name(node) == Some("Vt") {
            vec![node]
        } else {
            split_inline_macro_events(builder, node, inline_spacing_enabled, max_nodes, outcome)
        };
        // A SYNOPSIS Nm is a full block, but inline events split from the
        // same physical request line remain part of its Head.  When no
        // partial scope takes over, restore ordinary flow to the Body after
        // the complete line has been consumed.
        let mut synopsis_name_inline_restore = None::<(NodeId, NodeId)>;
        // Some fixed-argument macros delete a private punctuation event
        // together with their empty source element. The scanner has already
        // split that punctuation into this line's event stream, so defer its
        // suppression until the ordinary source-order loop reaches it.
        let mut suppressed_inline_events = BTreeSet::new();
        for (event_index, node) in inline_events.iter().copied().enumerate() {
            if suppressed_inline_events.contains(&node) {
                continue;
            }
            let macro_name = builder.node_macro_name(node).map(str::to_owned);
            if macro_name.as_deref() == Some("ft") {
                let children = builder.children(node).unwrap_or_default().to_vec();
                if let Some(font) = children.first().and_then(|child| builder.node_text(*child)) {
                    if !is_legacy_roff_font(font.as_bytes()) {
                        outcome.recoveries.push(Recovery::UnknownRoffFont {
                            font: font.into(),
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    // roff_valid_ft() retains only the first selector.  The
                    // scanner has already reported any surplus source word.
                    let _ = builder.replace_children(node, &children[..1]);
                } else if builder.node_count() < max_nodes {
                    if let Some(default_font) = builder.push(node, NodeKind::Text) {
                        let _ = builder.text(default_font, "P");
                        let _ =
                            builder.set_node_location(default_font, builder.node_location(node));
                    }
                } else if outcome.node_limit_location.is_none() {
                    outcome.node_limit_location = builder.node_location(node);
                }
                append_to_parent(builder, root, &mut root_children, active_body, node);
                continue;
            }
            if scopes.last().is_some_and(|scope| scope.close == "Re")
                // libmandoc's `post_rs()` begins with the second direct
                // child: its first child is retained without a content
                // warning, even when it is a transparent `Tg` node.
                && !builder
                    .children(active_body)
                    .is_some_and(<[NodeId]>::is_empty)
            {
                let reference_content = match macro_name.as_deref() {
                    Some(name) if is_reference_field_macro(name) || name == "Re" => None,
                    Some(name) => Some(name.into()),
                    None if builder.node_kind(node) == Some(NodeKind::Text) => Some("text".into()),
                    _ => None,
                };
                if let Some(content) = reference_content {
                    outcome.recoveries.push(Recovery::ReferenceContent {
                        content,
                        location: builder.node_location(node),
                    });
                }
            }
            if macro_name.as_deref() == Some("Ns")
                && no_space_macro_requires_warning(builder, node, &inline_events[event_index + 1..])
            {
                outcome.recoveries.push(Recovery::NoSpaceMacro {
                    location: builder.node_location(node),
                });
            }
            if macro_name.as_deref().is_some_and(|name| {
                is_inline_mdoc_macro(name) && mdoc_inline_argument_limit(name).is_none()
            }) {
                // `lookup()` diagnoses a known macro spelling when the
                // enclosing in-line macro has MDOC_PARSED but the nested
                // macro lacks MDOC_CALLABLE.  Fixed-argument macros consume
                // their prefix literally, so only the unbounded family gets
                // this lookup pass. Escaped spellings retain their `\\&`
                // projection in the package AST and intentionally do not
                // compare equal to a macro name here.
                for child in builder.children(node).unwrap_or_default() {
                    let Some(name) = builder.node_text(*child) else {
                        continue;
                    };
                    if is_mdoc_noncallable_macro(name) {
                        outcome.recoveries.push(Recovery::NonCallableMacro {
                            macro_name: name.into(),
                            location: builder.node_location(*child),
                        });
                    }
                }
            }
            if macro_name.as_deref() == Some("Ad")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
            {
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name: "Ad",
                    location: builder.node_location(node),
                });
                continue;
            }
            if macro_name.as_deref() == Some("Fd")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
            {
                // `post_fd()` discards an empty preprocessor directive after
                // recording its warning.  In particular it must not become
                // an empty SYNOPSIS child that affects following flow.
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name: "Fd",
                    location: builder.node_location(node),
                });
                continue;
            }
            let empty_function_macro = match macro_name.as_deref() {
                Some("Fa") => Some("Fa"),
                Some("Fn") => Some("Fn"),
                Some("Ft") => Some("Ft"),
                _ => None,
            };
            if let Some(macro_name) = empty_function_macro
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
            {
                // Function declarations use the same post-validation rule
                // for their empty field, name, and argument macros: retain
                // the finding but remove the syntax-only element before it
                // can alter a surrounding Fo declaration's public flow.
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name,
                    location: builder.node_location(node),
                });
                continue;
            }
            if macro_name.as_deref() == Some("No")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
                && scopes
                    .iter()
                    .rev()
                    .find(|frame| frame.close == "El")
                    .and_then(|list| list_types.get(&list.body))
                    .is_some_and(|list_type| *list_type == "diag")
            {
                // `No` is an inline spacing control only when it owns visible
                // content.  An empty request is discarded by post-validation
                // before it can become a column-list body child.
                if let Some(next) = inline_events.get(event_index + 1).copied()
                    && let Some(mut flags) = builder.node_flags(next)
                {
                    // The discarded request does not own a public node, so a
                    // following callable spelling becomes the first event of
                    // this physical list-body line.
                    flags.line_start = true;
                    let _ = builder.set_node_flags(next, flags);
                }
                outcome.recoveries.push(Recovery::EmptyMacro {
                    macro_name: "No",
                    location: builder.node_location(node),
                });
                continue;
            }
            if macro_name.as_deref() == Some("Ad") {
                let arguments = builder
                    .children(node)
                    .map(<[NodeId]>::to_vec)
                    .unwrap_or_default();
                if let Some(last) = arguments.last().copied()
                    && let Some(text) = builder.node_text(last)
                    && let Some((&delimiter, prefix)) = text.as_bytes().split_last()
                    && matches!(
                        delimiter,
                        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']'
                    )
                    && prefix
                        .last()
                        .is_some_and(|byte| !byte.is_ascii_whitespace())
                    && let Some(location) = builder.node_location(last).and_then(|span| {
                        span.end
                            .checked_sub(1)
                            .and_then(|start| SourceSpan::new(span.source, start, span.end).ok())
                    })
                {
                    let display = if arguments.len() == 1 {
                        text.to_owned()
                    } else {
                        format!("... {text}")
                    };
                    deferred
                        .post_validation
                        .push(Recovery::TrailingDelimiterSpacing {
                            macro_name: "Ad",
                            display: display.into(),
                            location: Some(location),
                        });
                }
            }
            if macro_name.as_deref() == Some("An")
                && builder.children(node).is_none_or(<[NodeId]>::is_empty)
                && let Some(delimiter) = inline_events.get(event_index + 1).copied()
                && builder
                    .node_text(delimiter)
                    .is_some_and(is_mdoc_closing_delimiter)
                && let Some(mut flags) = builder.node_flags(delimiter)
            {
                // With no author argument, punctuation is plain following
                // text rather than a closing delimiter for an An element.
                flags.delimiter_close = false;
                let _ = builder.set_node_flags(delimiter, flags);
            }
            // Existing explicit-partial handling permits `.Fl` and `.No` to
            // consume their close argument.  Extend that narrow path only to
            // a synthetic Xo owned by an `.It` Head: deployed mdoc uses
            // `.Oo … Oc Xc` for grouped item syntax, and that outer Xc must
            // be detached before `.Oo` consumes its own inline arguments.
            let item_head_xo = scopes.last().is_some_and(|frame| {
                frame.close == "Xc"
                    && builder.node_parent(frame.open).is_some_and(|parent| {
                        builder.node_kind(parent) == Some(NodeKind::Head)
                            && builder.node_macro_name(parent) == Some("It")
                    })
            });
            let direct_partial_close =
                if matches!(macro_name.as_deref(), Some("Fl" | "No")) || item_head_xo {
                    take_explicit_partial_close_argument(builder, node, &scopes)
                } else {
                    None
                };
            let paragraph_href = pending_paragraph_href.take();
            let list_open = scopes.iter().rev().any(|frame| frame.close == "El");
            let list_item_follower = macro_name.as_deref() == Some("It") && list_open;
            if builder.node_kind(node) == Some(NodeKind::Text) {
                if let Some((tag_node, tag)) = pending_manual_tag.take() {
                    if let Some(item) = active_column_item(builder, active_body) {
                        // A `.Tg` followed by more text in the current column
                        // row targets that row without turning its syntax node
                        // into a permalink.
                        if !tag.is_empty() {
                            mark_manual_target(builder, item, &tag);
                            mark_no_print(builder, tag_node);
                        }
                    } else if !tag.is_empty()
                        && let Some(paragraph) =
                            preceding_manual_tag_paragraph(builder, active_body, tag_node)
                    {
                        // `post_tg()` gives a tag before ordinary paragraph
                        // text to the preceding Pp rather than publishing a
                        // second text-owned destination.
                        mark_manual_target(builder, paragraph, &tag);
                        mark_no_print(builder, tag_node);
                    }
                }
            } else if !accepts_pending_manual_tag(macro_name.as_deref()) && !list_item_follower {
                // Other manual-target forms are deliberately left to their own
                // semantic families rather than allowing a pending Pp tag to
                // leak across an unrelated source event.
                pending_manual_tag = None;
            }
            if builder.node_kind(node) != Some(NodeKind::Text)
                && !matches!(
                    macro_name.as_deref(),
                    Some("Pp" | "Lp" | "Tg" | "Bk" | "Fn" | "Fo" | "br")
                )
            {
                pending_function_paragraph = None;
            }
            if !matches!(macro_name.as_deref(), Some("Dd" | "Dt" | "Os"))
                && builder.node_kind(node) != Some(NodeKind::Comment)
            {
                builder.metadata_mut().has_body = true;
            }
            // libmandoc carries the current synopsis presentation bit into
            // the next control line before that line's package validator can
            // switch the section state.  Mark the scanner-owned source node
            // first; newly constructed bodies below select their own state.
            if in_synopsis {
                mark_synopsis_pretty(builder, node);
            }
            if active_column_list(builder, active_body)
                && is_implicit_column_row_macro(macro_name.as_deref())
                && builder
                    .node_flags(node)
                    .is_some_and(|flags| flags.line_start)
                && structure_implicit_column_item(
                    builder,
                    active_body,
                    node,
                    spacing_enabled,
                    max_nodes,
                    outcome,
                    &mut scopes,
                )
            {
                continue;
            }
            if macro_name.as_deref() == Some("Ta")
                && let Some(item) = active_column_item(builder, active_body)
            {
                let tokens = builder
                    .children(node)
                    .map(<[NodeId]>::to_vec)
                    .unwrap_or_default();
                let location = builder.node_location(node);
                if let Some(body) = append_column_ta_cell(
                    builder,
                    active_body,
                    location.clone(),
                    &tokens,
                    spacing_enabled,
                    max_nodes,
                    outcome,
                    &mut scopes,
                ) {
                    if let Some(mut flags) = builder.node_flags(body) {
                        flags.line_start = builder
                            .node_flags(node)
                            .is_some_and(|node_flags| node_flags.line_start);
                        let _ = builder.set_node_flags(body, flags);
                    }
                    outcome
                        .recoveries
                        .push(Recovery::ColumnFirstMacro { location });
                    extend_pending_short_column_item(&mut pending_short_column_items, item);
                    active_body = body;
                    flow_parent = body;
                    continue;
                }
            }
            match macro_name.as_deref() {
                Some("Dd") => {
                    if saw_date_prologue {
                        outcome.recoveries.push(Recovery::DuplicatePrologue {
                            macro_name: "Dd",
                            location: builder.node_location(node),
                        });
                    } else if active_body != root {
                        outcome.recoveries.push(Recovery::LateDate {
                            location: builder.node_location(node),
                        });
                    } else if saw_title_prologue {
                        outcome.recoveries.push(Recovery::DateAfterTitle {
                            location: builder.node_location(node),
                        });
                    }
                    saw_date_prologue = true;
                    let is_first_date_prologue = first_date_prologue.is_none();
                    if is_first_date_prologue {
                        first_date_prologue = Some((
                            node_arguments(builder, node).join(" ").into_boxed_str(),
                            builder
                                .children(node)
                                .and_then(|children| children.first().copied())
                                .and_then(|argument| builder.node_location(argument)),
                        ));
                    }
                    record_date(builder, node, outcome);
                    // mandoc reports this OpenBSD-specific style finding with
                    // the first `.Dd`, after any validation of that date and
                    // before diagnostics from subsequent prologue requests.
                    if is_first_date_prologue
                        && saw_openbsd_rcs_id
                        && active_body == root
                        && let Some((date, location)) = &first_date_prologue
                        && !date.is_empty()
                        && !date.starts_with("$Mdocdate")
                    {
                        outcome.recoveries.push(Recovery::MdocDateMissing {
                            date: date.clone(),
                            location: location.clone(),
                        });
                    }
                    coalesce_text_children(builder, node);
                    mark_no_print(builder, node);
                    // A late date request is still no-printing metadata, but
                    // its source node remains in the active body rather than
                    // being hoisted back into the document prologue.
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Dt") => {
                    if active_body == root {
                        if saw_title_prologue {
                            outcome.recoveries.push(Recovery::DuplicatePrologue {
                                macro_name: "Dt",
                                location: builder.node_location(node),
                            });
                        } else if saw_operating_system_prologue {
                            outcome
                                .recoveries
                                .push(Recovery::TitleAfterOperatingSystem {
                                    location: builder.node_location(node),
                                });
                        }
                        saw_title_prologue = true;
                        record_title(builder, node, outcome);
                    } else {
                        outcome.recoveries.push(Recovery::LateTitle {
                            location: builder.node_location(node),
                        });
                    }
                    mark_no_print(builder, node);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Os") => {
                    if saw_operating_system_prologue {
                        outcome.recoveries.push(Recovery::DuplicatePrologue {
                            macro_name: "Os",
                            location: builder.node_location(node),
                        });
                    } else if active_body != root {
                        outcome.recoveries.push(Recovery::LateOperatingSystem {
                            location: builder.node_location(node),
                        });
                    }
                    saw_operating_system_request = true;
                    let values = node_arguments(builder, node);
                    let operating_system = values.join(" ");
                    if operating_system_flavour.is_none() && !operating_system.is_empty() {
                        operating_system_flavour =
                            Some(mdoc_operating_system_flavour(&operating_system));
                        netbsd_operating_system_validation = operating_system == "NetBSD";
                    }
                    saw_operating_system_prologue = true;
                    record_operating_system(builder, node);
                    if let (Some(flavour), Some(argument)) = (
                        operating_system_flavour,
                        builder
                            .children(node)
                            .and_then(|children| children.first().copied()),
                    ) {
                        if !operating_system.is_empty() {
                            outcome.recoveries.push(Recovery::OperatingSystemExplicit {
                                operating_system: operating_system.clone().into_boxed_str(),
                                flavour,
                                location: builder.node_location(argument),
                            });
                        }
                        if active_body == root
                            && netbsd_operating_system_validation
                            && let Some((date, location)) = &first_date_prologue
                            && date.starts_with("$Mdocdate")
                        {
                            outcome.recoveries.push(Recovery::MdocDateFound {
                                date: date.clone(),
                                location: location.clone(),
                            });
                        }
                    }
                    mark_no_print(builder, node);
                    if active_body == root {
                        root_children.push(node);
                    } else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                    }
                }
                Some("Sh") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Sh",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    flush_pending_nd_delimiters(
                        builder,
                        &mut pending_nd_delimiter_bodies,
                        &mut outcome.recoveries,
                    );
                    flush_pending_name_section(
                        builder,
                        &mut pending_name_section_body,
                        &mut outcome.recoveries,
                    );
                    flush_pending_authors_section(
                        builder,
                        &mut pending_authors_body,
                        &mut outcome.recoveries,
                    );
                    let raw_section_title = node_arguments(builder, node).join(" ");
                    outcome
                        .recoveries
                        .extend(mdoc_heading_tab_recoveries(builder, node));
                    let breaks_explicit_partial = scopes
                        .iter()
                        .any(|frame| is_explicit_partial_close(frame.close) || frame.close == "Xc");
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Sh",
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        root_children.push(node);
                        continue;
                    };
                    if breaks_explicit_partial && let Some(mut flags) = builder.node_flags(node) {
                        // A section that interrupts a cross-line delimiter
                        // block is retained as that block's continuation,
                        // not as an independent line-start event.
                        flags.line_start = false;
                        let _ = builder.set_node_flags(node, flags);
                    }
                    // A section title is one semantic end-of-line phrase;
                    // scanner words and callable macros remain separate only
                    // until this mdoc structural boundary.
                    let heading_events = split_mdoc_inline_children(
                        builder,
                        head,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let _ = builder.replace_children(head, &heading_events);
                    let heading_scopes = structure_unclosed_explicit_partial_blocks(
                        builder,
                        head,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    if !heading_scopes.is_empty()
                        && let Some(mut flags) = builder.node_flags(body)
                    {
                        // `blk_full()` opens the section Body while the
                        // header's cross-line partial is still active.  The
                        // generated Body consequently retains the request's
                        // line-start bit, even though no source text has been
                        // attached to it yet.
                        flags.line_start = true;
                        let _ = builder.set_node_flags(body, flags);
                    }
                    coalesce_adjacent_text_children(builder, head);
                    // `post_sh_head()` validates the rendered title text,
                    // not scanner spellings.  Thus `.Sh SEE Em ALSO` is the
                    // conventional SEE ALSO heading after its callable Em
                    // element has been formed.
                    let section_title =
                        visible_head_text(builder, head).unwrap_or(raw_section_title);
                    in_name_section = section_title.eq_ignore_ascii_case("NAME");
                    if !has_section && !in_name_section {
                        outcome.recoveries.push(Recovery::FirstSectionNotName {
                            section: section_title.clone().into(),
                            location: builder.node_location(node),
                        });
                    }
                    has_section = true;
                    let manual_section = builder.metadata_mut().section.clone();
                    if let Some((rank, canonical_title)) = named_mdoc_section(&section_title) {
                        if last_named_section == Some(rank) {
                            outcome.recoveries.push(Recovery::DuplicateSection {
                                section: canonical_title,
                                location: builder.node_location(node),
                            });
                        } else if last_named_section.is_some_and(|last| rank < last) {
                            outcome.recoveries.push(Recovery::SectionOutOfOrder {
                                section: canonical_title,
                                location: builder.node_location(node),
                            });
                        }
                        last_named_section = Some(rank);
                        if let Some(allowed_sections) =
                            unexpected_section_manuals(canonical_title, manual_section.as_deref())
                        {
                            outcome.recoveries.push(Recovery::UnexpectedSection {
                                section: canonical_title.into(),
                                allowed_sections,
                                location: builder.node_location(node),
                            });
                        }
                    }
                    let next_synopsis = section_title.eq_ignore_ascii_case("SYNOPSIS");
                    target_heads.push(head);
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        mark_target(builder, head, Some(&tag));
                        mark_no_print(builder, tag_node);
                    }
                    for frame in std::mem::take(&mut scopes) {
                        let macro_name = open_name(frame.close);
                        outcome.recoveries.push(Recovery::BrokenBlock {
                            breaker: "Sh",
                            macro_name,
                            location: builder.node_location(node),
                        });
                        if frame.close == "Ek"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            deferred.post_validation.push(Recovery::EmptyBlock {
                                macro_name: "Bk",
                                location: builder.node_location(frame.open),
                            });
                            discard_empty_block(
                                builder,
                                root,
                                &mut root_children,
                                frame.resume_flow,
                                frame.open,
                            );
                        }
                        implicitly_closed.push(frame.close);
                    }
                    section_parent = body;
                    if section_title.eq_ignore_ascii_case("NAME") {
                        pending_name_section_body = Some(body);
                    }
                    if section_title.eq_ignore_ascii_case("AUTHORS") {
                        pending_authors_body = Some(body);
                    }
                    flow_parent = body;
                    active_body = body;
                    if let Some(scope) = heading_scopes.last().copied() {
                        // A partial opener at the end of a section title owns
                        // following physical flow until its closer or the
                        // next section request.  Its resume point is the
                        // public section Head, not the new section Body.
                        flow_parent = scope.body;
                        active_body = scope.body;
                    }
                    scopes.extend(heading_scopes);
                    if in_synopsis {
                        // The just-created Head is still part of the current
                        // control line and inherits the old state.  The Body
                        // starts after `Sh` validation and therefore does not.
                        mark_synopsis_pretty(builder, head);
                    }
                    if next_synopsis {
                        // mandoc's `MDOC_SYNOPSIS` state begins at the section
                        // body, not at the Sh block or its heading.
                        mark_synopsis_pretty(builder, body);
                        synopsis_bodies.push(body);
                    }
                    in_synopsis = next_synopsis;
                    synopsis_from_register = false;
                    synopsis_name_body = None;
                    root_children.push(node);
                }
                Some("Ss") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Ss",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Ss",
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let heading_events = split_mdoc_inline_children(
                        builder,
                        head,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let _ = builder.replace_children(head, &heading_events);
                    coalesce_adjacent_text_children(builder, head);
                    target_heads.push(head);
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        mark_target(builder, head, Some(&tag));
                        mark_no_print(builder, tag_node);
                    }
                    append_to_parent(builder, root, &mut root_children, section_parent, node);
                    flow_parent = body;
                    active_body = body;
                    synopsis_name_body = None;
                    scopes.clear();
                }
                Some("Nm") if in_synopsis => {
                    // `ctx_synopsis()` dispatches Nm through `blk_full()`.
                    // A top-level name implicitly finishes the preceding
                    // name block, while a name inside an open Fo/Oo/... scope
                    // remains owned by that scope's active Body.
                    //
                    // An authored synopsis name is also the fallback document
                    // name when a malformed NAME section did not contribute
                    // one.  The ordinary Nm branch records this before its
                    // structural work; keep the full-block synopsis path
                    // equivalent rather than relying only on generated empty
                    // Nm expansion below.
                    record_name(builder, node);
                    let nested_scope = !scopes.is_empty();
                    let function_scope = scopes.iter().any(|frame| frame.close == "Fc");
                    if !nested_scope && synopsis_name_body.is_some() {
                        active_body = section_parent;
                        flow_parent = section_parent;
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Nm",
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if builder.children(head).is_some_and(<[NodeId]>::is_empty)
                        && !insert_generated_nm_name(builder, node, head, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    mark_synopsis_pretty(builder, node);
                    mark_synopsis_pretty(builder, head);
                    if synopsis_from_register {
                        clear_generated_synopsis_pretty_children(builder, head);
                    }
                    let parent = if nested_scope {
                        active_body
                    } else {
                        section_parent
                    };
                    append_to_parent(builder, root, &mut root_children, parent, node);
                    if function_scope {
                        // `Fc` closes the surrounding Fo on the same source
                        // line, so libmandoc retains the embedded Nm's Block
                        // and Head but no provisional Body.
                        let _ = builder.replace_children(node, &[head]);
                    } else {
                        if inline_events.get(event_index + 1).is_some() {
                            active_body = head;
                            flow_parent = head;
                            synopsis_name_inline_restore = Some((head, body));
                        } else {
                            active_body = body;
                            flow_parent = body;
                        }
                        if !nested_scope {
                            synopsis_name_body = Some(body);
                            synopsis_keep_boundary = false;
                        }
                    }
                }
                Some("Vt") if in_synopsis => {
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Vt",
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    mark_synopsis_pretty(builder, node);
                    mark_synopsis_pretty(builder, head);
                    mark_synopsis_pretty(builder, body);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Vt") => {
                    let events = split_inline_macro_events(
                        builder,
                        node,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    for (event_index, event) in events.into_iter().enumerate() {
                        // A Vt request can split into nested callable macro
                        // events and released punctuation. Only Vt Elements
                        // receive Vt's post-validation; the other events
                        // remain ordinary siblings in source order.
                        if builder.node_macro_name(event) != Some("Vt") {
                            append_to_parent(builder, root, &mut root_children, active_body, event);
                            continue;
                        }
                        if builder.children(event).is_none_or(<[NodeId]>::is_empty) {
                            // Outside SYNOPSIS, Vt is an ordinary inline
                            // element. `post_delim_nb()` reports then deletes
                            // its source-spelled empty form, unlike the
                            // partial block form above, which retains its
                            // Body. Later empty events are temporary
                            // delimiter-split restarts and stay private.
                            if event_index == 0 {
                                outcome.recoveries.push(Recovery::EmptyMacro {
                                    macro_name: "Vt",
                                    location: builder.node_location(event),
                                });
                            }
                            continue;
                        }
                        validate_no_break_trailing_delimiter(
                            builder,
                            event,
                            "Vt",
                            &mut deferred.post_validation,
                        );
                        append_to_parent(builder, root, &mut root_children, active_body, event);
                    }
                }
                Some("Eo") => {
                    // Eo is the exceptional explicit partial block: its first
                    // argument belongs to Head and an Ec later supplies a
                    // third Tail child only when Ec actually arrives.  An
                    // unclosed Eo retains its observable Head/Body prefix.
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Eo",
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    scopes.push(ScopeFrame {
                        close: "Ec",
                        open: node,
                        body,
                        tail_on_close: true,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    // Keep `head` bound in the branch: it is deliberately the
                    // parser-owned holder of Eo's one opening argument.
                    let _ = head;
                    flow_parent = body;
                    active_body = body;
                }
                Some(name) if explicit_partial_block_close(name).is_some() => {
                    let close = explicit_partial_block_close(name)
                        .expect("the guard checked this explicit partial block");
                    let closes_on_line = builder.children(node).is_some_and(|children| {
                        matching_explicit_partial_close_index(builder, children, close).is_some()
                    });
                    let tail = split_explicit_partial_block_tail(builder, node, close);
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if in_synopsis {
                        // The scanner marks the authored opener before this
                        // branch manufactures its structural Head/Body.
                        // Those generated containers inherit the current
                        // synopsis state even if a following `.nr nS 0`
                        // appears before the explicit closer.
                        mark_synopsis_pretty(builder, head);
                        mark_synopsis_pretty(builder, body);
                    }
                    structure_matched_explicit_partial_blocks(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let nested_scopes = if closes_on_line {
                        Vec::new()
                    } else {
                        structure_unclosed_explicit_partial_blocks(
                            builder,
                            body,
                            spacing_enabled,
                            max_nodes,
                            outcome,
                        )
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    clear_leading_explicit_partial_punctuation(builder, body);
                    move_explicit_leading_open_delimiter(builder, node, head, body);
                    structure_nested_implicit_partial_blocks(
                        builder,
                        body,
                        max_nodes,
                        outcome,
                        spacing_enabled,
                    );
                    if matches!(name, "Bo" | "Do" | "Po") {
                        // Scanner control arguments begin as separate lexical
                        // children. An ordinary `Bo in brackets` body is one
                        // mdoc phrase in the legacy owned AST, including when
                        // a later `Bc` is extended with `.am`.
                        coalesce_adjacent_text_children(builder, body);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if !closes_on_line {
                        scopes.push(ScopeFrame {
                            close,
                            open: node,
                            body,
                            tail_on_close: false,
                            transparent_target_taken: false,
                            suppress_implicit_ancestor_break: false,
                            resume_active: active_body,
                            resume_flow: flow_parent,
                        });
                        flow_parent = body;
                        active_body = body;
                        for nested_scope in nested_scopes {
                            active_body = nested_scope.body;
                            flow_parent = nested_scope.body;
                            scopes.push(nested_scope);
                        }
                    }
                    append_explicit_partial_tail(
                        builder,
                        root,
                        &mut root_children,
                        &mut scopes,
                        &mut implicitly_closed,
                        &mut active_body,
                        &mut flow_parent,
                        node,
                        &tail,
                        false,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                }
                Some(name) if is_implicit_partial_block_macro(name) => {
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    // An explicit partial closer in an implicit partial
                    // request is still callable syntax.  `mdoc_macro.c`
                    // splits the surrounding Body around it, inserts the
                    // closed explicit block's empty Body at the call site,
                    // and resumes parsing the remaining implicit argument.
                    // Keeping the source token as plain text would lose both
                    // the public boundary and the `Bo breaks Pq` recovery.
                    let raw_children = builder
                        .children(body)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    // An otherwise empty argument produced solely by a
                    // failed interpolation is parser-private for implicit
                    // partial blocks.  Keep authored `""` arguments (whose
                    // width delta is zero), but remove the placeholder before
                    // the block body is projected so `.Sq \\*[missing] .`
                    // has the legacy empty Body rather than a visible empty
                    // Text child.
                    let raw_children = raw_children
                        .into_iter()
                        .filter(|token| {
                            !(builder.node_text(*token) == Some("")
                                && builder.node_argument_expansion_width_delta(*token) < 0)
                        })
                        .collect::<Vec<_>>();
                    let mut enclosed_explicit_closes = Vec::new();
                    let mut pending_tokens = Vec::new();
                    let mut children = Vec::new();
                    for token in raw_children {
                        let close = builder
                            .node_text(token)
                            .filter(|close| is_explicit_partial_close(close))
                            .filter(|close| scopes.iter().any(|frame| frame.close == *close))
                            .map(str::to_owned);
                        let Some(close) = close else {
                            pending_tokens.push(token);
                            continue;
                        };
                        children.extend(split_mdoc_inline_tokens(
                            builder,
                            body,
                            &pending_tokens,
                            spacing_enabled,
                            max_nodes,
                            outcome,
                        ));
                        pending_tokens.clear();
                        let location = builder.node_location(token);
                        let Some(closed_body) = builder.push(body, NodeKind::Body) else {
                            if outcome.node_limit_location.is_none() {
                                outcome.node_limit_location = location;
                            }
                            pending_tokens.push(token);
                            continue;
                        };
                        if !builder.macro_name(closed_body, open_name(&close))
                            || !builder.set_node_location(closed_body, location.clone())
                        {
                            pending_tokens.push(token);
                            continue;
                        }
                        children.push(closed_body);
                        enclosed_explicit_closes.push((close, location, closed_body));
                    }
                    children.extend(split_mdoc_inline_tokens(
                        builder,
                        body,
                        &pending_tokens,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    ));
                    let mut children =
                        expand_fl_elements(builder, body, children, max_nodes, outcome);
                    insert_generated_system_names(builder, &children, max_nodes, outcome);
                    let tail = take_implicit_partial_tail(builder, &mut children);
                    let _ = builder.replace_children(body, &children);
                    move_leading_open_delimiters(builder, node, head, body);
                    clear_initial_implicit_body_delimiter_flags(builder, body);
                    clear_terminal_implicit_body_opening_flags(builder, body);
                    mark_implicit_partial_tail_sentence_ends(builder, &tail);
                    if spacing_enabled && name != "Op" {
                        coalesce_implicit_partial_body_text(builder, body);
                    }
                    structure_nested_implicit_partial_blocks(
                        builder,
                        body,
                        max_nodes,
                        outcome,
                        spacing_enabled,
                    );
                    // A direct explicit opener is an element of this
                    // implicit request (`.Op … Do …`), while an opener inside
                    // a nested implicit block is only discovered after that
                    // block has been projected.  Both must enter the physical
                    // closer stack: a later `.Dc` then reports that the
                    // enclosing `.Op` broke the `.Do` rather than becoming an
                    // inert text control.
                    let mut nested_implicit_scopes = structure_unclosed_explicit_partial_blocks(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    nested_implicit_scopes.extend(structure_nested_implicit_explicit_scopes(
                        builder,
                        body,
                        max_nodes,
                        outcome,
                        spacing_enabled,
                    ));
                    if spacing_enabled && name == "Op" {
                        // `Op` keeps the lexical argument boundaries that
                        // precede a crossed explicit closer, but prose resumed
                        // after that closer is once again an ordinary phrase.
                        // A nested implicit partial owns the closer when it
                        // is the immediately preceding construct.
                        for (_, _, closed_body) in &enclosed_explicit_closes {
                            let parent = relocate_crossed_closer_to_nested_implicit_body(
                                builder,
                                body,
                                *closed_body,
                            )
                            .unwrap_or(body);
                            coalesce_text_children_after(builder, parent, *closed_body);
                        }
                    }
                    if !tail.is_empty() {
                        let mut block_children = vec![head, body];
                        block_children.extend(tail);
                        let _ = builder.replace_children(node, &block_children);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for (close, location, _) in enclosed_explicit_closes {
                        if scopes.iter().any(|frame| frame.close == close) {
                            deferred.syntax_stage.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(&close),
                                interrupted: implicit_partial_block_name(name),
                                location,
                            });
                        }
                        close_explicit_partial_scope(
                            &mut scopes,
                            &mut implicitly_closed,
                            &mut active_body,
                            &mut flow_parent,
                            &close,
                        );
                    }
                    for scope in &mut nested_implicit_scopes {
                        // The surrounding implicit blocks close at the end
                        // of their source request.  A later explicit closer
                        // therefore resumes this request's outer flow, not
                        // an implicit ancestor that has no cross-line scope.
                        scope.resume_active = active_body;
                        scope.resume_flow = flow_parent;
                    }
                    for scope in nested_implicit_scopes {
                        active_body = scope.body;
                        flow_parent = scope.body;
                        scopes.push(scope);
                    }
                }
                Some("Sm") => {
                    let arguments = node_arguments(builder, node);
                    let children = builder.children(node).unwrap_or_default().to_vec();
                    let mut relocated_arguments = Vec::new();
                    match arguments.first().map(String::as_str) {
                        Some("off") => {
                            spacing_enabled = false;
                            relocated_arguments.extend_from_slice(&children[1..]);
                            let _ = builder.replace_children(node, &children[..1]);
                        }
                        Some("on") => {
                            spacing_enabled = true;
                            relocated_arguments.extend_from_slice(&children[1..]);
                            let _ = builder.replace_children(node, &children[..1]);
                        }
                        None => spacing_enabled = !spacing_enabled,
                        Some(argument) => {
                            outcome.recoveries.push(Recovery::InvalidBooleanArgument {
                                macro_name: "Sm",
                                argument: argument.into(),
                                location: argument_location(builder, node, 0),
                            });
                            // `post_sm()` detaches only an invalid first
                            // argument and its remaining source siblings,
                            // relinking them immediately after the control
                            // node.  Keeping that source-order flow makes
                            // later inline validation observe the same
                            // boundary as libmandoc.
                            relocated_arguments.extend_from_slice(&children);
                            let _ = builder.replace_children(node, &[]);
                        }
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for argument in relocated_arguments {
                        append_to_parent(builder, root, &mut root_children, active_body, argument);
                    }
                }
                Some(name) if is_reference_field_macro(name) => {
                    // Bibliographic fields inside an Rs block use the
                    // end-of-line argument grammar.  Only the fields marked
                    // MDOC_JOIN by the package coalesce ordinary source
                    // words; the numeric/page/URL fields retain individual
                    // text nodes.
                    if reference_field_joins_arguments(name) {
                        coalesce_adjacent_text_children(builder, node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Tg") => {
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // Consecutive manual destinations do not make the
                        // earlier tag transparent.  With no later eligible
                        // semantic target, `post_tg()` keeps it as its own
                        // deep-link destination.
                        mark_destination(builder, tag_node);
                    }
                    let reference_transparent =
                        scopes.last().is_some_and(|frame| frame.close == "Re");
                    let arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let xo_preceding_target = scopes
                        .last()
                        .filter(|frame| frame.close == "Xc")
                        .and_then(|frame| {
                            builder.children(frame.resume_active).and_then(|children| {
                                children
                                    .iter()
                                    .position(|child| *child == frame.open)
                                    .and_then(|index| {
                                        children[..index].iter().rev().copied().find(|candidate| {
                                            matches!(
                                                builder.node_macro_name(*candidate),
                                                Some("Pp" | "Lp")
                                            )
                                        })
                                    })
                            })
                        });
                    let fl_preceding_target = builder.children(active_body).and_then(|children| {
                        let last = children.last().copied()?;
                        (builder.node_macro_name(last) == Some("Fl")
                            && builder.children(last).is_some_and(<[NodeId]>::is_empty))
                        .then(|| {
                            children[..children.len().saturating_sub(1)]
                                .iter()
                                .rev()
                                .copied()
                                .find(|candidate| {
                                    matches!(builder.node_macro_name(*candidate), Some("Pp" | "Lp"))
                                })
                        })
                        .flatten()
                    });
                    if scopes.last().is_some_and(|frame| {
                        frame.close == "Fc" && (!in_synopsis || !frame.transparent_target_taken)
                    }) {
                        // Function argument validation treats an in-body Tg
                        // as a transparent destination node.  It remains
                        // visible syntax and does not expose its argument as
                        // the public tag string.
                        mark_destination(builder, node);
                        if in_synopsis {
                            scopes
                                .last_mut()
                                .expect("the matching Fo scope was just checked")
                                .transparent_target_taken = true;
                        }
                    } else if scopes.last().is_some_and(|frame| frame.close == "Fc") {
                        // Later transparent anchors in the same function
                        // block are validation-only syntax.
                        mark_no_print(builder, node);
                    }
                    if reference_transparent {
                        // Reference lists retain transparent tags as direct
                        // destinations, independent of their invalid-content
                        // recovery in `post_rs()`.
                        mark_destination(builder, node);
                    }
                    let first_tag = arguments.first().and_then(|argument| {
                        builder
                            .node_text(*argument)
                            .map(|tag| (*argument, tag.to_owned()))
                    });
                    if let Some((argument, tag)) = first_tag {
                        if tag.is_empty() {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "Tg",
                                location: builder.node_location(node),
                            });
                            if arguments.len() > 1 {
                                let excess = builder.node_text(arguments[1]).unwrap_or_default();
                                outcome.recoveries.push(Recovery::InvalidArguments {
                                    message: format!("skipping excess arguments: Tg ... {excess}")
                                        .into(),
                                    location: argument_location(builder, node, 1),
                                });
                            }
                            continue;
                        }
                        if let Some(offset) = tag
                            .bytes()
                            .position(|byte| byte.is_ascii_whitespace() || byte == b'\\')
                        {
                            outcome.recoveries.push(Recovery::InvalidTag {
                                tag: tag.into(),
                                location: text_offset_location(builder, argument, offset)
                                    .or_else(|| builder.node_location(argument)),
                            });
                            continue;
                        }
                        if arguments.len() > 1 {
                            let excess = arguments[1..]
                                .iter()
                                .filter_map(|argument| builder.node_text(*argument))
                                .collect::<Vec<_>>()
                                .join(" ");
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!("skipping excess arguments: Tg ... {excess}")
                                    .into(),
                                location: argument_location(builder, node, 1),
                            });
                            let _ = builder.replace_children(node, &arguments[..1]);
                        }
                        if let Some(target) = xo_preceding_target {
                            // A Tg nested in a cross-line Xo is transparent
                            // syntax. `post_tg()` returns its destination to
                            // the preceding outer-flow node rather than
                            // carrying it through Xc to later source flow.
                            mark_manual_target(builder, target, &tag);
                            mark_no_print(builder, node);
                        } else if let Some(target) = fl_preceding_target {
                            mark_manual_target(builder, target, &tag);
                            mark_no_print(builder, node);
                            pending_transparent_permalink = Some(tag);
                        } else if reference_transparent {
                            // The direct Tg remains the destination; it does
                            // not carry a public tag string forward.
                        } else {
                            pending_manual_tag = Some((node, tag));
                        }
                    } else if arguments.is_empty() && !reference_transparent {
                        // `.Tg` may borrow the first text child of the next
                        // node as its manual destination spelling.  Preserve
                        // that unresolved form until a supported follower can
                        // supply the text, rather than treating an empty Tg as
                        // an empty public tag.
                        pending_manual_tag = Some((node, String::new()));
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Pp" | "Lp") => {
                    // mandoc parses Pp as an in-line, end-of-line macro rather
                    // than a Head/Body block.  Its arguments remain observable
                    // children (and validation decides whether to diagnose them),
                    // while following source lines stay in the surrounding flow.
                    // Lp is an obsolete spelling that validation normalizes to Pp.
                    if macro_name.as_deref() == Some("Lp") {
                        let _ = builder.macro_name(node, "Pp");
                    }
                    if let Some(argument) = node_arguments(builder, node).first() {
                        deferred
                            .paragraph_arguments
                            .push(Recovery::InvalidArguments {
                                message: format!("skipping all arguments: Pp {argument}").into(),
                                location: builder.node_location(node),
                            });
                    }
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        mark_manual_target(builder, node, &tag);
                        mark_no_print(builder, tag_node);
                        pending_paragraph_href = Some(tag);
                    }
                    // `post_par()` restarts libmandoc's `fn_prio` even when
                    // this paragraph later normalizes away from public flow.
                    function_tag_priority = 2;
                    pending_function_paragraph = Some(node);
                    if in_synopsis && synopsis_keep_boundary {
                        // A paragraph boundary ends the current SYNOPSIS Nm
                        // block.  The Pp itself is a section-body sibling;
                        // keeping it in the Nm Body loses the observable
                        // input-line boundary before the next synopsis name.
                        flow_parent = section_parent;
                        synopsis_name_body = None;
                        synopsis_keep_boundary = false;
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    active_body = flow_parent;
                }
                Some("br") => {
                    let arguments = node_arguments(builder, node);
                    if !arguments.is_empty() {
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            message: format!("skipping all arguments: br {}", arguments.join(" "))
                                .into(),
                            location: argument_location(builder, node, 0),
                        });
                        let _ = builder.replace_children(node, &[]);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("sp") => {
                    let arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    if arguments.len() > 1 {
                        let excess = arguments[1..]
                            .iter()
                            .filter_map(|argument| builder.node_text(*argument))
                            .collect::<Vec<_>>()
                            .join(" ");
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            message: format!("skipping excess arguments: sp ... {excess}").into(),
                            location: argument_location(builder, node, 1),
                        });
                        let _ = builder.replace_children(node, &arguments[..1]);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Nd") => {
                    // A successive description request finishes the prior
                    // one before the new Body becomes active.  This mirrors
                    // libmandoc's post-order validation rather than checking
                    // only the control-line argument.
                    flush_pending_nd_delimiters(
                        builder,
                        &mut pending_nd_delimiter_bodies,
                        &mut outcome.recoveries,
                    );
                    let Some((_, body)) = make_block(
                        builder,
                        node,
                        "Nd",
                        ArgumentPlacement::Body,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if !in_name_section {
                        outcome.recoveries.push(Recovery::DescriptionOutsideName {
                            location: builder.node_location(node),
                        });
                    }
                    pending_nd_delimiter_bodies.push(body);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    active_body = body;
                    flow_parent = body;
                }
                Some("Nm") => {
                    record_name(builder, node);
                    if builder.children(node).is_some_and(<[NodeId]>::is_empty) {
                        if builder.metadata_mut().name.is_none() {
                            outcome.recoveries.push(Recovery::MissingName {
                                location: builder.node_location(node),
                            });
                        } else if !insert_generated_nm_name(builder, node, node, max_nodes)
                            && outcome.node_limit_location.is_none()
                        {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                    }
                    // `Nm` follows the no-break trailing-delimiter validator,
                    // rather than the generic tag validator: its one or more
                    // name words remain the element's complete phrase.
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        "Nm",
                        &mut deferred.post_validation,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if !matches!(macro_name.as_deref(), Some("Fl" | "No"))
                        && let Some(close) = scopes.last().map(|frame| frame.close)
                        && node_arguments(builder, node)
                            .iter()
                            .any(|argument| argument == close)
                    {
                        let frame = scopes.pop().expect("last scope was checked");
                        active_body = frame.resume_active;
                        flow_parent = frame.resume_flow;
                    }
                }
                Some("Fl") => {
                    let elements =
                        expand_fl_elements(builder, root, vec![node], max_nodes, outcome);
                    for element in &elements {
                        validate_tag(builder, *element, "Fl", &mut deferred.post_validation);
                    }
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && let Some(element) = elements.first()
                    {
                        if tag.is_empty() {
                            if builder
                                .children(*element)
                                .and_then(|children| children.first())
                                .and_then(|child| builder.node_text(*child))
                                .is_some_and(|text| !text.is_empty())
                            {
                                mark_target(builder, *element, None);
                                mark_no_print(builder, tag_node);
                            }
                        } else {
                            mark_target(builder, *element, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                    }
                    for element in elements {
                        append_to_parent(builder, root, &mut root_children, active_body, element);
                    }
                }
                Some("Ar") => {
                    // The argument macro has a semantic default rather than
                    // an empty rendering: mandoc synthesizes `file ...` as
                    // two generated words, including in SYNOPSIS.
                    if builder.children(node).is_some_and(<[NodeId]>::is_empty)
                        && !insert_generated_ar_default(builder, node, node, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Fn") => {
                    validate_tag(builder, node, "Fn", &mut deferred.post_validation);
                    validate_function_name(builder, node, &mut outcome.recoveries);
                    validate_function_argument_commas(builder, node, &mut outcome.recoveries);
                    let function_name = node_arguments(builder, node).first().cloned();
                    // mdoc's automatic function destination is the first
                    // space-delimited component of its first parsed argument
                    // (including when that argument came from a quoted
                    // prototype phrase), not the whole display spelling.
                    let function_tag = function_name
                        .as_deref()
                        .and_then(automatic_mdoc_function_tag);
                    let paragraph = pending_function_paragraph.take();
                    if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        if let Some(paragraph) = paragraph {
                            mark_manual_target(builder, paragraph, &tag);
                            mark_permalink(builder, node, Some(&tag));
                        } else {
                            mark_target(builder, node, Some(&tag));
                        }
                        mark_no_print(builder, tag_node);
                    } else if !in_synopsis && let Some(function_tag) = function_tag {
                        automatic_function_targets.push(AutomaticFunctionTarget {
                            destination: paragraph.unwrap_or(node),
                            permalink: paragraph.map(|_| node),
                            tag: function_tag.to_owned(),
                            priority: function_tag_priority,
                            exposes_tag: paragraph.is_none(),
                        });
                        function_tag_priority = function_tag_priority.saturating_add(1);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ft") => {
                    validate_tag(builder, node, "Ft", &mut deferred.post_validation);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Fa") => {
                    validate_tag(builder, node, "Fa", &mut deferred.post_validation);
                    validate_function_argument_commas(builder, node, &mut outcome.recoveries);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Lk") => {
                    // Unlike the ordinary tag macros, `Lk` keeps all source
                    // punctuation inside its element. A truly empty request
                    // has no public node; attached delimiters are checked by
                    // the same delayed validator used by the legacy parser.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Lk",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    mark_link_terminal_delimiter(builder, node);
                    validate_tag(builder, node, "Lk", &mut deferred.post_validation);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Mt") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty)
                        && !insert_generated_nonbreaking_default(builder, node, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    validate_tag(builder, node, "Mt", &mut deferred.post_validation);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ot") => {
                    // `Ot` is an obsolete spelling of `Ft`: the diagnostic
                    // retains the authored name, while public AST consumers
                    // receive the normalized contemporary macro.
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Ot",
                        location: builder.node_location(node),
                    });
                    let _ = builder.macro_name(node, "Ft");
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Fr") => {
                    // Unlike `Ot`, obsolete `Fr` retains its original public
                    // element identity after validation.
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Fr",
                        location: builder.node_location(node),
                    });
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("An") => {
                    validate_an(builder, node, outcome);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("At") => {
                    let siblings = validate_at(builder, node, spacing_enabled, max_nodes, outcome);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for sibling in siblings {
                        append_to_parent(builder, root, &mut root_children, active_body, sibling);
                    }
                }
                Some("St") => {
                    let Some(selector) = builder
                        .children(node)
                        .and_then(|children| children.first())
                        .copied()
                    else {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "St",
                            location: builder.node_location(node),
                        });
                        continue;
                    };
                    let Some(selector_text) = builder.node_text(selector).map(str::to_owned) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let Some(expanded) = standard_description(&selector_text) else {
                        // post_st() runs during the validator walk, after an
                        // empty later St has been diagnosed. Keep the error
                        // in that post-validation queue rather than exposing
                        // scanner source order as a compatibility difference.
                        deferred.post_validation.push(Recovery::UnknownStandard {
                            standard: selector_text.into(),
                            location: builder.node_location(selector),
                        });
                        continue;
                    };
                    if builder.node_count() >= max_nodes {
                        if outcome.node_limit_location.is_none() {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    }
                    let Some(expansion) = push_generated_text_at(
                        builder,
                        node,
                        expanded,
                        false,
                        builder.node_location(selector),
                    ) else {
                        if outcome.node_limit_location.is_none() {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    mark_no_print(builder, selector);
                    // `post_st()` inserts its source-less expansion ahead of
                    // the now-hidden authored selector in the public tree.
                    let _ = builder.replace_children(node, &[expansion, selector]);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Sx") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Sx",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ta")
                    if !scopes
                        .iter()
                        .rev()
                        .find(|frame| frame.close == "El")
                        .is_some_and(|list| {
                            builder.node_list_kind(list.body) == Some(NormalizedListKind::Column)
                        }) =>
                {
                    outcome.recoveries.push(Recovery::ColumnOutsideColumnList {
                        location: builder.node_location(node),
                    });
                }
                Some("Cd") => {
                    // Cd is an in-line callable macro with MDOC_JOIN: its
                    // ordinary direct arguments form one configuration
                    // phrase, while trailing punctuation remains outer flow.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        // Inline splitting may have detached leading closing
                        // punctuation and a later ordinary word from Cd. The
                        // empty element is still private syntax, but mandoc
                        // warns only when no non-delimiter flow remains.
                        let has_non_delimiter_follower = inline_events[event_index + 1..]
                            .iter()
                            .copied()
                            .any(|follower| {
                                builder
                                    .node_text(follower)
                                    .is_none_or(|text| !is_mdoc_closing_delimiter(text))
                            });
                        if !has_non_delimiter_follower {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "Cd",
                                location: builder.node_location(node),
                            });
                        }
                    } else {
                        coalesce_adjacent_text_children(builder, node);
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                    }
                }
                Some("In") => {
                    // `In` is a one-argument inline request.  Validation
                    // removes a truly empty request, while a populated final
                    // argument uses the normal no-break delimiter rule.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "In",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    validate_tag(builder, node, "In", &mut deferred.post_validation);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Xr") => {
                    let arguments = node_arguments(builder, node);
                    if arguments.is_empty() {
                        // `in_line_argn()` deletes a source-spelled empty
                        // cross reference. Any detached punctuation remains
                        // normal outer flow unless it was the sole closing
                        // delimiter owned by this empty request.
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Xr",
                            location: builder.node_location(node),
                        });
                        if let Some(delimiter) =
                            inline_events
                                .get(event_index + 1)
                                .copied()
                                .filter(|candidate| {
                                    builder
                                        .node_text(*candidate)
                                        .is_some_and(is_mdoc_closing_delimiter)
                                })
                        {
                            suppressed_inline_events.insert(delimiter);
                        }
                        continue;
                    }
                    if arguments.len() == 1 {
                        // post_xr() runs during the validator sweep, after
                        // later source-line empty requests have been seen.
                        // Keep this alongside delimiter styles to preserve
                        // the legacy document-order post-validation sequence.
                        deferred
                            .post_validation
                            .push(Recovery::MissingReferenceSection {
                                name: arguments[0].clone().into_boxed_str(),
                                location: builder.node_location(node),
                            });
                    }
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        "Xr",
                        &mut deferred.post_validation,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Lb") => {
                    let mut outer_delimiters = Vec::new();
                    if !validate_library(
                        builder,
                        node,
                        max_nodes,
                        outcome,
                        &mut deferred.post_validation,
                        &mut outer_delimiters,
                    ) {
                        continue;
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for delimiter in outer_delimiters {
                        append_to_parent(builder, root, &mut root_children, active_body, delimiter);
                    }
                }
                Some("Ex") => {
                    // `Ex -std` is semantic syntax rather than a renderer
                    // abbreviation: mdoc expands it into generated prose and
                    // generated Nm elements around the selected utilities.
                    // Keep non-standard invocations intact until the broader
                    // argument-validation family is implemented.
                    if !expand_standard_exit_status(builder, node, max_nodes, outcome)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Rv") => {
                    // `Rv -std` shares Ex's validated name-list grammar but
                    // expands into the standard return-value sentence.
                    if !expand_standard_return_value(builder, node, max_nodes, outcome)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Bx") => {
                    if !insert_generated_system_name(builder, node, "Bx", max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    // `mdoc_args()` retains an outer quote on a standalone
                    // delimiter. `append_delims()` preserves its delimiter
                    // role but does not mark it as an end of sentence (the
                    // Bx regression fixture uses `.Bx 4.4 "."`).
                    clear_quoted_bx_trailing_delimiter_sentence_end(
                        builder,
                        inline_events.get(event_index + 1).copied(),
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Db") => {
                    // `Db` remains a visible, end-of-line compatibility
                    // request; validation only marks each use obsolete.
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Db",
                        location: builder.node_location(node),
                    });
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some(name) if generated_system_name(name).is_some() => {
                    // These mdoc system-name macros have an AST-visible
                    // default word.  It must be allocated before the source
                    // punctuation is attached to the surrounding flow: the
                    // source parser gives Ux no arguments and gives the
                    // other variants at most their documented version/name
                    // prefix.
                    if !insert_generated_system_name(builder, node, name, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        system_macro_name(name),
                        &mut deferred.post_validation,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Pf") => {
                    let prior_instance = inline_events[..event_index].iter().any(|previous| {
                        builder.node_macro_name(*previous) == Some("Pf")
                            && builder.node_location(*previous) == builder.node_location(node)
                    });
                    if !prior_instance {
                        validate_prefix_following(
                            builder,
                            node,
                            &inline_events[event_index + 1..],
                            &mut deferred.post_validation,
                        );
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Pa") => {
                    // Like `.Mt`, an empty path macro has a semantic
                    // nonbreaking-space default. Delimiter splitting can
                    // leave the element empty before its punctuation is
                    // published into surrounding flow.
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty)
                        && !insert_generated_nonbreaking_default(builder, node, max_nodes)
                        && outcome.node_limit_location.is_none()
                    {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                    validate_no_break_trailing_delimiter(
                        builder,
                        node,
                        "Pa",
                        &mut deferred.post_validation,
                    );
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Tn") => {
                    if builder.children(node).is_none_or(<[NodeId]>::is_empty) {
                        outcome.recoveries.push(Recovery::EmptyMacro {
                            macro_name: "Tn",
                            location: builder.node_location(node),
                        });
                        continue;
                    }
                    deferred.post_validation.push(Recovery::UselessMacro {
                        macro_name: "Tn",
                        location: builder.node_location(node),
                    });
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Ud" | "Bt") => {
                    let (macro_name, generated_sentence) = match macro_name.as_deref() {
                        Some("Ud") => ("Ud", "currently under development."),
                        Some("Bt") => ("Bt", "is currently in beta test."),
                        _ => unreachable!("match arm fixes the compatibility macro spelling"),
                    };
                    outcome.recoveries.push(Recovery::UselessMacro {
                        macro_name,
                        location: builder.node_location(node),
                    });
                    let arguments = node_arguments(builder, node);
                    if let Some(first_argument) = arguments.first() {
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            // These obsolete macros discard their whole
                            // tail, while mandoc's diagnostic prints only the
                            // first argument as the representative spelling.
                            message: format!(
                                "skipping all arguments: {macro_name} {first_argument}"
                            )
                            .into(),
                            location: builder.node_location(node),
                        });
                    }
                    // These obsolete forms remain public Elements, but their
                    // complete visible effect is a generated sibling
                    // sentence.  Their authored argument nodes are private
                    // validator input and must not survive under the Element.
                    let _ = builder.replace_children(node, &[]);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if builder.node_count() >= max_nodes {
                        if outcome.node_limit_location.is_none() {
                            outcome.node_limit_location = builder.node_location(node);
                        }
                    } else if let Some(sentence) = push_generated_text_at(
                        builder,
                        active_body,
                        generated_sentence,
                        true,
                        builder.node_location(node),
                    ) {
                        // Root children are staged until the closing semantic
                        // pass; nested parents can retain the arena edge made
                        // by `push_generated_text_at` directly.
                        if active_body == root {
                            root_children.push(sentence);
                        }
                    } else if outcome.node_limit_location.is_none() {
                        outcome.node_limit_location = builder.node_location(node);
                    }
                }
                Some(
                    "Cm" | "Dv" | "Em" | "Er" | "Ev" | "Ic" | "Li" | "Ms" | "No" | "Sy" | "Va",
                ) => {
                    let is_empty = builder.children(node).is_none_or(<[NodeId]>::is_empty);
                    if builder.node_macro_name(node) == Some("Cm") && is_empty {
                        // `post_tag()` removes an empty Cm. A leading
                        // delimiter can reopen the same source request; that
                        // populated successor is the sole non-warning form.
                        if tag_empty_macro_requires_warning(
                            builder,
                            "Cm",
                            &inline_events[event_index + 1..],
                        ) {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "Cm",
                                location: builder.node_location(node),
                            });
                        }
                        continue;
                    }
                    if builder.node_macro_name(node) == Some("No") && is_empty {
                        // No keeps its empty compatibility Element in the
                        // public tree, but post-validation still reports a
                        // source-spelled empty request. A leading delimiter
                        // at the start of a request is private only when a
                        // populated No restart follows it; inline and
                        // isolated forms remain warnings.
                        let line_start = builder
                            .node_flags(node)
                            .is_some_and(|flags| flags.line_start);
                        let explicit_inline = builder
                            .node_source_position(node)
                            .is_some_and(|position| position.column > 2);
                        let reopened_by_later_name = line_start
                            && !tag_empty_macro_requires_warning(
                                builder,
                                "No",
                                &inline_events[event_index + 1..],
                            );
                        if explicit_inline || (line_start && !reopened_by_later_name) {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name: "No",
                                // Inline event splitting retains the logical
                                // start of the reclassified source spelling.
                                // Preserve that span directly: libmandoc
                                // reports the `N` in the inner `No`.
                                location: builder.node_location(node),
                            });
                        }
                        // `post_tag()` removes every empty `No` request. A
                        // delimiter that had been its only quoted argument
                        // remains in the surrounding flow, and a leading
                        // delimiter can reopen a later visible `No`.
                        continue;
                    }
                    if is_empty
                        && let Some(macro_name) = empty_tag_macro_name(macro_name.as_deref())
                    {
                        // The generic tag-style inline macros have no public
                        // zero-argument form.  libmandoc removes the element
                        // in post-validation, leaving only its warning.
                        let line_start = builder
                            .node_flags(node)
                            .is_some_and(|flags| flags.line_start);
                        let explicit_inline = builder
                            .node_source_position(node)
                            .is_some_and(|position| position.column > 2);
                        let preceded_by_opening_delimiter = macro_name == "Em"
                            && inline_events[..event_index]
                                .last()
                                .and_then(|previous| builder.node_text(*previous))
                                .is_some_and(|text| matches!(text, "(" | "["));
                        // A source-spelled inline tag macro is distinguishable
                        // from an internal empty element synthesized by
                        // delimiter splitting: it has its own later source
                        // column. It remains a warning even when its
                        // following delimiter is retained.
                        let report_empty = if macro_name == "Em" {
                            !preceded_by_opening_delimiter
                                && (explicit_inline
                                    || (line_start
                                        && tag_empty_macro_requires_warning(
                                            builder,
                                            macro_name,
                                            &inline_events[event_index + 1..],
                                        )))
                        } else {
                            // `in_line()` emits the empty-macro finding when
                            // the source request produced no element, even
                            // though `append_delims()` may have published one
                            // or more trailing punctuation nodes after it.
                            // Only a delimiter-separated, populated restart
                            // of the same macro makes this first element a
                            // private parser transient.
                            explicit_inline
                                || (line_start
                                    && tag_empty_macro_requires_warning(
                                        builder,
                                        macro_name,
                                        &inline_events[event_index + 1..],
                                    ))
                        };
                        if report_empty {
                            outcome.recoveries.push(Recovery::EmptyMacro {
                                macro_name,
                                location: builder.node_location(node),
                            });
                        }
                        // An empty tag element is always parser-private. A
                        // delimiter can leave it silent when a populated
                        // restart follows, while a true empty source request
                        // contributes the warning above; neither form owns a
                        // public AST node.
                        continue;
                    }
                    if let Some(macro_name) = tag_macro_name(macro_name.as_deref()) {
                        // libmandoc emits delimiter style findings during its
                        // post-validation sweep, after later empty-macro
                        // recoveries in the same document have been seen.
                        validate_tag(builder, node, macro_name, &mut deferred.post_validation);
                    }
                    if let Some(tag) = pending_transparent_permalink.take() {
                        mark_permalink(builder, node, Some(&tag));
                    } else if let Some((tag_node, tag)) = pending_manual_tag.take() {
                        if tag.is_empty() {
                            if builder
                                .children(node)
                                .and_then(|children| children.first())
                                .and_then(|child| builder.node_text(*child))
                                .is_some_and(|text| !text.is_empty())
                            {
                                // The tag text is this node's own child, so
                                // `tag_put()` sets NODE_ID without allocating
                                // a redundant public tag string.
                                mark_target(builder, node, None);
                                mark_no_print(builder, tag_node);
                            }
                        } else {
                            mark_target(builder, node, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if let Some((tail, location)) = inline_column_ta_tail.take()
                        && let Some(item) = active_column_item(builder, active_body)
                        && let Some(body) = append_column_ta_cell(
                            builder,
                            active_body,
                            location,
                            &tail,
                            spacing_enabled,
                            max_nodes,
                            outcome,
                            &mut scopes,
                        )
                    {
                        extend_pending_short_column_item(&mut pending_short_column_items, item);
                        active_body = body;
                        flow_parent = body;
                    }
                }
                Some("Es") => {
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "Es",
                        location: builder.node_location(node),
                    });
                    let children = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let values = children
                        .iter()
                        .filter_map(|child| builder.node_text(*child))
                        .map(str::to_owned)
                        .collect::<Vec<_>>();
                    enclosure = values.first().map(|opening| NormalizedEnclosure {
                        opening: opening.clone().into_boxed_str(),
                        closing: values
                            .get(1)
                            .map(|closing| closing.clone().into_boxed_str()),
                    });
                    // Es accepts only the opening/closing delimiter pair.
                    // Later words resume normal source flow instead of
                    // becoming hidden Es arguments.
                    let kept = children.len().min(2);
                    let siblings = split_mdoc_inline_tokens(
                        builder,
                        node,
                        &children[kept..],
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let _ = builder.replace_children(node, &children[..kept]);
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    for sibling in siblings {
                        append_to_parent(builder, root, &mut root_children, active_body, sibling);
                    }
                }
                Some("En") => {
                    outcome.recoveries.push(Recovery::Obsolete {
                        macro_name: "En",
                        location: builder.node_location(node),
                    });
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "En",
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        outcome,
                    ) else {
                        let _ = builder.set_node_enclosure(node, enclosure.clone());
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    move_leading_open_delimiter(builder, node, head, body);
                    coalesce_adjacent_text_children(builder, body);
                    for part in [node, head, body] {
                        let _ = builder.set_node_enclosure(part, enclosure.clone());
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Bl") => {
                    let attributes = list_attributes(builder, node, &mut deferred.post_validation);
                    if !attributes.compact
                        && let Some(previous) = discard_previous_paragraph_control(
                            builder,
                            root,
                            &mut root_children,
                            flow_parent,
                        )
                    {
                        let macro_name = match builder.node_macro_name(previous) {
                            Some("Pp") => "Pp",
                            Some("br") => "br",
                            _ => unreachable!(
                                "the paragraph-control predicate checked the macro name"
                            ),
                        };
                        deferred.post_validation.push(Recovery::ParagraphBoundary {
                            macro_name,
                            placement: "before",
                            blocker: "Bl",
                            location: builder.node_location(previous),
                        });
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bl",
                        ArgumentPlacement::Drop,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    apply_attributes(builder, &[node, head, body], &attributes);
                    list_types.insert(body, attributes.list_type);
                    if let Some(column_count) = attributes.column_count {
                        column_counts.insert(body, column_count);
                    }
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // `post_tg()` transfers an explicit tag before a
                        // list to its Body.  In particular, a column list
                        // has no independent first visible node that could
                        // own a paragraph-style permalink.
                        mark_manual_target(builder, body, &tag);
                        mark_no_print(builder, tag_node);
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close: "El",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("It") => {
                    if let Some(list_index) = scopes.iter().rposition(|frame| frame.close == "El")
                        && scopes
                            .get(list_index + 1)
                            .is_some_and(|frame| matches!(frame.close, "Ac" | "Ed"))
                    {
                        // A new item is a full structural boundary.  It
                        // closes an outstanding explicit delimiter or display
                        // scope inside the list before opening the next row;
                        // waiting for `.El` would instead misclassify this as
                        // list-on-block bad nesting.
                        let list = scopes[list_index];
                        let list_body = list.body;
                        deferred.list_content.extend(move_initial_list_content_out(
                            builder,
                            root,
                            &mut root_children,
                            list,
                        ));
                        let interrupted = scopes.split_off(list_index + 1);
                        for frame in interrupted.iter().rev() {
                            outcome.recoveries.push(Recovery::BrokenBlock {
                                breaker: "It",
                                macro_name: open_name(frame.close),
                                location: builder.node_location(node),
                            });
                            implicitly_closed.push(frame.close);
                        }
                        flow_parent = list_body;
                        active_body = list_body;
                    }
                    let Some(list) = scopes
                        .iter()
                        .rev()
                        .find(|frame| frame.close == "El")
                        .copied()
                    else {
                        let arguments = node_arguments(builder, node).join(" ");
                        outcome.recoveries.push(Recovery::ItemOutsideList {
                            arguments: arguments.into_boxed_str(),
                            location: builder.node_location(node),
                        });
                        let _ = builder.macro_name(node, "br");
                        let _ = builder.replace_children(node, &[]);
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    // `post_bl()` moves every direct prefix out of a list
                    // before its first item, including a nested block.  The
                    // nested block remains structurally intact; only its
                    // ownership changes to the surrounding flow.
                    outcome.recoveries.extend(move_initial_list_content_out(
                        builder,
                        root,
                        &mut root_children,
                        list,
                    ));
                    let list_body = list.body;
                    let list_is_innermost = scopes
                        .iter()
                        .rposition(|frame| frame.close == "El")
                        .is_some_and(|index| index + 1 == scopes.len());
                    let column_count = column_counts.get(&list_body).copied();
                    if column_count.is_some() {
                        finalize_last_empty_column_item(
                            builder,
                            list_body,
                            &mut pending_empty_column_items,
                            outcome,
                        );
                        finalize_short_column_items(
                            builder,
                            list_body,
                            &mut pending_short_column_items,
                            outcome,
                        );
                    }
                    if list_is_innermost
                        && let Some(list_type) = list_types.get(&list_body).copied()
                        && fixed_head_list_type(list_type)
                    {
                        finalize_last_fixed_head_list_item(
                            builder,
                            list_body,
                            list_type,
                            &deferred_fixed_head_argument_items,
                            outcome,
                        );
                    }
                    if builder.node_list_kind(list_body) != Some(NormalizedListKind::Column) {
                        let arguments = builder
                            .children(node)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default();
                        if arguments
                            .first()
                            .and_then(|argument| builder.node_text(*argument))
                            == Some("Ta")
                        {
                            let ta_location = arguments
                                .first()
                                .and_then(|argument| builder.node_location(*argument));
                            let retained = &arguments[1..];
                            let retained_text = retained
                                .iter()
                                .filter_map(|argument| builder.node_text(*argument))
                                .collect::<Vec<_>>()
                                .join(" ");
                            let _ = builder.replace_children(node, retained);
                            outcome.recoveries.push(Recovery::ColumnOutsideColumnList {
                                location: ta_location,
                            });
                            deferred.post_validation.push(Recovery::InvalidArguments {
                                message: format!("skipping all arguments: It {retained_text}")
                                    .into(),
                                location: builder.node_location(node),
                            });
                            if list_types
                                .get(&list_body)
                                .is_some_and(|list_type| fixed_head_list_type(list_type))
                            {
                                deferred_fixed_head_argument_items.insert(node);
                            }
                        }
                    }
                    let diag_list = list_types.get(&list_body) == Some(&"diag");
                    let opens_xo = !diag_list
                        && matches!(node_arguments(builder, node).as_slice(), [value] if value == "Xo");
                    let empty_column_item = column_count.is_some()
                        && !opens_xo
                        && builder.children(node).is_none_or(<[NodeId]>::is_empty);
                    if empty_column_item {
                        pending_empty_column_items.insert(node);
                    }
                    let column_cell_count = column_count
                        .filter(|_| !opens_xo)
                        .map(|_| column_item_cell_count(builder, node));
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "It",
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if matches!(
                        list_types.get(&list_body),
                        Some(&"hang" | &"ohang" | &"inset" | &"diag" | &"tag")
                    ) && builder.children(head).is_none_or(<[NodeId]>::is_empty)
                    {
                        outcome.recoveries.push(Recovery::EmptyListItemHead {
                            list_type: list_types
                                .get(&list_body)
                                .copied()
                                .expect("matched list type must exist"),
                            location: builder.node_location(node),
                        });
                    }
                    let column_list =
                        builder.node_list_kind(list_body) == Some(NormalizedListKind::Column);
                    if column_list
                        && !opens_xo
                        && let Some(column_cell_count) = column_cell_count
                        && let Some(bodies) = split_column_item_cells(
                            builder,
                            node,
                            head,
                            body,
                            spacing_enabled,
                            max_nodes,
                            outcome,
                            &mut scopes,
                        )
                    {
                        if let Some((tag_node, tag)) = pending_manual_tag.take()
                            && !tag.is_empty()
                        {
                            // `post_tg()` leaves an explicit tag before the
                            // next column row on the It block itself.
                            mark_target(builder, node, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                        let _ = builder.append_existing_child(list_body, node);
                        if let Some(columns) = column_count.filter(|_| !empty_column_item) {
                            let cells = column_cell_count;
                            if cells < columns {
                                pending_short_column_items.insert(node, (columns, cells));
                            } else if cells > columns.saturating_add(1) {
                                outcome.recoveries.push(Recovery::WrongNumberOfColumnCells {
                                    columns,
                                    cells,
                                    location: builder.node_location(node),
                                });
                            }
                        }
                        let cell = *bodies.last().expect("column items have one body");
                        if let Some(scope) = scopes
                            .last()
                            .copied()
                            .filter(|scope| scope.resume_active == cell)
                        {
                            // A partial explicit opener occurred inside this
                            // column cell. Until its ordinary closer arrives,
                            // physical follow-up input belongs to that nested
                            // Body rather than the cell itself.
                            flow_parent = scope.body;
                            active_body = scope.body;
                        } else {
                            flow_parent = cell;
                            active_body = cell;
                        }
                        continue;
                    }
                    // `Bl -diag` owns its item header as literal prose.  In
                    // particular, `Nx`, `Fl`, and an authored `Xo` spelling
                    // must not enter the callable-macro or partial-block
                    // paths that definition-list terms use.
                    let parsed_head = if diag_list {
                        builder
                            .children(head)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default()
                    } else {
                        let parsed = split_mdoc_inline_children(
                            builder,
                            head,
                            spacing_enabled,
                            max_nodes,
                            outcome,
                        );
                        collapse_long_option_prefixes(builder, &parsed)
                    };
                    let _ = builder.replace_children(head, &parsed_head);
                    // Definition-list terms use the same parsed mdoc
                    // argument grammar as ordinary flow.  In particular an
                    // implicit partial such as `.It Bq Er ENOENT` is a
                    // nested Block/Head/Body before the later tag pass sees
                    // the public term tree; leaving it as an Element loses
                    // both that structure and the nested callable macro.
                    structure_nested_implicit_partial_blocks(
                        builder,
                        head,
                        max_nodes,
                        outcome,
                        spacing_enabled,
                    );
                    if opens_xo {
                        if let Some(mut flags) = builder.node_flags(body) {
                            // `mdoc_macro.c` opens the item body while the
                            // `.It Xo` control line is still active.
                            flags.line_start = true;
                            let _ = builder.set_node_flags(body, flags);
                        }
                        let location = parsed_head
                            .first()
                            .and_then(|opening| builder.node_location(*opening));
                        let Some((xo, _, xo_body)) =
                            make_synthetic_block(builder, head, "Xo", location, max_nodes, outcome)
                        else {
                            let _ = builder.append_existing_child(list_body, node);
                            flow_parent = body;
                            active_body = body;
                            continue;
                        };
                        let _ = builder.replace_children(head, &[xo]);
                        let _ = builder.append_existing_child(list_body, node);
                        scopes.push(ScopeFrame {
                            close: "Xc",
                            open: xo,
                            body: xo_body,
                            tail_on_close: false,
                            transparent_target_taken: false,
                            suppress_implicit_ancestor_break: false,
                            resume_active: body,
                            resume_flow: body,
                        });
                        flow_parent = xo_body;
                        active_body = xo_body;
                    } else {
                        if let Some((tag_node, tag)) = pending_manual_tag.take()
                            && !tag.is_empty()
                        {
                            // `post_tg()` chooses a definition-list term but
                            // the content body for ordinary list rows.  The
                            // source-order pass knows that same list kind
                            // before following physical text is attached.
                            let target = if builder.node_list_kind(list_body)
                                == Some(NormalizedListKind::Definition)
                            {
                                head
                            } else {
                                body
                            };
                            mark_target(builder, target, Some(&tag));
                            mark_no_print(builder, tag_node);
                        }
                        mark_definition_item_head_targets(builder, list_body, head, &parsed_head);
                        // A non-column list item head is one semantic phrase
                        // in mandoc's public tree.  The scanner keeps words
                        // separate for roff execution, but those adjacent
                        // plain tokens have no remaining structural meaning.
                        // Column lists are different: their item arguments
                        // delimit cells and must remain independently owned.
                        if !column_list {
                            coalesce_adjacent_text_children(builder, head);
                        }
                        let _ = builder.append_existing_child(list_body, node);
                        if let Some(nested_scope) = structure_item_head_explicit_partial(
                            builder, head, body, max_nodes, outcome,
                        ) {
                            flow_parent = nested_scope.body;
                            active_body = nested_scope.body;
                            scopes.push(nested_scope);
                        } else {
                            flow_parent = body;
                            active_body = body;
                        }
                    }
                }
                Some("Bd") => {
                    if scopes
                        .iter()
                        .any(|frame| builder.node_macro_name(frame.open) == Some("Bd"))
                    {
                        outcome.recoveries.push(Recovery::NestedDisplay {
                            location: builder.node_location(node),
                        });
                    }
                    if builder.children(node).is_some_and(<[NodeId]>::is_empty) {
                        // mandoc deletes a completely argument-less display
                        // and relinks its Body into the surrounding flow.  Its
                        // matching closer remains syntactically consumed.
                        deferred
                            .post_validation
                            .push(Recovery::DisplayWithoutArguments {
                                location: builder.node_location(node),
                            });
                        implicitly_closed.push("Ed");
                        continue;
                    }
                    let mut immediate_display_recoveries = Vec::new();
                    let attributes = display_attributes(
                        builder,
                        node,
                        &mut immediate_display_recoveries,
                        &mut deferred.post_validation,
                    );
                    outcome.recoveries.extend(immediate_display_recoveries);
                    if !attributes.compact
                        && let Some(previous) = discard_previous_paragraph_control(
                            builder,
                            root,
                            &mut root_children,
                            flow_parent,
                        )
                    {
                        let macro_name = match builder.node_macro_name(previous) {
                            Some("Pp") => "Pp",
                            Some("br") => "br",
                            _ => unreachable!(
                                "the paragraph-control predicate checked the macro name"
                            ),
                        };
                        outcome.recoveries.push(Recovery::ParagraphBoundary {
                            macro_name,
                            placement: "before",
                            blocker: "Bd",
                            location: builder.node_location(previous),
                        });
                    }
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bd",
                        ArgumentPlacement::Drop,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    apply_attributes(builder, &[node, head, body], &attributes);
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // `post_tg()` attaches an explicit manual tag before
                        // a display to its body; the following visible text
                        // receives the matching permalink below through the
                        // same source-order path as a tagged paragraph.
                        mark_manual_target(builder, body, &tag);
                        mark_no_print(builder, tag_node);
                        pending_paragraph_href = Some(tag);
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close: "Ed",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("D1" | "Dl") => {
                    // D1 and Dl are one-line implicit display blocks.  They
                    // have the same observable Block/empty Head/Body shape
                    // as a multi-line Bd, but their body is completed from
                    // this request's argument phrases rather than a later
                    // `.Ed` scope.
                    let name = macro_name.as_deref().expect("matched display macro");
                    let Some((_head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::BodyTokens,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let children = split_mdoc_inline_children(
                        builder,
                        body,
                        spacing_enabled,
                        max_nodes,
                        outcome,
                    );
                    let _ = builder.replace_children(body, &children);
                    coalesce_mdoc_display_phrases(builder, body);
                    if let Some((tag_node, tag)) = pending_manual_tag.take()
                        && !tag.is_empty()
                    {
                        // Unlike Bd, this display's visible body is supplied
                        // on the same control line.  Transfer the matching
                        // permalink immediately instead of waiting for the
                        // next source event.
                        mark_manual_target(builder, body, &tag);
                        mark_first_visible_permalink(builder, body, &tag);
                        mark_no_print(builder, tag_node);
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                }
                Some("Bf") => {
                    let font_arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let attributes = font_attributes(builder, node, &mut deferred.post_validation);
                    let uses_option_form = font_arguments
                        .first()
                        .and_then(|argument| builder.node_text(*argument))
                        .is_some_and(is_bf_option);
                    let option_tail = uses_option_form.then(|| {
                        font_arguments[1..]
                            .iter()
                            .copied()
                            .filter(|argument| {
                                !builder.node_text(*argument).is_some_and(is_bf_option)
                            })
                            .collect::<Vec<_>>()
                    });
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bf",
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if uses_option_form {
                        let _ = builder.replace_children(head, &option_tail.unwrap_or_default());
                    }
                    apply_attributes(builder, &[node, head, body], &attributes);
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close: "Ef",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("Bk") => {
                    let arguments = builder
                        .children(node)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    let invalid_index = arguments
                        .iter()
                        .position(|argument| builder.node_text(*argument) != Some("-words"));
                    if let Some(argument) = invalid_index.and_then(|index| arguments.get(index)) {
                        outcome.recoveries.push(Recovery::InvalidArguments {
                            message: format!(
                                "skipping excess arguments: Bk ... {}",
                                builder.node_text(*argument).unwrap_or_default()
                            )
                            .into(),
                            location: builder.node_location(*argument),
                        });
                    }
                    let retained_head = invalid_index.map_or_else(Vec::new, |index| {
                        arguments[index.saturating_add(1)..]
                            .iter()
                            .copied()
                            .filter(|argument| {
                                !builder
                                    .node_text(*argument)
                                    .is_some_and(|value| value.starts_with('-'))
                            })
                            .collect::<Vec<_>>()
                    });
                    // Bk is a full explicit block whose `-words` control
                    // argument is validator-only.  The public tree exposes
                    // an empty Head and keeps all following source flow in
                    // Body until Ek consumes the scope.
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        "Bk",
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    let _ = builder.replace_children(head, &retained_head);
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    synopsis_keep_boundary = in_synopsis && synopsis_name_body.is_some();
                    scopes.push(ScopeFrame {
                        close: "Ek",
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some("Fo" | "Rs") => {
                    let name = macro_name.as_deref().expect("matched mdoc block macro");
                    let close = if name == "Fo" { "Fc" } else { "Re" };
                    let Some((head, body)) = make_block(
                        builder,
                        node,
                        name,
                        ArgumentPlacement::Head,
                        max_nodes,
                        outcome,
                    ) else {
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                        continue;
                    };
                    if name == "Rs" {
                        // A reference list has no public Head arguments.  The
                        // validator reports only the leading selector (which
                        // may itself be an inline macro), then discards the
                        // entire scanner argument subtree before publication.
                        if let Some((tag_node, _)) = pending_manual_tag.take() {
                            // `post_tg()` keeps a preceding transparent tag
                            // as its own destination when the following full
                            // block is an Rs reference list.
                            mark_destination(builder, tag_node);
                        }
                        if let Some(argument) = node_arguments(builder, head).first() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!("skipping all arguments: Rs {argument}").into(),
                                location: argument_location(builder, head, 0),
                            });
                            let _ = builder.replace_children(head, &[]);
                        }
                    } else if name == "Fo" {
                        let arguments = builder
                            .children(head)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default();
                        let has_excess_arguments = arguments.len() > 1;
                        if arguments.is_empty() {
                            outcome.recoveries.push(Recovery::MissingFunctionName {
                                location: builder.node_location(node),
                            });
                        } else if let Some(first) = arguments.first().copied()
                            && let Some(excess) = arguments.get(1).copied()
                        {
                            deferred.post_validation.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping excess arguments: Fo ... {}",
                                    builder.node_text(excess).unwrap_or_default()
                                )
                                .into(),
                                location: builder.node_location(excess),
                            });
                            let _ = builder.replace_children(head, &[first]);
                        }
                        if !arguments.is_empty() && !has_excess_arguments {
                            validate_function_name(builder, head, &mut outcome.recoveries);
                        }
                    }
                    if in_synopsis {
                        mark_synopsis_pretty(builder, head);
                        mark_synopsis_pretty(builder, body);
                    }
                    if name == "Fo" && !in_synopsis {
                        if let Some((tag_node, tag)) = pending_manual_tag.take() {
                            mark_target(builder, head, Some(&tag));
                            mark_no_print(builder, tag_node);
                        } else if let Some(function_tag) = node_arguments(builder, head)
                            .first()
                            .and_then(|name| automatic_mdoc_function_tag(name))
                        {
                            let paragraph = pending_function_paragraph.take();
                            automatic_function_targets.push(AutomaticFunctionTarget {
                                destination: paragraph.unwrap_or(head),
                                permalink: paragraph.map(|_| head),
                                tag: function_tag.to_owned(),
                                priority: function_tag_priority,
                                exposes_tag: paragraph.is_none(),
                            });
                            function_tag_priority = function_tag_priority.saturating_add(1);
                        }
                    }
                    append_to_parent(builder, root, &mut root_children, flow_parent, node);
                    scopes.push(ScopeFrame {
                        close,
                        open: node,
                        body,
                        tail_on_close: false,
                        transparent_target_taken: false,
                        suppress_implicit_ancestor_break: false,
                        resume_active: active_body,
                        resume_flow: flow_parent,
                    });
                    flow_parent = body;
                    active_body = body;
                }
                Some(
                    "Ac" | "Bc" | "Brc" | "Dc" | "Ec" | "Ek" | "El" | "Ed" | "Ef" | "Fc" | "Oc"
                    | "Pc" | "Qc" | "Re" | "Sc" | "Xc",
                ) => {
                    let close = macro_name.as_deref().expect("matched mdoc closer");
                    if close == "Ed" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: Ed {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if close == "Ef" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: Ef {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if close == "El" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: El {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if close == "Ek" {
                        let arguments = node_arguments(builder, node);
                        if !arguments.is_empty() {
                            outcome.recoveries.push(Recovery::InvalidArguments {
                                message: format!(
                                    "skipping all arguments: Ek {}",
                                    arguments.join(" ")
                                )
                                .into(),
                                location: builder.node_location(node),
                            });
                        }
                    }
                    if is_explicit_partial_close(close) {
                        let children = builder
                            .children(node)
                            .map(<[NodeId]>::to_vec)
                            .unwrap_or_default();
                        // Eo is exceptional among explicit partial blocks:
                        // an outer ordinary partial closer crossing its
                        // still-open Tail-owning scope is recoverable, but
                        // not ordinary nesting.  mandoc reports the authored
                        // close (`.Bo … .Eo … .Bc`) while retaining Eo's
                        // pending scope.  Other partial-pair repairs have
                        // distinct broken-body rules and are handled by their
                        // dedicated paths below.
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && let Some(interrupted) = scopes[index + 1..]
                                .iter()
                                .copied()
                                .find(|frame| frame.tail_on_close)
                        {
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                        }
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && scopes[index + 1..].iter().any(|frame| frame.tail_on_close)
                        {
                            // An outer ordinary partial closer crossing Eo
                            // does not close Eo.  Its closer becomes an empty
                            // Body inside Eo's active Body; Eo remains live
                            // until Ec supplies its real Tail.
                            let frame = scopes[index];
                            let _ = append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                outcome,
                            );
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the crossed Eo scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            continue;
                        }
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && let Some(interrupted) = scopes[index + 1..]
                                .iter()
                                .copied()
                                .find(|frame| frame.close == "Ek")
                        {
                            // A word-keep block is validation-closed by an
                            // outer explicit partial closer, but unlike a
                            // display/list it retains its existing public
                            // Body topology until the authored `.Ek` arrives.
                            // Only the crossed-block recovery is observable.
                            let frame = scopes[index];
                            let _ = append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                        }
                        let close_is_open = scopes.iter().any(|frame| frame.close == close)
                            || implicitly_closed.contains(&close);
                        let has_other_open_partial = scopes.iter().any(|frame| {
                            is_explicit_partial_close(frame.close) || frame.close == "Xc"
                        });
                        let reports_not_open = !close_is_open
                            && has_other_open_partial
                            && implicitly_closed.is_empty();
                        if reports_not_open {
                            // A bare partial closer remains inert, but one
                            // that conflicts with an active explicit partial
                            // must surface mandoc's not-open recovery without
                            // disturbing the still-active scope.
                            outcome.recoveries.push(Recovery::UnmatchedClose {
                                macro_name: close_name(close),
                                location: builder.node_location(node),
                            });
                        }
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && let Some(interrupted) =
                                scopes[index + 1..].iter().rev().copied().find(|inner| {
                                    is_explicit_partial_scope(inner)
                                        || matches!(inner.close, "Ed" | "Ef" | "El")
                                })
                        {
                            let frame = scopes[index];
                            let crossed_partial = is_explicit_partial_scope(&interrupted);
                            if crossed_partial {
                                coalesce_adjacent_text_children(builder, active_body);
                            }
                            if !interrupted.suppress_implicit_ancestor_break {
                                // A closer can cross an explicit child that
                                // itself sits inside an implicit partial
                                // parent.  libmandoc reports that parent at
                                // its authored opener before reporting the
                                // immediately crossed explicit scope (for
                                // example `.Aq … Bo … Bro` followed by
                                // `.Bc`).  The public tree is already
                                // complete; this is a distinct recovery edge.
                                for implicit in
                                    implicit_partial_ancestor_blocks(builder, interrupted.open)
                                {
                                    let Some(name) = builder.node_macro_name(implicit) else {
                                        continue;
                                    };
                                    let breaker = implicit_partial_block_name(name);
                                    let _ = append_broken_implicit_block_body(
                                        builder,
                                        active_body,
                                        implicit,
                                        max_nodes,
                                        outcome,
                                    );
                                    outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                        breaker,
                                        interrupted: open_name(interrupted.close),
                                        location: builder.node_location(implicit),
                                    });
                                }
                            }
                            append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the interrupted scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            append_explicit_partial_tail(
                                builder,
                                root,
                                &mut root_children,
                                &mut scopes,
                                &mut implicitly_closed,
                                &mut active_body,
                                &mut flow_parent,
                                node,
                                &children,
                                !crossed_partial,
                                spacing_enabled,
                                max_nodes,
                                outcome,
                            );
                            continue;
                        }
                        let crossed_parent_body = scopes
                            .last()
                            .filter(|frame| frame.close == close)
                            .and_then(|frame| builder.node_parent(frame.open))
                            .filter(|parent| {
                                matches!(
                                    builder.node_kind(*parent),
                                    Some(NodeKind::Body | NodeKind::Head)
                                )
                            });
                        if let Some(index) = scopes.iter().rposition(|frame| frame.close == close)
                            && index + 1 == scopes.len()
                        {
                            let frame = scopes[index];
                            if !frame.suppress_implicit_ancestor_break {
                                let implicit_ancestors =
                                    implicit_partial_ancestor_blocks(builder, frame.open);
                                let trailing_text =
                                    take_trailing_line_start_text_children(builder, active_body);
                                for implicit in implicit_ancestors {
                                    let Some(name) = builder.node_macro_name(implicit) else {
                                        continue;
                                    };
                                    let breaker = implicit_partial_block_name(name);
                                    let _ = append_broken_implicit_block_body(
                                        builder,
                                        active_body,
                                        implicit,
                                        max_nodes,
                                        outcome,
                                    );
                                    outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                        breaker,
                                        interrupted: open_name(close),
                                        location: builder.node_location(implicit),
                                    });
                                }
                                for text in trailing_text {
                                    let _ = builder.append_existing_child(active_body, text);
                                }
                            }
                        }
                        close_explicit_partial_scope(
                            &mut scopes,
                            &mut implicitly_closed,
                            &mut active_body,
                            &mut flow_parent,
                            close,
                        );
                        if let Some(parent) =
                            crossed_parent_body.filter(|parent| *parent != active_body)
                        {
                            if !children.is_empty()
                                && builder.node_kind(parent) == Some(NodeKind::Head)
                                && let Some(mut flags) = builder.node_flags(active_body)
                            {
                                // The following item body no longer begins at
                                // the extended `.It` header once its partial
                                // closer supplied a head-owned tail.
                                flags.line_start = false;
                                let _ = builder.set_node_flags(active_body, flags);
                            }
                            // A crossed outer partial is no longer on the
                            // active scope stack, but the tail of its child's
                            // authored closer remains in that structural
                            // parent (`Ao … Bo … Ac … Bc tail`, or an `.It`
                            // header).  Do not retain it as general flow
                            // after the tail unless that tail itself opens
                            // another cross-line partial.
                            let scope_count = scopes.len();
                            let mut tail_active = parent;
                            let mut tail_flow = parent;
                            append_explicit_partial_tail(
                                builder,
                                root,
                                &mut root_children,
                                &mut scopes,
                                &mut implicitly_closed,
                                &mut tail_active,
                                &mut tail_flow,
                                node,
                                &children,
                                true,
                                spacing_enabled,
                                max_nodes,
                                outcome,
                            );
                            if scopes.len() > scope_count {
                                // The tail was attached to an already
                                // crossed parent only for its local AST
                                // ownership.  A scope opened by that tail
                                // must resume the ordinary parser flow after
                                // its own closer, rather than trapping later
                                // physical lines in the historical parent.
                                for scope in &mut scopes[scope_count..] {
                                    scope.suppress_implicit_ancestor_break = true;
                                    scope.resume_active = active_body;
                                    scope.resume_flow = flow_parent;
                                }
                                active_body = tail_active;
                                flow_parent = tail_flow;
                            }
                        } else {
                            append_explicit_partial_tail(
                                builder,
                                root,
                                &mut root_children,
                                &mut scopes,
                                &mut implicitly_closed,
                                &mut active_body,
                                &mut flow_parent,
                                node,
                                &children,
                                true,
                                spacing_enabled,
                                max_nodes,
                                outcome,
                            );
                        }
                        if reports_not_open && builder.node_macro_name(active_body) == Some("Bo") {
                            // The skipped closer's ordinary tail remains in
                            // the active bracket body and extends its one
                            // semantic phrase across the control-line
                            // boundary (`.Bo bo` followed by `.Pc bc`).
                            coalesce_adjacent_text_children(builder, active_body);
                        }
                        continue;
                    }
                    if let Some(index) = scopes.iter().rposition(|frame| frame.close == close) {
                        // mdoc permits a list/display closer to break a nested
                        // compatible block.  This is not a malformed stack: the
                        // matching frame resumes the outer flow, and the popped
                        // inner frames are validation-closed by that request.
                        let frame = scopes[index];
                        if close == "Re" {
                            normalize_reference_field_order(builder, frame.body);
                        }
                        // `.Ec` is the sole explicit partial closer that is
                        // not in `is_explicit_partial_close()`: Eo owns a
                        // closer-created Tail.  Its close still diagnoses an
                        // intervening explicit partial block exactly like the
                        // ordinary Ac/Bc/… family does.
                        if frame.tail_on_close
                            && let Some(interrupted) = scopes[index + 1..]
                                .iter()
                                .copied()
                                .find(is_explicit_partial_scope)
                        {
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                        }
                        if frame.tail_on_close
                            && scopes[index + 1..].iter().any(is_explicit_partial_scope)
                        {
                            // Ec crossing an inner ordinary partial block is
                            // represented by a closer-owned Eo Body *inside*
                            // that block. The inner scope keeps the following
                            // source flow through its own closer; the crossed
                            // Eo frame is consumed without a Tail child.
                            let tail_remainder = append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                outcome,
                            )
                            .map(|body| {
                                complete_explicit_tail(
                                    builder,
                                    body,
                                    node,
                                    spacing_enabled,
                                    max_nodes,
                                    outcome,
                                )
                            })
                            .unwrap_or_default();
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the crossed partial scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            for remainder in tail_remainder {
                                append_to_parent(
                                    builder,
                                    root,
                                    &mut root_children,
                                    active_body,
                                    remainder,
                                );
                            }
                            continue;
                        }
                        if close == "El" && index + 1 == scopes.len() {
                            // A list with no `.It` can only establish that its
                            // leading content was invalid when it closes. Move
                            // it first so the following empty-list recovery
                            // observes the retained public topology.
                            outcome.recoveries.extend(move_initial_list_content_out(
                                builder,
                                root,
                                &mut root_children,
                                frame,
                            ));
                        }
                        if close == "El" && column_counts.contains_key(&frame.body) {
                            finalize_last_empty_column_item(
                                builder,
                                frame.body,
                                &mut pending_empty_column_items,
                                outcome,
                            );
                            finalize_short_column_items(
                                builder,
                                frame.body,
                                &mut pending_short_column_items,
                                outcome,
                            );
                        }
                        if close == "El"
                            && index + 1 == scopes.len()
                            && let Some(list_type) = list_types.get(&frame.body).copied()
                            && fixed_head_list_type(list_type)
                        {
                            finalize_last_fixed_head_list_item(
                                builder,
                                frame.body,
                                list_type,
                                &deferred_fixed_head_argument_items,
                                outcome,
                            );
                        }
                        if close == "El"
                            && let Some(item) = item_header_partial_scope(builder, &scopes, index)
                        {
                            // `post_bl()` does not close an enum list through
                            // a partial block embedded in an item's Head.  It
                            // retains a closer-owned list Body inside that
                            // partial Body, then reports both unclosed scopes
                            // at EOF.  In particular, the ordinary deferred
                            // Item Body is absent from the public AST.
                            append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(scopes[index + 1].close),
                                location: builder.node_location(node),
                            });
                            discard_item_body(builder, item);
                            deferred
                                .broken_items
                                .extend(broken_item_recoveries(builder, frame, item));
                            continue;
                        }
                        if let Some(interrupted) = scopes
                            .get(index + 1)
                            .copied()
                            .filter(|_inner| matches!(close, "Ed" | "Ef" | "El"))
                        {
                            append_broken_full_block_body(
                                builder,
                                active_body,
                                close,
                                frame,
                                node,
                                max_nodes,
                                outcome,
                            );
                            outcome.recoveries.push(Recovery::BadlyNestedBlock {
                                breaker: open_name(close),
                                interrupted: open_name(interrupted.close),
                                location: builder.node_location(node),
                            });
                            let mut surviving_scopes = scopes.split_off(index + 1);
                            scopes.truncate(index);
                            let first = surviving_scopes
                                .first_mut()
                                .expect("the interrupted scope was just selected");
                            first.resume_active = frame.resume_active;
                            first.resume_flow = frame.resume_flow;
                            scopes.extend(surviving_scopes);
                            continue;
                        }
                        let mut tail_remainder = Vec::new();
                        // Fc is a full-block closer, but closing punctuation
                        // on its control line resumes surrounding flow rather
                        // than becoming hidden close-macro syntax.  Other
                        // closers retain their existing dedicated recovery or
                        // validation paths until their argument grammars are
                        // implemented.
                        let close_remainder = if close == "Fc" {
                            let children = builder
                                .children(node)
                                .map(<[NodeId]>::to_vec)
                                .unwrap_or_default();
                            let remainder = split_mdoc_inline_tokens(
                                builder,
                                node,
                                &children,
                                spacing_enabled,
                                max_nodes,
                                outcome,
                            );
                            if let Some(first) = remainder.first()
                                // A callable macro quoted after Fc continues
                                // the same physical control line.  Only a
                                // literal tail token is promoted into the
                                // resumed flow's first line-start event.
                                && builder.node_macro_name(*first).is_none()
                                && !in_synopsis
                                && let Some(mut flags) = builder.node_flags(*first)
                            {
                                // Once Fc has closed the block, its trailing
                                // token begins a fresh flow event even though
                                // the scanner originally held it as a control
                                // argument at a later physical column.
                                flags.line_start = true;
                                let _ = builder.set_node_flags(*first, flags);
                            }
                            remainder
                        } else if close == "Xc" {
                            let children = builder
                                .children(node)
                                .map(<[NodeId]>::to_vec)
                                .unwrap_or_default();
                            let remainder = explicit_partial_tail_events(
                                builder,
                                node,
                                &children,
                                spacing_enabled,
                                max_nodes,
                                outcome,
                            );
                            mark_explicit_partial_close_tail_line_start(builder, &remainder);
                            remainder
                        } else {
                            Vec::new()
                        };
                        let empty_bk = close == "Ek"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty);
                        if empty_bk {
                            outcome.recoveries.push(Recovery::EmptyBlock {
                                macro_name: "Bk",
                                location: builder.node_location(frame.open),
                            });
                            discard_empty_block(
                                builder,
                                root,
                                &mut root_children,
                                frame.resume_flow,
                                frame.open,
                            );
                        }
                        if close == "Re"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            outcome.recoveries.push(Recovery::EmptyReferenceBlock {
                                location: builder.node_location(frame.open),
                            });
                        }
                        if close == "Ed"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            outcome.recoveries.push(Recovery::EmptyBlock {
                                macro_name: "Bd",
                                location: builder.node_location(frame.open),
                            });
                        }
                        if close == "El"
                            && builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                        {
                            // Unlike an empty Bk, an empty list remains a
                            // visible Block/Head/Body topology.  Validation
                            // reports it at its opener after the closer is
                            // consumed, independent of the selected list
                            // display kind.
                            outcome.recoveries.push(Recovery::EmptyBlock {
                                macro_name: "Bl",
                                location: builder.node_location(frame.open),
                            });
                        }
                        if frame.tail_on_close {
                            if builder.node_count() >= max_nodes {
                                if outcome.node_limit_location.is_none() {
                                    outcome.node_limit_location = builder.node_location(node);
                                }
                            } else if let Some(tail) = builder.push(frame.open, NodeKind::Tail) {
                                let _ = builder.macro_name(tail, "Eo");
                                tail_remainder = complete_explicit_tail(
                                    builder,
                                    tail,
                                    node,
                                    spacing_enabled,
                                    max_nodes,
                                    outcome,
                                );
                            }
                        }
                        implicitly_closed
                            .extend(scopes[index + 1..].iter().map(|frame| frame.close));
                        scopes.truncate(index);
                        active_body = frame.resume_active;
                        flow_parent = frame.resume_flow;
                        for remainder in tail_remainder {
                            append_to_parent(
                                builder,
                                root,
                                &mut root_children,
                                flow_parent,
                                remainder,
                            );
                        }
                        for remainder in close_remainder {
                            append_to_parent(
                                builder,
                                root,
                                &mut root_children,
                                flow_parent,
                                remainder,
                            );
                        }
                    } else if let Some(index) = implicitly_closed
                        .iter()
                        .rposition(|implicit| *implicit == close)
                    {
                        implicitly_closed.remove(index);
                    } else if close == "Xc" {
                        // Xc is also callable syntax inside an ordinary
                        // inline `.Xo … Xc` partial block.  Only consume it
                        // here when `.It Xo` established the cross-line scope.
                        append_to_parent(builder, root, &mut root_children, active_body, node);
                    } else {
                        if close == "Ec" {
                            recover_unmatched_ec(
                                builder,
                                root,
                                &mut root_children,
                                active_body,
                                node,
                                spacing_enabled,
                                max_nodes,
                                outcome,
                            );
                        }
                        outcome.recoveries.push(Recovery::UnmatchedClose {
                            macro_name: close_name(close),
                            location: builder.node_location(node),
                        });
                    }
                }
                _ => {
                    if let Some(close) = scopes.last().map(|frame| frame.close)
                        && is_explicit_partial_close(close)
                        && builder.node_text(node) == Some(close)
                    {
                        // A closer following a callable explicit opener can
                        // remain a bare inline token rather than becoming a
                        // control-line macro event.  It still restores the
                        // surrounding cross-line scope before the following
                        // token is attached (`.No a Oc Oo b Oc Oc Pq`).
                        close_explicit_partial_scope(
                            &mut scopes,
                            &mut implicitly_closed,
                            &mut active_body,
                            &mut flow_parent,
                            close,
                        );
                        continue;
                    }
                    if active_column_list(builder, active_body)
                        && builder
                            .node_flags(node)
                            .is_some_and(|flags| flags.line_start)
                        && structure_implicit_column_item(
                            builder,
                            active_body,
                            node,
                            spacing_enabled,
                            max_nodes,
                            outcome,
                            &mut scopes,
                        )
                    {
                        // A `Bl -column` list does not require explicit
                        // `.It` controls.  At a physical line boundary,
                        // mandoc turns ordinary mdoc macros and literal text
                        // into an implicit row before it processes `Ta` and
                        // tabs as cell boundaries.  The list body remains
                        // active for the following source line.
                        continue;
                    }
                    append_to_parent(builder, root, &mut root_children, active_body, node);
                    if let Some(tag) = paragraph_href
                        && builder.node_kind(node) == Some(NodeKind::Text)
                    {
                        move_paragraph_permalink(
                            builder,
                            node,
                            active_body,
                            &tag,
                            max_nodes,
                            outcome,
                        );
                    }
                    if let Some(close) = scopes.last().map(|frame| frame.close)
                        && node_arguments(builder, node)
                            .iter()
                            .any(|argument| argument == close)
                    {
                        let frame = scopes.pop().expect("last scope was checked");
                        active_body = frame.resume_active;
                        flow_parent = frame.resume_flow;
                    }
                }
            }
            if let Some((close, tail)) = direct_partial_close {
                close_explicit_partial_scope(
                    &mut scopes,
                    &mut implicitly_closed,
                    &mut active_body,
                    &mut flow_parent,
                    close,
                );
                append_explicit_partial_tail(
                    builder,
                    root,
                    &mut root_children,
                    &mut scopes,
                    &mut implicitly_closed,
                    &mut active_body,
                    &mut flow_parent,
                    node,
                    &tail,
                    false,
                    spacing_enabled,
                    max_nodes,
                    outcome,
                );
            }
        }
        if let Some((head, body)) = synopsis_name_inline_restore {
            if scopes.iter().any(|frame| frame.resume_active == head) {
                // A partial block opened from an Nm Head takes over the next
                // physical line.  libmandoc leaves the otherwise empty Nm
                // Body as the structural boundary and marks that delayed
                // flow transition as line-start.
                if let Some(mut flags) = builder.node_flags(body) {
                    flags.line_start = true;
                    let _ = builder.set_node_flags(body, flags);
                }
            } else if active_body == head && flow_parent == head {
                active_body = body;
                flow_parent = body;
            }
        }
    }
    machine.finish();

    if let Some((tag_node, tag)) = pending_manual_tag.take()
        && tag.is_empty()
    {
        outcome.recoveries.push(Recovery::EmptyMacro {
            macro_name: "Tg",
            location: builder.node_location(tag_node),
        });
        discard_node_from_parent(builder, root, &mut root_children, tag_node);
    }

    for frame in &scopes {
        if frame.close == "Re" {
            normalize_reference_field_order(builder, frame.body);
            if builder
                .children(frame.body)
                .is_some_and(<[NodeId]>::is_empty)
            {
                outcome.recoveries.push(Recovery::EmptyReferenceBlock {
                    location: builder.node_location(frame.open),
                });
            }
        }
        if frame.close == "El" && column_counts.contains_key(&frame.body) {
            finalize_last_empty_column_item(
                builder,
                frame.body,
                &mut pending_empty_column_items,
                outcome,
            );
            finalize_short_column_items(
                builder,
                frame.body,
                &mut pending_short_column_items,
                outcome,
            );
        }
    }

    // EOF closes the innermost retained semantic scope first, just as the
    // legacy post-validation walk does for a list held open inside a partial
    // block.
    for frame in scopes.into_iter().rev() {
        outcome.recoveries.push(Recovery::UnclosedBlock {
            macro_name: open_name(frame.close),
            location: builder.node_location(frame.open),
        });
    }
    flush_pending_nd_delimiters(
        builder,
        &mut pending_nd_delimiter_bodies,
        &mut outcome.recoveries,
    );
    flush_pending_name_section(
        builder,
        &mut pending_name_section_body,
        &mut outcome.recoveries,
    );
    flush_pending_authors_section(builder, &mut pending_authors_body, &mut outcome.recoveries);
    let syntax_stage_recoveries = deferred.flush_into(outcome);
    merge_syntax_recoveries(builder, outcome, syntax_stage_recoveries);
    PostValidation {
        builder,
        root,
        root_children: &root_children,
        outcome,
        synopsis_bodies: &synopsis_bodies,
        target_heads: &target_heads,
        automatic_function_targets: &automatic_function_targets,
        prologue: PrologueStatus {
            saw_title: saw_title_prologue,
            saw_date: saw_date_prologue,
            saw_operating_system_request,
        },
        netbsd: NetBsdValidation {
            enabled: netbsd_operating_system_validation,
            saw_rcs_id: saw_netbsd_rcs_id,
        },
    }
    .run();
    package.outcome
}
