use super::{
    ArgumentIssue, BranchOutcome, ControlEvent, DiagnosticCode, DocumentBuilder, EmitContext,
    EnvironmentRequestContext, EscapeIssueKind, IncludeRequest, InputTrap, MacroSet,
    ManIndentState, NodeFlags, NodeId, NodeKind, PackageToken, ParseSession, RequestHandling,
    RequestKind, ScanOutcome, ScannedLine, Scanner, ScopeCollector, ScopeFlow, ScopeLine,
    ScopeMachine, Severity, Source, SourceEvent, SourceMachine, SourcePosition, SourceResolver,
    SourceSpan, Syntax, TransparentRequestContext, append_node, append_text_node,
    append_textual_node, apply_environment_request, apply_string_request,
    collect_pending_macro_scope, condition_body_source_start_from_offset, condition_body_template,
    condition_body_template_from_offset, condition_parts, consume_ignore_block,
    contains_valid_utf8_non_ascii, copy_mode_reparse, definition_scope_remainder_line, diagnostic,
    emit_bad_comment_style, emit_declared_character_escape_warnings, emit_escape_issues,
    emit_escaped_condition_name, emit_filled_macro_argument_tabs, emit_filled_text_tabs,
    emit_font_request_diagnostics, emit_invalid_input_bytes, emit_long_input_line,
    emit_man_alternating_font_trailing_whitespace, emit_mdoc_control_trailing_whitespace,
    emit_mdoc_empty_display, emit_mdoc_implicit_trailing_delimiter_spacing,
    emit_outside_macro_argument_escapes, emit_trailing_whitespace,
    emit_unterminated_quoted_argument, emit_unterminated_register_reference_escapes,
    emit_unterminated_string_reference_escapes, emit_user_macro_leading_tabs,
    environment_error_diagnostic, evaluate_condition, execute_environment_request,
    execute_scope_line, execute_scope_macro_lines, execute_transparent_request,
    expand_copy_mode_definition, expand_declared_character_escapes, expand_environment,
    has_physical_line_continuation, has_protected_tabulation_escape, ignore_marker,
    inline_scope_body_template, is_bad_comment_style, is_builtin_package_macro,
    is_definition_terminator, is_environment_request, is_ignore_terminator,
    is_legacy_roff_font_selector, is_macro_comment_request, is_man_visible_argument_macro,
    is_scope_opener, join_arguments, legacy_table_input_text, lex_arguments,
    lex_condition_arguments, lex_user_macro_arguments, macro_argument_copy_mode_reparse,
    macro_body_control_column, macro_conditional_body_origin, macro_definition_directly_invokes,
    macro_scope_body_origin, normalize_character_request_arguments, normalize_document_escapes,
    normalize_macro_argument_number_escapes, normalize_roff_name_prefix, push_diagnostic,
    record_expansion_steps, record_suppressed_scope_definitions, recover_attached_control_name,
    recover_unterminated_quoted_arguments, retain_user_macro_tab_argument_prefix,
    roff_escape_name_width, scope_closer_offset, scope_line_start, scope_opener_remainder,
    scope_remainder_source_start, scope_replay_logical_start, set_first_root_child_logical_start,
    set_first_scope_child_logical_start, set_first_scope_child_opening_column,
    set_new_root_children_logical_start, split_escaped_condition_body, split_macro_control,
    strip_outside_macro_argument_escapes, trailing_whitespace_start, translate_visible,
    trim_horizontal_space, update_fill_mode, update_man_example_fill_presentation,
    update_man_indent_register, update_preprocessor_depth, update_table_preprocessor_depth,
    visible_bytes,
};

impl<R: SourceResolver + ?Sized> SourceMachine<'_, '_, '_, R> {
    pub(super) fn run(self) -> ScanOutcome {
        run_source(self)
    }
}

#[allow(clippy::too_many_lines)] // M2's explicit scanner-stage dispatch is kept in source order.
fn run_source<R: SourceResolver + ?Sized>(machine: SourceMachine<'_, '_, '_, R>) -> ScanOutcome {
    let SourceMachine {
        source,
        source_id,
        include_depth,
        session,
        outcome,
    } = machine;
    let config = session.config;
    let builder = &mut *session.builder;
    let environment = &mut *session.environment;
    let active_sources = &mut *session.active_sources;
    let resolver = &mut *session.resolver;
    let ScanOutcome {
        mut diagnostics,
        mut deferred_post_validation_diagnostics,
        mut source_bytes,
        mut source_files,
        mut text_bytes,
        mut expansion_steps,
        mut truncated,
        mut maximum_depth,
        mut previous_conditional,
        mut total_loop_iterations,
        mut saw_mdoc_operating_system,
    } = outcome;
    let limits = &config.limits;
    let root = DocumentBuilder::root();
    let mut scanner = Scanner::new(source.bytes, limits);
    let mut package_preprocessor_depth = 0_usize;
    let mut table_preprocessor_depth = 0_usize;
    // man(7)'s EX/EE style validation observes presentation toggles rather
    // than the nesting model used to retain no-fill AST flags.
    let mut man_example_fill_enabled = environment.is_filled();
    let mut man_indent_state = ManIndentState::default();
    let mut input_trap = InputTrap::default();
    // A bare `.if`/`.ie` owns the next physical line as a one-line scope.
    // Keeping this at the scanner boundary lets an active body take the
    // ordinary package parser path, while an inactive one is consumed before
    // it can create diagnostics, mutate state, or publish AST nodes.
    let mut next_line_condition = None::<BranchOutcome>;
    // mandoc reports an open `.while` when its caller resumes after an inner
    // macro closed that loop.  Scope collection has already consumed the
    // closer, so publish the recovery finding on the next physical line.
    let mut pending_while_out_of_scope = false;
    'lines: while let Some(line) = scanner.next_line() {
        let event = SourceEvent::from_scanned(line, builder.macro_set());
        let pending_next_line_condition = next_line_condition.take();
        if pending_while_out_of_scope {
            let (start, end) = event.range();
            push_diagnostic(
                &mut diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                    Severity::Unsupported,
                    source_id,
                    start,
                    end,
                    "end of scope with open .while loop",
                ),
                &mut truncated,
            );
            pending_while_out_of_scope = false;
        }
        // `.el` is the paired branch, not the preceding bare conditional's
        // next-line body.  In particular, a malformed predicate may end at
        // the physical line immediately before its `.el`; preserve the pair
        // so the false arm remains visible.
        let paired_else = event.is_else_request();
        if pending_next_line_condition.is_some_and(BranchOutcome::is_skipped) && !paired_else {
            continue;
        }
        match event {
            SourceEvent::TooLong { start, end } => {
                push_diagnostic(
                    &mut diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::LIMIT_LINE_BYTES,
                        Severity::Warning,
                        source_id,
                        start,
                        end,
                        "physical source line exceeds max_line_bytes and was skipped",
                    ),
                    &mut truncated,
                );
            }
            SourceEvent::Text { start, end, bytes } => {
                let authored_has_tab = bytes.contains(&b'\t');
                let authored_trailing_whitespace = trailing_whitespace_start(bytes).is_some();
                if is_bad_comment_style(
                    bytes,
                    scanner.escape_character(),
                    scanner.control_character(),
                ) {
                    emit_bad_comment_style(
                        bytes,
                        scanner.escape_character(),
                        scanner.control_character(),
                        start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    // `roff_getcontrol()` recognizes the escaped control
                    // character before text dispatch. A following quote is a
                    // malformed comment request, so it emits only the style
                    // finding and no public text/input-trap event.
                    continue;
                }
                // roff arms `.it` against physical input *text* lines.  The
                // triggering line remains visible first, then the configured
                // macro is reparsed at this line's source location.
                let sprung_input_trap = input_trap.consume_text_line();
                if builder.macro_set() != MacroSet::None && package_preprocessor_depth == 0 {
                    if builder.macro_set() == MacroSet::Mdoc || environment.is_filled() {
                        emit_trailing_whitespace(
                            bytes,
                            start,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                    }
                    emit_long_input_line(
                        bytes,
                        start,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    if environment.is_filled() {
                        emit_filled_text_tabs(
                            bytes,
                            start,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                    }
                }
                let has_invalid_input_bytes = emit_invalid_input_bytes(
                    bytes,
                    start,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let has_valid_utf8_non_ascii = contains_valid_utf8_non_ascii(bytes);
                let table_input_text = (has_invalid_input_bytes || has_valid_utf8_non_ascii)
                    .then(|| legacy_table_input_text(bytes));
                emit_unterminated_register_reference_escapes(
                    bytes,
                    scanner.escape_character(),
                    start,
                    end,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                emit_unterminated_string_reference_escapes(
                    bytes,
                    scanner.escape_character(),
                    start,
                    end,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                emit_outside_macro_argument_escapes(
                    bytes,
                    scanner.escape_character(),
                    start,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let Some(bytes) = expand_environment(
                    environment,
                    bytes,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    start,
                    end,
                    &mut expansion_steps,
                    &mut diagnostics,
                    &mut truncated,
                ) else {
                    break 'lines;
                };
                // A missing interpolation can leave the authored prefix with
                // terminal whitespace even when the physical source line did
                // not end in it (for example `name: \\*[missing]`).  The
                // validator observes that post-expansion line too, while an
                // authored trailing run was already checked above.
                if !authored_trailing_whitespace
                    && builder.macro_set() != MacroSet::None
                    && package_preprocessor_depth == 0
                    && (builder.macro_set() == MacroSet::Mdoc || environment.is_filled())
                {
                    emit_trailing_whitespace(
                        &bytes,
                        start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                // A recursive string expansion has a non-fatal legacy
                // recovery: its containing physical line disappears, while
                // the next input line remains independently parseable. Other
                // zero-byte results (notably blank/fill-mode input) retain
                // their normal package-level recovery behavior.
                let recursive_expansion = diagnostics.last().is_some_and(|diagnostic| {
                    diagnostic.code.as_str() == DiagnosticCode::LIMIT_EXPANSION_STEPS
                        && diagnostic.severity == Severity::Error
                        && diagnostic.message.as_ref()
                            == "input stack limit exceeded, infinite loop?"
                });
                if bytes.is_empty() && recursive_expansion {
                    continue;
                }
                let escape = scanner.escape_character();
                let Some(translated) = environment
                    .translate_text(&bytes, escape, limits.max_expanded_line_bytes)
                    .map_err(|error| {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            &mut truncated,
                        );
                        truncated = true;
                    })
                    .ok()
                else {
                    break 'lines;
                };
                // Definition copy mode can inject a literal tab into an
                // otherwise tab-free text line.  The physical input scan
                // above already owns authored tabs; report this expanded
                // form only when it cannot duplicate one of those findings.
                // Its byte position is still relative to the visible input
                // line, as in `.ds x<TAB>text` followed by `\\*[x]`.
                if environment.is_filled() && !authored_has_tab {
                    emit_filled_text_tabs(
                        &translated,
                        start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                emit_declared_character_escape_warnings(
                    &translated,
                    escape,
                    environment,
                    source_id,
                    start,
                    end,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let translated =
                    expand_declared_character_escapes(&translated, escape, environment);
                let result = normalize_document_escapes(builder, &translated, escape, limits);
                if !record_expansion_steps(
                    &mut expansion_steps,
                    result.steps,
                    limits,
                    source_id,
                    start,
                    end,
                    &mut diagnostics,
                    &mut truncated,
                ) {
                    break 'lines;
                }
                let suppress_table_continuation_escape = table_preprocessor_depth > 0
                    && has_physical_line_continuation(&translated, escape);
                if suppress_table_continuation_escape {
                    let translated_len = u32::try_from(translated.len())
                        .expect("parser line limits keep translated offsets public");
                    let issues = result
                        .issues
                        .iter()
                        .filter(|issue| {
                            !(issue.kind == EscapeIssueKind::Unterminated
                                && issue.offset.saturating_add(issue.length) == translated_len)
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    emit_escape_issues(
                        &issues,
                        start,
                        end,
                        &mut EmitContext::new(
                            source_id,
                            limits,
                            &mut text_bytes,
                            &mut diagnostics,
                            &mut truncated,
                        ),
                    );
                } else {
                    emit_escape_issues(
                        &result.issues,
                        start,
                        end,
                        &mut EmitContext::new(
                            source_id,
                            limits,
                            &mut text_bytes,
                            &mut diagnostics,
                            &mut truncated,
                        ),
                    );
                }
                truncated |= result.truncated;
                let flags = NodeFlags {
                    line_start: true,
                    line_continuation: result.line_continuation,
                    ..NodeFlags::default()
                };
                if append_text_node(
                    builder,
                    root,
                    start,
                    end,
                    flags,
                    result.text,
                    &mut EmitContext::new(
                        source_id,
                        limits,
                        &mut text_bytes,
                        &mut diagnostics,
                        &mut truncated,
                    ),
                ) {
                    if (has_invalid_input_bytes || has_valid_utf8_non_ascii)
                        && let Some(node) = builder
                            .children(root)
                            .and_then(|nodes| nodes.last())
                            .copied()
                    {
                        let _ = builder.set_node_input_unicode_provenance(
                            node,
                            has_invalid_input_bytes,
                            has_valid_utf8_non_ascii,
                        );
                        if let Some(table_input_text) = table_input_text {
                            let _ = builder.set_node_table_input_text(node, table_input_text);
                        }
                    }
                    maximum_depth = maximum_depth.max(2);
                }
                if let Some(name) = sprung_input_trap {
                    let name_end = name
                        .iter()
                        .position(u8::is_ascii_whitespace)
                        .unwrap_or(name.len());
                    if name_end == 0 {
                        continue;
                    }
                    let trap = ScopeLine::Control {
                        start,
                        end,
                        argument_start: start
                            .saturating_add(u32::try_from(name_end).unwrap_or(u32::MAX))
                            .saturating_add(1),
                        name: name[..name_end].to_vec(),
                        arguments: trim_horizontal_space(&name[name_end..]).to_vec(),
                    };
                    if matches!(
                        execute_scope_line(
                            &trap,
                            builder,
                            root,
                            source_id,
                            &mut scanner,
                            environment,
                            limits,
                            &mut text_bytes,
                            &mut expansion_steps,
                            &mut maximum_depth,
                            &mut total_loop_iterations,
                            &mut diagnostics,
                            &mut truncated,
                        ),
                        ScopeFlow::Halt
                    ) {
                        break 'lines;
                    }
                }
            }
            SourceEvent::Comment { start, end, bytes } => {
                // libmandoc preserves a comment as a distinct node, but does
                // not mark it as an implicit no-print node. Consumers use the
                // node kind to omit comments from rendered output.
                let flags = NodeFlags::default();
                if append_textual_node(
                    builder,
                    root,
                    NodeKind::Comment,
                    start..end,
                    flags,
                    visible_bytes(bytes),
                    &mut EmitContext::new(
                        source_id,
                        limits,
                        &mut text_bytes,
                        &mut diagnostics,
                        &mut truncated,
                    ),
                ) {
                    maximum_depth = maximum_depth.max(2);
                }
            }
            SourceEvent::Control(ControlEvent {
                start,
                control_start,
                mut end,
                name,
                request: raw_request,
                package: raw_package,
                arguments,
                raw_arguments,
                argument_start,
            }) => {
                // The physical scanner stops a control name at an adjacent
                // escape so condition openers such as `.el\{` can keep their
                // own grammar.  Roff names have a small, observable exception:
                // a doubled delimiter is a literal byte in a user-macro name.
                // Other adjacent escapes terminate the name and are diagnosed
                // before dispatching the valid prefix (for example
                // `.witharg\(enargument`).
                let attached_name = recover_attached_control_name(
                    name,
                    raw_arguments,
                    scanner.escape_character(),
                    raw_request.is_definition()
                        || raw_package.is_builtin(builder.macro_set())
                        || raw_package.is_mdoc_callable()
                        || environment.macro_definition(name).is_some()
                        || environment.is_suppressed_macro_name(name),
                );
                let attached_escape_width = attached_name
                    .as_ref()
                    .filter(|recovery| recovery.invalid_escape_preview.is_some())
                    .map(|_| roff_escape_name_width(raw_arguments, scanner.escape_character()));
                if let Some(recovery) = &attached_name
                    && let Some(preview) = &recovery.invalid_escape_preview
                {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_ESCAPED_NAME,
                            Severity::Error,
                            source_id,
                            start,
                            start.saturating_add(1),
                            format!(
                                "escaped character not allowed in a name: {}",
                                visible_bytes(preview)
                            ),
                        ),
                        &mut truncated,
                    );
                }
                let name = attached_name
                    .as_ref()
                    .map_or(name, |recovery| recovery.name.as_slice());
                let arguments = attached_name
                    .as_ref()
                    .map_or(arguments, |recovery| recovery.arguments.as_slice());
                let raw_arguments = attached_name
                    .as_ref()
                    .map_or(raw_arguments, |recovery| recovery.arguments.as_slice());
                let request = attached_name
                    .as_ref()
                    .map_or(raw_request, |_| RequestKind::classify(name));
                let package = attached_name.as_ref().map_or(raw_package, |_| {
                    PackageToken::classify(builder.macro_set(), name)
                });
                // A physical control line is outside every user-macro
                // argument frame.  Validate active `\$1`-style selectors
                // before its request-specific parser consumes or reparses
                // the arguments; copy-mode definitions retain doubled forms
                // and are therefore intentionally skipped by the helper.
                emit_outside_macro_argument_escapes(
                    arguments,
                    scanner.escape_character(),
                    argument_start,
                    source_id,
                    limits,
                    &mut diagnostics,
                    &mut truncated,
                );
                let sanitized_outside_macro_arguments =
                    strip_outside_macro_argument_escapes(arguments, scanner.escape_character());
                let arguments = sanitized_outside_macro_arguments.as_slice();
                // A recovered package macro begins its retained argument
                // after the full attached escape, rather than at the virtual
                // cursor used while its name is first recognized.
                let argument_start = match attached_escape_width {
                    Some(width) => argument_start.saturating_add(
                        u32::try_from(width)
                            .expect("attached escape width fits public source spans"),
                    ),
                    None => argument_start,
                };
                if request == RequestKind::OperatingSystem {
                    saw_mdoc_operating_system = true;
                }
                let mut continued_arguments = None;
                let mut continued_raw_arguments = None;
                let mut physical_continuation = false;
                let mut terminal_continuation_at_eof = false;
                // A terminal `\\{\\` on a conditional opener belongs to
                // the scope collector, not to this control line's argument
                // list.  Consuming its first body line here would prevent
                // the explicit scope executor from seeing it.
                if !request.owns_scope_continuation()
                    && has_physical_line_continuation(arguments, scanner.escape_character())
                {
                    let mut joined_arguments = arguments.to_vec();
                    let mut joined_raw_arguments = raw_arguments.to_vec();
                    while has_physical_line_continuation(
                        &joined_arguments,
                        scanner.escape_character(),
                    ) {
                        let Some(next_line) = scanner.next_line() else {
                            // Roff consumes a terminal odd escape together
                            // with the physical newline even at end of input.
                            // Retain the authored byte for the AST recovery,
                            // but remember that its otherwise generic escape
                            // finding must be suppressed below.
                            terminal_continuation_at_eof = true;
                            break;
                        };
                        match next_line {
                            ScannedLine::Text {
                                end: continuation_end,
                                bytes,
                                ..
                            } => {
                                let _ = joined_arguments.pop();
                                joined_arguments.extend_from_slice(bytes);
                                let _ = joined_raw_arguments.pop();
                                joined_raw_arguments.extend_from_slice(bytes);
                                physical_continuation = true;
                                end = continuation_end;
                            }
                            line => {
                                scanner.unread_line(line);
                                break;
                            }
                        }
                    }
                    if physical_continuation {
                        continued_arguments = Some(joined_arguments);
                        continued_raw_arguments = Some(joined_raw_arguments);
                    }
                }
                let arguments = continued_arguments.as_deref().unwrap_or(arguments);
                let raw_arguments = continued_raw_arguments.as_deref().unwrap_or(raw_arguments);
                let mut continued_argument_nodes = Vec::new();
                update_preprocessor_depth(&mut package_preprocessor_depth, name);
                update_table_preprocessor_depth(&mut table_preprocessor_depth, name);
                if let Some(message) = update_man_example_fill_presentation(
                    &mut man_example_fill_enabled,
                    builder.macro_set(),
                    name,
                ) {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::MAN_REDUNDANT_FILL_MODE,
                            Severity::Style,
                            source_id,
                            control_start,
                            end,
                            message,
                        ),
                        &mut truncated,
                    );
                }
                update_fill_mode(environment, builder.macro_set(), name, arguments);
                update_man_indent_register(
                    environment,
                    builder.macro_set(),
                    name,
                    arguments,
                    &mut man_indent_state,
                    limits,
                );
                if environment.is_filled()
                    && is_man_visible_argument_macro(builder.macro_set(), name)
                {
                    emit_filled_macro_argument_tabs(
                        arguments,
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                if builder.macro_set() == MacroSet::Mdoc {
                    // The argument parser owns the paired quote/tail
                    // recovery.  Emitting the generic mdoc tail finding
                    // first would both reverse mandoc's diagnostic order and
                    // duplicate the tail warning for an unterminated quote.
                    let unterminated_quote = matches!(
                        lex_arguments(raw_arguments, scanner.escape_character(), limits),
                        Err(ArgumentIssue::UnterminatedQuote)
                    );
                    if !unterminated_quote {
                        emit_mdoc_control_trailing_whitespace(
                            name,
                            raw_arguments,
                            end,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                    }
                    emit_mdoc_implicit_trailing_delimiter_spacing(
                        name,
                        raw_arguments,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    emit_mdoc_empty_display(
                        name,
                        arguments,
                        raw_arguments,
                        control_start,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                } else if builder.macro_set() == MacroSet::Man {
                    emit_man_alternating_font_trailing_whitespace(
                        name,
                        raw_arguments,
                        end,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                }
                if name == b"while"
                    && let Ok(arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                {
                    let Some(predicate_template) = arguments.first() else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff while request is missing its predicate",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let body_template = join_arguments(&arguments[1..]);
                    let empty_scope_finding = body_template.is_empty().then(|| {
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            control_start,
                            control_start.saturating_add(5),
                            "conditional request controls empty scope: while",
                        )
                    });
                    if let Some(finding) = &empty_scope_finding {
                        push_diagnostic(&mut diagnostics, limits, finding.clone(), &mut truncated);
                    }
                    let escape = scanner.escape_character();
                    let scope_remainder = scope_opener_remainder(&body_template, escape);
                    let scope_requested = scope_remainder.is_some();
                    // As with a multiline conditional, mandoc retains the
                    // trailing escape in a conventional `\\{\\` opener as
                    // the logical column of the first loop body node.  The
                    // token offset is relative to the raw argument slice.
                    let scope_opening_column = arguments
                        .get(1)
                        .filter(|argument| argument.bytes.starts_with(&[escape, b'{', escape]))
                        .and_then(|argument| {
                            u32::try_from(argument.offset)
                                .ok()
                                .and_then(|offset| argument_start.checked_add(offset))
                        })
                        .map(|body_start| body_start.saturating_add(2));
                    let mut scope = scope_requested.then(|| {
                        ScopeCollector {
                            scanner: &mut scanner,
                            source_id,
                            limits,
                            macro_set: builder.macro_set(),
                            diagnostics: &mut diagnostics,
                            truncated: &mut truncated,
                            emit_definition_tail_diagnostics: true,
                        }
                        .collect(control_start, end, Some(b"while"))
                    });
                    if let (Some(scope), Some(remainder)) = (&mut scope, scope_remainder)
                        && !remainder.is_empty()
                    {
                        scope.lines.insert(
                            0,
                            definition_scope_remainder_line(
                                remainder,
                                start,
                                end,
                                scanner.control_character(),
                                scanner.escape_character(),
                            ),
                        );
                    }
                    if scope_requested && scope.as_ref().is_some_and(|scope| !scope.terminated) {
                        continue;
                    }
                    let mut iterations = 0_usize;
                    loop {
                        let Some(predicate) = expand_environment(
                            environment,
                            &predicate_template.bytes,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        let Some(condition) = evaluate_condition(environment, &predicate) else {
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_CONDITION,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff while predicate is outside the M3 numeric/nroff subset",
                                ),
                                &mut truncated,
                            );
                            break;
                        };
                        if !condition {
                            break;
                        }
                        // A scope opener following an expanded numeric
                        // predicate is reparsed at the compacted roff input
                        // cursor.  The public node still points at the
                        // physical body line, but its logical column must use
                        // the expanded predicate width (and `\{`, not the
                        // continuation escape that follows it).
                        let virtual_scope_opening_position = scope_opening_column
                            .filter(|_| predicate.len() != predicate_template.bytes.len())
                            .and_then(|_| {
                                let opener = arguments.get(1)?;
                                let separator_width = opener.offset.checked_sub(
                                    predicate_template
                                        .offset
                                        .checked_add(predicate_template.bytes.len())?,
                                )?;
                                let control_span =
                                    SourceSpan::new(source_id, control_start, control_start)
                                        .ok()?;
                                let control_position = builder.source_position(&control_span)?;
                                let prefix_width =
                                    argument_start.saturating_sub(control_start) as usize;
                                let column = prefix_width
                                    .saturating_add(predicate.len())
                                    .saturating_add(separator_width)
                                    .saturating_add(2);
                                Some(SourcePosition {
                                    line: control_position.line,
                                    column: control_position.column.saturating_add(
                                        u32::try_from(column).expect(
                                            "bounded roff conditional widths fit source columns",
                                        ),
                                    ),
                                })
                            });
                        if iterations >= limits.max_loop_iterations {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::LIMIT_LOOP_ITERATIONS,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff while request exceeds max_loop_iterations",
                                ),
                                &mut truncated,
                            );
                            break;
                        }
                        if total_loop_iterations >= limits.max_total_loop_iterations {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::LIMIT_TOTAL_LOOP_ITERATIONS,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff while requests exceed max_total_loop_iterations",
                                ),
                                &mut truncated,
                            );
                            break;
                        }
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            1,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        iterations += 1;
                        total_loop_iterations += 1;
                        if let Some(scope) = &scope {
                            let first_scope_child =
                                builder.children(root).map_or(0, <[NodeId]>::len);
                            let scope_head_line = scope
                                .lines
                                .first()
                                .and_then(|line| {
                                    SourceSpan::new(
                                        source_id,
                                        scope_line_start(line),
                                        scope_line_start(line),
                                    )
                                    .ok()
                                })
                                .and_then(|span| builder.source_position(&span))
                                .map(|position| position.line);
                            let flow = ScopeMachine {
                                builder,
                                root,
                                source_id,
                                scanner: &mut scanner,
                                environment,
                                limits,
                                text_bytes: &mut text_bytes,
                                expansion_steps: &mut expansion_steps,
                                maximum_depth: &mut maximum_depth,
                                total_loop_iterations: &mut total_loop_iterations,
                                diagnostics: &mut diagnostics,
                                truncated: &mut truncated,
                            }
                            .run(&scope.lines);
                            let first_scope_child_is_scope_head = builder
                                .children(root)
                                .and_then(|children| children.get(first_scope_child))
                                .copied()
                                .and_then(|node| builder.node_source_position(node))
                                .zip(scope_head_line)
                                .is_some_and(|(position, line)| position.line == line);
                            // After the first replay, mandoc's roff input
                            // frame attributes retained scope output to the
                            // closing `\\}` line rather than repeatedly to
                            // the body's physical text line. Keep raw spans
                            // sliceable at their authored source while
                            // publishing that observable logical start.
                            if iterations == 1 {
                                if first_scope_child_is_scope_head
                                    && let Some(opener_start) = scope_opening_column
                                {
                                    set_first_scope_child_opening_column(
                                        builder,
                                        root,
                                        first_scope_child,
                                        source_id,
                                        opener_start,
                                    );
                                }
                            } else if let Some(replay_position) =
                                scope_replay_logical_start(builder, source_id, scope)
                            {
                                let position = scope_opening_column
                                    .filter(|_| first_scope_child_is_scope_head)
                                    .and_then(|opener_start| {
                                        SourceSpan::new(source_id, opener_start, opener_start)
                                            .ok()
                                            .and_then(|span| builder.source_position(&span))
                                    })
                                    .map_or(replay_position, |opening| SourcePosition {
                                        line: replay_position.line,
                                        column: opening.column,
                                    });
                                set_first_root_child_logical_start(
                                    builder,
                                    root,
                                    first_scope_child,
                                    position,
                                );
                                set_new_root_children_logical_start(
                                    builder,
                                    root,
                                    first_scope_child.saturating_add(1),
                                    replay_position,
                                );
                            }
                            match flow {
                                ScopeFlow::Break => break,
                                ScopeFlow::Continue | ScopeFlow::LoopContinue => continue,
                                ScopeFlow::CloseLoopInInnerScope { .. } => {
                                    if first_scope_child_is_scope_head
                                        && let Some(position) = virtual_scope_opening_position
                                    {
                                        set_first_scope_child_logical_start(
                                            builder,
                                            root,
                                            first_scope_child,
                                            position,
                                        );
                                    }
                                    pending_while_out_of_scope = true;
                                    break;
                                }
                                ScopeFlow::Halt => {
                                    break 'lines;
                                }
                            }
                        }
                        let Some(body) = expand_environment(
                            environment,
                            &body_template,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) && is_environment_request(request)
                        {
                            if matches!(request, b"ds" | b"as") {
                                if let Err(error) = apply_string_request(
                                    environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    request == b"as",
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            let Ok(arguments) =
                                lex_arguments(raw_arguments, scanner.escape_character(), limits)
                            else {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_LIMIT,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff while body arguments exceed configured parser limits",
                                    ),
                                    &mut truncated,
                                );
                                break;
                            };
                            if let Err(error) = apply_environment_request(
                                environment,
                                builder,
                                request,
                                scanner.escape_character(),
                                &arguments,
                                limits,
                            ) {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    environment_error_diagnostic(error, source_id, start, end),
                                    &mut truncated,
                                );
                                break;
                            }
                            continue;
                        }
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) && !is_builtin_package_macro(builder.macro_set(), request)
                            && let Some(definition) = environment.macro_definition(request).cloned()
                        {
                            let Ok(arguments) =
                                lex_arguments(raw_arguments, scanner.escape_character(), limits)
                            else {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_LIMIT,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff while macro arguments exceed configured parser limits",
                                    ),
                                    &mut truncated,
                                );
                                break;
                            };
                            let arguments = arguments
                                .into_iter()
                                .map(|argument| argument.bytes)
                                .collect::<Vec<_>>();
                            if !record_expansion_steps(
                                &mut expansion_steps,
                                1,
                                limits,
                                source_id,
                                start,
                                end,
                                &mut diagnostics,
                                &mut truncated,
                            ) {
                                break 'lines;
                            }
                            for line in definition.lines {
                                let line = copy_mode_reparse(&line, scanner.escape_character());
                                let Some(bytes) = expand_environment(
                                    environment,
                                    &line,
                                    scanner.escape_character(),
                                    &arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let result = normalize_document_escapes(
                                    builder,
                                    &bytes,
                                    scanner.escape_character(),
                                    limits,
                                );
                                if !record_expansion_steps(
                                    &mut expansion_steps,
                                    result.steps,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    break 'lines;
                                }
                                emit_escape_issues(
                                    &result.issues,
                                    start,
                                    end,
                                    &mut EmitContext::new(
                                        source_id,
                                        limits,
                                        &mut text_bytes,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ),
                                );
                                truncated |= result.truncated;
                                if append_text_node(
                                    builder,
                                    root,
                                    start,
                                    end,
                                    NodeFlags {
                                        line_start: true,
                                        line_continuation: result.line_continuation,
                                        ..NodeFlags::default()
                                    },
                                    result.text,
                                    &mut EmitContext::new(
                                        source_id,
                                        limits,
                                        &mut text_bytes,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ),
                                ) {
                                    maximum_depth = maximum_depth.max(2);
                                }
                            }
                            continue;
                        }
                        let result = normalize_document_escapes(
                            builder,
                            &body,
                            scanner.escape_character(),
                            limits,
                        );
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            result.steps,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        emit_escape_issues(
                            &result.issues,
                            start,
                            end,
                            &mut EmitContext::new(
                                source_id,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ),
                        );
                        truncated |= result.truncated;
                        let flags = NodeFlags {
                            line_start: true,
                            line_continuation: result.line_continuation,
                            ..NodeFlags::default()
                        };
                        let empty_while_body =
                            empty_scope_finding.is_some() && result.text.is_empty();
                        if append_text_node(
                            builder,
                            root,
                            start,
                            end,
                            flags,
                            result.text,
                            &mut EmitContext::new(
                                source_id,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ),
                        ) {
                            maximum_depth = maximum_depth.max(2);
                            if empty_while_body
                                && let Some(node) = builder
                                    .children(root)
                                    .and_then(|children| children.last())
                                    .copied()
                                && let Some(position) = builder.node_source_position(node)
                            {
                                let predicate_offset = u32::try_from(predicate_template.offset)
                                    .expect("argument offsets fit source positions");
                                let column = argument_start
                                    .saturating_sub(start)
                                    .saturating_add(predicate_offset)
                                    .saturating_add(2);
                                let _ = builder.set_node_logical_start(
                                    node,
                                    SourcePosition {
                                        line: position.line,
                                        column,
                                    },
                                );
                            }
                        }
                    }
                    if let Some(finding) = empty_scope_finding {
                        push_diagnostic(&mut diagnostics, limits, finding.clone(), &mut truncated);
                        // Reordering the deferred copy moves the first
                        // identical validator finding behind the physical
                        // input-line finding, yielding the upstream
                        // `while`, blank-line, `while` order.
                        deferred_post_validation_diagnostics.push(finding);
                    }
                    continue;
                }
                let raw_condition_arguments = arguments;
                if matches!(name, b"if" | b"ie" | b"el")
                    && let Ok(condition_arguments) =
                        lex_condition_arguments(arguments, scanner.escape_character(), limits)
                {
                    if environment.is_filled() {
                        let diagnostic_start = diagnostics.len();
                        emit_filled_macro_argument_tabs(
                            raw_condition_arguments,
                            argument_start,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        deferred_post_validation_diagnostics
                            .extend_from_slice(&diagnostics[diagnostic_start..]);
                    }
                    emit_escaped_condition_name(
                        &condition_arguments,
                        scanner.escape_character(),
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    let mut escaped_name_body_offset = None;
                    let (condition, body_start) = match name {
                        b"el" => (
                            previous_conditional
                                .take()
                                .map(BranchOutcome::inverse)
                                .map(BranchOutcome::is_taken),
                            0,
                        ),
                        b"if" | b"ie" => {
                            if name == b"ie"
                                && (condition_arguments.is_empty()
                                    || condition_arguments
                                        .first()
                                        .is_some_and(|argument| argument.bytes == b"!"))
                            {
                                // mandoc accepts an empty (also a lone
                                // negated-empty) `.ie` as false, leaving the
                                // following `.el` as the active arm.
                                previous_conditional = Some(BranchOutcome::Skipped);
                                (Some(false), condition_arguments.len())
                            } else {
                                let Some((predicate, body_start)) =
                                    condition_parts(&condition_arguments)
                                else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_CONDITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional is missing its predicate",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let (predicate, escaped_body_offset) =
                                    split_escaped_condition_body(
                                        &condition_arguments,
                                        scanner.escape_character(),
                                        &predicate,
                                    )
                                    .map_or_else(
                                        || (predicate, None),
                                        |(predicate, offset)| (predicate, Some(offset)),
                                    );
                                escaped_name_body_offset = escaped_body_offset;
                                let Some(predicate) = expand_environment(
                                    environment,
                                    &predicate,
                                    scanner.escape_character(),
                                    &[],
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let condition = evaluate_condition(environment, &predicate);
                                if name == b"ie" {
                                    previous_conditional = condition.map(BranchOutcome::from);
                                }
                                (condition, body_start)
                            }
                        }
                        _ => unreachable!("matches! limits the conditional request names"),
                    };
                    let Some(condition) = condition else {
                        if name == b"el" {
                            // An orphaned `.el` is a no-op in mandoc.  In
                            // particular, only the first else consumes the
                            // immediately preceding `.ie` state.
                            continue;
                        }
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff conditional predicate is outside the M3 numeric/nroff subset",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let body_template = condition_body_template_from_offset(
                        raw_condition_arguments,
                        &condition_arguments,
                        body_start,
                        escaped_name_body_offset,
                    );
                    let body_source_start = condition_body_source_start_from_offset(
                        raw_condition_arguments,
                        &condition_arguments,
                        body_start,
                        argument_start,
                        if body_template.is_empty() { end } else { start },
                        escaped_name_body_offset,
                    );
                    // In a same-line brace body, horizontal space directly
                    // after `\{` is scope grammar padding rather than
                    // visible prose.  Multiline scopes retain their original
                    // spelling for source-location accounting below.
                    let body_template_len = body_template.len();
                    let inline_scope_body = body_template
                        .strip_prefix(&[scanner.escape_character(), b'{'])
                        .and_then(|remainder| {
                            let trimmed = trim_horizontal_space(remainder);
                            scope_closer_offset(trimmed, scanner.escape_character())
                                .is_some()
                                .then_some(trimmed)
                        })
                        .filter(|trimmed| trimmed.len().saturating_add(2) != body_template_len);
                    let inline_scope_source_start = body_template
                        .strip_prefix(&[scanner.escape_character(), b'{'])
                        .filter(|remainder| {
                            scope_closer_offset(remainder, scanner.escape_character()).is_some()
                        })
                        .map(|_| {
                            scope_remainder_source_start(
                                &body_template,
                                body_source_start,
                                scanner.escape_character(),
                            )
                        });
                    let body_template = if let Some(trimmed) = inline_scope_body {
                        let mut normalized = Vec::with_capacity(trimmed.len().saturating_add(2));
                        normalized.extend_from_slice(&[scanner.escape_character(), b'{']);
                        normalized.extend_from_slice(trimmed);
                        normalized
                    } else {
                        body_template
                    };
                    let body_template =
                        inline_scope_body_template(&body_template, scanner.escape_character())
                            .unwrap_or(body_template);
                    let body_source_start = inline_scope_source_start.unwrap_or(body_source_start);
                    let escape = scanner.escape_character();
                    let scope_remainder = scope_opener_remainder(&body_template, escape);
                    let scope_requested = scope_remainder.is_some();
                    // The trailing escape in the conventional `\{\\` form
                    // owns the logical column of the first physical scope
                    // line, even though that line has its own byte span.
                    let scope_opening_column = body_template
                        .starts_with(&[escape, b'{', escape])
                        .then(|| body_source_start.saturating_add(2));
                    let bare_scope_opener = scope_remainder.is_some_and(<[u8]>::is_empty)
                        && !body_template.starts_with(&[escape, b'{', escape]);
                    let mut scope = scope_requested.then(|| {
                        ScopeCollector {
                            scanner: &mut scanner,
                            source_id,
                            limits,
                            macro_set: builder.macro_set(),
                            diagnostics: &mut diagnostics,
                            truncated: &mut truncated,
                            emit_definition_tail_diagnostics: condition,
                        }
                        .collect(control_start, end, Some(name))
                    });
                    // A bare `\{` (without the conventional continuation
                    // escape) starts its active roff scope with a vertical
                    // blank.  Preserve that event for man validation;
                    // `\{\` intentionally starts directly with the
                    // following physical line instead.
                    if builder.macro_set() == MacroSet::Man
                        && condition
                        && bare_scope_opener
                        && let Some(scope) = &mut scope
                    {
                        let blank_start =
                            scope_remainder_source_start(&body_template, body_source_start, escape);
                        scope.lines.insert(
                            0,
                            ScopeLine::Text {
                                start: blank_start,
                                end: blank_start,
                                bytes: Vec::new(),
                                terminal_inline: false,
                            },
                        );
                    }
                    if let (Some(scope), Some(remainder)) = (&mut scope, scope_remainder) {
                        let remainder = trim_horizontal_space(remainder);
                        if !remainder.is_empty() {
                            scope.lines.insert(
                                0,
                                definition_scope_remainder_line(
                                    remainder,
                                    scope_remainder_source_start(
                                        &body_template,
                                        body_source_start,
                                        escape,
                                    ),
                                    end,
                                    scanner.control_character(),
                                    scanner.escape_character(),
                                ),
                            );
                        }
                    }
                    if let Some(scope) = &scope {
                        if condition {
                            let first_scope_child =
                                builder.children(root).map_or(0, <[NodeId]>::len);
                            let flow = ScopeMachine {
                                builder,
                                root,
                                source_id,
                                scanner: &mut scanner,
                                environment,
                                limits,
                                text_bytes: &mut text_bytes,
                                expansion_steps: &mut expansion_steps,
                                maximum_depth: &mut maximum_depth,
                                total_loop_iterations: &mut total_loop_iterations,
                                diagnostics: &mut diagnostics,
                                truncated: &mut truncated,
                            }
                            .run(&scope.lines);
                            if let Some(opener_start) = scope_opening_column {
                                set_first_scope_child_opening_column(
                                    builder,
                                    root,
                                    first_scope_child,
                                    source_id,
                                    opener_start,
                                );
                            }
                            if matches!(flow, ScopeFlow::Halt) {
                                break 'lines;
                            }
                        }
                        if !condition {
                            record_suppressed_scope_definitions(
                                &scope.lines,
                                scanner.escape_character(),
                                environment,
                                limits,
                            );
                        }
                        continue;
                    }
                    let predicate_end = body_start
                        .checked_sub(1)
                        .and_then(|index| condition_arguments.get(index))
                        .map_or(0, |argument| argument.offset + argument.bytes.len());
                    let next_line_scope = body_template.is_empty()
                        && raw_condition_arguments
                            .get(predicate_end..)
                            .is_some_and(<[u8]>::is_empty);
                    if next_line_scope {
                        // `roff_cond()` uses a next-line scope only when
                        // nothing follows the predicate.  The `.ie` state
                        // remains available for the subsequent `.el` after
                        // this one physical input line has been consumed.
                        // In man input, the active next-line form also
                        // materializes the empty vertical request that
                        // terminates the preceding paragraph before the next
                        // physical line is scanned.  It is an authored
                        // layout event (unlike the private bare-`\{` marker
                        // above), so keep it printable and source-bound.
                        if builder.macro_set() == MacroSet::Man && condition {
                            let _ = append_text_node(
                                builder,
                                root,
                                end,
                                end,
                                NodeFlags {
                                    line_start: true,
                                    ..NodeFlags::default()
                                },
                                String::new(),
                                &mut EmitContext::new(
                                    source_id,
                                    limits,
                                    &mut text_bytes,
                                    &mut diagnostics,
                                    &mut truncated,
                                ),
                            );
                        }
                        next_line_condition = Some(condition.into());
                        continue;
                    }
                    if name != b"el" && body_template.is_empty() {
                        // Trailing horizontal input after the predicate turns
                        // an otherwise next-line conditional into an empty
                        // scope.  It neither consumes the next physical line
                        // nor depends on whether the predicate was true.
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!(
                                    "conditional request controls empty scope: {}",
                                    visible_bytes(name)
                                ),
                            ),
                            &mut truncated,
                        );
                    }
                    if condition {
                        let Some(body) = expand_environment(
                            environment,
                            &body_template,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            body_source_start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) {
                            if matches!(request, b"cc" | b"c2" | b"ec") {
                                scanner.apply_character_request(request, raw_arguments);
                                continue;
                            }
                            if is_environment_request(request) {
                                if matches!(request, b"ds" | b"as") {
                                    if let Err(error) = apply_string_request(
                                        environment,
                                        raw_arguments,
                                        scanner.escape_character(),
                                        request == b"as",
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut expansion_steps,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            environment_error_diagnostic(
                                                error, source_id, start, end,
                                            ),
                                            &mut truncated,
                                        );
                                    }
                                    continue;
                                }
                                let Ok(arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional body arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                if let Err(error) = apply_environment_request(
                                    environment,
                                    builder,
                                    request,
                                    scanner.escape_character(),
                                    &arguments,
                                    limits,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            // A same-line conditional can dispatch a man or
                            // mdoc package macro just like ordinary physical
                            // input.  Treating it as raw text loses semantic
                            // constructs such as Pod's `.el .IP ...` option
                            // terms, because the normal scanner dispatch is
                            // bypassed by the conditional executor.
                            if is_builtin_package_macro(builder.macro_set(), request) {
                                let body = ScopeLine::Control {
                                    start: body_source_start,
                                    end,
                                    argument_start: body_source_start
                                        .saturating_add(1)
                                        .saturating_add(
                                            u32::try_from(request.len())
                                                .expect("request names fit source spans"),
                                        )
                                        .saturating_add(u32::from(!raw_arguments.is_empty())),
                                    name: request.to_vec(),
                                    arguments: raw_arguments.to_vec(),
                                };
                                if matches!(
                                    execute_scope_line(
                                        &body,
                                        builder,
                                        root,
                                        source_id,
                                        &mut scanner,
                                        environment,
                                        limits,
                                        &mut text_bytes,
                                        &mut expansion_steps,
                                        &mut maximum_depth,
                                        &mut total_loop_iterations,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ),
                                    ScopeFlow::Halt
                                ) {
                                    break 'lines;
                                }
                                continue;
                            }
                            if !is_builtin_package_macro(builder.macro_set(), request)
                                && let Some(definition) =
                                    environment.macro_definition(request).cloned()
                            {
                                let Ok(arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "inline roff conditional macro arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let arguments = arguments
                                    .into_iter()
                                    .map(|argument| argument.bytes)
                                    .collect::<Vec<_>>();
                                if matches!(
                                    execute_scope_macro_lines(
                                        definition.lines,
                                        &arguments,
                                        1,
                                        builder,
                                        root,
                                        source_id,
                                        start,
                                        end,
                                        &mut scanner,
                                        environment,
                                        limits,
                                        &mut text_bytes,
                                        &mut expansion_steps,
                                        &mut maximum_depth,
                                        &mut total_loop_iterations,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ),
                                    ScopeFlow::Halt
                                ) {
                                    break 'lines;
                                }
                                continue;
                            }
                        }
                        let result = normalize_document_escapes(
                            builder,
                            &body,
                            scanner.escape_character(),
                            limits,
                        );
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            result.steps,
                            limits,
                            source_id,
                            body_source_start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        emit_escape_issues(
                            &result.issues,
                            body_source_start,
                            end,
                            &mut EmitContext::new(
                                source_id,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ),
                        );
                        truncated |= result.truncated;
                        let flags = NodeFlags {
                            line_start: true,
                            line_continuation: result.line_continuation,
                            ..NodeFlags::default()
                        };
                        if append_text_node(
                            builder,
                            root,
                            body_source_start,
                            end,
                            flags,
                            result.text,
                            &mut EmitContext::new(
                                source_id,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ),
                        ) {
                            if let Some(node) = builder
                                .children(root)
                                .and_then(|children| children.last())
                                .copied()
                            {
                                let _ = builder.set_node_terminal_inline_conditional(node, true);
                            }
                            maximum_depth = maximum_depth.max(2);
                        }
                    }
                    continue;
                }
                if name == b"return" {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_RETURN_OUTSIDE_MACRO,
                            Severity::Error,
                            source_id,
                            control_start,
                            control_start
                                .saturating_add(u32::try_from(name.len()).unwrap_or(u32::MAX)),
                            "ignoring request outside macro: return",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                // `.ab` is a formatter-side abort request.  A semantic
                // manual parser cannot perform its process-control effect,
                // but must retain libmandoc's recoverable unsupported
                // finding instead of letting mdoc validation reinterpret it
                // as NAME-section prose.
                if name == b"ab" {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNKNOWN_MACRO,
                            Severity::Unsupported,
                            source_id,
                            control_start,
                            end,
                            "unsupported roff request: ab",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if name == b"shift" {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_SHIFT,
                            Severity::Error,
                            source_id,
                            control_start,
                            control_start
                                .saturating_add(u32::try_from(name.len()).unwrap_or(u32::MAX)),
                            "ignoring request outside macro: shift",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if name == b"so" {
                    let Some(target) = expand_environment(
                        environment,
                        trim_horizontal_space(arguments),
                        scanner.escape_character(),
                        &[],
                        limits,
                        source_id,
                        start,
                        end,
                        &mut expansion_steps,
                        &mut diagnostics,
                        &mut truncated,
                    ) else {
                        break 'lines;
                    };
                    let target = trim_horizontal_space(&target);
                    if target.is_empty() {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_UNAVAILABLE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include request has no target",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    if include_depth >= limits.max_include_depth {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_INCLUDE_DEPTH,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include nesting exceeds max_include_depth",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let remaining_bytes =
                        limits.max_total_source_bytes.saturating_sub(source_bytes);
                    let resolution = resolver.resolve(IncludeRequest {
                        including: source.name,
                        raw_target: target,
                        remaining_depth: limits.max_include_depth - include_depth,
                        remaining_bytes,
                    });
                    let Ok(resolution) = resolution else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_RESOLVER,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include resolver rejected the requested target",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let Some(included) = resolution else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_UNAVAILABLE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff .so include target is unavailable from the configured resolver",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    if active_sources.iter().any(|active| active == &included.name) {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_INCLUDE_CYCLE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include target would re-enter the active include stack",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    if source_files >= limits.max_sources {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff include graph exceeds max_sources",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    if included.bytes.len() > limits.max_root_source_bytes
                        || source_bytes
                            .checked_add(included.bytes.len())
                            .is_none_or(|total| total > limits.max_total_source_bytes)
                    {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCE_BYTES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "resolved roff include exceeds the configured source-byte budget",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let resolved_lines = included.bytes.split(|byte| *byte == b'\n').count();
                    if resolved_lines > limits.max_source_lines {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCE_LINES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "resolved roff include exceeds max_source_lines",
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let Some(resolved_source_id) =
                        builder.add_source(Source::new(&included.name, &included.bytes))
                    else {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_SOURCES,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "resolved roff include cannot be represented in the source map",
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    source_bytes += included.bytes.len();
                    source_files += 1;
                    active_sources.push(included.name.clone());
                    let mut included_session =
                        ParseSession::new(config, builder, environment, active_sources, resolver);
                    let outcome = SourceMachine::new(
                        Source::new(&included.name, &included.bytes),
                        resolved_source_id,
                        include_depth + 1,
                        &mut included_session,
                        ScanOutcome {
                            diagnostics,
                            deferred_post_validation_diagnostics,
                            source_bytes,
                            source_files,
                            text_bytes,
                            expansion_steps,
                            truncated,
                            maximum_depth,
                            previous_conditional,
                            total_loop_iterations,
                            saw_mdoc_operating_system,
                        },
                    )
                    .run();
                    active_sources.pop();
                    diagnostics = outcome.diagnostics;
                    deferred_post_validation_diagnostics =
                        outcome.deferred_post_validation_diagnostics;
                    text_bytes = outcome.text_bytes;
                    expansion_steps = outcome.expansion_steps;
                    truncated = outcome.truncated;
                    maximum_depth = outcome.maximum_depth;
                    previous_conditional = outcome.previous_conditional;
                    total_loop_iterations = outcome.total_loop_iterations;
                    source_bytes = outcome.source_bytes;
                    source_files = outcome.source_files;
                    saw_mdoc_operating_system = outcome.saw_mdoc_operating_system;
                    continue;
                }
                if matches!(name, b"de" | b"de1" | b"am" | b"dei" | b"ami")
                    && let Ok(arguments) =
                        lex_arguments(arguments, scanner.escape_character(), limits)
                {
                    let Some(definition_name) = arguments.first() else {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_EMPTY_REQUEST,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!("skipping empty request: {}", visible_bytes(name)),
                            ),
                            &mut truncated,
                        );
                        continue;
                    };
                    let indirect = matches!(name, b"dei" | b"ami");
                    let name_terminates_at_tab = definition_name.separator_after == Some(b'\t');
                    if !indirect && !name_terminates_at_tab && arguments.get(2).is_some() {
                        let ignored_after_tab = arguments
                            .get(1)
                            .is_some_and(|terminator| terminator.separator_after == Some(b'\t'));
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_ALL_ARGUMENTS,
                                Severity::Error,
                                source_id,
                                argument_start,
                                end,
                                format!(
                                    "skipping excess arguments: .{} ... {}",
                                    visible_bytes(name),
                                    if ignored_after_tab {
                                        "ignored"
                                    } else {
                                        "excess arguments"
                                    },
                                ),
                            ),
                            &mut truncated,
                        );
                    }
                    let definition_name = if indirect {
                        let Some(definition_name) =
                            environment.indirect_string(&definition_name.bytes)
                        else {
                            let name_start = argument_start.saturating_add(
                                u32::try_from(definition_name.offset)
                                    .expect("definition-name offsets fit source positions"),
                            );
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                    Severity::Warning,
                                    source_id,
                                    name_start,
                                    end,
                                    format!(
                                        "undefined string, using \"\": {}",
                                        visible_bytes(&definition_name.bytes)
                                    ),
                                ),
                                &mut truncated,
                            );
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_EMPTY_REQUEST,
                                    Severity::Warning,
                                    source_id,
                                    control_start,
                                    control_start.saturating_add(2),
                                    format!("skipping empty request: {}", visible_bytes(name)),
                                ),
                                &mut truncated,
                            );
                            continue;
                        };
                        definition_name
                    } else {
                        let normalized = normalize_roff_name_prefix(
                            &definition_name.bytes,
                            scanner.escape_character(),
                        );
                        if let Some(preview) = normalized.invalid_escape_preview {
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_ESCAPED_NAME,
                                    Severity::Error,
                                    source_id,
                                    control_start,
                                    control_start.saturating_add(1),
                                    format!(
                                        "escaped character not allowed in a name: {}",
                                        visible_bytes(&preview)
                                    ),
                                ),
                                &mut truncated,
                            );
                        }
                        normalized.name
                    };
                    if definition_name.is_empty() {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_EMPTY_REQUEST,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!("skipping empty request: {}", visible_bytes(name)),
                            ),
                            &mut truncated,
                        );
                        continue;
                    }
                    let append = matches!(name, b"am" | b"ami");
                    let terminator = match arguments.get(1).filter(|_| !name_terminates_at_tab) {
                        None => vec![b'.'],
                        Some(argument) if !indirect => argument.bytes.clone(),
                        Some(argument) => {
                            if let Some(terminator) = environment.indirect_string(&argument.bytes) {
                                terminator
                            } else {
                                let terminator_start = argument_start.saturating_add(
                                    u32::try_from(argument.offset)
                                        .expect("terminator offsets fit source positions"),
                                );
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                        Severity::Warning,
                                        source_id,
                                        terminator_start,
                                        end,
                                        format!(
                                            "undefined string, using \"\": {}",
                                            visible_bytes(&argument.bytes)
                                        ),
                                    ),
                                    &mut truncated,
                                );
                                // An unresolved indirect end marker emits
                                // its string finding, then falls back to the
                                // traditional `..` terminator. The first
                                // indirect name is still a valid macro name,
                                // so following copy-mode input belongs to
                                // that recovered definition.
                                vec![b'.']
                            }
                        }
                    };
                    let definition_control = scanner.control_character();
                    let mut body = Vec::new();
                    let mut terminated = false;
                    while let Some(body_line) = scanner.next_raw_line() {
                        if is_definition_terminator(
                            body_line.bytes,
                            definition_control,
                            &terminator,
                        ) {
                            terminated = true;
                            break;
                        }
                        if body_line.too_long {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::LIMIT_LINE_BYTES,
                                    Severity::Warning,
                                    source_id,
                                    body_line.start,
                                    body_line.end,
                                    "copy-mode macro line exceeds max_line_bytes and was skipped",
                                ),
                                &mut truncated,
                            );
                            continue;
                        }
                        let Some(copy_mode_line) = expand_copy_mode_definition(
                            environment,
                            body_line.bytes,
                            scanner.escape_character(),
                            limits,
                            source_id,
                            body_line.start,
                            body_line.end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        body.push(copy_mode_reparse(
                            &copy_mode_line,
                            scanner.escape_character(),
                        ));
                    }
                    if !terminated {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff macro definition reached source end before its `..` terminator",
                            ),
                            &mut truncated,
                        );
                    }
                    let definition = if indirect {
                        environment.define_indirect_macro(&definition_name, body, append, limits)
                    } else {
                        environment.define_macro(&definition_name, body, append, limits)
                    };
                    if let Err(error) = definition {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            &mut truncated,
                        );
                    }
                    continue;
                }
                if name == b"ig" {
                    let arguments =
                        match lex_arguments(arguments, scanner.escape_character(), limits) {
                            Ok(arguments) => arguments,
                            Err(ArgumentIssue::UnterminatedQuote) => {
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff ignore-block marker contains an unterminated quote",
                                    ),
                                    &mut truncated,
                                );
                                Vec::new()
                            }
                            Err(ArgumentIssue::Limit) => {
                                truncated = true;
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_LIMIT,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff ignore-block marker exceeds configured parser limits",
                                    ),
                                    &mut truncated,
                                );
                                Vec::new()
                            }
                        };
                    if let [marker, excess, ..] = arguments.as_slice() {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                                Severity::Error,
                                source_id,
                                argument_start,
                                argument_start.saturating_add(
                                    u32::try_from(marker.bytes.len()).unwrap_or(u32::MAX),
                                ),
                                format!(
                                    "skipping excess arguments: .ig ... {}",
                                    visible_bytes(&excess.bytes)
                                ),
                            ),
                            &mut truncated,
                        );
                    }
                    let marker = arguments
                        .first()
                        .map_or_else(|| vec![b'.'], |argument| argument.bytes.clone());
                    let mut terminated = false;
                    while let Some(ignored) = scanner.next_raw_line() {
                        if is_ignore_terminator(ignored.bytes, scanner.control_character(), &marker)
                        {
                            terminated = true;
                            break;
                        }
                    }
                    if !terminated {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNCLOSED_IGNORE,
                                Severity::Error,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                "appending missing end of block: ig",
                            ),
                            &mut truncated,
                        );
                    }
                    continue;
                }
                if name == b"." {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNMATCHED_END,
                            Severity::Error,
                            source_id,
                            control_start,
                            control_start.saturating_add(1),
                            "skipping end of block that is not open: ..",
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if let RequestKind::Transparent(transparent_request) = request {
                    execute_transparent_request(TransparentRequestContext {
                        request: transparent_request,
                        arguments,
                        escape: scanner.escape_character(),
                        source_id,
                        end,
                        control_start,
                        argument_start,
                        environment,
                        input_trap: &mut input_trap,
                        limits,
                        diagnostics: &mut diagnostics,
                        truncated: &mut truncated,
                    });
                    continue;
                }
                if name == b"ft" {
                    emit_font_request_diagnostics(
                        arguments,
                        scanner.escape_character(),
                        argument_start,
                        source_id,
                        limits,
                        &mut diagnostics,
                        &mut truncated,
                    );
                    if builder.macro_set() == MacroSet::Man
                        && let Ok(font_arguments) =
                            lex_arguments(arguments, scanner.escape_character(), limits)
                        && let Some(font) = font_arguments.first()
                        && !is_legacy_roff_font_selector(&font.bytes)
                    {
                        let diagnostic_start = diagnostics.len();
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNKNOWN_FONT,
                                Severity::Warning,
                                source_id,
                                control_start,
                                control_start.saturating_add(2),
                                format!(
                                    "unknown font, skipping request: ft {}",
                                    visible_bytes(&font.bytes)
                                ),
                            ),
                            &mut truncated,
                        );
                        deferred_post_validation_diagnostics
                            .extend_from_slice(&diagnostics[diagnostic_start..]);
                        continue;
                    }
                }
                if let RequestKind::Environment(environment_request) = request
                    && matches!(
                        execute_environment_request(EnvironmentRequestContext {
                            request: environment_request,
                            arguments,
                            escape: scanner.escape_character(),
                            source_id,
                            start,
                            end,
                            control_start,
                            argument_start,
                            environment,
                            builder,
                            limits,
                            expansion_steps: &mut expansion_steps,
                            diagnostics: &mut diagnostics,
                            truncated: &mut truncated,
                        }),
                        RequestHandling::Handled
                    )
                {
                    continue;
                }
                if name == b"Os"
                    && (builder.macro_set() == MacroSet::Mdoc || config.syntax == Syntax::Mdoc)
                {
                    match lex_arguments(arguments, scanner.escape_character(), limits) {
                        Ok(arguments) if arguments.is_empty() => {
                            if let Some(operating_system) = config.operating_system.as_deref() {
                                builder.operating_system(operating_system);
                            }
                        }
                        Ok(arguments) => {
                            // The author-selected value wins over the session fallback.
                            // M6 will perform full mdoc argument semantics; scanner-stage
                            // metadata already uses the public visible-byte normalization.
                            builder.operating_system(visible_bytes(&join_arguments(&arguments)));
                        }
                        Err(ArgumentIssue::UnterminatedQuote) => push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "mdoc Os arguments contain an unterminated quote",
                            ),
                            &mut truncated,
                        ),
                        Err(ArgumentIssue::Limit) => {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "mdoc Os arguments exceed configured parser limits",
                                ),
                                &mut truncated,
                            );
                        }
                    }
                }
                let renamed_package_macro = environment.renamed_package_macro(name).is_some();
                let dispatched_package_macro =
                    environment.renamed_package_macro(name).unwrap_or(name);
                let dispatched_package_token = environment
                    .renamed_package_macro(name)
                    .map_or(package, |dispatched| {
                        PackageToken::classify(builder.macro_set(), dispatched)
                    });
                let builtin_package_macro =
                    dispatched_package_token.is_builtin(builder.macro_set());
                if !builtin_package_macro && environment.is_suppressed_macro_name(name) {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNKNOWN_MACRO,
                            Severity::Error,
                            source_id,
                            control_start,
                            end,
                            format!(
                                "skipping unknown macro: .{}",
                                attached_name.as_ref().map_or_else(
                                    || visible_bytes(name),
                                    |recovery| { visible_bytes(&recovery.display_name) }
                                )
                            ),
                        ),
                        &mut truncated,
                    );
                    continue;
                }
                if !builtin_package_macro && environment.is_conditionally_unknown_macro(name) {
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNKNOWN_MACRO,
                            Severity::Error,
                            source_id,
                            control_start,
                            end,
                            format!("skipping unknown macro: .{}", visible_bytes(name)),
                        ),
                        &mut truncated,
                    );
                    // `roff_userdef()` installs the observed unknown control
                    // as an empty user macro after reporting it.  A later
                    // `dname` condition therefore becomes true until a real
                    // `.de` replaces that placeholder.
                    if let Err(error) = environment.define_macro(name, Vec::new(), false, limits) {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            &mut truncated,
                        );
                    }
                    continue;
                }
                if !builtin_package_macro && environment.is_empty_string(name) {
                    continue;
                }
                let appended_package_macro =
                    builtin_package_macro && environment.has_appended_macro_definition(name);
                // mandoc keeps a renamed package macro's original argument
                // cursor while executing an `.am` body.  For a no-argument
                // invocation that cursor is the final byte of the authored
                // alias (for example `.myBc` after `.rn Bc myBc`), and its
                // generic argument reader emits the usual end-of-line style
                // finding there.  Keep this deliberately scoped to the
                // renamed-and-appended package path: ordinary no-argument
                // package controls do not imply trailing whitespace.
                if appended_package_macro && raw_arguments.is_empty() {
                    // The position advances by the original package name,
                    // not the (possibly longer) alias spelling.
                    let alias_end = control_start.saturating_add(
                        u32::try_from(dispatched_package_macro.len())
                            .expect("parsed control names fit public source offsets"),
                    );
                    push_diagnostic(
                        &mut diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                            Severity::Style,
                            source_id,
                            alias_end,
                            alias_end,
                            "whitespace at end of input line",
                        ),
                        &mut truncated,
                    );
                }
                if (!builtin_package_macro || appended_package_macro)
                    && let Some(definition) = environment.macro_definition(name).cloned()
                {
                    if environment.is_filled() {
                        let diagnostic_start = diagnostics.len();
                        emit_user_macro_leading_tabs(
                            raw_arguments,
                            control_start,
                            name.len(),
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        deferred_post_validation_diagnostics
                            .extend_from_slice(&diagnostics[diagnostic_start..]);
                    }
                    let unterminated_quote = matches!(
                        lex_user_macro_arguments(arguments, scanner.escape_character(), limits),
                        Err(ArgumentIssue::UnterminatedQuote)
                    );
                    if !unterminated_quote
                        && builder.macro_set() != MacroSet::Mdoc
                        && trailing_whitespace_start(arguments).is_some()
                    {
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                                Severity::Style,
                                source_id,
                                end,
                                end,
                                "whitespace at end of input line",
                            ),
                            &mut truncated,
                        );
                    }
                    let mut arguments = match lex_user_macro_arguments(
                        arguments,
                        scanner.escape_character(),
                        limits,
                    ) {
                        Ok(arguments) => arguments,
                        Err(ArgumentIssue::UnterminatedQuote) => {
                            emit_unterminated_quoted_argument(
                                arguments,
                                argument_start,
                                end,
                                source_id,
                                limits,
                                &mut diagnostics,
                                &mut truncated,
                            );
                            match recover_unterminated_quoted_arguments(
                                arguments,
                                scanner.escape_character(),
                                limits,
                            ) {
                                Ok(arguments) => arguments,
                                Err(ArgumentIssue::UnterminatedQuote) => unreachable!(
                                    "the synthetic closing quote always completes a bounded token"
                                ),
                                Err(ArgumentIssue::Limit) => {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "macro invocation arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                }
                            }
                        }
                        Err(ArgumentIssue::Limit) => {
                            truncated = true;
                            push_diagnostic(
                                &mut diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "macro invocation arguments exceed configured parser limits",
                                ),
                                &mut truncated,
                            );
                            continue;
                        }
                    };
                    retain_user_macro_tab_argument_prefix(&mut arguments, raw_arguments);
                    if appended_package_macro {
                        let Some(element) = append_node(
                            builder,
                            root,
                            NodeKind::Element,
                            control_start,
                            end,
                            NodeFlags {
                                line_start: true,
                                ..NodeFlags::default()
                            },
                            &mut EmitContext::new(
                                source_id,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ),
                        ) else {
                            continue;
                        };
                        if !builder.macro_name(element, visible_bytes(dispatched_package_macro)) {
                            truncated = true;
                            continue;
                        }
                        maximum_depth = maximum_depth.max(2);
                        for argument in &arguments {
                            let argument_offset = u32::try_from(argument.offset)
                                .expect("argument offsets are bounded by line length");
                            if !append_text_node(
                                builder,
                                element,
                                argument_start
                                    .checked_add(argument_offset)
                                    .expect("parser checks public span offsets first"),
                                end,
                                NodeFlags::default(),
                                visible_bytes(&argument.bytes),
                                &mut EmitContext::new(
                                    source_id,
                                    limits,
                                    &mut text_bytes,
                                    &mut diagnostics,
                                    &mut truncated,
                                ),
                            ) {
                                break 'lines;
                            }
                            maximum_depth = maximum_depth.max(3);
                        }
                    }
                    // Macro-generated mdoc arguments normally inherit the
                    // caller's first argument column.  The empty alias form
                    // has no such byte: mandoc carries its argument cursor
                    // at the alias's final byte while it runs the appended
                    // body.  Preserve that source provenance for controls
                    // emitted by the body (notably `.Pq` in rn/append).
                    let macro_generated_argument_start =
                        if appended_package_macro && raw_arguments.is_empty() {
                            argument_start.saturating_sub(1)
                        } else {
                            argument_start
                        };
                    let arguments = arguments
                        .into_iter()
                        .map(|argument| {
                            macro_argument_copy_mode_reparse(
                                &argument.bytes,
                                scanner.escape_character(),
                            )
                        })
                        .collect::<Vec<_>>();
                    if !record_expansion_steps(
                        &mut expansion_steps,
                        1,
                        limits,
                        source_id,
                        start,
                        end,
                        &mut diagnostics,
                        &mut truncated,
                    ) {
                        break 'lines;
                    }
                    let mut pending = definition
                        .lines
                        .into_iter()
                        .rev()
                        .map(|line| (line, arguments.clone(), 1_usize, 0_u32, None, false))
                        .collect::<Vec<_>>();
                    let mut macro_conditionals = Vec::<(usize, bool)>::new();
                    while let Some((
                        source_line,
                        macro_arguments,
                        macro_depth,
                        macro_origin,
                        text_origin,
                        scope_reparse,
                    )) = pending.pop()
                    {
                        let body_line = normalize_macro_argument_number_escapes(
                            &copy_mode_reparse(&source_line, scanner.escape_character()),
                            scanner.escape_character(),
                            start,
                            builder,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        if let Some((request, raw_arguments)) = split_macro_control(
                            &body_line,
                            scanner.control_character(),
                            scanner.escape_character(),
                        ) {
                            // Physical comments are removed by `Scanner`, but
                            // a copy-mode macro body is re-dispatched here.
                            // Treat its `\"` request identically instead of
                            // publishing Sphinx's bookkeeping comments as
                            // ordinary text between transparent indents.
                            if is_macro_comment_request(request, scanner.escape_character()) {
                                continue;
                            }
                            if matches!(request, b"cc" | b"c2" | b"ec") {
                                scanner.apply_character_request(request, raw_arguments);
                                continue;
                            }
                            if request == b"return" {
                                pending.retain(|(_, _, depth, _, _, _)| *depth < macro_depth);
                                continue;
                            }
                            if request == b"shift" {
                                let count = match lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) {
                                    Ok(arguments) => {
                                        arguments.first().map_or(Ok(1_usize), |argument| {
                                            std::str::from_utf8(&argument.bytes)
                                                .ok()
                                                .and_then(|value| value.parse::<usize>().ok())
                                                .ok_or(())
                                        })
                                    }
                                    Err(_) => Err(()),
                                };
                                let request_argument_start = start.saturating_add(
                                    u32::try_from(
                                        body_line.len().saturating_sub(raw_arguments.len()),
                                    )
                                    .expect("bounded macro body offsets fit public spans"),
                                );
                                let count = if let Ok(count) = count {
                                    count
                                } else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_SHIFT,
                                            Severity::Error,
                                            source_id,
                                            request_argument_start,
                                            request_argument_start.saturating_add(
                                                u32::try_from(raw_arguments.len()).expect(
                                                    "bounded macro body offsets fit public spans",
                                                ),
                                            ),
                                            format!(
                                                "argument is not numeric, using 1: shift {}",
                                                visible_bytes(raw_arguments)
                                            ),
                                        ),
                                        &mut truncated,
                                    );
                                    1
                                };
                                let maximum = pending
                                    .iter()
                                    .filter(|(_, _, depth, _, _, _)| *depth == macro_depth)
                                    .map(|(_, arguments, _, _, _, _)| arguments.len())
                                    .max()
                                    .unwrap_or_default();
                                if count > maximum {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_SHIFT,
                                            Severity::Error,
                                            source_id,
                                            start.saturating_add(
                                                u32::try_from(body_line.len()).expect(
                                                    "bounded macro body offsets fit public spans",
                                                ),
                                            ),
                                            start.saturating_add(
                                                u32::try_from(body_line.len()).expect(
                                                    "bounded macro body offsets fit public spans",
                                                ),
                                            ),
                                            format!(
                                                "excessive shift: {count}, but max is {maximum}"
                                            ),
                                        ),
                                        &mut truncated,
                                    );
                                }
                                for (_, pending_arguments, depth, _, _, _) in &mut pending {
                                    if *depth == macro_depth {
                                        let count = count.min(pending_arguments.len());
                                        pending_arguments.drain(..count);
                                    }
                                }
                                continue;
                            }
                            if request == b"tr" {
                                environment
                                    .define_translation(raw_arguments, scanner.escape_character());
                                continue;
                            }
                            if matches!(request, b"if" | b"ie" | b"el") {
                                let Ok(condition_arguments) = lex_condition_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional arguments in a macro exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let (condition, body_start, predicate_width) = match request {
                                    b"el" => {
                                        let condition = macro_conditionals
                                            .iter()
                                            .rposition(|(depth, _)| *depth == macro_depth)
                                            .map(|index| !macro_conditionals.remove(index).1);
                                        (condition, 0, None)
                                    }
                                    b"if" | b"ie" => {
                                        if request == b"ie"
                                            && (condition_arguments.is_empty()
                                                || condition_arguments
                                                    .first()
                                                    .is_some_and(|argument| argument.bytes == b"!"))
                                        {
                                            macro_conditionals
                                                .retain(|(depth, _)| *depth != macro_depth);
                                            macro_conditionals.push((macro_depth, false));
                                            (Some(false), condition_arguments.len(), None)
                                        } else {
                                            let Some((predicate, body_start)) =
                                                condition_parts(&condition_arguments)
                                            else {
                                                push_diagnostic(
                                                    &mut diagnostics,
                                                    limits,
                                                    diagnostic(
                                                        DiagnosticCode::ROFF_CONDITION,
                                                        Severity::Warning,
                                                        source_id,
                                                        start,
                                                        end,
                                                        "roff conditional in a macro is missing its predicate",
                                                    ),
                                                    &mut truncated,
                                                );
                                                continue;
                                            };
                                            let Some(predicate) = expand_environment(
                                                environment,
                                                &predicate,
                                                scanner.escape_character(),
                                                &macro_arguments,
                                                limits,
                                                source_id,
                                                start,
                                                end,
                                                &mut expansion_steps,
                                                &mut diagnostics,
                                                &mut truncated,
                                            ) else {
                                                break 'lines;
                                            };
                                            let condition =
                                                evaluate_condition(environment, &predicate);
                                            if request == b"ie"
                                                && let Some(condition) = condition
                                            {
                                                macro_conditionals
                                                    .retain(|(depth, _)| *depth != macro_depth);
                                                macro_conditionals.push((macro_depth, condition));
                                            }
                                            (condition, body_start, Some(predicate.len()))
                                        }
                                    }
                                    _ => unreachable!("conditional request was filtered above"),
                                };
                                let Some(condition) = condition else {
                                    if request == b"el" {
                                        continue;
                                    }
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_CONDITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff conditional in a macro is outside the M3 numeric/nroff subset",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let body_template = condition_body_template(
                                    raw_arguments,
                                    &condition_arguments,
                                    body_start,
                                );
                                let escape = scanner.escape_character();
                                if is_scope_opener(&body_template, escape) {
                                    let Some(scope) = collect_pending_macro_scope(
                                        &mut pending,
                                        macro_depth,
                                        scanner.control_character(),
                                        escape,
                                        limits,
                                    ) else {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
                                                Severity::Warning,
                                                source_id,
                                                start,
                                                end,
                                                "roff macro conditional reached its caller before its `\\}` terminator",
                                            ),
                                            &mut truncated,
                                        );
                                        continue;
                                    };
                                    if condition {
                                        let mut scope = scope;
                                        if macro_origin == 0 {
                                            let scope_origin = macro_scope_body_origin(
                                                &body_line,
                                                scanner.control_character(),
                                                predicate_width,
                                            );
                                            for (index, line) in scope.iter_mut().enumerate() {
                                                if index == 0 {
                                                    if let Some(origin) = scope_origin {
                                                        line.3 = origin;
                                                    }
                                                } else {
                                                    line.3 = 0;
                                                }
                                                line.5 = true;
                                            }
                                        }
                                        pending.extend(scope.into_iter().rev());
                                    }
                                    continue;
                                }
                                if condition && !body_template.is_empty() {
                                    pending.push((
                                        body_template,
                                        macro_arguments,
                                        macro_depth,
                                        macro_origin,
                                        macro_conditional_body_origin(
                                            &body_line,
                                            raw_arguments,
                                            &condition_arguments,
                                            body_start,
                                            predicate_width,
                                        ),
                                        false,
                                    ));
                                }
                                continue;
                            }
                            if request == b"ig" {
                                let marker = match ignore_marker(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) {
                                    Ok(marker) => marker,
                                    Err(ArgumentIssue::UnterminatedQuote) => {
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                                Severity::Warning,
                                                source_id,
                                                start,
                                                end,
                                                "roff ignore-block marker in a macro contains an unterminated quote",
                                            ),
                                            &mut truncated,
                                        );
                                        vec![b'.']
                                    }
                                    Err(ArgumentIssue::Limit) => {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::ARGUMENT_LIMIT,
                                                Severity::Warning,
                                                source_id,
                                                start,
                                                end,
                                                "roff ignore-block marker in a macro exceeds configured parser limits",
                                            ),
                                            &mut truncated,
                                        );
                                        vec![b'.']
                                    }
                                };
                                consume_ignore_block(&mut scanner, &marker);
                                continue;
                            }
                            if matches!(request, b"de" | b"de1" | b"am" | b"dei" | b"ami") {
                                let Ok(definition_arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "generated roff macro definition arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let Some(definition_name) = definition_arguments.first() else {
                                    continue;
                                };
                                let indirect = matches!(request, b"dei" | b"ami");
                                let Some(definition_name) =
                                    (!indirect).then(|| definition_name.bytes.clone()).or_else(
                                        || environment.indirect_string(&definition_name.bytes),
                                    )
                                else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "generated indirect roff macro definition names an undefined string",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let terminator = match definition_arguments.get(1) {
                                    None => vec![b'.'],
                                    Some(argument) if !indirect => argument.bytes.clone(),
                                    Some(argument) => {
                                        let Some(terminator) =
                                            environment.indirect_string(&argument.bytes)
                                        else {
                                            push_diagnostic(
                                                &mut diagnostics,
                                                limits,
                                                diagnostic(
                                                    DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                                    Severity::Warning,
                                                    source_id,
                                                    start,
                                                    end,
                                                    "generated indirect roff macro terminator names an undefined string",
                                                ),
                                                &mut truncated,
                                            );
                                            continue;
                                        };
                                        terminator
                                    }
                                };
                                let definition_control = scanner.control_character();
                                let mut body = Vec::new();
                                let mut terminated = false;
                                // A nested direct `.de` starts from the
                                // caller macro's remaining copy-mode lines;
                                // if its terminator lies beyond that stored
                                // body, capture the following physical input
                                // as one definition (`de/startde`).
                                if matches!(request, b"de" | b"de1") {
                                    while pending
                                        .last()
                                        .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
                                    {
                                        let (body_line, _, _, _, _, _) =
                                            pending.pop().expect("checked macro depth");
                                        if is_definition_terminator(
                                            &body_line,
                                            definition_control,
                                            &terminator,
                                        ) {
                                            terminated = true;
                                            break;
                                        }
                                        body.push(body_line);
                                    }
                                }
                                while !terminated && let Some(body_line) = scanner.next_raw_line() {
                                    if is_definition_terminator(
                                        body_line.bytes,
                                        definition_control,
                                        &terminator,
                                    ) {
                                        terminated = true;
                                        break;
                                    }
                                    if body_line.too_long {
                                        truncated = true;
                                        push_diagnostic(
                                            &mut diagnostics,
                                            limits,
                                            diagnostic(
                                                DiagnosticCode::LIMIT_LINE_BYTES,
                                                Severity::Warning,
                                                source_id,
                                                body_line.start,
                                                body_line.end,
                                                "copy-mode generated macro line exceeds max_line_bytes and was skipped",
                                            ),
                                            &mut truncated,
                                        );
                                        continue;
                                    }
                                    body.push(body_line.bytes.to_vec());
                                }
                                if !terminated {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "generated roff macro definition reached source end before its terminator",
                                        ),
                                        &mut truncated,
                                    );
                                }
                                let definition = if matches!(request, b"dei" | b"ami") {
                                    environment.define_indirect_macro(
                                        &definition_name,
                                        body,
                                        matches!(request, b"am" | b"ami"),
                                        limits,
                                    )
                                } else {
                                    environment.define_macro(
                                        &definition_name,
                                        body,
                                        matches!(request, b"am" | b"ami"),
                                        limits,
                                    )
                                };
                                if let Err(error) = definition {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            if request == b"while"
                                && let Ok(while_arguments) =
                                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                                && let Some((predicate_template, body)) =
                                    while_arguments.split_first()
                                && is_scope_opener(
                                    &join_arguments(body),
                                    scanner.escape_character(),
                                )
                            {
                                let escape = scanner.escape_character();
                                let scope = ScopeCollector {
                                    scanner: &mut scanner,
                                    source_id,
                                    limits,
                                    macro_set: builder.macro_set(),
                                    diagnostics: &mut diagnostics,
                                    truncated: &mut truncated,
                                    emit_definition_tail_diagnostics: true,
                                }
                                .collect(
                                    start,
                                    end,
                                    Some(b"while"),
                                );
                                if !scope.terminated {
                                    break 'lines;
                                }
                                // This `.while` originated in a macro body,
                                // while `collect_scope` consumed its closing
                                // `\\}` from the caller's physical input.  mandoc
                                // keeps the resulting AST recovery but reports
                                // both halves of that cross-input boundary.
                                push_diagnostic(
                                    &mut diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_WHILE_OUT_OF_SCOPE,
                                        Severity::Unsupported,
                                        source_id,
                                        start,
                                        end,
                                        "end of scope with open .while loop",
                                    ),
                                    &mut truncated,
                                );
                                if let Some(close_start) = scope.lines.last().map(|line| {
                                    let end = match line {
                                        ScopeLine::Text { end, .. }
                                        | ScopeLine::Control { end, .. }
                                        | ScopeLine::Loop { end, .. }
                                        | ScopeLine::Conditional { end, .. }
                                        | ScopeLine::Else { end, .. } => *end,
                                    };
                                    end.saturating_add(1)
                                }) {
                                    let diagnostic_start = close_start.saturating_add(3);
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
                                            Severity::Unsupported,
                                            source_id,
                                            diagnostic_start,
                                            diagnostic_start,
                                            "cannot continue this .while loop",
                                        ),
                                        &mut truncated,
                                    );
                                }
                                let Some(predicate) = expand_environment(
                                    environment,
                                    &predicate_template.bytes,
                                    escape,
                                    &macro_arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let Some(condition) = evaluate_condition(environment, &predicate)
                                else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_CONDITION,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "roff while predicate in a macro is outside the M3 numeric/nroff subset",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                if !condition {
                                    continue;
                                }
                                // A macro-local loop reaches the end of that
                                // macro before the caller's collected `\\}`.
                                // Execute its retained body once, then let the
                                // caller's scope run once as ordinary input;
                                // iterating the caller body here incorrectly
                                // drives the register to zero.
                                let mut macro_loop_body = Vec::new();
                                while pending
                                    .last()
                                    .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
                                {
                                    let (line, _, _, _, _, _) =
                                        pending.pop().expect("checked macro depth");
                                    macro_loop_body.push(line);
                                }
                                let first_macro_loop_child =
                                    builder.children(root).map_or(0, <[NodeId]>::len);
                                match execute_scope_macro_lines(
                                    macro_loop_body,
                                    &macro_arguments,
                                    macro_depth + 1,
                                    builder,
                                    root,
                                    source_id,
                                    start,
                                    end,
                                    &mut scanner,
                                    environment,
                                    limits,
                                    &mut text_bytes,
                                    &mut expansion_steps,
                                    &mut maximum_depth,
                                    &mut total_loop_iterations,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    ScopeFlow::Halt => break 'lines,
                                    ScopeFlow::CloseLoopInInnerScope { .. }
                                    | ScopeFlow::Break
                                    | ScopeFlow::Continue
                                    | ScopeFlow::LoopContinue => {}
                                }
                                // The copied `.while` body began in a user
                                // macro but closes in the caller's physical
                                // scope. Its first visible output inherits
                                // the macro-input cursor: the caller's
                                // invocation width followed by the copied
                                // opener line, rather than column one of the
                                // physical invocation span.
                                let scope_cursor = end.saturating_sub(start).saturating_add(
                                    u32::try_from(body_line.len()).expect(
                                        "bounded macro body lines fit public source columns",
                                    ),
                                );
                                set_first_scope_child_logical_start(
                                    builder,
                                    root,
                                    first_macro_loop_child,
                                    SourcePosition {
                                        line: 0,
                                        column: scope_cursor,
                                    },
                                );
                                match (ScopeMachine {
                                    builder,
                                    root,
                                    source_id,
                                    scanner: &mut scanner,
                                    environment,
                                    limits,
                                    text_bytes: &mut text_bytes,
                                    expansion_steps: &mut expansion_steps,
                                    maximum_depth: &mut maximum_depth,
                                    total_loop_iterations: &mut total_loop_iterations,
                                    diagnostics: &mut diagnostics,
                                    truncated: &mut truncated,
                                })
                                .run(&scope.lines)
                                {
                                    ScopeFlow::Halt => break 'lines,
                                    ScopeFlow::CloseLoopInInnerScope { .. } => {
                                        pending_while_out_of_scope = true;
                                    }
                                    ScopeFlow::Break
                                    | ScopeFlow::Continue
                                    | ScopeFlow::LoopContinue => {}
                                }
                                continue;
                            }
                            if matches!(request, b"ds" | b"as") {
                                if let Err(error) = apply_string_request(
                                    environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    request == b"as",
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            if is_environment_request(request) {
                                let Some(expanded_arguments) = expand_environment(
                                    environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    &macro_arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let Ok(arguments) = lex_arguments(
                                    &expanded_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    continue;
                                };
                                if let Err(error) = apply_environment_request(
                                    environment,
                                    builder,
                                    request,
                                    scanner.escape_character(),
                                    &arguments,
                                    limits,
                                ) {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        environment_error_diagnostic(error, source_id, start, end),
                                        &mut truncated,
                                    );
                                }
                                continue;
                            }
                            if !is_builtin_package_macro(builder.macro_set(), request)
                                && let Some(nested) = environment.macro_definition(request).cloned()
                            {
                                if macro_definition_directly_invokes(
                                    &nested,
                                    request,
                                    scanner.control_character(),
                                ) {
                                    // A direct self-call exhausts mandoc's
                                    // input stack at the caller boundary.
                                    // Do not expand it through the generic
                                    // nesting budget: that produces a second,
                                    // later warning and leaves the wrong
                                    // recovery text in the public report.
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::LIMIT_EXPANSION_STEPS,
                                            Severity::Error,
                                            source_id,
                                            end,
                                            end,
                                            "input stack limit exceeded, infinite loop?",
                                        ),
                                        &mut truncated,
                                    );
                                    pending.retain(|(_, _, depth, _, _, _)| *depth < macro_depth);
                                    continue;
                                }
                                if macro_depth >= limits.max_macro_depth {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ROFF_MACRO_DEPTH_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "nested roff macro expansion exceeds max_macro_depth",
                                        ),
                                        &mut truncated,
                                    );
                                    break 'lines;
                                }
                                let Ok(nested_arguments) = lex_arguments(
                                    raw_arguments,
                                    scanner.escape_character(),
                                    limits,
                                ) else {
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "nested macro invocation arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                let mut expanded_arguments =
                                    Vec::with_capacity(nested_arguments.len());
                                for argument in nested_arguments {
                                    let Some(bytes) = expand_environment(
                                        environment,
                                        &argument.bytes,
                                        scanner.escape_character(),
                                        &macro_arguments,
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut expansion_steps,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) else {
                                        break 'lines;
                                    };
                                    expanded_arguments.push(bytes);
                                }
                                if !record_expansion_steps(
                                    &mut expansion_steps,
                                    1,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    break 'lines;
                                }
                                // A nested macro is reparsed from the current
                                // macro-input cursor.  That cursor is seeded
                                // by the invoking body line, then retained by
                                // recursive calls of the same nested frame.
                                let nested_origin = if scope_reparse {
                                    0
                                } else if macro_origin == 0 {
                                    u32::try_from(body_line.len().saturating_add(1))
                                        .expect("bounded macro body lines fit source columns")
                                } else {
                                    macro_origin
                                };
                                pending.extend(nested.lines.into_iter().rev().map(|line| {
                                    (
                                        line,
                                        expanded_arguments.clone(),
                                        macro_depth + 1,
                                        nested_origin,
                                        None,
                                        false,
                                    )
                                }));
                                continue;
                            }
                            let mdoc_callable =
                                PackageToken::classify(builder.macro_set(), request)
                                    .is_mdoc_callable();
                            // Macro output keeps the caller's physical span
                            // for safe source slicing, but libmandoc exposes
                            // the generated control's column *inside the
                            // copied body*.  This is independently observable
                            // for both mdoc callable macros and their text.
                            let generated_control_start = control_start;
                            let generated_argument_start = if mdoc_callable {
                                macro_generated_argument_start
                            } else {
                                start
                            };
                            let flags = NodeFlags {
                                line_start: true,
                                ..NodeFlags::default()
                            };
                            let Some(element) = append_node(
                                builder,
                                root,
                                NodeKind::Element,
                                generated_control_start,
                                end,
                                flags,
                                &mut EmitContext::new(
                                    source_id,
                                    limits,
                                    &mut text_bytes,
                                    &mut diagnostics,
                                    &mut truncated,
                                ),
                            ) else {
                                continue;
                            };
                            if !builder.macro_name(element, visible_bytes(request)) {
                                truncated = true;
                                continue;
                            }
                            let generated_control_position = builder
                                .node_source_position(element)
                                .map(|position| SourcePosition {
                                    line: position.line,
                                    column: macro_origin.saturating_add(macro_body_control_column(
                                        &body_line,
                                        scanner.control_character(),
                                    )),
                                });
                            if let Some(position) = generated_control_position {
                                let _ = builder.set_node_logical_start(element, position);
                            }
                            let generated_argument_position =
                                generated_control_position.map(|position| {
                                    let offset = u32::try_from(
                                        body_line.len().saturating_sub(raw_arguments.len()),
                                    )
                                    .expect(
                                        "parser line bounds keep macro argument offsets public",
                                    );
                                    SourcePosition {
                                        line: position.line,
                                        column: macro_origin
                                            .saturating_add(offset)
                                            .saturating_add(1),
                                    }
                                });
                            maximum_depth = maximum_depth.max(2);
                            if !raw_arguments.is_empty() {
                                let Some(bytes) = expand_environment(
                                    environment,
                                    raw_arguments,
                                    scanner.escape_character(),
                                    &macro_arguments,
                                    limits,
                                    source_id,
                                    start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                let escape = scanner.escape_character();
                                let macro_body_separator_widths =
                                    lex_arguments(raw_arguments, escape, limits)
                                        .ok()
                                        .map(|arguments| {
                                            arguments
                                                .into_iter()
                                                .map(|argument| {
                                                    u32::try_from(argument.separator_width).expect(
                                                "macro argument separators fit public columns",
                                            )
                                                })
                                                .collect::<Vec<_>>()
                                        })
                                        .unwrap_or_default();
                                let Ok(arguments) = lex_arguments(&bytes, escape, limits) else {
                                    truncated = true;
                                    push_diagnostic(
                                        &mut diagnostics,
                                        limits,
                                        diagnostic(
                                            DiagnosticCode::ARGUMENT_LIMIT,
                                            Severity::Warning,
                                            source_id,
                                            start,
                                            end,
                                            "macro-generated control arguments exceed configured parser limits",
                                        ),
                                        &mut truncated,
                                    );
                                    continue;
                                };
                                // A macro body's `\$@` is one scanner atom,
                                // but libmandoc publishes every expanded
                                // argument at a distinct logical column: the
                                // next argument follows the previous visible
                                // spelling plus the three-byte `\$@` source
                                // atom. Retain that provenance without
                                // altering the physical invocation span.
                                let all_arguments_expansion = raw_arguments == b"\\$@";
                                let all_arguments_atom_width = u32::try_from(raw_arguments.len())
                                    .expect("macro argument atom length fits public columns");
                                let mut next_generated_argument_position =
                                    generated_argument_position;
                                let mut expanded_argument_index = 0_usize;
                                for argument in arguments {
                                    let Some(bytes) = translate_visible(
                                        environment,
                                        &argument.bytes,
                                        escape,
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) else {
                                        break 'lines;
                                    };
                                    let result =
                                        normalize_document_escapes(builder, &bytes, escape, limits);
                                    if !record_expansion_steps(
                                        &mut expansion_steps,
                                        result.steps,
                                        limits,
                                        source_id,
                                        start,
                                        end,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ) {
                                        break 'lines;
                                    }
                                    emit_escape_issues(
                                        &result.issues,
                                        start,
                                        end,
                                        &mut EmitContext::new(
                                            source_id,
                                            limits,
                                            &mut text_bytes,
                                            &mut diagnostics,
                                            &mut truncated,
                                        ),
                                    );
                                    truncated |= result.truncated;
                                    let logical_text_width = u32::try_from(result.text.len())
                                        .expect("expanded macro arguments fit public columns");
                                    if append_text_node(
                                        builder,
                                        element,
                                        generated_argument_start,
                                        end,
                                        NodeFlags {
                                            line_continuation: result.line_continuation,
                                            ..NodeFlags::default()
                                        },
                                        result.text,
                                        &mut EmitContext::new(
                                            source_id,
                                            limits,
                                            &mut text_bytes,
                                            &mut diagnostics,
                                            &mut truncated,
                                        ),
                                    ) {
                                        if let Some(position) = next_generated_argument_position
                                            && let Some(argument) = builder
                                                .children(element)
                                                .and_then(|children| children.last())
                                                .copied()
                                        {
                                            let _ =
                                                builder.set_node_logical_start(argument, position);
                                        }
                                        if all_arguments_expansion {
                                            next_generated_argument_position =
                                                next_generated_argument_position.map(|position| {
                                                    SourcePosition {
                                                        line: position.line,
                                                        column: position
                                                            .column
                                                            .saturating_add(logical_text_width)
                                                            .saturating_add(
                                                                all_arguments_atom_width,
                                                            ),
                                                    }
                                                });
                                        } else {
                                            let separator_width = macro_body_separator_widths
                                                .get(expanded_argument_index)
                                                .copied()
                                                .unwrap_or_default();
                                            next_generated_argument_position =
                                                next_generated_argument_position.map(|position| {
                                                    SourcePosition {
                                                        line: position.line,
                                                        column: position
                                                            .column
                                                            .saturating_add(logical_text_width)
                                                            .saturating_add(separator_width),
                                                    }
                                                });
                                        }
                                        expanded_argument_index =
                                            expanded_argument_index.saturating_add(1);
                                        maximum_depth = maximum_depth.max(3);
                                    }
                                }
                            }
                            continue;
                        }
                        let Some(bytes) = expand_environment(
                            environment,
                            &body_line,
                            scanner.escape_character(),
                            &macro_arguments,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut expansion_steps,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        let escape = scanner.escape_character();
                        let Some(bytes) = translate_visible(
                            environment,
                            &bytes,
                            escape,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) else {
                            break 'lines;
                        };
                        let result = normalize_document_escapes(builder, &bytes, escape, limits);
                        if !record_expansion_steps(
                            &mut expansion_steps,
                            result.steps,
                            limits,
                            source_id,
                            start,
                            end,
                            &mut diagnostics,
                            &mut truncated,
                        ) {
                            break 'lines;
                        }
                        emit_escape_issues(
                            &result.issues,
                            start,
                            end,
                            &mut EmitContext::new(
                                source_id,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ),
                        );
                        truncated |= result.truncated;
                        let flags = NodeFlags {
                            line_start: true,
                            line_continuation: result.line_continuation,
                            ..NodeFlags::default()
                        };
                        if append_text_node(
                            builder,
                            root,
                            start,
                            end,
                            flags,
                            result.text,
                            &mut EmitContext::new(
                                source_id,
                                limits,
                                &mut text_bytes,
                                &mut diagnostics,
                                &mut truncated,
                            ),
                        ) {
                            if let Some(column) = text_origin
                                && let Some(node) = builder
                                    .children(root)
                                    .and_then(|children| children.last())
                                    .copied()
                                && let Some(physical) = builder.node_source_position(node)
                            {
                                let _ = builder.set_node_logical_start(
                                    node,
                                    SourcePosition {
                                        line: physical.line,
                                        column: column.saturating_add(1),
                                    },
                                );
                            }
                            maximum_depth = maximum_depth.max(2);
                        }
                    }
                    continue;
                }
                let flags = NodeFlags {
                    line_start: true,
                    ..NodeFlags::default()
                };
                let Some(element) = append_node(
                    builder,
                    root,
                    NodeKind::Element,
                    control_start,
                    end,
                    flags,
                    &mut EmitContext::new(
                        source_id,
                        limits,
                        &mut text_bytes,
                        &mut diagnostics,
                        &mut truncated,
                    ),
                ) else {
                    continue;
                };
                if !builder.macro_name(element, visible_bytes(dispatched_package_macro)) {
                    truncated = true;
                    continue;
                }
                maximum_depth = maximum_depth.max(2);
                let character_request = matches!(dispatched_package_macro, b"cc" | b"c2" | b"ec");
                // A renamed package macro retains the original package
                // spelling's logical argument column.  Its physical byte span
                // remains anchored at the alias, so consumers can still slice
                // source safely while canonical locations match mandoc.
                let renamed_package_argument_position =
                    renamed_package_macro
                        .then(|| {
                            let span = SourceSpan::new(source_id, control_start, control_start)
                                .expect("control source offsets are monotonic");
                            builder.source_position(&span).map(|position| {
                                SourcePosition {
                            line: position.line,
                            column: position
                                .column
                                .saturating_add(
                                    u32::try_from(dispatched_package_macro.len()).expect(
                                        "parsed control names fit public source positions",
                                    ),
                                )
                                .saturating_add(
                                    u32::try_from(raw_arguments.len() - arguments.len()).expect(
                                        "scanner argument widths fit public source positions",
                                    ),
                                ),
                        }
                            })
                        })
                        .flatten();
                let argument_escape = if character_request {
                    b'\\'
                } else {
                    scanner.escape_character()
                };
                let parsed_arguments = match lex_arguments(arguments, argument_escape, limits) {
                    Ok(arguments) => Ok(arguments),
                    Err(ArgumentIssue::UnterminatedQuote) => {
                        emit_unterminated_quoted_argument(
                            arguments,
                            argument_start,
                            end,
                            source_id,
                            limits,
                            &mut diagnostics,
                            &mut truncated,
                        );
                        // Package macros still consume the recovered token:
                        // mandoc synthesizes the missing closing delimiter
                        // after publishing its style finding, so an `.IB
                        // "one` retains `one` instead of becoming an empty
                        // element.
                        recover_unterminated_quoted_arguments(arguments, argument_escape, limits)
                    }
                    Err(ArgumentIssue::Limit) => Err(ArgumentIssue::Limit),
                };
                match parsed_arguments {
                    Ok(mut arguments) => {
                        if terminal_continuation_at_eof
                            && let Some(argument) = arguments.last_mut()
                            && argument
                                .bytes
                                .last()
                                .is_some_and(|byte| *byte == scanner.escape_character())
                        {
                            // The terminal escape consumes its physical
                            // newline.  Keep the complete preceding argument
                            // text while removing that private continuation
                            // control before package-macro lowering.
                            let _ = argument.bytes.pop();
                        }
                        if character_request {
                            normalize_character_request_arguments(
                                dispatched_package_macro,
                                &mut arguments,
                                source_id,
                                argument_start,
                                limits,
                                &mut diagnostics,
                                &mut truncated,
                            );
                        }
                        for argument in arguments {
                            let argument_offset = u32::try_from(argument.offset)
                                .expect("argument offsets are bounded by line length");
                            let argument_quoted = argument.quoted;
                            let separator_after = argument.separator_after;
                            let separator_contains_tab = argument.separator_contains_tab;
                            let embedded_tab_count = argument.embedded_tab_count;
                            let separator_width = argument.separator_width;
                            let has_invalid_argument_bytes =
                                std::str::from_utf8(&argument.bytes).is_err();
                            let lexical_width = i32::try_from(argument.bytes.len())
                                .expect("argument bytes are bounded below i32::MAX");
                            // Copy-mode turns `\\\\e` into the public `\\e`
                            // spelling.  The AST intentionally exposes that
                            // shorter spelling, but libmandoc still anchors
                            // the following mdoc argument after all three
                            // authored source bytes.  It is therefore not an
                            // expansion-width delta for the later-argument
                            // rebasing pass.
                            let preserves_copy_mode_e_width =
                                argument.bytes.windows(3).any(|atom| {
                                    atom == [
                                        scanner.escape_character(),
                                        scanner.escape_character(),
                                        b'e',
                                    ]
                                });
                            let protected_tabulation_escape = !character_request
                                && has_protected_tabulation_escape(
                                    &argument.bytes,
                                    scanner.escape_character(),
                                );
                            let argument_start = argument_start
                                .checked_add(argument_offset)
                                .expect("parser checks public span offsets first");
                            let expanded = if character_request {
                                argument.bytes
                            } else {
                                // man(7) and mdoc(7) reparse control
                                // arguments in copy mode before resolving
                                // delayed strings. In particular, `\\\\*x`
                                // becomes the active `\\*x` reference, while
                                // ordinary roff text keeps its literal
                                // escaped spelling.
                                let reparsed = (builder.macro_set() != MacroSet::None).then(|| {
                                    copy_mode_reparse(&argument.bytes, scanner.escape_character())
                                });
                                let argument_bytes =
                                    reparsed.as_deref().unwrap_or(argument.bytes.as_slice());
                                let Some(bytes) = expand_environment(
                                    environment,
                                    argument_bytes,
                                    scanner.escape_character(),
                                    &[],
                                    limits,
                                    source_id,
                                    argument_start,
                                    end,
                                    &mut expansion_steps,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) else {
                                    break 'lines;
                                };
                                bytes
                            };
                            let result = (!character_request).then(|| {
                                normalize_document_escapes(
                                    builder,
                                    &expanded,
                                    scanner.escape_character(),
                                    limits,
                                )
                            });
                            if let Some(result) = &result {
                                if !record_expansion_steps(
                                    &mut expansion_steps,
                                    result.steps,
                                    limits,
                                    source_id,
                                    argument_start,
                                    end,
                                    &mut diagnostics,
                                    &mut truncated,
                                ) {
                                    break 'lines;
                                }
                                emit_escape_issues(
                                    &result.issues,
                                    argument_start,
                                    end,
                                    &mut EmitContext::new(
                                        source_id,
                                        limits,
                                        &mut text_bytes,
                                        &mut diagnostics,
                                        &mut truncated,
                                    ),
                                );
                                truncated |= result.truncated;
                            }
                            let text = result
                                .map_or_else(|| visible_bytes(&expanded), |result| result.text);
                            let expansion_width_delta = if preserves_copy_mode_e_width
                                && text
                                    .as_bytes()
                                    .windows(2)
                                    .any(|atom| atom == [scanner.escape_character(), b'e'])
                            {
                                0
                            } else {
                                i32::try_from(text.len())
                                    .expect("normalized argument bytes are bounded below i32::MAX")
                                    .saturating_sub(lexical_width)
                            };
                            if append_text_node(
                                builder,
                                element,
                                argument_start,
                                end,
                                NodeFlags::default(),
                                text,
                                &mut EmitContext::new(
                                    source_id,
                                    limits,
                                    &mut text_bytes,
                                    &mut diagnostics,
                                    &mut truncated,
                                ),
                            ) {
                                if let Some(argument_node) = builder
                                    .children(element)
                                    .and_then(|children| children.last())
                                    .copied()
                                {
                                    let _ = builder
                                        .set_node_separator_after(argument_node, separator_after);
                                    let _ = builder.set_node_separator_contains_tab(
                                        argument_node,
                                        separator_contains_tab,
                                    );
                                    let _ = builder.set_node_embedded_tab_count(
                                        argument_node,
                                        embedded_tab_count,
                                    );
                                    let _ = builder
                                        .set_node_separator_width(argument_node, separator_width);
                                    let _ = builder.set_node_protected_tabulation_escape(
                                        argument_node,
                                        protected_tabulation_escape,
                                    );
                                    let _ = builder.set_node_argument_expansion_width_delta(
                                        argument_node,
                                        expansion_width_delta,
                                    );
                                    let _ = builder
                                        .set_node_argument_quoted(argument_node, argument_quoted);
                                    // Package validators normally use the visible UTF-8 byte
                                    // offset to place a diagnostic inside an argument. Preserve
                                    // malformed-input provenance so they can instead count one
                                    // source byte per Latin-1-mapped character. Otherwise an
                                    // invalid byte before an ASCII finding makes a public span
                                    // run past its raw source range.
                                    if has_invalid_argument_bytes {
                                        let _ = builder.set_node_input_unicode_provenance(
                                            argument_node,
                                            true,
                                            false,
                                        );
                                    }
                                    if let Some(position) = renamed_package_argument_position {
                                        let _ = builder.set_node_logical_start(
                                            argument_node,
                                            SourcePosition {
                                                line: position.line,
                                                column: position
                                                    .column
                                                    .saturating_add(argument_offset),
                                            },
                                        );
                                    }
                                    if physical_continuation {
                                        continued_argument_nodes
                                            .push((argument_node, argument_offset));
                                    }
                                }
                                maximum_depth = maximum_depth.max(3);
                            } else {
                                break;
                            }
                        }
                    }
                    Err(ArgumentIssue::UnterminatedQuote) => {}
                    Err(ArgumentIssue::Limit) => {
                        truncated = true;
                        push_diagnostic(
                            &mut diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_LIMIT,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "control-line arguments exceed configured parser limits",
                            ),
                            &mut truncated,
                        );
                    }
                }
                if physical_continuation {
                    let _ = builder.rebase_node_location_to_final_line(element);
                    if let Some(children) = builder.children(element).map(<[NodeId]>::to_vec) {
                        for child in children {
                            let _ = builder.rebase_node_location_to_final_line(child);
                        }
                    }
                    let source_end =
                        usize::try_from(end).expect("parser checks public span offsets first");
                    let final_line = u32::try_from(
                        memchr::memchr_iter(b'\n', &source.bytes[..source_end]).count() + 1,
                    )
                    .expect("source line count fits the public source limit");
                    let argument_offset = usize::try_from(argument_start)
                        .expect("parser checks public span offsets first");
                    let logical_base_column = source.bytes[..argument_offset]
                        .iter()
                        .rposition(|byte| *byte == b'\n')
                        .map_or_else(
                            || argument_start.saturating_add(1),
                            |line_start| {
                                argument_start.saturating_sub(
                                    u32::try_from(line_start)
                                        .expect("source offsets fit the public source limit"),
                                )
                            },
                        );
                    for (node, offset) in continued_argument_nodes {
                        let _ = builder.set_node_logical_start(
                            node,
                            SourcePosition {
                                line: final_line,
                                column: logical_base_column.saturating_add(offset),
                            },
                        );
                    }
                }
            }
        }
    }
    ScanOutcome {
        diagnostics,
        deferred_post_validation_diagnostics,
        source_bytes,
        source_files,
        text_bytes,
        expansion_steps,
        truncated,
        maximum_depth,
        previous_conditional,
        total_loop_iterations,
        saw_mdoc_operating_system,
    }
}
