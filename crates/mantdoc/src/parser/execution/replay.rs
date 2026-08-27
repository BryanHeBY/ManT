use super::super::{
    ArgumentIssue, BranchOutcome, Diagnostic, DiagnosticCode, DocumentBuilder, EmitContext,
    Environment, Limits, NodeFlags, NodeId, NodeKind, Scanner, ScopeFlow, ScopeLine, Severity,
    SourcePosition, SourceSpan, append_node, append_text_node, apply_environment_request,
    apply_string_request, condition_body_source_start_from_offset, condition_body_template,
    condition_parts, consume_ignore_block, copy_mode_reparse, diagnostic, emit_escape_issues,
    environment_error_diagnostic, evaluate_condition, expand_environment, ignore_marker,
    is_builtin_package_macro, is_definition_terminator, is_environment_request,
    is_macro_comment_request, is_scope_closer, is_scope_ignore_terminator, is_scope_opener,
    join_arguments, lex_arguments, lex_condition_arguments, normalize_document_escapes,
    push_diagnostic, record_expansion_steps, scope_line_end, scope_line_start,
    set_new_root_children_logical_start, split_macro_control, translate_visible,
    trim_horizontal_space, visible_bytes,
};
use super::collect::collect_pending_macro_scope;

enum ReplayFrame<'a> {
    Lines {
        lines: &'a [ScopeLine],
        next: usize,
        previous_conditional: Option<BranchOutcome>,
    },
    Loop {
        start: u32,
        end: u32,
        predicate: &'a [u8],
        lines: &'a [ScopeLine],
        iterations: usize,
        /// A nested `.while` is executed in mandoc's active input frame,
        /// then causes its enclosing loop to stop rather than resuming the
        /// outer scope after the inner predicate becomes false.
        break_after: bool,
    },
    /// Apply the copied-input provenance of a nested loop only after its
    /// replayed body has emitted nodes at the direct scope root.
    SetNewRootChildrenLogicalStart {
        first_child: usize,
        position: SourcePosition,
    },
}

pub(in crate::parser) struct ReplayMachine<'state, 'source> {
    pub(in crate::parser) builder: &'state mut DocumentBuilder,
    pub(in crate::parser) root: NodeId,
    pub(in crate::parser) source_id: crate::SourceId,
    pub(in crate::parser) scanner: &'state mut Scanner<'source>,
    pub(in crate::parser) environment: &'state mut Environment,
    pub(in crate::parser) limits: &'state Limits,
    pub(in crate::parser) text_bytes: &'state mut usize,
    pub(in crate::parser) expansion_steps: &'state mut usize,
    pub(in crate::parser) maximum_depth: &'state mut usize,
    pub(in crate::parser) total_loop_iterations: &'state mut usize,
    pub(in crate::parser) diagnostics: &'state mut Vec<Diagnostic>,
    pub(in crate::parser) truncated: &'state mut bool,
}

impl ReplayMachine<'_, '_> {
    #[allow(clippy::too_many_lines)] // An explicit frame stack avoids recursive execution of untrusted nested scopes.
    pub(in crate::parser) fn run(self, lines: &[ScopeLine]) -> ScopeFlow {
        let Self {
            builder,
            root,
            source_id,
            scanner,
            environment,
            limits,
            text_bytes,
            expansion_steps,
            maximum_depth,
            total_loop_iterations,
            diagnostics,
            truncated,
        } = self;
        let mut closed_loop_from_inner_scope = None;
        let mut frames = vec![ReplayFrame::Lines {
            lines,
            next: 0,
            previous_conditional: None,
        }];
        while let Some(frame) = frames.pop() {
            match frame {
                ReplayFrame::SetNewRootChildrenLogicalStart {
                    first_child,
                    position,
                } => {
                    set_new_root_children_logical_start(builder, root, first_child, position);
                }
                ReplayFrame::Lines {
                    lines,
                    next,
                    previous_conditional,
                } => {
                    let Some(line) = lines.get(next) else {
                        continue;
                    };
                    if let Some(consumed) = execute_collected_scope_definition(
                        line,
                        &lines[next + 1..],
                        scanner,
                        environment,
                        limits,
                        source_id,
                        diagnostics,
                        truncated,
                    ) {
                        frames.push(ReplayFrame::Lines {
                            lines,
                            next: next + consumed + 1,
                            previous_conditional: None,
                        });
                        continue;
                    }
                    if let ScopeLine::Control {
                        start,
                        end,
                        argument_start,
                        name,
                        arguments,
                        ..
                    } = line
                        && matches!(name.as_slice(), b"if" | b"ie" | b"el")
                    {
                        if name == b"el" {
                            if !previous_conditional.is_some_and(BranchOutcome::is_skipped) {
                                frames.push(ReplayFrame::Lines {
                                    lines,
                                    next: next + 1,
                                    previous_conditional: None,
                                });
                                continue;
                            }
                            let body = trim_horizontal_space(arguments);
                            if body.is_empty() {
                                frames.push(ReplayFrame::Lines {
                                    lines,
                                    next: next + 1,
                                    previous_conditional: None,
                                });
                                continue;
                            }
                            let body_start = arguments.len().saturating_sub(body.len());
                            let body_source_start = argument_start.saturating_add(
                                u32::try_from(body_start)
                                    .expect("scope conditional body offsets fit source spans"),
                            );
                            let body = inline_scope_body_line(
                                body.to_vec(),
                                body_source_start,
                                *end,
                                scanner.control_character(),
                                scanner.escape_character(),
                            );
                            if let Some(consumed) = execute_collected_scope_definition(
                                &body,
                                &lines[next + 1..],
                                scanner,
                                environment,
                                limits,
                                source_id,
                                diagnostics,
                                truncated,
                            ) {
                                frames.push(ReplayFrame::Lines {
                                    lines,
                                    next: next + consumed + 1,
                                    previous_conditional: None,
                                });
                                continue;
                            }
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + 1,
                                previous_conditional: None,
                            });
                            match execute_scope_line(
                                &body,
                                builder,
                                root,
                                source_id,
                                scanner,
                                environment,
                                limits,
                                text_bytes,
                                expansion_steps,
                                maximum_depth,
                                total_loop_iterations,
                                diagnostics,
                                truncated,
                            ) {
                                ScopeFlow::Continue => {}
                                flow => return flow,
                            }
                            continue;
                        }
                        let Ok(condition_arguments) =
                            lex_condition_arguments(arguments, scanner.escape_character(), limits)
                        else {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    *start,
                                    *end,
                                    "inline roff conditional arguments in a scope exceed configured parser limits",
                                ),
                                truncated,
                            );
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + 1,
                                previous_conditional: None,
                            });
                            continue;
                        };
                        let Some((predicate, body_start)) = condition_parts(&condition_arguments)
                        else {
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_CONDITION,
                                    Severity::Warning,
                                    source_id,
                                    *start,
                                    *end,
                                    "inline roff conditional in a scope is missing its predicate",
                                ),
                                truncated,
                            );
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + 1,
                                previous_conditional: None,
                            });
                            continue;
                        };
                        let Some(predicate) = expand_environment(
                            environment,
                            &predicate,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            *start,
                            *end,
                            expansion_steps,
                            diagnostics,
                            truncated,
                        ) else {
                            return ScopeFlow::Halt;
                        };
                        let Some(condition) =
                            evaluate_condition(environment, &predicate, scanner.escape_character())
                        else {
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_CONDITION,
                                    Severity::Warning,
                                    source_id,
                                    *start,
                                    *end,
                                    "inline roff conditional predicate in a scope is outside the M3 numeric/nroff subset",
                                ),
                                truncated,
                            );
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + 1,
                                previous_conditional: None,
                            });
                            continue;
                        };
                        let body =
                            condition_body_template(arguments, &condition_arguments, body_start);
                        let previous_conditional =
                            (name == b"ie").then(|| BranchOutcome::from(condition));
                        if !condition || body.is_empty() {
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + 1,
                                previous_conditional,
                            });
                            continue;
                        }
                        let body_source_start = condition_body_source_start_from_offset(
                            arguments,
                            &condition_arguments,
                            body_start,
                            *argument_start,
                            *start,
                            None,
                        );
                        let body = inline_scope_body_line(
                            body,
                            body_source_start,
                            *end,
                            scanner.control_character(),
                            scanner.escape_character(),
                        );
                        if let Some(consumed) = execute_collected_scope_definition(
                            &body,
                            &lines[next + 1..],
                            scanner,
                            environment,
                            limits,
                            source_id,
                            diagnostics,
                            truncated,
                        ) {
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + consumed + 1,
                                previous_conditional,
                            });
                            continue;
                        }
                        frames.push(ReplayFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional,
                        });
                        match execute_scope_line(
                            &body,
                            builder,
                            root,
                            source_id,
                            scanner,
                            environment,
                            limits,
                            text_bytes,
                            expansion_steps,
                            maximum_depth,
                            total_loop_iterations,
                            diagnostics,
                            truncated,
                        ) {
                            ScopeFlow::Continue => {}
                            flow => return flow,
                        }
                        continue;
                    }
                    if let ScopeLine::Control {
                        start,
                        end,
                        name,
                        arguments,
                        ..
                    } = line
                        && name == b"ig"
                    {
                        let marker = match ignore_marker(
                            arguments,
                            scanner.escape_character(),
                            limits,
                        ) {
                            Ok(marker) => marker,
                            Err(ArgumentIssue::UnterminatedQuote) => {
                                push_diagnostic(
                                    diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                        Severity::Warning,
                                        source_id,
                                        *start,
                                        *end,
                                        "roff ignore-block marker in a collected scope contains an unterminated quote",
                                    ),
                                    truncated,
                                );
                                vec![b'.']
                            }
                            Err(ArgumentIssue::Limit) => {
                                *truncated = true;
                                push_diagnostic(
                                    diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ARGUMENT_LIMIT,
                                        Severity::Warning,
                                        source_id,
                                        *start,
                                        *end,
                                        "roff ignore-block marker in a collected scope exceeds configured parser limits",
                                    ),
                                    truncated,
                                );
                                vec![b'.']
                            }
                        };
                        let next = lines[next + 1..]
                            .iter()
                            .position(|candidate| is_scope_ignore_terminator(candidate, &marker))
                            .map_or(lines.len(), |offset| next + offset + 2);
                        frames.push(ReplayFrame::Lines {
                            lines,
                            next,
                            previous_conditional: None,
                        });
                        continue;
                    }
                    if let ScopeLine::Conditional {
                        start,
                        end,
                        predicate,
                        else_eligible,
                        lines: conditional_lines,
                    } = line
                    {
                        if frames.len() >= limits.max_tree_depth {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::LIMIT_SCOPE_DEPTH,
                                    Severity::Warning,
                                    source_id,
                                    *start,
                                    *end,
                                    "nested roff scope execution exceeds max_tree_depth",
                                ),
                                truncated,
                            );
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + 1,
                                previous_conditional: None,
                            });
                            continue;
                        }
                        let Some(expanded_predicate) = expand_environment(
                            environment,
                            predicate,
                            scanner.escape_character(),
                            &[],
                            limits,
                            source_id,
                            *start,
                            *end,
                            expansion_steps,
                            diagnostics,
                            truncated,
                        ) else {
                            return ScopeFlow::Halt;
                        };
                        let Some(condition) = evaluate_condition(
                            environment,
                            &expanded_predicate,
                            scanner.escape_character(),
                        ) else {
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_CONDITION,
                                    Severity::Warning,
                                    source_id,
                                    *start,
                                    *end,
                                    "nested roff conditional predicate is outside the M3 numeric/nroff subset",
                                ),
                                truncated,
                            );
                            frames.push(ReplayFrame::Lines {
                                lines,
                                next: next + 1,
                                previous_conditional: None,
                            });
                            continue;
                        };
                        frames.push(ReplayFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: else_eligible.then(|| condition.into()),
                        });
                        if condition {
                            frames.push(ReplayFrame::Lines {
                                lines: conditional_lines,
                                next: 0,
                                previous_conditional: None,
                            });
                        }
                        continue;
                    }
                    if let ScopeLine::Else {
                        lines: else_lines, ..
                    } = line
                    {
                        frames.push(ReplayFrame::Lines {
                            lines,
                            next: next + 1,
                            previous_conditional: None,
                        });
                        if previous_conditional.is_some_and(BranchOutcome::is_skipped) {
                            frames.push(ReplayFrame::Lines {
                                lines: else_lines,
                                next: 0,
                                previous_conditional: None,
                            });
                        }
                        continue;
                    }
                    frames.push(ReplayFrame::Lines {
                        lines,
                        next: next + 1,
                        previous_conditional: None,
                    });
                    if let ScopeLine::Loop {
                        start,
                        end,
                        predicate,
                        lines: loop_lines,
                    } = line
                    {
                        if environment.mark_nested_while_recovery(*start) {
                            // mandoc's roff input buffer retains the nested
                            // request and its first replayed body line together.
                            // Preserve that observable logical column while the
                            // physical span remains sliceable at the request.
                            let logical_start =
                                loop_lines.first().map(scope_line_end).and_then(|body_end| {
                                    let body_end =
                                        SourceSpan::new(source_id, body_end, body_end).ok()?;
                                    let position = builder.source_position(&body_end)?;
                                    Some(SourcePosition {
                                        line: position.line,
                                        column: position.column.saturating_add(
                                            end.saturating_sub(*start).saturating_sub(1),
                                        ),
                                    })
                                });
                            let nested_span = SourceSpan::new(source_id, *start, *end)
                                .expect("collected scope spans are ordered")
                                .with_logical_start(logical_start.unwrap_or_else(|| {
                                    builder
                                        .source_position(
                                            &SourceSpan::new(source_id, *start, *start)
                                                .expect("collected scope starts are ordered"),
                                        )
                                        .unwrap_or(SourcePosition { line: 1, column: 1 })
                                }));
                            push_diagnostic(
                                diagnostics,
                                limits,
                                Diagnostic::new(
                                    DiagnosticCode::new(DiagnosticCode::ROFF_WHILE_NESTED)
                                        .expect("static diagnostic code is valid"),
                                    Severity::Unsupported,
                                    "nested .while loops",
                                )
                                .with_primary(nested_span),
                                truncated,
                            );
                            if let Some(outer_closer) = lines.get(next + 1).map(scope_line_end) {
                                let closer_start = outer_closer.saturating_add(4);
                                push_diagnostic(
                                    diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_WHILE_CANNOT_CONTINUE,
                                        Severity::Unsupported,
                                        source_id,
                                        closer_start,
                                        closer_start,
                                        "cannot continue this .while loop",
                                    ),
                                    truncated,
                                );
                            }
                        }
                        // Mandoc's recovery puts the inner loop into the active
                        // input frame.  Once that loop is exhausted, it abandons
                        // the enclosing `.while` rather than replaying its
                        // remaining sibling lines (notably the outer register
                        // decrement).  Model that explicitly rather than
                        // flattening one body line into the parent frame.
                        frames.clear();
                        frames.push(ReplayFrame::Loop {
                            start: *start,
                            end: *end,
                            predicate,
                            lines: loop_lines,
                            iterations: 0,
                            break_after: true,
                        });
                        continue;
                    }
                    match execute_scope_line(
                        line,
                        builder,
                        root,
                        source_id,
                        scanner,
                        environment,
                        limits,
                        text_bytes,
                        expansion_steps,
                        maximum_depth,
                        total_loop_iterations,
                        diagnostics,
                        truncated,
                    ) {
                        ScopeFlow::Continue => {}
                        ScopeFlow::Halt => return ScopeFlow::Halt,
                        ScopeFlow::Break => {
                            let mut consumed = false;
                            while let Some(frame) = frames.pop() {
                                if matches!(frame, ReplayFrame::Loop { .. }) {
                                    consumed = true;
                                    break;
                                }
                            }
                            if !consumed {
                                return ScopeFlow::Break;
                            }
                        }
                        ScopeFlow::CloseLoopInInnerScope { invocation_start } => {
                            // A macro reparses in a nested input frame in mandoc.
                            // Its `\\}` closes the active outer loop but the caller's
                            // remaining physical scope lines still run.  Drop only
                            // the loop frame, retain those continuations, and
                            // propagate the later out-of-scope recovery to the
                            // scanner boundary.
                            let diagnostic_start = invocation_start.saturating_add(4);
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_WHILE_INNER_SCOPE,
                                    Severity::Unsupported,
                                    source_id,
                                    diagnostic_start,
                                    diagnostic_start,
                                    "end of .while loop in inner scope",
                                ),
                                truncated,
                            );
                            let mut continuations = Vec::new();
                            let mut consumed = false;
                            while let Some(frame) = frames.pop() {
                                if matches!(frame, ReplayFrame::Loop { .. }) {
                                    consumed = true;
                                    break;
                                }
                                continuations.push(frame);
                            }
                            for frame in continuations.into_iter().rev() {
                                frames.push(frame);
                            }
                            // The outermost `.while` is driven by the caller of
                            // this function rather than a `Loop` frame.  In that
                            // case there is nothing local to remove, but the
                            // continuation lines must still execute before the
                            // recovery is returned to that caller.
                            if !consumed {
                                closed_loop_from_inner_scope = Some(invocation_start);
                                continue;
                            }
                            closed_loop_from_inner_scope = Some(invocation_start);
                        }
                        ScopeFlow::LoopContinue => {
                            let mut loop_frame = None;
                            while let Some(frame) = frames.pop() {
                                if matches!(frame, ReplayFrame::Loop { .. }) {
                                    loop_frame = Some(frame);
                                    break;
                                }
                            }
                            let Some(loop_frame) = loop_frame else {
                                return ScopeFlow::LoopContinue;
                            };
                            frames.push(loop_frame);
                        }
                    }
                }
                ReplayFrame::Loop {
                    start,
                    end,
                    predicate,
                    lines,
                    iterations,
                    break_after,
                } => {
                    let Some(expanded_predicate) = expand_environment(
                        environment,
                        predicate,
                        scanner.escape_character(),
                        &[],
                        limits,
                        source_id,
                        start,
                        end,
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) else {
                        return ScopeFlow::Halt;
                    };
                    let Some(condition) = evaluate_condition(
                        environment,
                        &expanded_predicate,
                        scanner.escape_character(),
                    ) else {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "nested roff while predicate is outside the M3 numeric/nroff subset",
                            ),
                            truncated,
                        );
                        continue;
                    };
                    if !condition {
                        if break_after {
                            return ScopeFlow::Break;
                        }
                        continue;
                    }
                    if iterations >= limits.max_loop_iterations {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_LOOP_ITERATIONS,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "nested roff while request exceeds max_loop_iterations",
                            ),
                            truncated,
                        );
                        continue;
                    }
                    if *total_loop_iterations >= limits.max_total_loop_iterations {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_TOTAL_LOOP_ITERATIONS,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "nested roff while requests exceed max_total_loop_iterations",
                            ),
                            truncated,
                        );
                        continue;
                    }
                    if !record_expansion_steps(
                        expansion_steps,
                        1,
                        limits,
                        source_id,
                        start,
                        end,
                        diagnostics,
                        truncated,
                    ) {
                        return ScopeFlow::Halt;
                    }
                    *total_loop_iterations += 1;
                    frames.push(ReplayFrame::Loop {
                        start,
                        end,
                        predicate,
                        lines,
                        iterations: iterations + 1,
                        break_after,
                    });
                    if break_after {
                        let control_column = lines
                            .first()
                            .and_then(|line| match line {
                                ScopeLine::Control {
                                    start,
                                    argument_start,
                                    name,
                                    ..
                                } => argument_start.saturating_sub(*start).checked_sub(
                                    u32::try_from(name.len())
                                        .expect("scope request names fit public source columns"),
                                ),
                                _ => None,
                            })
                            .unwrap_or(1);
                        let replay_offset = if iterations == 0 {
                            lines.first().map(scope_line_start)
                        } else {
                            lines
                                .last()
                                .map(scope_line_end)
                                .map(|end| end.saturating_add(1))
                        };
                        if let Some(replay_offset) = replay_offset
                            && let Some(replay_position) = builder.source_position(
                                &SourceSpan::new(source_id, replay_offset, replay_offset)
                                    .expect("collected scope positions are ordered"),
                            )
                        {
                            frames.push(ReplayFrame::SetNewRootChildrenLogicalStart {
                                first_child: builder.children(root).map_or(0, <[NodeId]>::len),
                                position: SourcePosition {
                                    line: replay_position.line,
                                    column: end
                                        .saturating_sub(start)
                                        .saturating_add(control_column)
                                        .saturating_sub(1),
                                },
                            });
                        }
                    }
                    frames.push(ReplayFrame::Lines {
                        lines,
                        next: 0,
                        previous_conditional: None,
                    });
                }
            }
        }
        closed_loop_from_inner_scope.map_or(ScopeFlow::Continue, |invocation_start| {
            ScopeFlow::CloseLoopInInnerScope { invocation_start }
        })
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Macro copy-mode reparsing stays iterative at the scope boundary.
pub(in crate::parser) fn execute_scope_macro_lines(
    lines: Vec<Vec<u8>>,
    arguments: &[Vec<u8>],
    charge_entry: bool,
    scope_depth: usize,
    builder: &mut DocumentBuilder,
    root: NodeId,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    scanner: &mut Scanner<'_>,
    environment: &mut Environment,
    limits: &Limits,
    text_bytes: &mut usize,
    expansion_steps: &mut usize,
    maximum_depth: &mut usize,
    total_loop_iterations: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> ScopeFlow {
    if charge_entry
        && !record_expansion_steps(
            expansion_steps,
            1,
            limits,
            source_id,
            start,
            end,
            diagnostics,
            truncated,
        )
    {
        return ScopeFlow::Halt;
    }
    let mut pending = lines
        .into_iter()
        .rev()
        .map(|line| (line, arguments.to_vec(), 1_usize, 0_u32, None, false))
        .collect::<Vec<_>>();
    let mut macro_conditionals = Vec::<(usize, bool)>::new();
    while let Some((
        source_line,
        macro_arguments,
        macro_depth,
        macro_origin,
        text_origin,
        _scope_reparse,
    )) = pending.pop()
    {
        let line = copy_mode_reparse(&source_line, scanner.escape_character());
        if let Some((request, raw_arguments)) = split_macro_control(
            &line,
            scanner.control_character(),
            scanner.escape_character(),
        ) {
            if is_macro_comment_request(request, scanner.escape_character()) {
                continue;
            }
            if is_scope_closer(request, scanner.escape_character()) {
                return ScopeFlow::CloseLoopInInnerScope {
                    invocation_start: start,
                };
            }
            if request == b"continue" {
                return ScopeFlow::LoopContinue;
            }
            if matches!(request, b"cc" | b"c2" | b"ec") {
                scanner.apply_character_request(request, raw_arguments);
                continue;
            }
            if request == b"return" {
                break;
            }
            // `.nop` suppresses only its own request spelling.  The
            // remainder is re-read as ordinary input, rather than becoming
            // an observable unknown roff element.  Requeue it so copied
            // macro arguments and escapes follow the normal text path.
            if request == b"nop" {
                pending.push((
                    raw_arguments.to_vec(),
                    macro_arguments,
                    macro_depth,
                    macro_origin,
                    text_origin,
                    false,
                ));
                continue;
            }
            if request == b"tr" {
                environment.define_translation(raw_arguments, scanner.escape_character());
                continue;
            }
            if request == b"while"
                && let Ok(while_arguments) =
                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                && let Some((predicate_template, body)) = while_arguments.split_first()
                && !is_scope_opener(&join_arguments(body), scanner.escape_character())
            {
                if scope_depth >= limits.max_tree_depth {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::LIMIT_SCOPE_DEPTH,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "nested single-line roff while exceeds max_tree_depth",
                        ),
                        truncated,
                    );
                    continue;
                }
                let body = join_arguments(body);
                let mut iterations = 0_usize;
                loop {
                    let Some(predicate) = expand_environment(
                        environment,
                        &predicate_template.bytes,
                        scanner.escape_character(),
                        &macro_arguments,
                        limits,
                        source_id,
                        start,
                        end,
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) else {
                        return ScopeFlow::Halt;
                    };
                    let Some(condition) =
                        evaluate_condition(environment, &predicate, scanner.escape_character())
                    else {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "single-line roff while predicate in a scope macro is outside the M3 numeric/nroff subset",
                            ),
                            truncated,
                        );
                        break;
                    };
                    if !condition {
                        break;
                    }
                    if iterations >= limits.max_loop_iterations
                        || *total_loop_iterations >= limits.max_total_loop_iterations
                    {
                        *truncated = true;
                        let (code, message) = if iterations >= limits.max_loop_iterations {
                            (
                                DiagnosticCode::LIMIT_LOOP_ITERATIONS,
                                "single-line roff while in a scope macro exceeds max_loop_iterations",
                            )
                        } else {
                            (
                                DiagnosticCode::LIMIT_TOTAL_LOOP_ITERATIONS,
                                "single-line roff while requests in scope macros exceed max_total_loop_iterations",
                            )
                        };
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(code, Severity::Warning, source_id, start, end, message),
                            truncated,
                        );
                        break;
                    }
                    if !record_expansion_steps(
                        expansion_steps,
                        1,
                        limits,
                        source_id,
                        start,
                        end,
                        diagnostics,
                        truncated,
                    ) {
                        return ScopeFlow::Halt;
                    }
                    iterations += 1;
                    *total_loop_iterations += 1;
                    match execute_scope_macro_lines(
                        vec![body.clone()],
                        &macro_arguments,
                        false,
                        scope_depth + 1,
                        builder,
                        root,
                        source_id,
                        start,
                        end,
                        scanner,
                        environment,
                        limits,
                        text_bytes,
                        expansion_steps,
                        maximum_depth,
                        total_loop_iterations,
                        diagnostics,
                        truncated,
                    ) {
                        ScopeFlow::Continue | ScopeFlow::LoopContinue => {}
                        ScopeFlow::Break => break,
                        flow @ ScopeFlow::CloseLoopInInnerScope { .. } => return flow,
                        ScopeFlow::Halt => return ScopeFlow::Halt,
                    }
                }
                continue;
            }
            if request == b"while"
                && let Ok(while_arguments) =
                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                && let Some((predicate_template, body)) = while_arguments.split_first()
                && is_scope_opener(&join_arguments(body), scanner.escape_character())
            {
                if scope_depth >= limits.max_tree_depth {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::LIMIT_SCOPE_DEPTH,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "nested roff while scope in a scope macro exceeds max_tree_depth",
                        ),
                        truncated,
                    );
                    continue;
                }
                let Some(scope) = collect_pending_macro_scope(
                    &mut pending,
                    macro_depth,
                    scanner.control_character(),
                    scanner.escape_character(),
                    limits,
                ) else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff while in a scope macro reached its caller before its `\\}` terminator",
                        ),
                        truncated,
                    );
                    continue;
                };
                let scope_lines = scope
                    .into_iter()
                    .map(|(line, _, _, _, _, _)| line)
                    .collect::<Vec<_>>();
                let mut iterations = 0_usize;
                loop {
                    let Some(predicate) = expand_environment(
                        environment,
                        &predicate_template.bytes,
                        scanner.escape_character(),
                        &macro_arguments,
                        limits,
                        source_id,
                        start,
                        end,
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) else {
                        return ScopeFlow::Halt;
                    };
                    let Some(condition) =
                        evaluate_condition(environment, &predicate, scanner.escape_character())
                    else {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_CONDITION,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff while predicate in a scope macro is outside the M3 numeric/nroff subset",
                            ),
                            truncated,
                        );
                        break;
                    };
                    if !condition {
                        break;
                    }
                    if iterations >= limits.max_loop_iterations
                        || *total_loop_iterations >= limits.max_total_loop_iterations
                    {
                        *truncated = true;
                        let (code, message) = if iterations >= limits.max_loop_iterations {
                            (
                                DiagnosticCode::LIMIT_LOOP_ITERATIONS,
                                "roff while request in a scope macro exceeds max_loop_iterations",
                            )
                        } else {
                            (
                                DiagnosticCode::LIMIT_TOTAL_LOOP_ITERATIONS,
                                "roff while requests in scope macros exceed max_total_loop_iterations",
                            )
                        };
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(code, Severity::Warning, source_id, start, end, message),
                            truncated,
                        );
                        break;
                    }
                    iterations += 1;
                    *total_loop_iterations += 1;
                    match execute_scope_macro_lines(
                        scope_lines.clone(),
                        &macro_arguments,
                        true,
                        scope_depth + 1,
                        builder,
                        root,
                        source_id,
                        start,
                        end,
                        scanner,
                        environment,
                        limits,
                        text_bytes,
                        expansion_steps,
                        maximum_depth,
                        total_loop_iterations,
                        diagnostics,
                        truncated,
                    ) {
                        ScopeFlow::Continue | ScopeFlow::LoopContinue => {}
                        ScopeFlow::Break => break,
                        flow @ ScopeFlow::CloseLoopInInnerScope { .. } => return flow,
                        ScopeFlow::Halt => return ScopeFlow::Halt,
                    }
                }
                continue;
            }
            if matches!(request, b"if" | b"ie" | b"el") {
                let Ok(condition_arguments) =
                    lex_condition_arguments(raw_arguments, scanner.escape_character(), limits)
                else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ARGUMENT_LIMIT,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional arguments in a scope macro exceed configured parser limits",
                        ),
                        truncated,
                    );
                    continue;
                };
                let (condition, body_start) = match request {
                    b"el" => {
                        let condition = macro_conditionals
                            .iter()
                            .rposition(|(depth, _)| *depth == macro_depth)
                            .map(|index| !macro_conditionals.remove(index).1);
                        (condition, 0)
                    }
                    b"if" | b"ie" => {
                        if request == b"ie"
                            && (condition_arguments.is_empty()
                                || condition_arguments
                                    .first()
                                    .is_some_and(|argument| argument.bytes == b"!"))
                        {
                            macro_conditionals.retain(|(depth, _)| *depth != macro_depth);
                            macro_conditionals.push((macro_depth, false));
                            (Some(false), condition_arguments.len())
                        } else {
                            let Some((predicate, body_start)) =
                                condition_parts(&condition_arguments)
                            else {
                                push_diagnostic(
                                    diagnostics,
                                    limits,
                                    diagnostic(
                                        DiagnosticCode::ROFF_CONDITION,
                                        Severity::Warning,
                                        source_id,
                                        start,
                                        end,
                                        "roff conditional in a scope macro is missing its predicate",
                                    ),
                                    truncated,
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
                                expansion_steps,
                                diagnostics,
                                truncated,
                            ) else {
                                return ScopeFlow::Halt;
                            };
                            let condition = evaluate_condition(
                                environment,
                                &predicate,
                                scanner.escape_character(),
                            );
                            if request == b"ie"
                                && let Some(condition) = condition
                            {
                                macro_conditionals.retain(|(depth, _)| *depth != macro_depth);
                                macro_conditionals.push((macro_depth, condition));
                            }
                            (condition, body_start)
                        }
                    }
                    _ => unreachable!("conditional request was filtered above"),
                };
                let Some(condition) = condition else {
                    if request == b"el" {
                        continue;
                    }
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional in a scope macro is outside the M3 numeric/nroff subset",
                        ),
                        truncated,
                    );
                    continue;
                };
                let body_template =
                    condition_body_template(raw_arguments, &condition_arguments, body_start);
                let escape = scanner.escape_character();
                if is_scope_opener(&body_template, escape) {
                    let Some(scope) = collect_pending_macro_scope(
                        &mut pending,
                        macro_depth,
                        scanner.control_character(),
                        escape,
                        limits,
                    ) else {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff scope macro conditional reached its caller before its `\\}` terminator",
                            ),
                            truncated,
                        );
                        continue;
                    };
                    if condition {
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
                        text_origin,
                        false,
                    ));
                }
                continue;
            }
            if matches!(request, b"de" | b"de1" | b"am" | b"dei" | b"ami") {
                let Ok(definition_arguments) =
                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ARGUMENT_LIMIT,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "generated roff macro definition in a scope macro exceeds configured parser limits",
                        ),
                        truncated,
                    );
                    continue;
                };
                let Some(definition_name) = definition_arguments.first() else {
                    continue;
                };
                let indirect = matches!(request, b"dei" | b"ami");
                let Some(definition_name) = (!indirect)
                    .then(|| definition_name.bytes.clone())
                    .or_else(|| environment.indirect_string(&definition_name.bytes))
                else {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "generated indirect roff macro definition in a scope names an undefined string",
                        ),
                        truncated,
                    );
                    continue;
                };
                let terminator = match definition_arguments.get(1) {
                    None => vec![b'.'],
                    Some(argument) if !indirect => argument.bytes.clone(),
                    Some(argument) => {
                        let Some(terminator) = environment.indirect_string(&argument.bytes) else {
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "generated indirect roff macro terminator in a scope names an undefined string",
                                ),
                                truncated,
                            );
                            continue;
                        };
                        terminator
                    }
                };
                let definition_control = scanner.control_character();
                let mut body = Vec::new();
                let mut terminated = false;
                // A definition opened from a macro body first consumes that
                // caller's remaining copy-mode lines.  The original outer
                // definition may have stopped at the first `..`, while this
                // nested definition continues into physical input after the
                // macro invocation (`de/startde`).  Only then resume the
                // scanner for the remainder of the new definition.
                if matches!(request, b"de" | b"de1") {
                    while pending
                        .last()
                        .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
                    {
                        let (body_line, _, _, _, _, _) =
                            pending.pop().expect("checked macro depth");
                        if is_definition_terminator(&body_line, definition_control, &terminator) {
                            terminated = true;
                            break;
                        }
                        body.push(body_line);
                    }
                }
                while !terminated && let Some(body_line) = scanner.next_raw_line() {
                    if is_definition_terminator(body_line.bytes, definition_control, &terminator) {
                        terminated = true;
                        break;
                    }
                    if body_line.too_long {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::LIMIT_LINE_BYTES,
                                Severity::Warning,
                                source_id,
                                body_line.start,
                                body_line.end,
                                "copy-mode generated macro line in a scope exceeds max_line_bytes and was skipped",
                            ),
                            truncated,
                        );
                        continue;
                    }
                    body.push(body_line.bytes.to_vec());
                }
                if !terminated {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "generated roff macro definition in a scope reached source end before its terminator",
                        ),
                        truncated,
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
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                continue;
            }
            if request == b"ig" {
                let marker = match ignore_marker(raw_arguments, scanner.escape_character(), limits)
                {
                    Ok(marker) => marker,
                    Err(ArgumentIssue::UnterminatedQuote) => {
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff ignore-block marker in a scope macro contains an unterminated quote",
                            ),
                            truncated,
                        );
                        vec![b'.']
                    }
                    Err(ArgumentIssue::Limit) => {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            diagnostic(
                                DiagnosticCode::ARGUMENT_LIMIT,
                                Severity::Warning,
                                source_id,
                                start,
                                end,
                                "roff ignore-block marker in a scope macro exceeds configured parser limits",
                            ),
                            truncated,
                        );
                        vec![b'.']
                    }
                };
                consume_ignore_block(scanner, &marker);
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
                        expansion_steps,
                        diagnostics,
                        truncated,
                    ) {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            truncated,
                        );
                    }
                    continue;
                }
                let Ok(arguments) =
                    lex_arguments(raw_arguments, scanner.escape_character(), limits)
                else {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ARGUMENT_LIMIT,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "macro body arguments in a loop scope exceed configured parser limits",
                        ),
                        truncated,
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
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                continue;
            }
            let Some(element) = append_node(
                builder,
                root,
                NodeKind::Element,
                start,
                end,
                NodeFlags {
                    line_start: true,
                    ..NodeFlags::default()
                },
                &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
            ) else {
                continue;
            };
            if !builder.macro_name(element, visible_bytes(request)) {
                *truncated = true;
                continue;
            }
            *maximum_depth = (*maximum_depth).max(2);
            if raw_arguments.is_empty() {
                continue;
            }
            let Some(bytes) = expand_environment(
                environment,
                raw_arguments,
                scanner.escape_character(),
                &macro_arguments,
                limits,
                source_id,
                start,
                end,
                expansion_steps,
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
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
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
            };
            let result = normalize_document_escapes(builder, &bytes, escape, limits);
            if !record_expansion_steps(
                expansion_steps,
                result.steps,
                limits,
                source_id,
                start,
                end,
                diagnostics,
                truncated,
            ) {
                return ScopeFlow::Halt;
            }
            emit_escape_issues(
                &result.issues,
                start,
                end,
                &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
            );
            *truncated |= result.truncated;
            if append_text_node(
                builder,
                element,
                start,
                end,
                NodeFlags {
                    line_continuation: result.line_continuation,
                    ..NodeFlags::default()
                },
                result.text,
                &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
            ) {
                *maximum_depth = (*maximum_depth).max(3);
            }
            continue;
        }
        let Some(bytes) = expand_environment(
            environment,
            &line,
            scanner.escape_character(),
            &macro_arguments,
            limits,
            source_id,
            start,
            end,
            expansion_steps,
            diagnostics,
            truncated,
        ) else {
            return ScopeFlow::Halt;
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
            diagnostics,
            truncated,
        ) else {
            return ScopeFlow::Halt;
        };
        let result = normalize_document_escapes(builder, &bytes, escape, limits);
        if !record_expansion_steps(
            expansion_steps,
            result.steps,
            limits,
            source_id,
            start,
            end,
            diagnostics,
            truncated,
        ) {
            return ScopeFlow::Halt;
        }
        emit_escape_issues(
            &result.issues,
            start,
            end,
            &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
        );
        *truncated |= result.truncated;
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
            &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
        ) {
            *maximum_depth = (*maximum_depth).max(2);
        }
    }
    ScopeFlow::Continue
}

/// Turn an inline conditional body back into one dispatchable scope line.
pub(in crate::parser) fn inline_scope_body_line(
    bytes: Vec<u8>,
    start: u32,
    end: u32,
    control: u8,
    escape: u8,
) -> ScopeLine {
    match split_macro_control(&bytes, control, escape) {
        Some((name, arguments)) => ScopeLine::Control {
            start,
            end,
            argument_start: start
                .saturating_add(1)
                .saturating_add(
                    u32::try_from(name.len()).expect("inline scope request names fit source spans"),
                )
                .saturating_add(u32::from(!arguments.is_empty())),
            name: name.to_vec(),
            arguments: arguments.to_vec(),
        },
        None => ScopeLine::Text {
            start,
            end,
            bytes,
            terminal_inline: false,
        },
    }
}

/// Define a macro whose copy-mode body was already retained by a surrounding
/// brace scope.
///
/// A physical `.de` normally advances the scanner through its body.  When the
/// request sits inside a collected scope, those physical lines are instead
/// represented by `following`; consume precisely that local range so neither a
/// later sibling nor the caller's scanner position is stolen.  The returned
/// count excludes the definition request and includes its terminator.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Direct and indirect definition recovery mirrors the physical path.
pub(in crate::parser) fn execute_collected_scope_definition(
    line: &ScopeLine,
    following: &[ScopeLine],
    scanner: &Scanner<'_>,
    environment: &mut Environment,
    limits: &Limits,
    source_id: crate::SourceId,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<usize> {
    let ScopeLine::Control {
        start,
        end,
        name,
        arguments,
        ..
    } = line
    else {
        return None;
    };
    if !matches!(name.as_slice(), b"de" | b"de1" | b"am" | b"dei" | b"ami") {
        return None;
    }
    let definition_arguments = lex_arguments(arguments, scanner.escape_character(), limits).ok()?;
    let definition_name = definition_arguments.first()?;
    let indirect = matches!(name.as_slice(), b"dei" | b"ami");
    let definition_name = (!indirect)
        .then(|| definition_name.bytes.clone())
        .or_else(|| environment.indirect_string(&definition_name.bytes));
    let Some(definition_name) = definition_name else {
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                Severity::Warning,
                source_id,
                *start,
                *end,
                "indirect roff macro definition in a collected scope names an undefined string",
            ),
            truncated,
        );
        return Some(0);
    };
    let terminator = match definition_arguments.get(1) {
        None => vec![b'.'],
        Some(argument) if !indirect => argument.bytes.clone(),
        Some(argument) => {
            let Some(terminator) = environment.indirect_string(&argument.bytes) else {
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                        Severity::Warning,
                        source_id,
                        *start,
                        *end,
                        "indirect roff macro terminator in a collected scope names an undefined string",
                    ),
                    truncated,
                );
                return Some(0);
            };
            terminator
        }
    };
    let control = scanner.control_character();
    let escape = scanner.escape_character();
    let mut body = Vec::new();
    let mut consumed = 0_usize;
    let mut terminated = false;
    for candidate in following {
        let copy_mode_lines = scope_line_copy_mode_lines(candidate, control, escape);
        if copy_mode_lines
            .first()
            .is_some_and(|bytes| is_definition_terminator(bytes, control, &terminator))
        {
            consumed += 1;
            terminated = true;
            break;
        }
        consumed += 1;
        body.extend(copy_mode_lines);
    }
    if !terminated {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_UNTERMINATED_DEFINITION,
                Severity::Warning,
                source_id,
                *start,
                *end,
                "roff macro definition in a collected scope reached its scope end before its terminator",
            ),
            truncated,
        );
    }
    let definition = if matches!(name.as_slice(), b"dei" | b"ami") {
        environment.define_indirect_macro(
            &definition_name,
            body,
            matches!(name.as_slice(), b"am" | b"ami"),
            limits,
        )
    } else {
        environment.define_macro(
            &definition_name,
            body,
            matches!(name.as_slice(), b"am" | b"ami"),
            limits,
        )
    };
    if let Err(error) = definition {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            environment_error_diagnostic(error, source_id, *start, *end),
            truncated,
        );
    }
    Some(consumed)
}

/// Reconstruct one retained scope line as copy-mode macro bytes.
///
/// Nested scope frames were structurally recognized before a surrounding macro
/// definition could claim them.  Re-emitting their request spelling keeps the
/// definition's later iterative macro execution independent from the collector
/// and preserves the same control/escape characters that delimit it.
pub(in crate::parser) fn scope_line_copy_mode_lines(
    line: &ScopeLine,
    control: u8,
    escape: u8,
) -> Vec<Vec<u8>> {
    match line {
        ScopeLine::Text { bytes, .. } => vec![bytes.clone()],
        ScopeLine::Control {
            name, arguments, ..
        } => {
            let mut bytes = Vec::with_capacity(
                1 + name.len() + usize::from(!arguments.is_empty()) + arguments.len(),
            );
            bytes.push(control);
            bytes.extend_from_slice(name);
            if !arguments.is_empty() {
                bytes.push(b' ');
                bytes.extend_from_slice(arguments);
            }
            vec![bytes]
        }
        ScopeLine::Loop {
            predicate, lines, ..
        } => scope_line_copy_mode_scope(b"while", predicate, lines, control, escape),
        ScopeLine::Conditional {
            predicate,
            else_eligible,
            lines,
            ..
        } => scope_line_copy_mode_scope(
            if *else_eligible { b"ie" } else { b"if" },
            predicate,
            lines,
            control,
            escape,
        ),
        ScopeLine::Else { lines, .. } => {
            scope_line_copy_mode_scope(b"el", &[], lines, control, escape)
        }
    }
}

pub(in crate::parser) fn scope_line_copy_mode_scope(
    request: &[u8],
    predicate: &[u8],
    lines: &[ScopeLine],
    control: u8,
    escape: u8,
) -> Vec<Vec<u8>> {
    let mut opener = Vec::with_capacity(
        1 + request.len() + predicate.len() + usize::from(!predicate.is_empty()) + 4,
    );
    opener.push(control);
    opener.extend_from_slice(request);
    if !predicate.is_empty() {
        opener.push(b' ');
        opener.extend_from_slice(predicate);
    }
    opener.extend_from_slice(&[b' ', escape, b'{', escape]);
    let mut copy_mode = vec![opener];
    for line in lines {
        copy_mode.extend(scope_line_copy_mode_lines(line, control, escape));
    }
    copy_mode.push(vec![control, escape, b'}']);
    copy_mode
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Iterative scope dispatch keeps untrusted roff control flow non-recursive.
pub(in crate::parser) fn execute_scope_line(
    line: &ScopeLine,
    builder: &mut DocumentBuilder,
    root: NodeId,
    source_id: crate::SourceId,
    scanner: &mut Scanner<'_>,
    environment: &mut Environment,
    limits: &Limits,
    text_bytes: &mut usize,
    expansion_steps: &mut usize,
    maximum_depth: &mut usize,
    total_loop_iterations: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> ScopeFlow {
    let (start, end) = match line {
        ScopeLine::Text { start, end, .. }
        | ScopeLine::Control { start, end, .. }
        | ScopeLine::Loop { start, end, .. }
        | ScopeLine::Conditional { start, end, .. }
        | ScopeLine::Else { start, end, .. } => (*start, *end),
    };
    match line {
        ScopeLine::Text {
            bytes,
            terminal_inline,
            ..
        } => {
            let Some(bytes) = expand_environment(
                environment,
                bytes,
                scanner.escape_character(),
                &[],
                limits,
                source_id,
                start,
                end,
                expansion_steps,
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
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
                diagnostics,
                truncated,
            ) else {
                return ScopeFlow::Halt;
            };
            let result = normalize_document_escapes(builder, &bytes, escape, limits);
            if !record_expansion_steps(
                expansion_steps,
                result.steps,
                limits,
                source_id,
                start,
                end,
                diagnostics,
                truncated,
            ) {
                return ScopeFlow::Halt;
            }
            emit_escape_issues(
                &result.issues,
                start,
                end,
                &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
            );
            *truncated |= result.truncated;
            if append_text_node(
                builder,
                root,
                start,
                end,
                NodeFlags {
                    line_start: true,
                    // A bare conditional opener contributes a vertical blank
                    // with the opener's nonempty source span.
                    generated: result.text.is_empty() && start < end,
                    line_continuation: result.line_continuation,
                    ..NodeFlags::default()
                },
                result.text,
                &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
            ) {
                if *terminal_inline
                    && let Some(node) = builder
                        .children(root)
                        .and_then(|children| children.last())
                        .copied()
                {
                    let _ = builder.set_node_terminal_inline_conditional(node, true);
                }
                *maximum_depth = (*maximum_depth).max(2);
            }
        }
        ScopeLine::Control {
            argument_start,
            name,
            arguments,
            ..
        } => {
            // Collected scope controls retain their full physical line span,
            // whose first byte is the roff control character. Public macro
            // locations instead begin at the request name, and their
            // arguments begin after that name and its separating blank.
            // Ordinary scanning records these offsets directly; recover the
            // equivalent positions while replaying a stored scope line.
            let control_start = start.saturating_add(1);
            let control_argument_start = *argument_start;
            if matches!(name.as_slice(), b"while" | b"nop") {
                let mut generated = Vec::with_capacity(
                    1 + name.len() + usize::from(!arguments.is_empty()) + arguments.len(),
                );
                generated.push(scanner.control_character());
                generated.extend_from_slice(name);
                if !arguments.is_empty() {
                    generated.push(b' ');
                    generated.extend_from_slice(arguments);
                }
                return execute_scope_macro_lines(
                    vec![generated],
                    &[],
                    false,
                    1,
                    builder,
                    root,
                    source_id,
                    start,
                    end,
                    scanner,
                    environment,
                    limits,
                    text_bytes,
                    expansion_steps,
                    maximum_depth,
                    total_loop_iterations,
                    diagnostics,
                    truncated,
                );
            }
            if matches!(name.as_slice(), b"cc" | b"c2" | b"ec") {
                scanner.apply_character_request(name, arguments);
                return ScopeFlow::Continue;
            }
            if name == b"break" {
                return ScopeFlow::Break;
            }
            if name == b"continue" {
                return ScopeFlow::LoopContinue;
            }
            // `.nop` consumes its request name and lets the remainder flow
            // through the ordinary text parser.  In particular, it must not
            // leave an unknown `nop` element in the public AST.
            if name == b"nop" {
                let text = ScopeLine::Text {
                    start,
                    end,
                    bytes: arguments.clone(),
                    terminal_inline: false,
                };
                return execute_scope_line(
                    &text,
                    builder,
                    root,
                    source_id,
                    scanner,
                    environment,
                    limits,
                    text_bytes,
                    expansion_steps,
                    maximum_depth,
                    total_loop_iterations,
                    diagnostics,
                    truncated,
                );
            }
            if matches!(name.as_slice(), b"ds" | b"as") {
                if let Err(error) = apply_string_request(
                    environment,
                    arguments,
                    scanner.escape_character(),
                    name == b"as",
                    limits,
                    source_id,
                    start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                return ScopeFlow::Continue;
            }
            let raw_arguments = arguments.as_slice();
            let Ok(arguments) = lex_arguments(arguments, scanner.escape_character(), limits) else {
                *truncated = true;
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ARGUMENT_LIMIT,
                        Severity::Warning,
                        source_id,
                        start,
                        end,
                        "roff scope control arguments exceed configured parser limits",
                    ),
                    truncated,
                );
                return ScopeFlow::Continue;
            };
            if name == b"tr" {
                environment
                    .define_translation(&join_arguments(&arguments), scanner.escape_character());
                return ScopeFlow::Continue;
            }
            if name == b"if" {
                let Some((predicate_template, body_start)) = condition_parts(&arguments) else {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional in a loop scope is missing its predicate",
                        ),
                        truncated,
                    );
                    return ScopeFlow::Continue;
                };
                let Some(predicate) = expand_environment(
                    environment,
                    &predicate_template,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) else {
                    return ScopeFlow::Halt;
                };
                let Some(condition) =
                    evaluate_condition(environment, &predicate, scanner.escape_character())
                else {
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_CONDITION,
                            Severity::Warning,
                            source_id,
                            start,
                            end,
                            "roff conditional in a loop scope is outside the M3 numeric/nroff subset",
                        ),
                        truncated,
                    );
                    return ScopeFlow::Continue;
                };
                if !condition {
                    return ScopeFlow::Continue;
                }
                let body_template = condition_body_template(raw_arguments, &arguments, body_start);
                let body_source_start = condition_body_source_start_from_offset(
                    raw_arguments,
                    &arguments,
                    body_start,
                    control_argument_start,
                    start,
                    None,
                );
                let Some(body) = expand_environment(
                    environment,
                    &body_template,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    body_source_start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) else {
                    return ScopeFlow::Halt;
                };
                if split_macro_control(
                    &body,
                    scanner.control_character(),
                    scanner.escape_character(),
                )
                .is_some_and(|(request, _)| {
                    matches!(request, b"if" | b"ie" | b"el" | b"while" | b"nop")
                }) {
                    return execute_scope_macro_lines(
                        vec![body],
                        &[],
                        false,
                        1,
                        builder,
                        root,
                        source_id,
                        start,
                        end,
                        scanner,
                        environment,
                        limits,
                        text_bytes,
                        expansion_steps,
                        maximum_depth,
                        total_loop_iterations,
                        diagnostics,
                        truncated,
                    );
                }
                if let Some((request, raw_arguments)) = split_macro_control(
                    &body,
                    scanner.control_character(),
                    scanner.escape_character(),
                ) {
                    if matches!(request, b"break" | b"continue") {
                        return if request == b"break" {
                            ScopeFlow::Break
                        } else {
                            ScopeFlow::LoopContinue
                        };
                    }
                    if matches!(request, b"cc" | b"c2" | b"ec") {
                        scanner.apply_character_request(request, raw_arguments);
                        return ScopeFlow::Continue;
                    }
                    if request == b"tr" {
                        let Ok(arguments) =
                            lex_arguments(raw_arguments, scanner.escape_character(), limits)
                        else {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "inline roff conditional translation arguments in a scope exceed configured parser limits",
                                ),
                                truncated,
                            );
                            return ScopeFlow::Continue;
                        };
                        environment.define_translation(
                            &join_arguments(&arguments),
                            scanner.escape_character(),
                        );
                        return ScopeFlow::Continue;
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
                                expansion_steps,
                                diagnostics,
                                truncated,
                            ) {
                                *truncated = true;
                                push_diagnostic(
                                    diagnostics,
                                    limits,
                                    environment_error_diagnostic(error, source_id, start, end),
                                    truncated,
                                );
                            }
                            return ScopeFlow::Continue;
                        }
                        let Ok(arguments) =
                            lex_arguments(raw_arguments, scanner.escape_character(), limits)
                        else {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "roff conditional body in a loop scope exceeds argument limits",
                                ),
                                truncated,
                            );
                            return ScopeFlow::Continue;
                        };
                        if let Err(error) = apply_environment_request(
                            environment,
                            builder,
                            request,
                            scanner.escape_character(),
                            &arguments,
                            limits,
                        ) {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                environment_error_diagnostic(error, source_id, start, end),
                                truncated,
                            );
                        }
                        return ScopeFlow::Continue;
                    }
                    if !is_builtin_package_macro(builder.macro_set(), request)
                        && let Some(definition) = environment.macro_definition(request).cloned()
                    {
                        let Ok(arguments) =
                            lex_arguments(raw_arguments, scanner.escape_character(), limits)
                        else {
                            *truncated = true;
                            push_diagnostic(
                                diagnostics,
                                limits,
                                diagnostic(
                                    DiagnosticCode::ARGUMENT_LIMIT,
                                    Severity::Warning,
                                    source_id,
                                    start,
                                    end,
                                    "inline roff conditional macro arguments in a scope exceed configured parser limits",
                                ),
                                truncated,
                            );
                            return ScopeFlow::Continue;
                        };
                        let arguments = arguments
                            .into_iter()
                            .map(|argument| argument.bytes)
                            .collect::<Vec<_>>();
                        return execute_scope_macro_lines(
                            definition.lines,
                            &arguments,
                            true,
                            1,
                            builder,
                            root,
                            source_id,
                            start,
                            end,
                            scanner,
                            environment,
                            limits,
                            text_bytes,
                            expansion_steps,
                            maximum_depth,
                            total_loop_iterations,
                            diagnostics,
                            truncated,
                        );
                    }
                }
                let result =
                    normalize_document_escapes(builder, &body, scanner.escape_character(), limits);
                if !record_expansion_steps(
                    expansion_steps,
                    result.steps,
                    limits,
                    source_id,
                    body_source_start,
                    end,
                    diagnostics,
                    truncated,
                ) {
                    return ScopeFlow::Halt;
                }
                emit_escape_issues(
                    &result.issues,
                    body_source_start,
                    end,
                    &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
                );
                *truncated |= result.truncated;
                if append_text_node(
                    builder,
                    root,
                    body_source_start,
                    end,
                    NodeFlags {
                        line_start: true,
                        line_continuation: result.line_continuation,
                        ..NodeFlags::default()
                    },
                    result.text,
                    &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
                ) {
                    *maximum_depth = (*maximum_depth).max(2);
                }
                return ScopeFlow::Continue;
            }
            if is_environment_request(name) {
                if let Err(error) = apply_environment_request(
                    environment,
                    builder,
                    name,
                    scanner.escape_character(),
                    &arguments,
                    limits,
                ) {
                    *truncated = true;
                    push_diagnostic(
                        diagnostics,
                        limits,
                        environment_error_diagnostic(error, source_id, start, end),
                        truncated,
                    );
                }
                return ScopeFlow::Continue;
            }
            if !is_builtin_package_macro(builder.macro_set(), name)
                && let Some(definition) = environment.macro_definition(name).cloned()
            {
                let arguments = arguments
                    .into_iter()
                    .map(|argument| argument.bytes)
                    .collect::<Vec<_>>();
                return execute_scope_macro_lines(
                    definition.lines,
                    &arguments,
                    true,
                    1,
                    builder,
                    root,
                    source_id,
                    start,
                    end,
                    scanner,
                    environment,
                    limits,
                    text_bytes,
                    expansion_steps,
                    maximum_depth,
                    total_loop_iterations,
                    diagnostics,
                    truncated,
                );
            }
            let Some(element) = append_node(
                builder,
                root,
                NodeKind::Element, // `br` parsed while replaying a conditional scope keeps the
                // physical control-column location in the legacy tree.  It
                // is a roff layout request rather than a visible package
                // macro, unlike `.B` and the other font controls above.
                if name == b"br" { start } else { control_start },
                end,
                NodeFlags {
                    line_start: true,
                    ..NodeFlags::default()
                },
                &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
            ) else {
                return ScopeFlow::Continue;
            };
            if !builder.macro_name(element, visible_bytes(name)) {
                *truncated = true;
                return ScopeFlow::Continue;
            }
            *maximum_depth = (*maximum_depth).max(2);
            for argument in arguments {
                let argument_source_start = control_argument_start.saturating_add(
                    u32::try_from(argument.offset)
                        .expect("scope argument offsets fit source spans"),
                );
                let Some(bytes) = expand_environment(
                    environment,
                    &argument.bytes,
                    scanner.escape_character(),
                    &[],
                    limits,
                    source_id,
                    argument_source_start,
                    end,
                    expansion_steps,
                    diagnostics,
                    truncated,
                ) else {
                    return ScopeFlow::Halt;
                };
                // Package-macro arguments keep the source-visible roff
                // formatter spelling in the public AST.  The normal control
                // scanner expands environments here but deliberately does
                // not apply `.tr`: translation is an execution concern and
                // would erase controls such as the `\&` synthesized around
                // an attached scope closer.  Scope replay must use that same
                // projection.
                let escape = scanner.escape_character();
                let result = normalize_document_escapes(builder, &bytes, escape, limits);
                if !record_expansion_steps(
                    expansion_steps,
                    result.steps,
                    limits,
                    source_id,
                    argument_source_start,
                    end,
                    diagnostics,
                    truncated,
                ) {
                    return ScopeFlow::Halt;
                }
                emit_escape_issues(
                    &result.issues,
                    start,
                    end,
                    &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
                );
                *truncated |= result.truncated;
                if append_text_node(
                    builder,
                    element,
                    argument_source_start,
                    end,
                    NodeFlags {
                        line_continuation: result.line_continuation,
                        ..NodeFlags::default()
                    },
                    result.text,
                    &mut EmitContext::new(source_id, limits, text_bytes, diagnostics, truncated),
                ) {
                    *maximum_depth = (*maximum_depth).max(3);
                }
            }
        }
        ScopeLine::Loop { .. } | ScopeLine::Conditional { .. } | ScopeLine::Else { .. } => {
            unreachable!("nested scopes are dispatched by the explicit scope execution stack")
        }
    }
    ScopeFlow::Continue
}
