//! First structural pass for the traditional man(7) macro package.
//!
//! Roff execution deliberately emits a flat, source-ordered event stream.
//! This pass reorganizes those already-expanded nodes instead of rescanning
//! input bytes, so generated macro calls and resolver-owned source positions
//! retain the same arena records.  M4 grows this table incrementally; unknown
//! macros remain ordinary elements in the active body.

use crate::{MacroSet, NodeId, NodeKind, SourcePosition, SourceSpan, ast::DocumentBuilder};

/// Bounded semantic-restructuring result consumed by the parser boundary.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct StructureOutcome {
    /// First source location whose `Head`/`Body` pair could not fit.
    pub(crate) node_limit_location: Option<SourceSpan>,
    /// Recoverable man scope findings, retained in source order.
    pub(crate) recoveries: Vec<Recovery>,
    /// No `.TH` request occurred, so a configured OS fallback must not be
    /// applied as if this were a titled man document.
    pub(crate) missing_title_control: bool,
}

/// One macro-package recovery that the parser boundary classifies as a typed
/// diagnostic after applying the shared report budget.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Recovery {
    /// The `.TH` manual title contains lower-case ASCII letters.
    TitleNotUppercase {
        /// 用于兼容可见诊断的原始标题拼写。
        title: Box<str>,
        /// Source location of the title argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` date remains visible but does not use a supported date form.
    TitleDateUnparseable {
        /// Authored date spelling retained in metadata.
        date: Box<str>,
        /// Source location of the date argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` date argument was explicitly empty.
    TitleDateMissing {
        /// Source location of the empty date argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` request omitted or emptied its title argument.
    TitleArgumentMissing {
        /// Source location of the title request or explicit empty argument.
        location: Option<SourceSpan>,
    },
    /// The `.TH` request omitted or emptied its section argument.
    TitleSectionMissing {
        /// Authored title used by the validator to identify the request.
        title: Option<Box<str>>,
        /// Source location of the title request or explicit empty argument.
        location: Option<SourceSpan>,
    },
    /// The document omitted a usable `.TH` title.
    MissingManualTitle,
    /// The document omitted a usable `.TH` date.
    MissingManualDate,
    /// The document had no visible body after its title metadata.
    NoDocumentBody,
    /// A closing macro did not correspond to any active semantic block.
    UnmatchedClose {
        /// Closing macro spelling.
        macro_name: &'static str,
        /// Source location of the closer, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// An open semantic block reached end of input without a closer.
    UnclosedBlock {
        /// Opening macro spelling.
        macro_name: &'static str,
        /// Source location of the opener, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// A next-line font element reached end of input before it received text.
    LineScopeBroken {
        /// Opening font macro spelling.
        macro_name: &'static str,
        /// Source location of the opener, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// A pending section title was interrupted by a following macro.
    LineScopeInterrupted {
        scope: Box<str>,
        breaker: Box<str>,
        location: Option<SourceSpan>,
    },
    /// A blank physical input line was skipped while a font scope remained open.
    BlankLineInScope {
        /// Source location of the blank line, when retained by the scanner.
        location: Option<SourceSpan>,
    },
    /// An empty implicit paragraph was discarded before a later scope boundary.
    EmptyParagraph {
        /// Paragraph macro spelling.
        macro_name: &'static str,
        /// Source location of the empty opener.
        location: Option<SourceSpan>,
    },
    /// A paragraph control immediately followed a section opener.
    ParagraphAfterSection {
        /// Authored paragraph-like request spelling.
        macro_name: &'static str,
        /// Section-level request that owns the empty body.
        section_name: &'static str,
        /// Source location of the paragraph opener.
        location: Option<SourceSpan>,
    },
    /// A redundant paragraph-like roff request was removed by its immediate
    /// sibling or containing man block validation.
    ParagraphSkip {
        /// Discarded request spelling.
        macro_name: &'static str,
        /// Fixed contextual relation, such as `before` or `after`.
        relation: &'static str,
        /// Request or macro spelling completing the diagnostic.
        context: &'static str,
        /// Source location of the discarded request, or its predecessor when
        /// an incoming `sp` discards a preceding `br`.
        location: Option<SourceSpan>,
    },
    /// A line-break request completed a section body.  mandoc retains the
    /// authored roff node but diagnoses its otherwise redundant placement.
    ParagraphAtSectionEnd {
        /// Authored paragraph-control spelling.
        macro_name: &'static str,
        /// Owning section-level macro spelling.
        section_name: &'static str,
        /// Source location of the retained request.
        location: Option<SourceSpan>,
    },
    ExcessArguments {
        macro_name: &'static str,
        argument: Box<str>,
        location: Option<SourceSpan>,
    },
    MissingResource {
        macro_name: &'static str,
        location: Option<SourceSpan>,
    },
    MissingOption {
        macro_name: &'static str,
        location: Option<SourceSpan>,
    },
    AllArguments {
        macro_name: &'static str,
        first_argument: Box<str>,
        has_more: bool,
        location: Option<SourceSpan>,
    },
    /// A roff request ignores its complete argument tail, whose full spelling
    /// is retained in the legacy diagnostic rather than abbreviated.
    IgnoredArguments {
        macro_name: &'static str,
        arguments: Box<str>,
        location: Option<SourceSpan>,
    },
    FewerIndents {
        target: usize,
        location: Option<SourceSpan>,
    },
    RedundantFillMode {
        message: &'static str,
        location: Option<SourceSpan>,
    },
    EmptyBlock {
        macro_name: &'static str,
        location: Option<SourceSpan>,
    },
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
    // source-ordered list of already-expanded roff events.  Keep all man
    // scope handling here, after roff execution, so macro expansion cannot
    // bypass document structure merely because it came from a `.de` body.
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
    let c_blank_followers = suppress_filled_c_blank_lines(builder, &flat);
    let centered_input_lines = attach_centered_input_lines(builder, &flat);

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

    for node in flat {
        if c_blank_followers.contains(&node) {
            continue;
        }
        if centered_input_lines.contains(&node) {
            continue;
        }
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
                            .filter(|title| !title.is_empty())
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

fn remove_child(
    builder: &mut DocumentBuilder,
    root: NodeId,
    root_children: &mut Vec<NodeId>,
    parent: NodeId,
    child: NodeId,
) {
    if parent == root {
        root_children.retain(|candidate| *candidate != child);
    } else if let Some(mut children) = builder.children(parent).map(<[NodeId]>::to_vec) {
        children.retain(|candidate| *candidate != child);
        let _ = builder.replace_children(parent, &children);
    }
}

/// Validate the finite argument surface of the man-ext optional-argument
/// macro without changing its ordinary inline AST projection.  Unlike `MT`,
/// `OP` retains all scanner children even after reporting a superfluous one.
fn validate_option_arguments(
    builder: &DocumentBuilder,
    node: NodeId,
    outcome: &mut StructureOutcome,
) {
    let arguments = builder.children(node).unwrap_or_default();
    if arguments.is_empty() {
        outcome.recoveries.push(Recovery::MissingOption {
            macro_name: "OP",
            location: builder.node_location(node),
        });
    } else if let Some(excess) = arguments.get(2).copied() {
        outcome.recoveries.push(Recovery::ExcessArguments {
            macro_name: "OP",
            argument: builder.node_text(excess).unwrap_or_default().into(),
            location: builder.node_location(excess),
        });
    }
}

/// Report and remove the first ignored argument of a fixed-arity man extension
/// macro.  `.PD` publishes only its accepted spacing argument in the AST.
fn validate_maximum_arguments(
    builder: &mut DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    maximum: usize,
    outcome: &mut StructureOutcome,
) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(excess) = arguments.get(maximum).copied() else {
        return;
    };
    outcome.recoveries.push(Recovery::ExcessArguments {
        macro_name,
        argument: builder.node_text(excess).unwrap_or_default().into(),
        location: builder.node_location(excess),
    });
    let _ = builder.replace_children(node, &arguments[..maximum]);
}

/// Report the legacy summary for arguments ignored by zero-argument paragraph
/// controls. These controls retain their scanner children in the public tree;
/// their compatibility behavior is solely the source-order diagnostic.
fn validate_no_arguments(
    builder: &DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    outcome: &mut StructureOutcome,
) {
    let Some(arguments) = builder.children(node) else {
        return;
    };
    let Some(first) = arguments.first().copied() else {
        return;
    };
    outcome.recoveries.push(Recovery::AllArguments {
        macro_name,
        first_argument: builder.node_text(first).unwrap_or_default().into(),
        has_more: arguments.len() > 1,
        location: builder.node_location(node),
    });
}

/// Roff's fill-mode requests accept no arguments.  Unlike the man paragraph
/// aliases, their entire raw tail is discarded and the native finding keeps
/// the complete spelling used by `MANDOCERR_ARG_SKIP`.
fn validate_and_discard_all_arguments(
    builder: &mut DocumentBuilder,
    node: NodeId,
    macro_name: &'static str,
    outcome: &mut StructureOutcome,
) {
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(first) = arguments.first().copied() else {
        return;
    };
    let arguments = arguments
        .iter()
        .filter_map(|argument| builder.node_text(*argument))
        .collect::<Vec<_>>()
        .join(" ");
    outcome.recoveries.push(Recovery::IgnoredArguments {
        macro_name,
        arguments: arguments.into(),
        location: builder.node_location(first),
    });
    let _ = builder.replace_children(node, &[]);
}

/// Parse `.RE`'s optional numeric target and preserve the legacy distinction
/// between a suffix on that number and a separately lexed argument.
fn re_target(
    builder: &DocumentBuilder,
    node: NodeId,
    outcome: &mut StructureOutcome,
) -> Option<usize> {
    let arguments = builder.children(node)?;
    let first = *arguments.first()?;
    let text = builder.node_text(first).unwrap_or_default();
    let prefix = text.bytes().take_while(u8::is_ascii_digit).count();
    if prefix < text.len() {
        outcome.recoveries.push(Recovery::ExcessArguments {
            macro_name: "RE",
            argument: text[prefix..].into(),
            location: argument_suffix_location(builder, first, prefix),
        });
    }
    for extra in &arguments[1..] {
        outcome.recoveries.push(Recovery::ExcessArguments {
            macro_name: "RE",
            argument: builder.node_text(*extra).unwrap_or_default().into(),
            location: builder.node_location(*extra),
        });
    }
    Some(text[..prefix].parse::<usize>().unwrap_or(0).max(1))
}

fn argument_suffix_location(
    builder: &DocumentBuilder,
    argument: NodeId,
    prefix: usize,
) -> Option<SourceSpan> {
    let mut location = builder.node_location(argument)?;
    let offset =
        u32::try_from(prefix + usize::from(builder.node_argument_quoted(argument))).ok()?;
    location.start = location.start.checked_add(offset)?;
    if let Some(position) = location.logical_start.as_mut() {
        position.column = position.column.checked_add(offset)?;
    }
    (location.start <= location.end).then_some(location)
}

fn blank_line_location(builder: &DocumentBuilder, node: NodeId) -> Option<SourceSpan> {
    let mut location = builder.node_location(node)?;
    let position = builder.node_source_position(node)?;
    location.logical_start = Some(SourcePosition {
        line: position.line,
        column: 1,
    });
    Some(location)
}

#[derive(Clone, Copy)]
struct ScopeFrame {
    open: NodeId,
    body: NodeId,
    resume_active: NodeId,
    resume_flow: NodeId,
}

enum IndentClose {
    Closed { frames: Vec<ScopeFrame> },
    NotOpen,
    FewerOpen { target: usize },
}

#[derive(Clone, Copy)]
struct ExplicitFrame {
    close: &'static str,
    open: NodeId,
    body: NodeId,
    resume_active: NodeId,
    resume_flow: NodeId,
}

fn normalize_pending_term_indent(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(argument) = builder
        .children(node)
        .and_then(|children| children.first())
        .copied()
    else {
        return;
    };
    let Some(value) = builder.node_text(argument) else {
        return;
    };
    if value.is_empty() || value.starts_with('+') || value.starts_with('-') {
        return;
    }
    let _ = builder.set_node_text(argument, format!("+{value}"));
}

#[derive(Clone, Copy, Debug)]
struct PendingHead {
    head: NodeId,
    body: Option<NodeId>,
    is_term: bool,
    block_parent: Option<(NodeId, NodeId)>,
}

impl PendingHead {
    const fn ordinary(block: NodeId, head: NodeId, body: NodeId, parent: NodeId) -> Self {
        Self {
            head,
            body: Some(body),
            is_term: false,
            block_parent: Some((block, parent)),
        }
    }

    const fn term(block: NodeId, head: NodeId, body: NodeId, parent: NodeId) -> Self {
        Self {
            head,
            body: Some(body),
            is_term: true,
            block_parent: Some((block, parent)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PendingElement {
    node: NodeId,
    parent: NodeId,
}

fn clear_pending(
    pending_head: &mut Option<PendingHead>,
    pending_element: &mut Option<PendingElement>,
) {
    *pending_head = None;
    *pending_element = None;
}

fn block_head_is_pending(builder: &DocumentBuilder, block: NodeId, name: &str) -> bool {
    matches!(name, "SH" | "SS")
        && builder
            .children(block)
            .and_then(|parts| parts.first())
            .and_then(|head| builder.children(*head))
            .is_some_and(<[NodeId]>::is_empty)
}

fn is_next_line_scoped_element(name: &str) -> bool {
    matches!(name, "SM" | "SB" | "R" | "B" | "I")
}

fn is_line_scoped_element(name: &str) -> bool {
    matches!(name, "SM" | "SB" | "R" | "B" | "I")
}

fn section_scope_name(builder: &DocumentBuilder, body: NodeId) -> &'static str {
    if builder.node_macro_name(body) == Some("SS") {
        "SS"
    } else {
        "SH"
    }
}

fn is_term_scope_breaker(name: &str) -> bool {
    matches!(
        name,
        "SH" | "SS"
            | "TP"
            | "TQ"
            | "IP"
            | "HP"
            | "LP"
            | "PP"
            | "P"
            | "RS"
            | "RE"
            | "UR"
            | "UE"
            | "MT"
            | "ME"
            | "SY"
            | "YS"
            | "TS"
    )
}

fn is_line_scope_breaker(name: &str) -> bool {
    is_term_scope_breaker(name)
}

fn line_scope_macro_name(builder: &DocumentBuilder, node: NodeId) -> Option<&'static str> {
    match builder.node_macro_name(node) {
        Some("B") => Some("B"),
        Some("I") => Some("I"),
        Some("R") => Some("R"),
        Some("SM") => Some("SM"),
        Some("SB") => Some("SB"),
        _ => None,
    }
}

/// Return line-scoped font Elements on the root-to-target path, excluding the
/// target itself. A nested empty font at EOF needs one validator finding for
/// each still-open outer scope, emitted from inner to outer.
fn line_scope_ancestors(builder: &DocumentBuilder, root: NodeId, target: NodeId) -> Vec<NodeId> {
    fn visit(
        builder: &DocumentBuilder,
        node: NodeId,
        target: NodeId,
        ancestors: &mut Vec<NodeId>,
    ) -> bool {
        if node == target {
            return true;
        }
        let scoped = line_scope_macro_name(builder, node).is_some();
        if scoped {
            ancestors.push(node);
        }
        if let Some(children) = builder.children(node) {
            for child in children {
                if visit(builder, *child, target, ancestors) {
                    return true;
                }
            }
        }
        if scoped {
            let _ = ancestors.pop();
        }
        false
    }

    let mut ancestors = Vec::new();
    if visit(builder, root, target, &mut ancestors) {
        ancestors
    } else {
        Vec::new()
    }
}

/// man(7) exposes an effective `\t` in a visible macro argument as a layout
/// tab, unlike the generic package AST escape projection.  The scanner keeps
/// a private copy-mode bit so an authored `\\t` remains literal text.
fn normalize_visible_macro_tabulation_escapes(builder: &mut DocumentBuilder, node: NodeId) {
    if !builder
        .node_macro_name(node)
        .is_some_and(is_visible_macro_with_tabulation_arguments)
    {
        return;
    }
    let Some(arguments) = builder.children(node).map(<[NodeId]>::to_vec) else {
        return;
    };
    for argument in arguments {
        let value = decode_man_tabulation_escapes(builder, argument);
        if value != builder.node_text(argument).unwrap_or_default() {
            let _ = builder.set_node_text(argument, value);
        }
    }
}

fn is_visible_macro_with_tabulation_arguments(name: &str) -> bool {
    matches!(
        name,
        "B" | "I"
            | "R"
            | "SM"
            | "SB"
            | "BR"
            | "BI"
            | "IB"
            | "IR"
            | "RB"
            | "RI"
            | "IP"
            | "HP"
            | "TP"
            | "TQ"
    )
}

/// The scanner tokenizes arguments for safe roff expansion, whereas these
/// man inline macros retain one line argument in libmandoc's public AST.
/// Reusing the first token preserves its exact source position without a
/// second arena allocation.
fn coalesce_text_children(builder: &mut DocumentBuilder, node: NodeId) {
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

/// `man_validate` treats tab-separated `.IP` tag words as one tag argument,
/// while retaining a later space-separated width as a second head child.  The
/// scanner deliberately tokenizes both separators so roff expansion remains
/// safe; reconstruct the package-level tag shape here before the temporary
/// separators are discarded.
fn coalesce_ip_tab_separated_tag(builder: &mut DocumentBuilder, head: NodeId) {
    let Some(children) = builder.children(head).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(&first) = children.first() else {
        return;
    };

    let mut value = decode_man_tabulation_escapes(builder, first);
    let mut changed = value.as_str() != builder.node_text(first).unwrap_or_default();
    let mut last = 0;
    while last + 1 < children.len() && builder.node_separator_after(children[last]) == Some(b'\t') {
        last += 1;
    }
    for index in 1..=last {
        let width = builder.node_separator_width(children[index - 1]).max(1);
        value.extend(std::iter::repeat_n('\t', width as usize));
        value.push_str(&decode_man_tabulation_escapes(builder, children[index]));
        changed = true;
    }
    if !changed || !builder.set_node_text(first, value) {
        return;
    }
    if last == 0 {
        return;
    }
    let _ = builder.set_node_separator_after(first, builder.node_separator_after(children[last]));
    let _ = builder
        .set_node_separator_width(first, builder.node_separator_width(children[last]) as usize);

    let mut retained = Vec::with_capacity(children.len() - last);
    retained.push(first);
    retained.extend_from_slice(&children[(last + 1)..]);
    let _ = builder.replace_children(head, &retained);
}

fn decode_man_tabulation_escapes(builder: &DocumentBuilder, node: NodeId) -> String {
    let value = builder.node_text(node).unwrap_or_default();
    if builder.node_has_protected_tabulation_escape(node) {
        value.to_owned()
    } else {
        decode_unescaped_tabulation_escapes(value)
    }
}

/// Decode a tabulation escape only when it was not protected by a preceding
/// escaped backslash.  `man_validate` treats an effective `\t` in an `.IP`
/// tag as the same layout tabulation as a literal tab, but `\\t` remains
/// authored text.
fn decode_unescaped_tabulation_escapes(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut decoded = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\\' && bytes.get(cursor + 1) == Some(&b'\\') {
            decoded.push('\\');
            cursor += 2;
        } else if bytes[cursor] == b'\\' && bytes.get(cursor + 1) == Some(&b't') {
            decoded.push('\t');
            cursor += 2;
        } else {
            let Some(character) = value[cursor..].chars().next() else {
                break;
            };
            decoded.push(character);
            cursor += character.len_utf8();
        }
    }
    decoded
}

/// Attach the consecutive input lines owned by roff's `.ce` and `.rj`
/// requests before man block restructuring.  The roff parser records these
/// as Element children, even though the request itself is presentation-only.
/// A following macro aborts the pending request rather than becoming content.
fn attach_centered_input_lines(builder: &mut DocumentBuilder, flat: &[NodeId]) -> Vec<NodeId> {
    let mut attached = Vec::new();
    for (index, node) in flat.iter().copied().enumerate() {
        if !matches!(builder.node_macro_name(node), Some("ce" | "rj")) {
            continue;
        }
        let lines = builder
            .children(node)
            .and_then(|children| children.first())
            .and_then(|argument| builder.node_text(*argument))
            .and_then(|argument| argument.parse::<usize>().ok())
            .filter(|lines| *lines > 0)
            .unwrap_or(1);
        let mut text_nodes = Vec::with_capacity(lines);
        for candidate in flat.iter().copied().skip(index + 1) {
            if text_nodes.len() == lines || builder.node_kind(candidate) != Some(NodeKind::Text) {
                break;
            }
            text_nodes.push(candidate);
        }
        if text_nodes.is_empty() {
            continue;
        }
        let mut children = builder
            .children(node)
            .map(<[NodeId]>::to_vec)
            .unwrap_or_default();
        children.extend(text_nodes.iter().copied());
        if builder.replace_children(node, &children) {
            attached.extend(text_nodes);
        }
    }
    attached
}

fn close_indents(
    _builder: &DocumentBuilder,
    indents: &mut Vec<ScopeFrame>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
    target: Option<usize>,
) -> IndentClose {
    let levels = match target {
        None => 1,
        Some(target) if target > indents.len() => return IndentClose::FewerOpen { target },
        Some(target) => indents.len() + 1 - target,
    };
    let mut frames = Vec::with_capacity(levels);
    for _ in 0..levels {
        let Some(frame) = indents.pop() else {
            return IndentClose::NotOpen;
        };
        *active_body = frame.resume_active;
        *flow_parent = frame.resume_flow;
        frames.push(frame);
    }
    IndentClose::Closed { frames }
}

fn close_explicit(
    close: &str,
    explicit_blocks: &mut Vec<ExplicitFrame>,
    active_body: &mut NodeId,
    flow_parent: &mut NodeId,
) -> Option<ExplicitFrame> {
    let index = explicit_blocks
        .iter()
        .rposition(|frame| frame.close == close)?;
    let frame = explicit_blocks[index];
    explicit_blocks.truncate(index);
    *active_body = frame.resume_active;
    *flow_parent = frame.resume_flow;
    Some(frame)
}

/// Apply the man presentation flags that are stateful but do not affect roff
/// expansion.  This must run against source-order scanner events before they
/// are moved into blocks, otherwise a later `.fi` could accidentally change
/// the interpretation of text that preceded it in the arena.
fn apply_presentation_flags(builder: &mut DocumentBuilder, flat: &[NodeId]) {
    let mut no_fill = false;
    for node in flat {
        let macro_name = builder.node_macro_name(*node).map(str::to_owned);
        let presentation_toggle = matches!(macro_name.as_deref(), Some("nf" | "fi" | "EX" | "EE"));
        if presentation_toggle {
            // libmandoc records the controlling macro in the state that
            // preceded it, while the macro's arguments already observe its
            // new fill mode. For example, `.EX args` itself is filled but
            // its arguments are no-fill; `.EE args` is no-fill while its
            // arguments are filled. Preserve that distinction in the arena
            // rather than applying one flag to the entire scanner subtree.
            if no_fill {
                mark_node_no_fill(builder, *node);
            }
            no_fill = matches!(macro_name.as_deref(), Some("nf" | "EX"));
            if no_fill {
                mark_children_no_fill(builder, *node);
            }
        } else if no_fill {
            mark_subtree_no_fill(builder, *node);
        }
        mark_sentence_end(builder, *node);
    }
}

/// In filled man input, a final `\c` immediately before a blank physical
/// line is recovered as ordinary text without the control spelling; the
/// blank line itself is not published. In no-fill mode both source events
/// remain observable. Perform this after the presentation pass, when the
/// scanner-owned nodes carry their effective fill state but before they are
/// assigned to public man blocks.
fn suppress_filled_c_blank_lines(builder: &mut DocumentBuilder, flat: &[NodeId]) -> Vec<NodeId> {
    let mut suppressed = Vec::new();
    for pair in flat.windows(2) {
        let [text, blank] = pair else {
            continue;
        };
        if builder.node_kind(*text) != Some(NodeKind::Text)
            || builder.node_kind(*blank) != Some(NodeKind::Text)
            || builder.node_text(*blank) != Some("")
        {
            continue;
        }
        let Some(value) = builder.node_text(*text).map(str::to_owned) else {
            continue;
        };
        let Some(value) = value.strip_suffix("\\c") else {
            continue;
        };
        let Some(mut flags) = builder.node_flags(*text) else {
            continue;
        };
        if flags.no_fill || !flags.line_continuation || value.ends_with("\\z") {
            continue;
        }
        if builder.text(*text, value) {
            flags.line_continuation = false;
            let _ = builder.set_node_flags(*text, flags);
            suppressed.push(*blank);
        }
    }
    suppressed
}

fn mark_subtree_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        mark_node_no_fill(builder, node);
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied());
        }
    }
}

#[allow(clippy::unnecessary_to_owned)] // The owned edge list ends the arena borrow before mutation.
fn mark_children_no_fill(builder: &mut DocumentBuilder, root: NodeId) {
    let Some(children) = builder.children(root) else {
        return;
    };
    // Copy the narrow edge slice before recursive flag mutation so the arena
    // does not carry an immutable borrow across its mutable traversal.
    let children = children.to_vec();
    for child in children {
        mark_subtree_no_fill(builder, child);
    }
}

fn mark_node_no_fill(builder: &mut DocumentBuilder, node: NodeId) {
    let Some(mut flags) = builder.node_flags(node) else {
        return;
    };
    flags.no_fill = true;
    let _ = builder.set_node_flags(node, flags);
}

/// The source-order fill pass runs before man blocks have their `Head` and
/// `Body` nodes.  A no-fill mode therefore temporarily reaches a raw `TP` or
/// `IP` request and its lexical arguments.  In the public tree mandoc keeps
/// that state only on body flow, never on the structural nodes or on a term
/// head.  Normalize these staging-only flags after the tree is assembled.
fn clear_no_fill_from_man_structure(builder: &mut DocumentBuilder) {
    let mut pending = vec![(DocumentBuilder::root(), false)];
    while let Some((node, inside_head)) = pending.pop() {
        let kind = builder.node_kind(node);
        let head_context = inside_head || kind == Some(NodeKind::Head);
        if (inside_head
            || matches!(
                kind,
                Some(NodeKind::Block | NodeKind::Head | NodeKind::Body)
            ))
            && let Some(mut flags) = builder.node_flags(node)
        {
            flags.no_fill = false;
            let _ = builder.set_node_flags(node, flags);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().map(|child| (*child, head_context)));
        }
    }
}

/// A terminal dot in a man section title is heading text, not a filled-flow
/// sentence boundary.  The source-order presentation pass runs before Heads
/// exist, so normalize this package distinction after block construction.
fn clear_sentence_end_from_section_heads(builder: &mut DocumentBuilder) {
    let mut pending = vec![DocumentBuilder::root()];
    while let Some(node) = pending.pop() {
        let section_head = builder.node_kind(node) == Some(NodeKind::Head)
            && matches!(builder.node_macro_name(node), Some("SH" | "SS"));
        if section_head {
            if let Some(children) = builder.children(node).map(<[NodeId]>::to_vec) {
                for child in children {
                    if let Some(mut flags) = builder.node_flags(child) {
                        // A heading supplied as an omitted `.SH`/`.SS`
                        // argument is the next physical text line. It keeps
                        // ordinary text sentence state, unlike an authored
                        // same-line heading argument.
                        if !flags.line_start {
                            flags.sentence_end = false;
                            let _ = builder.set_node_flags(child, flags);
                        }
                    }
                }
            }
            continue;
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().rev().copied());
        }
    }
}

fn mark_sentence_end(builder: &mut DocumentBuilder, node: NodeId) {
    let candidate = builder
        .node_text(node)
        .is_some()
        .then_some(node)
        .or_else(|| {
            builder
                .children(node)
                .and_then(|children| children.last())
                .copied()
        });
    let Some(candidate) = candidate else {
        return;
    };
    let Some(text) = builder.node_text(candidate) else {
        return;
    };
    let terminal = text.trim_end_matches(['"', '\'', ')', ']', '}']);
    if !terminal.ends_with(['.', '!', '?']) {
        return;
    }
    let Some(mut flags) = builder.node_flags(candidate) else {
        return;
    };
    // Literal/no-fill lines preserve their physical presentation verbatim;
    // mandoc does not promote a terminal dot in that mode into sentence
    // punctuation for later layout decisions.
    if flags.no_fill {
        return;
    }
    flags.sentence_end = true;
    let _ = builder.set_node_flags(candidate, flags);
}

/// `man_validate` discards a leading non-empty `.PP` after a section heading
/// and relinks its body into the section. This preserves the parser's visible
/// tree without needing to move arena records or expose a special recovery
/// wrapper to consumers.
fn flatten_leading_section_paragraphs(
    builder: &mut DocumentBuilder,
    sections: &[NodeId],
    blockers: &[NodeId],
) {
    for section in sections {
        // `man_validate` only inspects the section's original first child.
        // If that was an ignored `br`/`sp`, removing it does not cause the
        // following PP to be reconsidered and flattened.
        if blockers.contains(section) {
            continue;
        }
        let Some(children) = builder.children(*section).map(<[NodeId]>::to_vec) else {
            continue;
        };
        let Some(paragraph) = children.first().copied() else {
            continue;
        };
        if builder.node_macro_name(paragraph) != Some("PP") {
            continue;
        }
        let Some(parts) = builder.children(paragraph) else {
            continue;
        };
        let Some(paragraph_body) = parts.get(1).copied() else {
            continue;
        };
        let Some(body_children) = builder.children(paragraph_body) else {
            continue;
        };
        if body_children.is_empty() {
            continue;
        }
        let mut replacement = body_children.to_vec();
        replacement.extend_from_slice(&children[1..]);
        let _ = builder.replace_children(*section, &replacement);
    }
}

/// Apply the roff-level paragraph-request checks that operate within a
/// completed man body.  This deliberately runs before section validation:
/// libmandoc reports local `br`/`sp` and PP-body recoveries in source order,
/// then validates enclosing section bodies afterwards.
fn validate_inline_paragraph_controls(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    recoveries: &mut Vec<Recovery>,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for child in &children {
        validate_inline_paragraph_controls(builder, *child, recoveries);
    }

    let parent_is_paragraph_body = builder.node_kind(parent) == Some(NodeKind::Body)
        && builder.node_macro_name(parent) == Some("PP");
    let mut retained = builder
        .children(parent)
        .map(<[NodeId]>::to_vec)
        .unwrap_or_default();
    let mut index = 0;
    while index < retained.len() {
        let node = retained[index];
        // An empty completed tbl range lowers to the legacy closing-spacing
        // node. It is public `sp` syntax, but it is not an authored paragraph
        // control and must not receive a second man(7) recovery.
        if builder.node_preprocessor_opener(node) == Some("TS") {
            index += 1;
            continue;
        }
        let macro_name = builder.node_macro_name(node);
        let previous = index
            .checked_sub(1)
            .and_then(|previous| retained.get(previous).copied())
            .and_then(|previous| builder.node_macro_name(previous));
        // `node_macro_name` borrows the arena.  Retain only the three
        // paragraph-control names this validator recognises so recovery data
        // remains independent of that borrow (and intentionally static).
        let previous_control = match previous {
            Some("br") => Some("br"),
            Some("sp") => Some("sp"),
            Some("PP") => Some("PP"),
            _ => None,
        };
        match macro_name {
            Some("br") => {
                // `check_par()` treats only the first request inside a PP
                // body as following the paragraph macro.  Text before a
                // later `.br` is ordinary flow, not a PP-control sibling.
                let context =
                    previous_control.or((index == 0 && parent_is_paragraph_body).then_some("PP"));
                if let Some(context @ ("br" | "sp" | "PP")) = context {
                    recoveries.push(Recovery::ParagraphSkip {
                        macro_name: "br",
                        relation: "after",
                        context,
                        location: builder.node_location(node),
                    });
                    retained.remove(index);
                    continue;
                }
            }
            Some("sp") => {
                if previous_control == Some("br") {
                    let previous = retained[index - 1];
                    recoveries.push(Recovery::ParagraphSkip {
                        macro_name: "br",
                        relation: "before",
                        context: "sp",
                        location: builder.node_location(previous),
                    });
                    retained.remove(index - 1);
                    index = index.saturating_sub(1);
                    // A bare conditional opener contributes a generated
                    // `sp`.  Mandoc discards the preceding `br` during this
                    // recovery, but retains the resulting vertical space.
                    continue;
                }
                if index == 0 && parent_is_paragraph_body {
                    recoveries.push(Recovery::ParagraphSkip {
                        macro_name: "sp",
                        relation: "after",
                        context: "PP",
                        location: builder.node_location(node),
                    });
                    retained.remove(index);
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }
    let _ = builder.replace_children(parent, &retained);
}

/// Validate a section body after its nested bodies.  Upstream keeps this
/// separate from the local roff validation above, which is why an initial
/// `sp` is diagnosed after all ordinary sibling-level paragraph recoveries.
fn validate_section_paragraph_controls(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    recoveries: &mut Vec<Recovery>,
    deferred: &mut Vec<(NodeId, Recovery)>,
    flatten_blockers: &mut Vec<NodeId>,
) {
    let Some(children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    for child in children {
        validate_section_paragraph_controls(builder, child, recoveries, deferred, flatten_blockers);
    }
    let Some(section_name) = (match builder.node_macro_name(parent) {
        Some("SH") => Some("SH"),
        Some("SS") => Some("SS"),
        _ => None,
    }) else {
        return;
    };
    if builder.node_kind(parent) != Some(NodeKind::Body) {
        return;
    }
    let mut remaining = Vec::with_capacity(deferred.len());
    for (owner, recovery) in std::mem::take(deferred) {
        if owner == parent {
            if matches!(
                &recovery,
                Recovery::ParagraphAfterSection {
                    macro_name: "br" | "sp",
                    ..
                }
            ) && !flatten_blockers.contains(&parent)
            {
                flatten_blockers.push(parent);
            }
            recoveries.push(recovery);
        } else {
            remaining.push((owner, recovery));
        }
    }
    *deferred = remaining;
    let Some(section_children) = builder.children(parent).map(<[NodeId]>::to_vec) else {
        return;
    };
    let Some(first) = section_children.first().copied() else {
        return;
    };
    if builder.node_preprocessor_opener(first) == Some("TS") {
        return;
    }
    if let Some(macro_name) = match builder.node_macro_name(first) {
        Some("br") => Some("br"),
        Some("sp") => Some("sp"),
        _ => None,
    } {
        recoveries.push(Recovery::ParagraphSkip {
            macro_name,
            relation: "after",
            context: section_name,
            location: builder.node_location(first),
        });
        if !flatten_blockers.contains(&parent) {
            flatten_blockers.push(parent);
        }
        let _ = builder.replace_children(parent, &section_children[1..]);
        return;
    }
    if let Some(last) = section_children.last().copied()
        && builder.node_macro_name(last) == Some("br")
    {
        // `man_validate` discards a terminal line break after recording this
        // post-validation recovery.
        recoveries.push(Recovery::ParagraphAtSectionEnd {
            macro_name: "br",
            section_name,
            location: builder.node_location(last),
        });
        let _ = builder.replace_children(parent, &section_children[..section_children.len() - 1]);
    }
}

fn paragraph_recovery_offset(recovery: &Recovery) -> u32 {
    match recovery {
        Recovery::EmptyParagraph { location, .. }
        | Recovery::ParagraphSkip { location, .. }
        | Recovery::ParagraphAtSectionEnd { location, .. } => location
            .as_ref()
            .map_or(u32::MAX, |location| location.start),
        _ => u32::MAX,
    }
}

/// Mark the man heading nodes that libmandoc validates as same-document
/// destinations. The common one-word form deliberately keeps `tag` absent:
/// libmandoc reuses that child text as the destination instead of allocating a
/// second string, while the two boolean flags preserve the public semantic
/// contract used by navigation lowering. Multiword SH/SS tags are fallback
/// names, so a duplicate suppresses both targets rather than silently choosing
/// an arbitrary source-order winner.
fn mark_man_targets(builder: &mut DocumentBuilder, heads: &[NodeId]) {
    let mut tags = std::collections::BTreeMap::<String, TagEntry>::new();
    for head in heads {
        let Some(macro_name) = builder.node_macro_name(*head).map(str::to_owned) else {
            continue;
        };
        if matches!(macro_name.as_str(), "SH" | "SS") {
            let heading = visible_head_text(builder, *head);
            let tag = heading.split_whitespace().collect::<Vec<_>>().join("_");
            let Some(text) = first_head_text(builder, *head) else {
                continue;
            };
            if tag.is_empty() {
                continue;
            }
            let direct_text = builder
                .children(*head)
                .and_then(|children| children.first())
                .and_then(|child| builder.node_text(*child));
            let priority = if tag == heading {
                TagPriority::Strong
            } else {
                TagPriority::Fallback
            };
            let explicit = direct_text != Some(text) || tag != text;
            register_man_tag(
                builder,
                &mut tags,
                *head,
                &TagCandidate {
                    name: tag,
                    priority,
                    explicit,
                },
            );
            continue;
        }

        let candidate_text = match macro_name.as_str() {
            "IP" => builder
                .children(*head)
                .and_then(|children| children.first())
                .and_then(|child| builder.node_text(*child).map(|text| (*child, text))),
            "TP" | "TQ" => first_man_term_text(builder, *head),
            _ => None,
        };
        let Some((text_node, text)) = candidate_text else {
            continue;
        };
        let Some((name, priority, start, end)) = parse_man_tag(text) else {
            continue;
        };
        let direct_text_node = builder
            .children(*head)
            .and_then(|children| children.first())
            .copied()
            .filter(|child| builder.node_text(*child).is_some());
        register_man_tag(
            builder,
            &mut tags,
            *head,
            &TagCandidate {
                name,
                priority,
                explicit: direct_text_node != Some(text_node) || start != 0 || end != text.len(),
            },
        );
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum TagPriority {
    Strong,
    Weak,
    Fallback,
    Deleted,
}

#[derive(Debug)]
struct TagCandidate {
    name: String,
    priority: TagPriority,
    explicit: bool,
}

#[derive(Debug)]
struct TagEntry {
    priority: TagPriority,
    heads: Vec<NodeId>,
}

/// Register a man(7) automatic target with the same priority transitions as
/// libmandoc's `tag_put`: strong names win over weak names, fallback section
/// names only survive when unique, and equal-strength names retain every
/// source-order occurrence.
fn register_man_tag(
    builder: &mut DocumentBuilder,
    tags: &mut std::collections::BTreeMap<String, TagEntry>,
    head: NodeId,
    candidate: &TagCandidate,
) {
    use std::collections::btree_map::Entry;

    match tags.entry(candidate.name.clone()) {
        Entry::Vacant(entry) => {
            mark_target(
                builder,
                head,
                candidate.explicit.then_some(candidate.name.as_str()),
            );
            entry.insert(TagEntry {
                priority: candidate.priority,
                heads: vec![head],
            });
        }
        Entry::Occupied(mut occupied) => {
            let entry = occupied.get_mut();
            if entry.priority < candidate.priority {
                return;
            }
            if entry.priority > candidate.priority || candidate.priority == TagPriority::Fallback {
                for previous in entry.heads.drain(..) {
                    clear_target(builder, previous);
                }
                if candidate.priority == TagPriority::Fallback {
                    entry.priority = TagPriority::Deleted;
                    return;
                }
                entry.priority = candidate.priority;
            }
            mark_target(
                builder,
                head,
                candidate.explicit.then_some(candidate.name.as_str()),
            );
            entry.heads.push(head);
        }
    }
}

/// Mirror `man_validate.c:check_tag` over the preserved AST spelling.  The
/// parser intentionally retains the small escape subset used for automatic
/// anchors so source-compatible tags do not depend on renderer state.
fn parse_man_tag(text: &str) -> Option<(String, TagPriority, usize, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    let mut priority = TagPriority::Strong;
    loop {
        match *bytes.get(cursor)? {
            b' ' | b'\t' => {
                priority = TagPriority::Weak;
                cursor += 1;
            }
            b'-' => cursor += 1,
            b'\\' => cursor = skip_tag_escape(bytes, cursor)?,
            byte if byte.is_ascii_alphabetic() => {
                let start = cursor;
                while let Some(byte) = bytes.get(cursor) {
                    if matches!(*byte, b' ' | b'\t' | b'\\') {
                        break;
                    }
                    cursor += 1;
                }
                if cursor == start {
                    return None;
                }
                if cursor != bytes.len() && priority == TagPriority::Strong {
                    priority = TagPriority::Weak;
                }
                return Some((text[start..cursor].to_owned(), priority, start, cursor));
            }
            _ => return None,
        }
    }
}

/// Advance over the exact leading escape families accepted by libmandoc's
/// tag validator: font changes, ignored zero-width controls, and the two
/// printable one-character special escapes it treats as a dash/backslash.
fn skip_tag_escape(bytes: &[u8], slash: usize) -> Option<usize> {
    let kind = *bytes.get(slash + 1)?;
    match kind {
        b'&' | b'-' | b'e' => Some(slash + 2),
        b'f' => match *bytes.get(slash + 2)? {
            b'[' => bytes[slash + 3..]
                .iter()
                .position(|byte| *byte == b']')
                .map(|offset| slash + 4 + offset),
            b'(' => bytes.get(slash + 4).map(|_| slash + 5),
            _ => Some(slash + 3),
        },
        _ => None,
    }
}

/// `post_TP` considers the first node that starts on a subsequent physical
/// source line.  If it is one of six font macros, it peels exactly one level;
/// deeper macro nesting deliberately does not become an automatic target.
fn first_man_term_text(builder: &DocumentBuilder, head: NodeId) -> Option<(NodeId, &str)> {
    let term = builder.children(head)?.iter().copied().find(|node| {
        builder
            .node_flags(*node)
            .is_some_and(|flags| flags.line_start)
    })?;
    let term = match builder.node_macro_name(term) {
        Some("B" | "BI" | "BR" | "I" | "IB" | "IR") => *builder.children(term)?.first()?,
        _ => term,
    };
    builder.node_text(term).map(|text| (term, text))
}

fn mark_target(builder: &mut DocumentBuilder, head: NodeId, tag: Option<&str>) {
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

fn clear_target(builder: &mut DocumentBuilder, head: NodeId) {
    let Some(mut flags) = builder.node_flags(head) else {
        return;
    };
    flags.deep_link_target = false;
    flags.permalink = false;
    let _ = builder.set_node_flags(head, flags);
    let _ = builder.clear_node_tag(head);
}

fn first_head_text(builder: &DocumentBuilder, head: NodeId) -> Option<&str> {
    let first = *builder.children(head)?.first()?;
    builder.node_text(first).or_else(|| {
        builder
            .children(first)
            .and_then(|children| children.first())
            .and_then(|child| builder.node_text(*child))
    })
}

fn visible_head_text(builder: &DocumentBuilder, head: NodeId) -> String {
    let mut text = Vec::new();
    let mut pending = builder
        .children(head)
        .map(|children| children.iter().copied().rev().collect::<Vec<_>>())
        .unwrap_or_default();
    while let Some(node) = pending.pop() {
        if let Some(value) = builder.node_text(node) {
            text.push(value);
        }
        if let Some(children) = builder.children(node) {
            pending.extend(children.iter().copied().rev());
        }
    }
    text.join(" ")
}

fn append_to_active(
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

fn make_block(
    builder: &mut DocumentBuilder,
    block: NodeId,
    macro_name: &str,
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
        || !builder.replace_children(head, &arguments)
    {
        return None;
    }
    Some((head, body))
}

fn record_title_metadata(builder: &mut DocumentBuilder, title: NodeId) {
    let values = builder
        .children(title)
        .into_iter()
        .flatten()
        .filter_map(|argument| builder.node_text(*argument))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let metadata = builder.metadata_mut();
    metadata.title = Some(values.first().cloned().unwrap_or_default().into_boxed_str());
    metadata.section = Some(values.get(1).cloned().unwrap_or_default().into_boxed_str());
    metadata.date = Some(
        values
            .get(2)
            .map_or_else(String::new, |date| normalize_title_date(date))
            .into_boxed_str(),
    );
    metadata.os = values.get(3).map(|value| value.clone().into_boxed_str());
    metadata.volume = values
        .get(4)
        .cloned()
        .or_else(|| metadata.section.as_deref().and_then(default_volume))
        .map(String::into_boxed_str);
}

fn title_lowercase(builder: &DocumentBuilder, title: NodeId) -> Option<(Box<str>, SourceSpan)> {
    let argument = builder
        .children(title)
        .and_then(|arguments| arguments.first())
        .copied()?;
    let title = builder.node_text(argument)?;
    let location = builder.node_location(argument)?;
    // `decode_visible_bytes` maps each malformed source byte to a Unicode
    // scalar. Its UTF-8 representation may take more than one byte, so a
    // string-byte index cannot be added to a raw source offset in that case.
    let offset = if builder.node_has_invalid_input_bytes(argument) {
        title
            .chars()
            .position(|character| character.is_ascii_lowercase())?
    } else {
        title.bytes().position(|byte| byte.is_ascii_lowercase())?
    };
    let offset = u32::try_from(offset).ok()?;
    // Expansion recovery can make the visible spelling wider than its
    // authored argument. Keep every public recovery location within the
    // argument's validated physical source range even in that degraded case.
    let start = location.start.saturating_add(offset).min(location.end);
    let end = start.saturating_add(1).min(location.end);
    let location = SourceSpan::new(location.source, start, end).ok()?;
    Some((title.to_owned().into_boxed_str(), location))
}

fn title_unparseable_date(
    builder: &DocumentBuilder,
    title: NodeId,
) -> Option<(Box<str>, Option<SourceSpan>)> {
    let argument = title_date_argument(builder, title)?;
    let date = builder.node_text(argument)?;
    (!is_supported_title_date(date))
        .then(|| (date.to_owned().into(), builder.node_location(argument)))
}

fn title_date_argument(builder: &DocumentBuilder, title: NodeId) -> Option<NodeId> {
    builder.children(title)?.get(2).copied()
}

fn title_argument(builder: &DocumentBuilder, title: NodeId) -> Option<NodeId> {
    builder.children(title)?.first().copied()
}

fn title_section_argument(builder: &DocumentBuilder, title: NodeId) -> Option<NodeId> {
    builder.children(title)?.get(1).copied()
}

fn title_argument_missing(builder: &DocumentBuilder, title: NodeId) -> bool {
    title_argument(builder, title)
        .and_then(|argument| builder.node_text(argument))
        .is_none_or(str::is_empty)
}

fn title_section_missing(builder: &DocumentBuilder, title: NodeId) -> bool {
    title_section_argument(builder, title)
        .and_then(|argument| builder.node_text(argument))
        .is_none_or(str::is_empty)
}

fn title_missing_date(builder: &DocumentBuilder, title: NodeId) -> bool {
    let explicit_empty_date = title_date_argument(builder, title)
        .and_then(|argument| builder.node_text(argument))
        .is_some_and(str::is_empty);
    explicit_empty_date || title_section_missing(builder, title)
}

/// Accept the stable man(7) date spellings that mandoc normalizes without a
/// recovery finding. Unknown author-supplied text remains public metadata,
/// but is reported through `TitleDateUnparseable`.
fn is_supported_title_date(date: &str) -> bool {
    if date.is_empty() {
        return true;
    }
    let numeric = date.as_bytes();
    if numeric.len() == 10
        && numeric[4] == b'-'
        && numeric[7] == b'-'
        && numeric
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
    {
        return true;
    }
    let Some((day, month, year)) = date.split_once('-').and_then(|(day, rest)| {
        let (month, year) = rest.split_once('-')?;
        Some((day, month, year))
    }) else {
        return matches!(date.split_whitespace().collect::<Vec<_>>().as_slice(), [month, day, year]
            if month_name(month)
                && day.strip_suffix(',').is_some_and(|day| day.parse::<u8>().is_ok())
                && year.len() == 4
                && year.bytes().all(|byte| byte.is_ascii_digit()));
    };
    day.parse::<u8>().is_ok()
        && month_name(month)
        && year.len() == 4
        && year.bytes().all(|byte| byte.is_ascii_digit())
}

/// Canonicalize the named month accepted by man(7)'s title-date grammar.
///
/// The owned AST retains the authored `.TH` argument, while document metadata
/// follows mandoc's stable long-month presentation. This keeps abbreviated
/// Sphinx dates such as `Jul 31, 2026` equivalent to `July 31, 2026` without
/// rewriting unsupported author text.
fn normalize_title_date(date: &str) -> String {
    let mut fields = date.split_whitespace();
    let Some(month) = fields.next().and_then(normalize_title_month) else {
        return date.to_owned();
    };
    let Some(day) = fields
        .next()
        .and_then(|day| day.strip_suffix(',').unwrap_or(day).parse::<u8>().ok())
    else {
        return date.to_owned();
    };
    let Some(year) = fields.next().and_then(|year| year.parse::<u16>().ok()) else {
        return date.to_owned();
    };
    if fields.next().is_some() || day == 0 {
        return date.to_owned();
    }
    format!("{month} {day}, {year:04}")
}

fn normalize_title_month(value: &str) -> Option<&'static str> {
    match value {
        "Jan" | "January" => Some("January"),
        "Feb" | "February" => Some("February"),
        "Mar" | "March" => Some("March"),
        "Apr" | "April" => Some("April"),
        "May" => Some("May"),
        "Jun" | "June" => Some("June"),
        "Jul" | "July" => Some("July"),
        "Aug" | "August" => Some("August"),
        "Sep" | "September" => Some("September"),
        "Oct" | "October" => Some("October"),
        "Nov" | "November" => Some("November"),
        "Dec" | "December" => Some("December"),
        _ => None,
    }
}

fn month_name(value: &str) -> bool {
    matches!(
        value,
        "Jan"
            | "Feb"
            | "Mar"
            | "Apr"
            | "May"
            | "Jun"
            | "Jul"
            | "Aug"
            | "Sep"
            | "Oct"
            | "Nov"
            | "Dec"
            | "January"
            | "February"
            | "March"
            | "April"
            | "June"
            | "July"
            | "August"
            | "September"
            | "October"
            | "November"
            | "December"
    )
}

fn default_volume(section: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use crate::{DiagnosticCode, NodeKind, NodeRef, Parser, Source, SourceName};

    #[test]
    fn structures_sections_terms_and_indents_from_executed_scanner_nodes() {
        let name = SourceName::new("man-structure.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH STRUCTURE 1 \"August 25, 2026\" x Manual\n.SH FIRST\nouter\n.SS CHILD\n.TP\nterm\ndefinition\n.RS\nindented\n.RE\n",
            ))
            .unwrap();
        let document = &report.document;
        assert_eq!(document.metadata().title.as_deref(), Some("STRUCTURE"));
        assert_eq!(document.metadata().section.as_deref(), Some("1"));
        assert_eq!(document.metadata().date.as_deref(), Some("August 25, 2026"));
        assert_eq!(document.metadata().os.as_deref(), Some("x"));
        assert_eq!(document.metadata().volume.as_deref(), Some("Manual"));

        let root = document.node(document.root()).unwrap();
        let section = root
            .children()
            .find(|node| node.macro_name() == Some("SH"))
            .unwrap();
        assert_eq!(section.kind(), NodeKind::Block);
        let mut section_parts = section.children();
        let head = section_parts.next().unwrap();
        let body = section_parts.next().unwrap();
        assert_eq!(head.kind(), NodeKind::Head);
        assert_eq!(head.macro_name(), Some("SH"));
        assert_eq!(head.children().next().unwrap().text(), Some("FIRST"));
        assert_eq!(body.kind(), NodeKind::Body);
        let subsection = body
            .children()
            .find(|node| node.macro_name() == Some("SS"))
            .unwrap();
        let subsection_body = subsection.children().nth(1).unwrap();
        let term = subsection_body
            .children()
            .find(|node| node.macro_name() == Some("TP"))
            .unwrap();
        assert_eq!(term.kind(), NodeKind::Block);
        let term_head = term.children().next().unwrap();
        assert_eq!(term_head.kind(), NodeKind::Head);
        assert_eq!(term_head.children().next().unwrap().text(), Some("term"));
        assert!(section.children().next().unwrap().flags().deep_link_target);
        assert!(section.children().next().unwrap().flags().permalink);
    }

    #[test]
    fn structures_man_indents_emitted_by_a_user_macro() {
        let name = SourceName::new("man-macro-indent.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".de1 INDENT\n. RS \\\\$1\n..\n.de UNINDENT\n. RE\n..\n.TH INDENT 1\n.SH DESCRIPTION\nintro\n.INDENT 0.0\n.TP\nterm\ndescription\n.UNINDENT\n",
            ))
            .unwrap();
        let indent = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("RS"))
            .expect("macro-generated RS block");
        assert_eq!(indent.kind(), NodeKind::Block);
        let mut parts = indent.children();
        let head = parts.next().expect("RS head");
        assert_eq!(head.kind(), NodeKind::Head);
        assert_eq!(head.children().next().and_then(NodeRef::text), Some("0.0"));
        let body = parts.next().expect("RS body");
        assert_eq!(body.kind(), NodeKind::Body);
        assert!(body.children().any(|node| node.macro_name() == Some("TP")));
    }

    #[test]
    fn normalizes_abbreviated_title_months_in_metadata() {
        let name = SourceName::new("man-title-date.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TITLE 1 \"Jul 31, 2026\"\n.SH NAME\ntitle\n",
            ))
            .unwrap();
        assert_eq!(
            report.document.metadata().date.as_deref(),
            Some("July 31, 2026")
        );
    }

    #[test]
    fn inline_conditional_dispatches_man_request_body() {
        let name = SourceName::new("man-conditional-pod.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH OPTION 1\n.SH DESCRIPTION\n.ie n .IP \"*<\"\"\\-fallthrough\"\">\" 4\nbody\n.el .IP *<\\f(CW\\-other\\fR> 4\n",
            ))
            .unwrap();
        let heads = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("IP"))
            .map(|head| head.children().map(NodeRef::text).collect::<Vec<_>>())
            .collect::<Vec<_>>();
        assert_eq!(
            heads,
            [[Some("*<\"\\-fallthrough\">"), Some("4")],].as_slice()
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn filled_c_before_a_blank_line_discards_only_the_recovery_pair() {
        let name = SourceName::new("man-c-blank.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH C-BLANK 1\n.SH DESCRIPTION\nfilled\\c\n\nnext\n.nf\nliteral\\c\n\nlater\n.fi\n",
            ))
            .unwrap();
        let texts = report
            .document
            .preorder()
            .filter_map(NodeRef::text)
            .collect::<Vec<_>>();
        assert!(texts.contains(&"filled"));
        assert!(!texts.contains(&"filled\\c"));
        assert!(texts.contains(&"literal\\c"));
        assert_eq!(texts.iter().filter(|text| text.is_empty()).count(), 1);
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn a_continued_line_keeps_next_line_scopes_open() {
        let name = SourceName::new("man-c-scope.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH C-SCOPE 1\n.SH DESCRIPTION\n.B\none\\c\nword\n.TP\nterm\\c\nword\ndefinition\n",
            ))
            .unwrap();
        let bold = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("B"))
            .unwrap();
        assert_eq!(
            bold.children()
                .filter_map(NodeRef::text)
                .collect::<Vec<_>>(),
            ["one\\c", "word"]
        );
        let term = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
            .unwrap();
        assert_eq!(
            term.children()
                .next()
                .unwrap()
                .children()
                .filter_map(NodeRef::text)
                .collect::<Vec<_>>(),
            ["term\\c", "word"]
        );
        assert!(report.diagnostics.is_empty());
    }

    #[test]
    fn unmatched_re_breaks_out_of_the_current_implicit_term() {
        let name = SourceName::new("man-unmatched-re.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH UNMATCHED-RE 1\n.SH DESCRIPTION\n.TP 6n\ntag\nbody\n.RE\noutside\n",
            ))
            .unwrap();
        let body = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .find(|node| node.macro_name() == Some("SH"))
            .unwrap()
            .children()
            .nth(1)
            .unwrap();
        let children = body.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].macro_name(), Some("TP"));
        assert_eq!(children[1].kind(), NodeKind::Element);
        assert_eq!(children[1].macro_name(), Some("br"));
        assert_eq!(children[2].text(), Some("outside"));
        assert!(
            report
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_str() == DiagnosticCode::MAN_UNMATCHED_CLOSE)
        );
    }

    #[test]
    fn paragraph_distance_keeps_next_line_man_scopes_open() {
        let name = SourceName::new("man-pd-nextline.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH PD-NEXTLINE 1\n.SH\n.PD 0v\nSECTION\n.TP\n.PD 0v\ntag\nbody\n.B\n.PD 0v\nbold\n",
            ))
            .unwrap();
        let section = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .find(|node| node.macro_name() == Some("SH"))
            .unwrap();
        let head = section.children().next().unwrap();
        let head_children = head.children().collect::<Vec<_>>();
        assert_eq!(head_children[0].macro_name(), Some("PD"));
        assert_eq!(head_children[1].text(), Some("SECTION"));

        let body = section.children().nth(1).unwrap();
        let term = body
            .children()
            .find(|node| node.macro_name() == Some("TP"))
            .unwrap();
        let term_head = term.children().next().unwrap();
        let term_children = term_head.children().collect::<Vec<_>>();
        assert_eq!(term_children[0].macro_name(), Some("PD"));
        assert_eq!(term_children[1].text(), Some("tag"));

        let term_body = term.children().nth(1).unwrap();
        let bold = term_body
            .children()
            .find(|node| node.macro_name() == Some("B"))
            .unwrap();
        let bold_children = bold.children().collect::<Vec<_>>();
        assert_eq!(bold_children[0].macro_name(), Some("PD"));
        assert_eq!(bold_children[1].text(), Some("bold"));
    }

    #[test]
    fn rs_closes_an_implicit_indent_before_restoring_outer_flow() {
        let name = SourceName::new("man-rs-implicit-parent.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH RS-IMPLICIT-PARENT 1\n.SH DESCRIPTION\n.IP tag 6n\nterm body\n.RS\nindented\n.RE\nafter indent\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let body = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .find(|node| node.macro_name() == Some("SH"))
            .unwrap()
            .children()
            .nth(1)
            .unwrap();
        let children = body.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].macro_name(), Some("IP"));
        assert_eq!(children[1].macro_name(), Some("RS"));
        assert_eq!(children[2].text(), Some("after indent"));
    }

    #[test]
    fn centering_and_right_adjustment_own_their_following_input_lines() {
        let name = SourceName::new("man-center.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH CENTER 1\n.SH DESCRIPTION\n.ce 2\nfirst centered\nsecond centered\n.rj 1\nright adjusted\nafter\n",
            ))
            .unwrap();
        assert!(report.diagnostics.is_empty(), "{:#?}", report.diagnostics);
        let elements = report
            .document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("ce" | "rj")))
            .collect::<Vec<_>>();
        assert_eq!(elements.len(), 2);
        assert_eq!(
            elements[0]
                .children()
                .filter_map(NodeRef::text)
                .collect::<Vec<_>>(),
            ["2", "first centered", "second centered"]
        );
        assert_eq!(
            elements[1]
                .children()
                .filter_map(NodeRef::text)
                .collect::<Vec<_>>(),
            ["1", "right adjusted"]
        );
    }

    #[test]
    fn th_is_metadata_only_and_derives_a_known_section_volume() {
        let name = SourceName::new("metadata.3").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH METADATA 3 25-Aug-2026\n.SH NAME\nmetadata\n",
            ))
            .unwrap();
        assert_eq!(
            report.document.metadata().title.as_deref(),
            Some("METADATA")
        );
        assert_eq!(report.document.metadata().section.as_deref(), Some("3"));
        assert_eq!(
            report.document.metadata().volume.as_deref(),
            Some("Library Functions Manual")
        );
        assert!(
            report
                .document
                .node(report.document.root())
                .unwrap()
                .children()
                .all(|node| node.macro_name() != Some("TH"))
        );
    }

    #[test]
    fn section_openers_select_man_without_th_and_recover_missing_metadata() {
        let name = SourceName::new("no-th.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".SH NAME\nno-th \\- title macro missing\n.SH DESCRIPTION\ntext\n",
            ))
            .unwrap();
        assert_eq!(report.document.macro_set(), crate::MacroSet::Man);
        assert_eq!(report.document.metadata().title.as_deref(), Some(""));
        assert_eq!(report.document.metadata().section.as_deref(), Some(""));
        assert_eq!(report.document.metadata().date.as_deref(), Some(""));
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "missing manual title, using \"\"",
                "missing date, using \"\""
            ]
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| { node.kind() == NodeKind::Block && node.macro_name() == Some("SH") })
        );
    }

    #[test]
    fn complete_title_without_visible_body_reports_the_legacy_warning() {
        let name = SourceName::new("man-no-body.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(&name, b".TH NO-BODY 1 \"August 25, 2026\"\n"))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [DiagnosticCode::MAN_NO_DOCUMENT_BODY]
        );
        assert_eq!(report.diagnostics[0].message.as_ref(), "no document body");
    }

    #[test]
    fn unparseable_th_dates_remain_metadata_and_report_their_argument() {
        let name = SourceName::new("bad-th-date.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH BAD-DATE 1 \"May 2001\"\n.SH NAME\nbad-date\n",
            ))
            .unwrap();
        assert_eq!(report.document.metadata().date.as_deref(), Some("May 2001"));
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::MAN_TITLE_DATE_UNPARSEABLE
        );
        assert_eq!(
            diagnostic.message.as_ref(),
            "cannot parse date, using it verbatim: TH May 2001"
        );
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (1, 16));
    }

    #[test]
    fn empty_th_date_remains_metadata_and_reports_the_empty_argument() {
        let name = SourceName::new("empty-th-date.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH EMPTY-DATE 1 \"\" source\n.SH NAME\nempty-date\n",
            ))
            .unwrap();
        assert_eq!(report.document.metadata().date.as_deref(), Some(""));
        let diagnostic = report.diagnostics.first().unwrap();
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::MAN_TITLE_DATE_MISSING
        );
        assert_eq!(diagnostic.message.as_ref(), "missing date, using \"\": TH");
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (1, 18));
    }

    #[test]
    fn empty_ip_is_removed_before_the_next_paragraph_boundary() {
        let name = SourceName::new("empty-ip.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH EMPTY-IP 1\n.SH DESCRIPTION\n.IP\n.IP tag\nbody\n",
            ))
            .unwrap();
        let ips = report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("IP"))
            .collect::<Vec<_>>();
        assert_eq!(ips.len(), 1);
        assert_eq!(
            ips[0]
                .children()
                .next()
                .unwrap()
                .children()
                .next()
                .unwrap()
                .text(),
            Some("tag")
        );
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::MAN_EMPTY_PARAGRAPH
        );
        assert_eq!(
            diagnostic.message.as_ref(),
            "skipping paragraph macro: IP empty"
        );
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (3, 2));
    }

    #[test]
    fn mt_validates_uri_arguments_and_returns_me_tail_to_outer_flow() {
        let name = SourceName::new("mt-args.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH MT-ARGS 1\n.SH DESCRIPTION\n.MT first second\ntext\n.ME tail args\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("MT"))
            .unwrap();
        assert_eq!(
            block
                .children()
                .next()
                .unwrap()
                .children()
                .next()
                .unwrap()
                .text(),
            Some("first")
        );
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::MAN_EXCESS_ARGUMENTS
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "skipping excess arguments: MT ... second"
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("tail args"))
        );
    }

    #[test]
    fn op_reports_missing_and_superfluous_option_arguments_without_rewriting_flow() {
        let name = SourceName::new("op-args.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH OP-ARGS 1\n.SH DESCRIPTION\n.OP\n.OP -f arg bogus\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::MAN_MISSING_OPTION,
                DiagnosticCode::MAN_EXCESS_ARGUMENTS,
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "missing option string, using \"\": OP",
                "skipping excess arguments: OP ... bogus",
            ]
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("bogus"))
        );
    }

    #[test]
    fn pd_reports_and_removes_its_first_excess_argument() {
        let name = SourceName::new("pd-args.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH PD-ARGS 1\n.SH DESCRIPTION\n.PD 0 zzz\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::MAN_EXCESS_ARGUMENTS
        );
        assert_eq!(
            diagnostic.message.as_ref(),
            "skipping excess arguments: PD ... zzz"
        );
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (3, 7));
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.text() == Some("zzz"))
        );
    }

    #[test]
    fn sp_reports_and_removes_its_first_excess_argument() {
        let name = SourceName::new("sp-args.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH SP-ARGS 1\n.SH DESCRIPTION\nbody\n.sp 3v 2i\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::MAN_EXCESS_ARGUMENTS
        );
        assert_eq!(
            diagnostic.message.as_ref(),
            "skipping excess arguments: sp ... 2i"
        );
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (4, 8));
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.text() == Some("2i"))
        );
    }

    #[test]
    fn paragraph_controls_report_but_retain_ignored_arguments() {
        let name = SourceName::new("paragraph-args.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH PARAGRAPH-ARGS 1\n.SH DESCRIPTION\n.PP arg\n.LP arg1 arg2\n.P arg\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::MAN_ALL_ARGUMENTS,
                DiagnosticCode::MAN_ALL_ARGUMENTS,
                DiagnosticCode::MAN_ALL_ARGUMENTS,
            ]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping all arguments: PP arg",
                "skipping all arguments: PP arg1 ...",
                "skipping all arguments: PP arg",
            ]
        );
        assert!(
            report
                .document
                .preorder()
                .any(|node| node.text() == Some("arg2"))
        );
    }

    #[test]
    fn empty_paragraph_controls_report_empty_and_after_section_recovery() {
        let name = SourceName::new("paragraph-empty.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH PARAGRAPH-EMPTY 1\n.SH DESCRIPTION\n.PP\nheading paragraph\n.PP\n.PP\nbody\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping paragraph macro: PP empty",
                "skipping paragraph macro: PP after SH",
            ]
        );
        let locations = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let location = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (location.line, location.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(locations, [(5, 2), (3, 2)]);
    }

    #[test]
    fn terminal_section_break_is_removed_and_reported() {
        let name = SourceName::new("terminal-break.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TERMINAL-BREAK 1\n.SH DESCRIPTION\nvisible text\n.br\n",
            ))
            .unwrap();
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "skipping paragraph macro: br at the end of SH"
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (4, 2));
        assert!(
            !report
                .document
                .preorder()
                .any(|node| node.macro_name() == Some("br"))
        );
    }

    #[test]
    fn structures_paragraphs_tq_and_next_line_term_heads() {
        let name = SourceName::new("man-lists.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH LISTS 1\n.SH DESCRIPTION\n.PP\nfirst paragraph\n.TP\nfirst term\nfirst definition\n.TQ\nsecond term\nsecond definition\n.IP marker 4\nindented definition\n.HP 4\nhanging definition\n",
            ))
            .unwrap();
        let section_body = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .find(|node| node.macro_name() == Some("SH"))
            .unwrap()
            .children()
            .nth(1)
            .unwrap();
        let blocks = section_body.children().collect::<Vec<_>>();
        assert_eq!(
            blocks
                .iter()
                .map(|node| node.macro_name())
                .collect::<Vec<_>>(),
            [None, Some("TP"), Some("TQ"), Some("IP"), Some("HP")]
        );
        assert_eq!(blocks[0].kind(), NodeKind::Text);
        assert!(
            blocks[1..]
                .iter()
                .all(|node| node.kind() == NodeKind::Block)
        );

        assert_eq!(blocks[0].text(), Some("first paragraph"));

        let term_head = blocks[1].children().next().unwrap();
        assert_eq!(
            term_head.children().next().unwrap().text(),
            Some("first term")
        );
        let tq_head = blocks[2].children().next().unwrap();
        assert_eq!(
            tq_head.children().next().unwrap().text(),
            Some("second term")
        );
        let ip_head = blocks[3].children().next().unwrap();
        assert_eq!(
            ip_head.children().map(NodeRef::text).collect::<Vec<_>>(),
            [Some("marker"), Some("4")]
        );
        let hp_head = blocks[4].children().next().unwrap();
        assert_eq!(hp_head.children().next().unwrap().text(), Some("4"));
        assert!(term_head.flags().deep_link_target);
        assert!(term_head.flags().permalink);
        assert!(ip_head.flags().deep_link_target);
    }

    #[test]
    fn nested_empty_font_macros_finish_a_pending_tp_term_at_its_text() {
        let name = SourceName::new("man-tp-nested-font-term.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH NESTED 1\n.SH DESCRIPTION\n.TP\n.B\n.I\nterm\ndefinition\n",
            ))
            .unwrap();
        let block = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
            .unwrap();
        let mut parts = block.children();
        let head = parts.next().unwrap();
        let body = parts.next().unwrap();
        let bold = head.children().next().unwrap();
        let italic = bold.children().next().unwrap();
        let term = italic.children().next().unwrap();
        assert_eq!(term.text(), Some("term"));
        assert_eq!(
            body.location().unwrap().start,
            term.location().unwrap().start
        );
    }

    #[test]
    fn pending_tp_head_retains_indent_request_before_its_term() {
        let name = SourceName::new("man-tp-indent.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH INDENT 1\n.SH DESCRIPTION\n.TP 8n\n.in 3n\ntag\nbody\n",
            ))
            .unwrap();
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("TP"))
            .unwrap();
        let children = head.children().collect::<Vec<_>>();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0].text(), Some("8n"));
        assert_eq!(children[1].macro_name(), Some("in"));
        assert_eq!(children[1].children().next().unwrap().text(), Some("+3n"));
        assert_eq!(children[2].text(), Some("tag"));
    }

    #[test]
    fn structures_explicit_link_mail_and_synopsis_blocks() {
        let name = SourceName::new("man-explicit.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH EXPLICIT 1\n.SH LINKS\n.UR https://example.test\nlink body\n.UE\n.MT mail@example.test\nmail body\n.ME\n.SY command\nargument\n.YS\n.B\nbold next line\n",
            ))
            .unwrap();
        let section_body = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .find(|node| node.macro_name() == Some("SH"))
            .unwrap()
            .children()
            .nth(1)
            .unwrap();
        let children = section_body.children().collect::<Vec<_>>();
        assert_eq!(
            children
                .iter()
                .map(|node| node.macro_name())
                .collect::<Vec<_>>(),
            [Some("UR"), Some("MT"), Some("SY"), Some("YS"), Some("B")]
        );
        for block in &children[..3] {
            assert_eq!(block.kind(), NodeKind::Block);
            assert_eq!(block.children().nth(1).unwrap().kind(), NodeKind::Body);
        }
        assert_eq!(
            children[0]
                .children()
                .nth(1)
                .unwrap()
                .children()
                .next()
                .unwrap()
                .text(),
            Some("link body")
        );
        assert_eq!(
            children[4].children().next().unwrap().text(),
            Some("bold next line")
        );
        assert!(
            report
                .document
                .preorder()
                .all(|node| !matches!(node.macro_name(), Some("UE" | "ME")))
        );
    }

    #[test]
    fn eof_drops_an_unfilled_next_line_font_scope_with_a_typed_warning() {
        let name = SourceName::new("man-font-eof.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FONT-EOF 1\n.SH DESCRIPTION\ntext before scope\n.B\n",
            ))
            .unwrap();
        assert!(
            report
                .document
                .preorder()
                .all(|node| node.macro_name() != Some("B"))
        );
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::MAN_LINE_SCOPE_BROKEN
        );
        assert_eq!(
            diagnostic.message.as_ref(),
            "line scope broken: EOF breaks B"
        );
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (4, 2));
    }

    #[test]
    fn blank_lines_are_skipped_without_closing_a_next_line_font_scope() {
        let name = SourceName::new("man-font-blank.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FONT-BLANK 1\n.SH DESCRIPTION\n.B\n\nbold\nafter\n",
            ))
            .unwrap();
        let bold = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("B"))
            .unwrap();
        assert_eq!(bold.children().next().unwrap().text(), Some("bold"));
        assert_eq!(report.diagnostics.len(), 1);
        let diagnostic = &report.diagnostics[0];
        assert_eq!(
            diagnostic.code.as_str(),
            DiagnosticCode::MAN_BLANK_LINE_SCOPE
        );
        assert_eq!(
            diagnostic.message.as_ref(),
            "skipping blank line in line scope"
        );
        let location = report
            .document
            .source_position(diagnostic.primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((location.line, location.column), (4, 1));
    }

    #[test]
    fn propagates_no_fill_and_sentence_state_in_source_order() {
        let name = SourceName::new("man-presentation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH PRESENTATION 1\n.SH EXAMPLES\n.nf\nfirst literal.\n.B bold literal\n.fi\nfilled sentence.\n.EX\nexample line\n.EE\nfinal sentence.\n",
            ))
            .unwrap();
        let section_body = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .find(|node| node.macro_name() == Some("SH"))
            .unwrap()
            .children()
            .nth(1)
            .unwrap();
        let nodes = section_body.children().collect::<Vec<_>>();
        let first_literal = nodes
            .iter()
            .find(|node| node.text() == Some("first literal."))
            .unwrap();
        assert!(first_literal.flags().no_fill);
        assert!(!first_literal.flags().sentence_end);
        let bold = nodes
            .iter()
            .find(|node| node.macro_name() == Some("B"))
            .unwrap();
        assert!(bold.flags().no_fill);
        assert!(bold.children().next().unwrap().flags().no_fill);
        let filled = nodes
            .iter()
            .find(|node| node.text() == Some("filled sentence."))
            .unwrap();
        assert!(!filled.flags().no_fill);
        assert!(filled.flags().sentence_end);
        let example_start = nodes
            .iter()
            .find(|node| node.macro_name() == Some("EX"))
            .unwrap();
        assert!(!example_start.flags().no_fill);
        let example = nodes
            .iter()
            .find(|node| node.text() == Some("example line"))
            .unwrap();
        assert!(example.flags().no_fill);
        let example_end = nodes
            .iter()
            .find(|node| node.macro_name() == Some("EE"))
            .unwrap();
        assert!(example_end.flags().no_fill);
        let final_sentence = nodes
            .iter()
            .find(|node| node.text() == Some("final sentence."))
            .unwrap();
        assert!(!final_sentence.flags().no_fill);
        assert!(final_sentence.flags().sentence_end);
    }

    #[test]
    fn assigns_and_suppresses_man_destination_tags_like_libmandoc() {
        let name = SourceName::new("man-tags.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TAGS 1\n.SH NAME\ntags\n.SH \"SEE ALSO\"\nfirst\n.SS \"SEE ALSO\"\nsecond\n.TP\n-term\ndefinition\n",
            ))
            .unwrap();
        let document = &report.document;
        let section_heads = document
            .preorder()
            .filter(|node| matches!(node.macro_name(), Some("SH" | "SS")))
            .filter(|node| node.kind() == NodeKind::Head)
            .collect::<Vec<_>>();
        assert_eq!(section_heads.len(), 3);
        assert!(section_heads[0].flags().deep_link_target);
        assert_eq!(section_heads[0].tag(), None);
        assert!(
            section_heads[1..]
                .iter()
                .all(|head| !head.flags().deep_link_target && head.tag().is_none())
        );

        let term_head = document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("TP"))
            .unwrap();
        assert!(term_head.flags().deep_link_target);
        assert!(term_head.flags().permalink);
        assert_eq!(term_head.tag(), Some("term"));

        let width_name = SourceName::new("man-width-tag.1").unwrap();
        let width_report = Parser::default()
            .parse(Source::new(
                &width_name,
                b".TH WIDTH 1\n.SH NAME\nwidth\n.SH DESCRIPTION\n.TP 6n\n.BI bold italic\nbody\n",
            ))
            .unwrap();
        let width_term_head = width_report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("TP"))
            .unwrap();
        assert!(width_term_head.flags().deep_link_target);
        assert_eq!(width_term_head.tag(), Some("bold"));

        let priority_name = SourceName::new("man-tag-priority.1").unwrap();
        let priority_report = Parser::default()
            .parse(Source::new(
                &priority_name,
                b".TH TAGS 1\n.SH DESCRIPTION\n.TP\n.I \" plain\"\nfirst\n.TP\nplain\nsecond\n.TP\n.I \"plain \"\nthird\n.HP\n.B not-a-term\nhanging\n.IP \" weak\"\nfirst indent\n.IP -weak\nsecond indent\n",
            ))
            .unwrap();
        let heads = priority_report
            .document
            .preorder()
            .filter(|node| {
                node.kind() == NodeKind::Head
                    && matches!(node.macro_name(), Some("TP" | "HP" | "IP"))
            })
            .collect::<Vec<_>>();
        assert_eq!(heads.len(), 6);
        assert!(
            !heads[0].flags().deep_link_target
                && heads[1].flags().deep_link_target
                && !heads[2].flags().deep_link_target
        );
        assert_eq!(heads[1].tag(), None);
        assert_eq!(heads[2].tag(), None);
        assert!(!heads[3].flags().deep_link_target);
        assert_eq!(heads[3].children().count(), 0);
        assert!(
            !heads[4].flags().deep_link_target
                && heads[5].flags().deep_link_target
                && heads[5].tag() == Some("weak")
        );
    }

    #[test]
    fn reports_unmatched_closers_and_end_of_input_open_blocks() {
        let name = SourceName::new("man-recovery.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH RECOVERY 1\n.RE\n.UR https://example.test\nunclosed link\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [
                DiagnosticCode::MAN_UNMATCHED_CLOSE,
                DiagnosticCode::MAN_UNCLOSED_BLOCK,
            ]
        );
        assert!(
            report
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.primary.is_some())
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping end of block that is not open: RE",
                "appending missing end of block: UR",
            ]
        );
    }

    #[test]
    fn reports_eof_for_a_pending_section_title_and_removes_the_empty_section() {
        let name = SourceName::new("section-eof.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH SECTION-EOF 1\n.SH DESCRIPTION\ntext\n.SH\n",
            ))
            .unwrap();
        let sections = report
            .document
            .node(report.document.root())
            .unwrap()
            .children()
            .filter(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("SH"))
            .collect::<Vec<_>>();
        assert_eq!(sections.len(), 1);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(
            report.diagnostics[0].code.as_str(),
            DiagnosticCode::MAN_LINE_SCOPE_BROKEN
        );
        assert_eq!(
            report.diagnostics[0].message.as_ref(),
            "line scope broken: EOF breaks SH"
        );
    }

    #[test]
    fn propagates_eof_through_an_empty_font_scope_in_a_pending_section_title() {
        let name = SourceName::new("section-font-eof.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH SECTION-FONT-EOF 1\n.SH DESCRIPTION\ntext\n.SH\n.B\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "line scope broken: EOF breaks B",
                "line scope broken: EOF breaks SH"
            ]
        );
    }

    #[test]
    fn empty_section_heads_use_fill_toggles_to_start_the_body() {
        let name = SourceName::new("section-macro-break.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH SECTION-BREAK 1\n.SH DESCRIPTION\n.SH\n.nf\nliteral\n.SH\n.fi\nfilled\n",
            ))
            .unwrap();
        let literal = report
            .document
            .preorder()
            .find(|node| node.text() == Some("literal"))
            .unwrap();
        let filled = report
            .document
            .preorder()
            .find(|node| node.text() == Some("filled"))
            .unwrap();
        let fill_restore = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("fi"))
            .unwrap();
        assert!(literal.flags().no_fill);
        assert!(!fill_restore.flags().no_fill);
        assert!(!filled.flags().no_fill);
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [DiagnosticCode::MAN_REDUNDANT_FILL_MODE]
        );
    }

    #[test]
    fn fill_toggles_preserve_macro_and_argument_state_boundaries() {
        let name = SourceName::new("man-fill-toggle.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FILL 1\n.SH DESCRIPTION\n.EX opening argument\nliteral\n.EE closing argument\nregular\n",
            ))
            .unwrap();
        let ex = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("EX"))
            .unwrap();
        assert!(!ex.flags().no_fill);
        assert!(ex.children().all(|argument| argument.flags().no_fill));

        let ee = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("EE"))
            .unwrap();
        assert!(ee.flags().no_fill);
        assert!(ee.children().all(|argument| !argument.flags().no_fill));
    }

    #[test]
    fn fill_mode_requests_discard_and_report_their_complete_argument_tail() {
        let name = SourceName::new("man-fill-arguments.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FILL-ARGS 1\n.SH DESCRIPTION\n.nf arg1 arg2 arg3\nliteral\n.fi arg1 arg2 arg3\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "skipping all arguments: nf arg1 arg2 arg3",
                "skipping all arguments: fi arg1 arg2 arg3",
            ]
        );
        let positions = report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                let position = report
                    .document
                    .source_position(diagnostic.primary.as_ref().unwrap())
                    .unwrap();
                (position.line, position.column)
            })
            .collect::<Vec<_>>();
        assert_eq!(positions, [(3, 5), (5, 5)]);
        assert!(
            report
                .document
                .preorder()
                .filter(|node| matches!(node.macro_name(), Some("nf" | "fi")))
                .all(|node| node.children().next().is_none())
        );
    }

    #[test]
    fn line_break_requests_discard_and_report_their_complete_argument_tail() {
        let name = SourceName::new("man-break-arguments.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH BR-ARGS 1\n.SH DESCRIPTION\nsome\ntext\n.br arg1 arg2 arg3\nmore\ntext\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            ["skipping all arguments: br arg1 arg2 arg3"]
        );
        let position = report
            .document
            .source_position(report.diagnostics[0].primary.as_ref().unwrap())
            .unwrap();
        assert_eq!((position.line, position.column), (5, 5));
        let break_node = report
            .document
            .preorder()
            .find(|node| node.macro_name() == Some("br"))
            .unwrap();
        assert!(break_node.children().next().is_none());
    }

    #[test]
    fn no_fill_keeps_man_term_structure_filled_but_marks_body_flow() {
        let name = SourceName::new("man-no-fill-term.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FILLTERM 1\n.SH DESCRIPTION\n.nf\n.TP 4n\nterm\nliteral body\n",
            ))
            .unwrap();
        let term = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
            .unwrap();
        assert!(!term.flags().no_fill);
        let mut parts = term.children();
        let head = parts.next().unwrap();
        let body = parts.next().unwrap();
        assert!(!head.flags().no_fill);
        assert!(head.children().all(|node| !node.flags().no_fill));
        assert!(!body.flags().no_fill);
        assert!(body.children().all(|node| node.flags().no_fill));
    }

    #[test]
    fn fill_toggle_after_tp_stays_in_the_pending_term_head() {
        let name = SourceName::new("man-no-fill-pending-term.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH FILLTERM 1\n.SH DESCRIPTION\n.TP\n.nf\nterm\nliteral body\n",
            ))
            .unwrap();
        let term = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Block && node.macro_name() == Some("TP"))
            .unwrap();
        let mut parts = term.children();
        let head = parts.next().unwrap();
        let body = parts.next().unwrap();
        assert_eq!(
            head.children()
                .map(|node| (node.macro_name(), node.text()))
                .collect::<Vec<_>>(),
            [(Some("nf"), None), (None, Some("term"))]
        );
        assert_eq!(
            body.children().next().and_then(NodeRef::text),
            Some("literal body")
        );
    }

    #[test]
    fn ip_tab_separated_tag_stays_one_head_argument_before_the_width() {
        let name = SourceName::new("man-ip-tab.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH IPTAB 1\n.SH DESCRIPTION\n.IP single\ttab 3n\nbody\n",
            ))
            .unwrap();
        let head = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("IP"))
            .unwrap();
        assert_eq!(
            head.children().map(NodeRef::text).collect::<Vec<_>>(),
            [Some("single\ttab"), Some("3n")]
        );
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.as_str())
                .collect::<Vec<_>>(),
            [DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT]
        );
        assert_eq!(report.diagnostics[0].message.as_ref(), "tab in filled text");
    }

    #[test]
    fn section_title_punctuation_is_not_a_flow_sentence_boundary() {
        let name = SourceName::new("man-heading-punctuation.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH HEADING 1\n.SH \"A heading.\"\ntext\n",
            ))
            .unwrap();
        let heading = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("SH"))
            .unwrap()
            .children()
            .next()
            .unwrap();
        assert!(!heading.flags().sentence_end);
    }

    #[test]
    fn deferred_subsection_title_retains_its_text_sentence_boundary() {
        let name = SourceName::new("man-deferred-subsection.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH HEADING 1\n.SH DESCRIPTION\n.SS\nA deferred subsection title.\nbody\n",
            ))
            .unwrap();
        let heading = report
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Head && node.macro_name() == Some("SS"))
            .unwrap()
            .children()
            .next()
            .unwrap();
        assert!(heading.flags().line_start);
        assert!(heading.flags().sentence_end);
    }

    #[test]
    fn tbl_openers_break_pending_man_line_scopes_without_leaking_controls() {
        let name = SourceName::new("man-tbl-break.1").unwrap();
        let report = Parser::default()
            .parse(Source::new(
                &name,
                b".TH TBL-BREAK 1\n.SH DESCRIPTION\n.TP 6n\n.TS\nl.\nfirst\n.TE\n.SH\n.TS\nl.\nsecond\n.TE\n.SS\n.TS\nl.\nthird\n.TE\n.B\n.TS\nl.\nfourth\n.TE\nfinal\n",
            ))
            .unwrap();
        assert_eq!(
            report
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.message.as_ref())
                .collect::<Vec<_>>(),
            [
                "line scope broken: TS breaks TP",
                "line scope broken: TS breaks SH",
                "line scope broken: TS breaks SS",
                "line scope broken: TS breaks B",
            ]
        );
        assert_eq!(
            report
                .document
                .preorder()
                .filter(|node| node.kind() == NodeKind::Table)
                .count(),
            4
        );
        assert!(
            !report
                .document
                .preorder()
                .any(|node| { matches!(node.macro_name(), Some("TP" | "SS" | "B" | "TS")) })
        );
    }
}
