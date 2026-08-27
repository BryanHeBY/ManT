use super::{
    DocumentBuilder, ExplicitFrame, IndentClose, MacroSet, NodeId, NodeKind, PendingElement,
    PendingHead, Recovery, ScopeFrame, StructureOutcome, append_to_active,
    apply_presentation_flags, attach_centered_input_lines, blank_line_location,
    block_head_is_pending, clear_no_fill_from_man_structure, clear_pending,
    clear_sentence_end_from_section_heads, close_explicit, close_indents,
    coalesce_ip_tab_separated_tag, coalesce_text_children, flatten_leading_section_paragraphs,
    is_line_scope_breaker, is_line_scoped_element, is_next_line_scoped_element,
    is_term_scope_breaker, line_scope_ancestors, line_scope_macro_name, make_block,
    mark_man_targets, normalize_pending_term_indent, normalize_visible_macro_tabulation_escapes,
    paragraph_recovery_offset, re_target, record_title_metadata, remove_child, section_scope_name,
    suppress_filled_c_blank_lines, title_argument, title_argument_missing, title_date_argument,
    title_lowercase, title_missing_date, title_section_argument, title_section_missing,
    title_unparseable_date, validate_and_discard_all_arguments, validate_inline_paragraph_controls,
    validate_maximum_arguments, validate_no_arguments, validate_option_arguments,
    validate_section_paragraph_controls,
};
use std::collections::BTreeSet;

struct ManStructureMachine {
    nodes: std::vec::IntoIter<NodeId>,
    suppressed_nodes: BTreeSet<NodeId>,
}

impl ManStructureMachine {
    fn prepare(builder: &mut DocumentBuilder, flat: Vec<NodeId>) -> Self {
        // Scanner output is normalized once before structural transitions.
        // Keeping this phase separate makes `step` a cheap source-order
        // cursor with no repeated package classification or allocation.
        for node in &flat {
            normalize_visible_macro_tabulation_escapes(builder, *node);
            if builder
                .node_macro_name(*node)
                .is_some_and(is_line_scoped_element)
            {
                coalesce_text_children(builder, *node);
            }
        }
        apply_presentation_flags(builder, &flat);
        let mut suppressed_nodes =
            BTreeSet::from_iter(suppress_filled_c_blank_lines(builder, &flat));
        suppressed_nodes.extend(attach_centered_input_lines(builder, &flat));
        Self {
            nodes: flat.into_iter(),
            suppressed_nodes,
        }
    }

    fn step(&mut self) -> Option<NodeId> {
        self.nodes
            .by_ref()
            .find(|node| !self.suppressed_nodes.contains(node))
    }

    fn finish(self) {
        debug_assert!(self.nodes.len() == 0, "man event machine finished early");
    }
}

/// Convert the implemented man block families from scanner elements into
/// `Block`/`Head`/`Body` syntax nodes.
#[allow(clippy::too_many_lines)] // One source-order state machine keeps man scope transitions auditable.
pub(crate) fn structure(builder: &mut DocumentBuilder, max_nodes: usize) -> StructureOutcome {
    let mut outcome = StructureOutcome::default();
    if builder.macro_set() != MacroSet::Man {
        return outcome;
    }

    let root = DocumentBuilder::root();
    let Some(flat) = builder.children(root).map(<[NodeId]>::to_vec) else {
        return outcome;
    };

    // The scanner intentionally has no man(7)-specific state: it produces a
    // source-ordered list of already-expanded roff events. Keep package
    // normalization and event traversal in an explicit prepare/step/finish
    // machine so generated `.de` input follows the same transition path.
    let mut machine = ManStructureMachine::prepare(builder, flat);

    // `subsection_parent` is the current SH body receiving sibling `.SS`
    // blocks; `flow_parent` is the current container receiving structural
    // siblings, while `active_body` receives ordinary text.  They differ for
    // a term or paragraph: a following `.TP` belongs beside it, while source
    // text belongs in its body.
    let mut subsection_parent = root;
    let mut flow_parent = root;
    let mut active_body = root;
    let mut indents = Vec::new();
    let mut explicit_blocks = Vec::new();
    let mut pending_head = None::<PendingHead>;
    let mut pending_element = None::<PendingElement>;
    let mut root_children = Vec::new();
    let mut section_bodies = Vec::new();
    let mut target_heads = Vec::new();
    let mut preserve_leading_comments = true;
    let mut pending_empty_ip = None::<(NodeId, NodeId)>;
    let mut pending_empty_paragraph = None::<(NodeId, NodeId)>;
    let mut pending_empty_paragraph_before_re = None::<(NodeId, NodeId)>;
    let mut deferred_mt_recoveries = Vec::new();
    let mut deferred_empty_paragraph_recoveries = Vec::new();
    // These recoveries are emitted by `man_validate` during a post-order
    // walk of section bodies.  Keep the owning body until that pass so an
    // inner `.SS` reports before its enclosing `.SH`.
    let mut deferred_after_section_recoveries = Vec::<(NodeId, Recovery)>::new();
    let mut deferred_section_interrupt_recoveries = Vec::new();
    let mut deferred_fill_recoveries = Vec::new();
    // Leading physical blank input after a section heading is transparent in
    // the public tree, but it changes the validator's handling of a later
    // PP/br/sp request. Keep that distinction private to this one structure
    // pass instead of publishing blank Text nodes as content.
    let mut section_leading_blanks = Vec::new();
    let mut saw_title_control = false;
    let mut saw_complete_title_control = false;

    while let Some(node) = machine.step() {
        if builder.node_kind(node) == Some(NodeKind::Comment) {
            // man(7) retains the source preamble comment but discards comments
            // interspersed with parsed document content from its public tree.
            if preserve_leading_comments {
                root_children.push(node);
            }
            continue;
        }
        preserve_leading_comments = false;
        let is_empty_text =
            builder.node_kind(node) == Some(NodeKind::Text) && builder.node_text(node) == Some("");
        if pending_head.is_none()
            && is_empty_text
            && section_bodies.contains(&active_body)
            && builder
                .children(active_body)
                .is_some_and(<[NodeId]>::is_empty)
        {
            if !section_leading_blanks.contains(&active_body) {
                section_leading_blanks.push(active_body);
            }
            continue;
        }
        let macro_name = builder
            .node_macro_name(node)
            .or_else(|| builder.node_preprocessor_opener(node))
            .map(str::to_owned);
        if matches!(macro_name.as_deref(), Some("cc" | "c2" | "ec")) {
            // Roff formatter controls have already affected the scanner's
            // following input. The man public tree intentionally excludes
            // the request itself, matching mandoc's package projection.
            continue;
        }
        let after_section_blank = section_leading_blanks.contains(&flow_parent)
            && section_bodies.contains(&flow_parent)
            && builder
                .children(flow_parent)
                .is_some_and(<[NodeId]>::is_empty);
        if macro_name.as_deref() == Some("OP") {
            validate_option_arguments(builder, node, &mut outcome);
        }
        if macro_name.as_deref() == Some("PD") {
            validate_maximum_arguments(builder, node, "PD", 1, &mut outcome);
        }
        if macro_name.as_deref() == Some("sp") {
            validate_maximum_arguments(builder, node, "sp", 1, &mut outcome);
        }
        if macro_name.as_deref() == Some("RS") {
            validate_maximum_arguments(builder, node, "RS", 1, &mut outcome);
        }
        if macro_name.as_deref() == Some("TH") {
            validate_maximum_arguments(builder, node, "TH", 5, &mut outcome);
        }
        if macro_name.as_deref() == Some("ft") {
            // roff_valid_ft() retains the selected font only. The scanner has
            // already reported a surplus source word, so this is an AST-only
            // projection matching the mdoc structural path.
            let children = builder.children(node).unwrap_or_default().to_vec();
            if let Some(first) = children.first() {
                let _ = builder.replace_children(node, std::slice::from_ref(first));
            } else if builder.node_count() < max_nodes {
                if let Some(default_font) = builder.push(node, NodeKind::Text) {
                    let _ = builder.text(default_font, "P");
                    let _ = builder.set_node_location(default_font, builder.node_location(node));
                }
            } else if outcome.node_limit_location.is_none() {
                outcome.node_limit_location = builder.node_location(node);
            }
        }
        if let Some(macro_name) = match macro_name.as_deref() {
            Some("nf") => Some("nf"),
            Some("fi") => Some("fi"),
            _ => None,
        } {
            validate_and_discard_all_arguments(builder, node, macro_name, &mut outcome);
        }
        if macro_name.as_deref() == Some("br") {
            // The roff line-break request takes no operands in man input.
            // Keep the request as a structural boundary, but mirror
            // `MANDOCERR_ARG_SKIP` by dropping and reporting its complete
            // source tail before later paragraph-control validation runs.
            validate_and_discard_all_arguments(builder, node, "br", &mut outcome);
        }
        if let Some(paragraph_macro) = match macro_name.as_deref() {
            // `LP` is an obsolete spelling of `PP`; man_validate reports the
            // normalized macro name in its ignored-argument diagnostic.
            Some("LP" | "PP" | "P") => Some("PP"),
            _ => None,
        } {
            validate_no_arguments(builder, node, paragraph_macro, &mut outcome);
        }
        if let Some(pending) = pending_head
            && !pending.is_term
            && let Some(breaker) = macro_name.as_deref()
            && !matches!(breaker, "nf" | "fi" | "PD")
            && let Some(scope) = builder.node_macro_name(pending.head)
        {
            let recovery = Recovery::LineScopeInterrupted {
                scope: scope.into(),
                breaker: breaker.into(),
                location: builder.node_location(pending.head),
            };
            if is_next_line_scoped_element(breaker) {
                deferred_section_interrupt_recoveries.push(recovery);
            } else {
                outcome.recoveries.push(recovery);
            }
            if let Some((block, parent)) = pending.block_parent {
                remove_child(builder, root, &mut root_children, parent, block);
                flow_parent = parent;
                active_body = parent;
                subsection_parent = parent;
            }
            pending_head = None;
        }
        if let Some(pending) = pending_head
            && pending.is_term
            && let Some(breaker) = macro_name.as_deref()
            && is_term_scope_breaker(breaker)
            && let Some(scope) = builder.node_macro_name(pending.head)
        {
            // A structural request cannot provide a deferred TP/TQ term. It
            // breaks the line scope before its own scope action, and the
            // empty term block is not published.
            outcome.recoveries.push(Recovery::LineScopeInterrupted {
                scope: scope.into(),
                breaker: breaker.into(),
                location: builder.node_location(pending.head),
            });
            if let Some((block, parent)) = pending.block_parent {
                remove_child(builder, root, &mut root_children, parent, block);
                flow_parent = parent;
                active_body = parent;
                subsection_parent = parent;
            }
            pending_head = None;
        }
        if let Some(element) = pending_element
            && let Some(breaker) = macro_name.as_deref()
            && is_line_scope_breaker(breaker)
            && let Some(scope) = line_scope_macro_name(builder, element.node)
        {
            outcome.recoveries.push(Recovery::LineScopeInterrupted {
                scope: scope.into(),
                breaker: breaker.into(),
                location: builder.node_location(element.node),
            });
            if element.parent == root {
                root_children.retain(|child| *child != element.node);
            } else if let Some(mut children) =
                builder.children(element.parent).map(<[NodeId]>::to_vec)
            {
                children.retain(|child| *child != element.node);
                let _ = builder.replace_children(element.parent, &children);
            }
            pending_element = None;
        }
        if macro_name.as_deref() == Some("fi")
            && pending_head.is_some_and(|pending| !pending.is_term)
        {
            deferred_fill_recoveries.push(Recovery::RedundantFillMode {
                message: "fill mode already enabled, skipping: fi",
                location: builder.node_location(node),
            });
        }
        if let Some((empty_ip, parent)) = pending_empty_ip.take()
            && matches!(macro_name.as_deref(), Some("IP" | "LP" | "PP" | "P" | "RS"))
        {
            outcome.recoveries.push(Recovery::EmptyParagraph {
                macro_name: "IP",
                location: builder.node_location(empty_ip),
            });
            remove_child(builder, root, &mut root_children, parent, empty_ip);
        }
        if let Some((empty_paragraph, parent)) = pending_empty_paragraph.take()
            && matches!(
                macro_name.as_deref(),
                Some("SH" | "SS" | "TP" | "TQ" | "IP" | "HP" | "LP" | "PP" | "P" | "RS" | "RE")
            )
        {
            deferred_empty_paragraph_recoveries.push(Recovery::EmptyParagraph {
                macro_name: "PP",
                location: builder.node_location(empty_paragraph),
            });
            // `blk_close()` moves an otherwise empty paragraph immediately
            // before `.RE` behind the closed indentation block. Keep it for
            // the closer to relocate, rather than deleting it in place.
            if macro_name.as_deref() == Some("RE") {
                pending_empty_paragraph_before_re = Some((empty_paragraph, parent));
            } else {
                remove_child(builder, root, &mut root_children, parent, empty_paragraph);
            }
        }
        if macro_name.as_deref() != Some("TH") {
            builder.metadata_mut().has_body = true;
        }
        if after_section_blank
            && let Some(macro_name) = match macro_name.as_deref() {
                Some("LP" | "PP" | "P") => Some("PP"),
                Some("br") => Some("br"),
                Some("sp") => Some("sp"),
                _ => None,
            }
        {
            deferred_after_section_recoveries.push((
                flow_parent,
                Recovery::ParagraphAfterSection {
                    macro_name,
                    section_name: section_scope_name(builder, flow_parent),
                    location: builder.node_location(node),
                },
            ));
            continue;
        }
        match macro_name.as_deref() {
            Some("TH") => {
                saw_title_control = true;
                saw_complete_title_control |= title_date_argument(builder, node).is_some();
                clear_pending(&mut pending_head, &mut pending_element);
                if let Some((title, location)) = title_lowercase(builder, node) {
                    outcome.recoveries.push(Recovery::TitleNotUppercase {
                        title,
                        location: Some(location),
                    });
                }
                if title_argument_missing(builder, node) {
                    outcome.recoveries.push(Recovery::TitleArgumentMissing {
                        location: title_argument(builder, node)
                            .and_then(|argument| builder.node_location(argument))
                            .or_else(|| builder.node_location(node)),
                    });
                }
                if title_section_missing(builder, node) {
                    outcome.recoveries.push(Recovery::TitleSectionMissing {
                        title: title_argument(builder, node)
                            .and_then(|argument| builder.node_text(argument))
                            .map(|title| title.to_owned().into_boxed_str()),
                        location: title_section_argument(builder, node)
                            .and_then(|argument| builder.node_location(argument))
                            .or_else(|| builder.node_location(node)),
                    });
                }
                if title_missing_date(builder, node) {
                    outcome.recoveries.push(Recovery::TitleDateMissing {
                        location: title_date_argument(builder, node)
                            .and_then(|argument| builder.node_location(argument))
                            .or_else(|| builder.node_location(node)),
                    });
                } else if let Some((date, location)) = title_unparseable_date(builder, node) {
                    outcome
                        .recoveries
                        .push(Recovery::TitleDateUnparseable { date, location });
                }
                record_title_metadata(builder, node);
                // man_validate consumes `.TH` after deriving metadata. Keep
                // its scanner record unreachable rather than exposing a
                // title-control element to downstream document traversal.
            }
            Some("SH") => {
                clear_pending(&mut pending_head, &mut pending_element);
                let Some((head, body)) = make_block(builder, node, "SH", max_nodes, &mut outcome)
                else {
                    root_children.push(node);
                    continue;
                };
                builder.metadata_mut().has_body = true;
                section_bodies.push(body);
                target_heads.push(head);
                subsection_parent = body;
                flow_parent = body;
                active_body = body;
                indents.clear();
                explicit_blocks.clear();
                root_children.push(node);
                if block_head_is_pending(builder, node, "SH") {
                    pending_head = builder
                        .children(node)
                        .and_then(|parts| parts.first())
                        .copied()
                        .map(|head| PendingHead::ordinary(node, head, body, root));
                }
            }
            Some("SS") => {
                clear_pending(&mut pending_head, &mut pending_element);
                let section_parent = subsection_parent;
                let Some((head, body)) = make_block(builder, node, "SS", max_nodes, &mut outcome)
                else {
                    append_to_active(builder, root, &mut root_children, active_body, node);
                    continue;
                };
                builder.metadata_mut().has_body = true;
                section_bodies.push(body);
                target_heads.push(head);
                append_to_active(builder, root, &mut root_children, section_parent, node);
                flow_parent = body;
                active_body = body;
                indents.clear();
                explicit_blocks.clear();
                if block_head_is_pending(builder, node, "SS") {
                    pending_head = builder
                        .children(node)
                        .and_then(|parts| parts.first())
                        .copied()
                        .map(|head| PendingHead::ordinary(node, head, body, section_parent));
                }
            }
            Some("TP" | "TQ" | "IP" | "HP") => {
                clear_pending(&mut pending_head, &mut pending_element);
                let name = macro_name.as_deref().expect("matched man macro name");
                let Some((head, body)) = make_block(builder, node, name, max_nodes, &mut outcome)
                else {
                    append_to_active(builder, root, &mut root_children, active_body, node);
                    continue;
                };
                let empty_ip =
                    name == "IP" && builder.children(head).is_some_and(<[NodeId]>::is_empty);
                if name == "IP" {
                    coalesce_ip_tab_separated_tag(builder, head);
                }
                if after_section_blank && matches!(name, "TP" | "IP") {
                    // The canonical tree intentionally omits a leading blank
                    // under a section heading, but the terminal device also
                    // needs to know that it was consumed before this field.
                    // Keep this private renderer provenance on the block.
                    let _ = builder.set_node_terminal_suppressed_leading_blank(node, true);
                }
                append_to_active(builder, root, &mut root_children, flow_parent, node);
                if empty_ip {
                    pending_empty_ip = Some((node, flow_parent));
                }
                active_body = body;
                target_heads.push(head);
                // `TP` and `TQ` always accept their term on the following
                // line.  `IP` and `HP` only keep an empty head open in the C
                // parser until the next macro dispatch; that macro belongs to
                // their body, not to the head.  Modelling the latter as a
                // pending head used to make `.HP\n.B term` look like a tagged
                // term paragraph, which disagrees with `man_validate`.
                if matches!(name, "TP" | "TQ") {
                    pending_head = Some(PendingHead::term(node, head, body, flow_parent));
                }
            }
            Some("LP" | "PP" | "P") => {
                clear_pending(&mut pending_head, &mut pending_element);
                let follows_section = section_bodies.contains(&flow_parent)
                    && builder
                        .children(flow_parent)
                        .is_some_and(<[NodeId]>::is_empty);
                let Some((head, body)) = make_block(builder, node, "PP", max_nodes, &mut outcome)
                else {
                    append_to_active(builder, root, &mut root_children, active_body, node);
                    continue;
                };
                let empty = builder.children(head).is_some_and(<[NodeId]>::is_empty);
                append_to_active(builder, root, &mut root_children, flow_parent, node);
                if follows_section && empty {
                    deferred_after_section_recoveries.push((
                        flow_parent,
                        Recovery::ParagraphAfterSection {
                            macro_name: "PP",
                            section_name: section_scope_name(builder, flow_parent),
                            location: builder.node_location(node),
                        },
                    ));
                }
                if empty {
                    pending_empty_paragraph = Some((node, flow_parent));
                }
                active_body = body;
            }
            Some("RS") => {
                clear_pending(&mut pending_head, &mut pending_element);
                // `rew_scope(MAN_RS)` finishes an implicit TP/IP/HP/PP
                // before opening the indent, while preserving the nearest
                // SH/SS/explicit parent.  `flow_parent` is precisely that
                // surviving parent; restoring the previous `active_body`
                // would route post-RE text back into a completed term.
                let resume_parent = flow_parent;
                let Some((_, body)) = make_block(builder, node, "RS", max_nodes, &mut outcome)
                else {
                    append_to_active(builder, root, &mut root_children, active_body, node);
                    continue;
                };
                append_to_active(builder, root, &mut root_children, flow_parent, node);
                indents.push(ScopeFrame {
                    open: node,
                    body,
                    resume_active: resume_parent,
                    resume_flow: flow_parent,
                });
                flow_parent = body;
                active_body = body;
            }
            Some("RE") => {
                clear_pending(&mut pending_head, &mut pending_element);
                match close_indents(
                    builder,
                    &mut indents,
                    &mut active_body,
                    &mut flow_parent,
                    re_target(builder, node, &mut outcome),
                ) {
                    IndentClose::Closed { frames } => {
                        for frame in frames {
                            if builder
                                .children(frame.body)
                                .is_some_and(<[NodeId]>::is_empty)
                            {
                                outcome.recoveries.push(Recovery::EmptyBlock {
                                    macro_name: "RS",
                                    location: builder.node_location(frame.open),
                                });
                            }
                        }
                        if let Some((paragraph, parent)) = pending_empty_paragraph_before_re.take()
                        {
                            remove_child(builder, root, &mut root_children, parent, paragraph);
                            append_to_active(
                                builder,
                                root,
                                &mut root_children,
                                flow_parent,
                                paragraph,
                            );
                            if let Some(body) = builder
                                .children(paragraph)
                                .and_then(|parts| parts.get(1))
                                .copied()
                            {
                                active_body = body;
                            }
                        }
                    }
                    IndentClose::FewerOpen { target } => {
                        outcome.recoveries.push(Recovery::FewerIndents {
                            target,
                            location: builder.node_location(node),
                        });
                    }
                    IndentClose::NotOpen => {
                        outcome.recoveries.push(Recovery::UnmatchedClose {
                            macro_name: "RE",
                            location: builder.node_location(node),
                        });
                        // An unmatched `.RE` uses man(7)'s `br` recovery: it
                        // remains a visible line-break element beside the
                        // implicit term/paragraph it interrupts, and following
                        // text resumes in that outer flow.  Dropping the node
                        // leaves the current TP/IP/HP body active and swallows
                        // all following recovery text into it.
                        let _ = builder.macro_name(node, "br");
                        let _ = builder.replace_children(node, &[]);
                        append_to_active(builder, root, &mut root_children, flow_parent, node);
                        active_body = flow_parent;
                    }
                }
            }
            Some("UR" | "MT" | "SY") => {
                clear_pending(&mut pending_head, &mut pending_element);
                let name = macro_name.as_deref().expect("matched man macro name");
                let Some((head, body)) = make_block(builder, node, name, max_nodes, &mut outcome)
                else {
                    append_to_active(builder, root, &mut root_children, active_body, node);
                    continue;
                };
                append_to_active(builder, root, &mut root_children, active_body, node);
                if matches!(name, "MT" | "UR") {
                    let arguments = builder
                        .children(head)
                        .map(<[NodeId]>::to_vec)
                        .unwrap_or_default();
                    if let Some(excess) = arguments.get(1).copied() {
                        outcome.recoveries.push(Recovery::ExcessArguments {
                            macro_name: if name == "MT" { "MT" } else { "UR" },
                            argument: builder.node_text(excess).unwrap_or_default().into(),
                            location: builder.node_location(excess),
                        });
                        let _ = builder.replace_children(head, &arguments[..1]);
                    }
                }
                explicit_blocks.push(ExplicitFrame {
                    close: match name {
                        "UR" => "UE",
                        "MT" => "ME",
                        "SY" => "YS",
                        _ => unreachable!("only known explicit man blocks are matched"),
                    },
                    open: node,
                    body,
                    resume_active: active_body,
                    resume_flow: flow_parent,
                });
                flow_parent = body;
                active_body = body;
            }
            Some("UE" | "ME" | "YS") => {
                clear_pending(&mut pending_head, &mut pending_element);
                let close = macro_name.as_deref().expect("matched man macro name");
                if let Some(frame) = close_explicit(
                    close,
                    &mut explicit_blocks,
                    &mut active_body,
                    &mut flow_parent,
                ) {
                    if matches!(close, "ME" | "UE") {
                        let macro_name = if close == "ME" { "MT" } else { "UR" };
                        let parts = builder.children(frame.open).unwrap_or_default();
                        let head = parts.first().copied();
                        let missing_resource = head
                            .and_then(|head| builder.children(head))
                            .is_some_and(<[NodeId]>::is_empty);
                        if builder
                            .children(frame.body)
                            .is_some_and(<[NodeId]>::is_empty)
                        {
                            deferred_mt_recoveries.push(Recovery::EmptyBlock {
                                macro_name,
                                location: builder.node_location(frame.open),
                            });
                        } else if missing_resource {
                            deferred_mt_recoveries.push(Recovery::MissingResource {
                                macro_name,
                                location: builder.node_location(frame.open),
                            });
                        }
                        if matches!(close, "ME" | "UE") {
                            let arguments = builder
                                .children(node)
                                .map(<[NodeId]>::to_vec)
                                .unwrap_or_default();
                            if let Some(text) = arguments.first().copied() {
                                let joined = arguments
                                    .iter()
                                    .filter_map(|argument| builder.node_text(*argument))
                                    .collect::<Vec<_>>()
                                    .join(" ");
                                if !joined.is_empty() {
                                    let _ = builder.set_node_text(text, joined);
                                    let _ = builder
                                        .set_node_location(text, builder.node_location(node));
                                    if let Some(mut flags) = builder.node_flags(node) {
                                        flags.delimiter_close = true;
                                        let _ = builder.set_node_flags(text, flags);
                                    }
                                    append_to_active(
                                        builder,
                                        root,
                                        &mut root_children,
                                        flow_parent,
                                        text,
                                    );
                                }
                            }
                        }
                    }
                    // The synopsis closer is a public zero-argument element
                    // after ending a SY block.  UE and ME are validator-only
                    // closers, but mandoc needs YS to preserve synopsis
                    // spacing in the owned tree.
                    if close == "YS" {
                        append_to_active(builder, root, &mut root_children, flow_parent, node);
                    }
                } else {
                    outcome.recoveries.push(Recovery::UnmatchedClose {
                        macro_name: match close {
                            "UE" => "UE",
                            "ME" => "ME",
                            "YS" => "YS",
                            _ => unreachable!("only explicit man closers are matched"),
                        },
                        location: builder.node_location(node),
                    });
                }
            }
            Some("in") if pending_head.is_some_and(|pending| pending.is_term) => {
                // `man_valid_pre()` keeps an indentation request inside a
                // pending TP/TQ term head. The following source text remains
                // the term, rather than being redirected into the body.
                let pending = pending_head.expect("guarded pending term head");
                normalize_pending_term_indent(builder, node);
                append_to_active(builder, root, &mut root_children, pending.head, node);
            }
            Some("nf" | "fi") if pending_head.is_some_and(|pending| !pending.is_term) => {
                // A fill-mode request after an argument-less section opener
                // starts that section's Body without becoming its Head text.
                // It is validation-transparent, but no longer leaves a
                // next-line heading scope open for following prose.
                //
                // This is also a narrow presentation boundary: mandoc
                // publishes the `fi` request in its newly restored fill
                // state when it breaks an empty section head.  The ordinary
                // source-order pass correctly retains the preceding
                // no-fill state for a standalone `fi`, so normalize only
                // this recovered line-scope form here.
                if builder.node_macro_name(node) == Some("fi")
                    && let Some(mut flags) = builder.node_flags(node)
                {
                    flags.no_fill = false;
                    let _ = builder.set_node_flags(node, flags);
                }
                pending_head = None;
                append_to_active(builder, root, &mut root_children, active_body, node);
            }
            Some("nf" | "fi" | "EX" | "EE")
                if pending_head.is_some_and(|pending| pending.is_term) =>
            {
                // Presentation toggles are transparent to the next-line
                // term grammar.  In particular, `.TP` followed by `.nf`
                // still takes the following text as its Head rather than
                // prematurely opening the Body.
                let pending = pending_head.expect("guarded pending term head");
                append_to_active(builder, root, &mut root_children, pending.head, node);
            }
            Some("PD") => {
                // Paragraph-distance state is transparent to man(7)'s
                // next-line scopes. It belongs inside a pending SH/SS/TP
                // Head (or a one-line font Element), but it must not consume
                // that pending state: the immediately following input still
                // supplies the heading, term, or formatted text.
                if let Some(element) = pending_element {
                    append_to_active(builder, root, &mut root_children, element.node, node);
                } else if let Some(pending) = pending_head {
                    append_to_active(builder, root, &mut root_children, pending.head, node);
                } else {
                    append_to_active(builder, root, &mut root_children, active_body, node);
                }
            }
            _ => {
                pending_empty_ip = None;
                pending_empty_paragraph = None;
                let line_continues = builder
                    .node_flags(node)
                    .is_some_and(|flags| flags.line_continuation);
                let mut attached_parent = active_body;
                if let Some(element) = pending_element.take() {
                    if builder.node_kind(node) == Some(NodeKind::Text)
                        && builder.node_text(node) == Some("")
                    {
                        outcome.recoveries.push(Recovery::BlankLineInScope {
                            location: builder.node_location(node),
                        });
                        pending_element = Some(element);
                        continue;
                    }
                    attached_parent = element.node;
                    append_to_active(builder, root, &mut root_children, element.node, node);
                    if line_continues {
                        // A final `\c` keeps a next-line font element open
                        // across the next physical text event. The following
                        // line still belongs to this same Element.
                        pending_element = Some(element);
                    } else if builder.node_kind(node) == Some(NodeKind::Text)
                        && let Some(pending) = pending_head.take()
                        && let Some(body) = pending.body
                    {
                        // A next-line font run can consume the TP/TQ term
                        // through one or more empty font controls. The term
                        // completes at its first text node, not at the next
                        // ordinary body line after the nested element has
                        // unwound.
                        let _ = builder.set_node_location(body, builder.node_location(node));
                    }
                } else if let Some(pending) = pending_head.take() {
                    if builder.node_kind(node) == Some(NodeKind::Text)
                        && builder.node_text(node) == Some("")
                    {
                        outcome.recoveries.push(Recovery::BlankLineInScope {
                            location: blank_line_location(builder, node),
                        });
                        pending_head = Some(pending);
                        continue;
                    }
                    attached_parent = pending.head;
                    append_to_active(builder, root, &mut root_children, pending.head, node);
                    if line_continues {
                        // TP/TQ terms use the same physical continuation
                        // rule: close their Head only after the first
                        // non-continuing source line has arrived.
                        pending_head = Some(pending);
                    } else if macro_name
                        .as_deref()
                        .is_some_and(is_next_line_scoped_element)
                        && builder.children(node).is_some_and(<[NodeId]>::is_empty)
                    {
                        // An empty B/I/R/SM/SB is not the term itself: it
                        // opens a next-line font run that remains inside the
                        // pending TP/TQ head. Keep its deferred Body until
                        // that run receives actual source text.
                        pending_head = Some(pending);
                    } else if let Some(body) = pending.body {
                        // libmandoc only allocates a TP/TQ body after its
                        // first term line has closed the head.  Retaining
                        // that source location makes the public tree agree
                        // even though the native arena allocates both halves
                        // together for bounded, iterative restructuring.
                        let _ = builder.set_node_location(body, builder.node_location(node));
                    }
                } else {
                    // Outside an open line scope and after the special
                    // section-leading case above, a filled man blank line
                    // is one vertical-space request in the public tree.
                    // Keep its authored source span so the later paragraph
                    // validator can apply the same sibling rules as `.sp`.
                    if is_empty_text && builder.node_flags(node).is_some_and(|flags| !flags.no_fill)
                    {
                        let _ = builder.set_node_kind(node, NodeKind::Element);
                        let _ = builder.macro_name(node, "sp");
                        let _ = builder.clear_node_text(node);
                    }
                    append_to_active(builder, root, &mut root_children, active_body, node);
                }
                if macro_name
                    .as_deref()
                    .is_some_and(is_next_line_scoped_element)
                    && builder.children(node).is_some_and(<[NodeId]>::is_empty)
                {
                    pending_element = Some(PendingElement {
                        node,
                        parent: attached_parent,
                    });
                }
            }
        }
    }
    machine.finish();
    for frame in &indents {
        outcome.recoveries.push(Recovery::UnclosedBlock {
            macro_name: "RS",
            location: builder.node_location(frame.open),
        });
    }
    for frame in &explicit_blocks {
        outcome.recoveries.push(Recovery::UnclosedBlock {
            macro_name: match frame.close {
                "UE" => "UR",
                "ME" => "MT",
                "YS" => "SY",
                _ => unreachable!("only explicit man closers are stored"),
            },
            location: builder.node_location(frame.open),
        });
    }
    outcome.recoveries.extend(deferred_mt_recoveries);
    let mut eof_pending_element = None;
    if let Some(element) = pending_element {
        let macro_name = line_scope_macro_name(builder, element.node)
            .expect("only next-line font elements can remain pending");
        eof_pending_element = Some(macro_name);
        outcome.recoveries.push(Recovery::LineScopeBroken {
            macro_name,
            location: builder.node_location(element.node),
        });
        for outer in line_scope_ancestors(builder, root, element.node)
            .into_iter()
            .rev()
        {
            let macro_name = line_scope_macro_name(builder, outer)
                .expect("line-scope ancestry contains only font elements");
            outcome.recoveries.push(Recovery::LineScopeBroken {
                macro_name,
                location: builder.node_location(outer),
            });
        }
        if element.parent == root {
            root_children.retain(|child| *child != element.node);
        } else if let Some(mut children) = builder.children(element.parent).map(<[NodeId]>::to_vec)
        {
            children.retain(|child| *child != element.node);
            let _ = builder.replace_children(element.parent, &children);
        }
    }
    if let Some(element) = eof_pending_element {
        // A still-empty next-line font scope has not actually supplied a
        // section-title breaker. mandoc attributes the enclosing section's
        // deferred recovery to EOF as well, while retaining the element's
        // own EOF finding first.
        for recovery in &mut deferred_section_interrupt_recoveries {
            if let Recovery::LineScopeInterrupted { breaker, .. } = recovery
                && breaker.as_ref() == element
            {
                *breaker = "EOF".into();
            }
        }
    }
    outcome
        .recoveries
        .extend(deferred_section_interrupt_recoveries);
    if let Some(pending) = pending_head
        && pending.is_term
        && let Some(macro_name) = match builder.node_macro_name(pending.head) {
            Some("TP") => Some("TP"),
            Some("TQ") => Some("TQ"),
            _ => None,
        }
    {
        outcome.recoveries.push(Recovery::LineScopeBroken {
            macro_name,
            location: builder.node_location(pending.head),
        });
        if let Some((block, parent)) = pending.block_parent {
            remove_child(builder, root, &mut root_children, parent, block);
        }
    }
    if let Some(pending) = pending_head
        && !pending.is_term
        && let Some(macro_name) = match builder.node_macro_name(pending.head) {
            Some("SH") => Some("SH"),
            Some("SS") => Some("SS"),
            _ => None,
        }
    {
        outcome.recoveries.push(Recovery::LineScopeBroken {
            macro_name,
            location: builder.node_location(pending.head),
        });
        if let Some((block, parent)) = pending.block_parent {
            remove_child(builder, root, &mut root_children, parent, block);
        }
    }
    outcome.recoveries.extend(deferred_fill_recoveries);
    let _ = builder.replace_children(root, &root_children);
    let mut paragraph_recoveries = Vec::new();
    validate_inline_paragraph_controls(builder, root, &mut paragraph_recoveries);
    paragraph_recoveries.extend(deferred_empty_paragraph_recoveries);
    paragraph_recoveries.sort_by_key(paragraph_recovery_offset);
    outcome.recoveries.extend(paragraph_recoveries);
    let mut section_flatten_blockers = Vec::new();
    validate_section_paragraph_controls(
        builder,
        root,
        &mut outcome.recoveries,
        &mut deferred_after_section_recoveries,
        &mut section_flatten_blockers,
    );
    // Every queued item should belong to a published section body.  Retain a
    // safe fallback for malformed/recovered structures rather than dropping a
    // user-visible diagnostic if a prior recovery removed that body.
    outcome.recoveries.extend(
        deferred_after_section_recoveries
            .into_iter()
            .map(|(_, recovery)| recovery),
    );
    flatten_leading_section_paragraphs(builder, &section_bodies, &section_flatten_blockers);
    clear_no_fill_from_man_structure(builder);
    clear_sentence_end_from_section_heads(builder);
    mark_man_targets(builder, &target_heads);
    let missing_manual_title = !saw_title_control && builder.metadata_mut().title.is_none();
    if missing_manual_title {
        outcome.missing_title_control = true;
        outcome.recoveries.push(Recovery::MissingManualTitle);
        outcome.recoveries.push(Recovery::MissingManualDate);
        // A forced man parse may contain no `.TH`. Keep the metadata shape
        // usable and deterministic instead of borrowing the host date used by
        // legacy mandoc's recovery path.
        let metadata = builder.metadata_mut();
        metadata.title = Some("".into());
        metadata.section = Some("".into());
        metadata.date = Some("".into());
    }
    if saw_complete_title_control && !builder.metadata_mut().has_body {
        outcome.recoveries.push(Recovery::NoDocumentBody);
    }
    outcome
}
