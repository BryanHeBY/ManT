use super::super::{
    CollectedScope, Diagnostic, DiagnosticCode, Environment, Limits, MacroSet, PendingMacroLine,
    PendingScope, ScannedLine, Scanner, ScopeKind, ScopeLine, Severity, condition_parts,
    copy_mode_reparse, diagnostic, font_macro_arguments_without_scope_closers,
    innermost_scope_is_statically_inactive, is_man_visible_argument_macro, is_scope_closer,
    is_scope_opener, join_arguments, lex_arguments, lex_condition_arguments, push_diagnostic,
    scope_closer_offset, scope_closer_text, scope_opener_remainder, scope_remainder_source_start,
    split_macro_control, visible_bytes,
};

pub(in crate::parser) struct ScopeCollector<'state, 'source> {
    pub(in crate::parser) scanner: &'state mut Scanner<'source>,
    pub(in crate::parser) source_id: crate::SourceId,
    pub(in crate::parser) limits: &'state Limits,
    pub(in crate::parser) macro_set: MacroSet,
    pub(in crate::parser) diagnostics: &'state mut Vec<Diagnostic>,
    pub(in crate::parser) truncated: &'state mut bool,
    pub(in crate::parser) emit_definition_tail_diagnostics: bool,
}

impl ScopeCollector<'_, '_> {
    pub(in crate::parser) fn collect(
        self,
        scope_start: u32,
        scope_end: u32,
        unterminated_scope_name: Option<&[u8]>,
    ) -> CollectedScope {
        collect_scope(
            self.scanner,
            self.source_id,
            self.limits,
            self.macro_set,
            self.diagnostics,
            self.truncated,
            self.emit_definition_tail_diagnostics,
            scope_start,
            scope_end,
            unterminated_scope_name,
        )
    }
}

#[allow(clippy::too_many_arguments)] // Private collector core; callers use `ScopeCollector`.
#[allow(clippy::too_many_lines)] // Collection mirrors scanner cases while retaining nested scopes without recursion.
fn collect_scope(
    scanner: &mut Scanner<'_>,
    source_id: crate::SourceId,
    limits: &Limits,
    macro_set: MacroSet,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
    emit_definition_tail_diagnostics: bool,
    scope_start: u32,
    scope_end: u32,
    unterminated_scope_name: Option<&[u8]>,
) -> CollectedScope {
    let character_state = scanner.character_state();
    macro_rules! finish_scope {
        ($scope:expr) => {{
            scanner.restore_character_state(character_state);
            return $scope;
        }};
    }
    let mut frames = vec![PendingScope {
        start: scope_start,
        end: scope_end,
        kind: None,
        lines: Vec::new(),
    }];
    let mut discarded_nesting = 0_usize;
    loop {
        // `next_line` applies `.cc`/`.c2`/`.ec` after lexing a request.  Use
        // the state that was active before consuming this physical line, then
        // observe its replacement on the next line just as normal execution
        // does.
        let escape = scanner.escape_character();
        let Some(line) = scanner.next_line() else {
            break;
        };
        match line {
            ScannedLine::TooLong { start, end } => {
                *truncated = true;
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::LIMIT_LINE_BYTES,
                        Severity::Warning,
                        source_id,
                        start,
                        end,
                        "roff scope line exceeds max_line_bytes and was skipped",
                    ),
                    truncated,
                );
            }
            ScannedLine::Comment { .. } => {}
            ScannedLine::Control { start, name, .. } if is_scope_closer(name, escape) => {
                if discarded_nesting > 0 {
                    discarded_nesting -= 1;
                    continue;
                }
                if let Some(scope) = close_collected_scope(&mut frames, start) {
                    finish_scope!(scope);
                }
            }
            ScannedLine::Control {
                start,
                no_break: _,
                name,
                arguments,
                ..
            } if name.starts_with(&[escape, b'}']) => {
                // The scanner keeps `\}middle` as one control name so that
                // normal macro parsing can preserve it.  Inside a collected
                // scope, however, it is a closing request: mandoc discards
                // its argument tail.  Further closers in that tail still
                // unwind nested frames, but their intervening text is also
                // not a scope body.
                let mut remaining = name[2..].to_vec();
                remaining.extend_from_slice(arguments);
                if discarded_nesting > 0 {
                    discarded_nesting -= 1;
                } else if let Some(scope) = close_collected_scope(&mut frames, start) {
                    finish_scope!(scope);
                }
                while let Some(close) = scope_closer_offset(&remaining, escape) {
                    remaining.drain(..close + 2);
                    if discarded_nesting > 0 {
                        discarded_nesting -= 1;
                        continue;
                    }
                    if let Some(scope) = close_collected_scope(&mut frames, start) {
                        finish_scope!(scope);
                    }
                }
            }
            ScannedLine::Control {
                start,
                end,
                no_break: _,
                name,
                arguments,
                argument_start,
                ..
            } => {
                // A scope closer can be appended to a request, most commonly
                // `.br\}`.  Retain the request itself, then close the active
                // scope.  Treating only a standalone `.\}` as a closer lets
                // an outer scope consume subsequent `.el` branches as its
                // body and eventually exposes the opener as ordinary text.
                let close = scope_closer_offset(arguments, escape);
                // In a collected conditional body, an attached scope closer
                // belongs to the scope grammar even when it occurs *inside*
                // a visible man font argument (`.B word\}suffix`).  Replay
                // the package macro with the closer removed, retain the
                // legacy `\&` join, then unwind the scope.  Restricting this
                // recovery to a leading closer loses the middle-of-argument
                // form exercised by regress/roff/cond/if.
                let attached_font_scope_closer =
                    is_man_visible_argument_macro(macro_set, name) && close.is_some();
                let malformed_attached_font_name =
                    attached_font_scope_closer && arguments.starts_with(&[escape, b'}']);
                if malformed_attached_font_name && !innermost_scope_is_statically_inactive(&frames)
                {
                    let mut preview = Vec::with_capacity(name.len().saturating_add(2));
                    preview.extend_from_slice(name);
                    preview.extend_from_slice(&[escape, b'&']);
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_ESCAPED_NAME,
                            Severity::Error,
                            source_id,
                            start,
                            start.saturating_add(1),
                            format!(
                                "escaped character not allowed in a name: {}",
                                visible_bytes(&preview).trim_end()
                            ),
                        ),
                        truncated,
                    );
                }
                if attached_font_scope_closer {
                    let retained_arguments =
                        font_macro_arguments_without_scope_closers(arguments, escape);
                    if discarded_nesting == 0 && !retained_arguments.is_empty() {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Control {
                                start,
                                end,
                                argument_start: argument_start.saturating_add(
                                    if arguments.starts_with(&[escape, b'}']) {
                                        2
                                    } else {
                                        0
                                    },
                                ),
                                name: name.to_vec(),
                                arguments: retained_arguments,
                            });
                    }
                    let mut remaining = arguments;
                    while let Some(close) = scope_closer_offset(remaining, escape) {
                        remaining = &remaining[close + 2..];
                        if discarded_nesting > 0 {
                            discarded_nesting -= 1;
                            continue;
                        }
                        if let Some(scope) = close_collected_scope(&mut frames, start) {
                            finish_scope!(scope);
                        }
                    }
                    continue;
                }
                if emit_definition_tail_diagnostics
                    && name == b"."
                    && let Some(close) = close
                    && !arguments[close + 2..].is_empty()
                {
                    let diagnostic_start = argument_start
                        .checked_add(
                            u32::try_from(close).expect("scope line offsets fit source positions"),
                        )
                        .expect("scope scanner spans are monotonic");
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_ALL_ARGUMENTS,
                            Severity::Error,
                            source_id,
                            diagnostic_start,
                            end,
                            format!(
                                "skipping all arguments: .. \\&{}",
                                visible_bytes(&arguments[close + 2..])
                            ),
                        ),
                        truncated,
                    );
                }
                // A closer attached to an ordinary request ends the enclosing
                // conditional scope, but it is not an argument boundary for
                // that request.  In `.B bold\}tail`, mandoc gives `B` the
                // visible `boldtail` argument before closing the scope. Keep
                // a later closer separate so it can still unwind an outer
                // collected frame.
                let (retained_arguments, attached_suffix_width) = close.map_or_else(
                    || (arguments.to_vec(), 0_usize),
                    |offset| {
                        let suffix = &arguments[offset + 2..];
                        let suffix_width =
                            scope_closer_offset(suffix, escape).unwrap_or(suffix.len());
                        let mut retained = Vec::with_capacity(offset.saturating_add(suffix_width));
                        retained.extend_from_slice(&arguments[..offset]);
                        retained.extend_from_slice(&suffix[..suffix_width]);
                        (retained, suffix_width)
                    },
                );
                let scope_kind =
                    scoped_request_kind(name, &retained_arguments, argument_start, escape, limits);
                if let Some(kind) = scope_kind {
                    if discarded_nesting > 0 {
                        discarded_nesting += 1;
                        continue;
                    }
                    if frames.len() >= limits.max_tree_depth {
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
                                "nested roff scope exceeds max_tree_depth and was skipped",
                            ),
                            truncated,
                        );
                        discarded_nesting = 1;
                        continue;
                    }
                    frames.push(PendingScope {
                        start,
                        end,
                        kind: Some(kind),
                        lines: Vec::new(),
                    });
                } else if discarded_nesting == 0 {
                    frames
                        .last_mut()
                        .expect("scope collector always retains a root frame")
                        .lines
                        .push(ScopeLine::Control {
                            start,
                            end,
                            argument_start,
                            name: name.to_vec(),
                            arguments: retained_arguments,
                        });
                }
                if let Some(close) = close {
                    let discard_line_tail = innermost_scope_is_statically_inactive(&frames);
                    let mut remaining = &arguments[close + 2 + attached_suffix_width..];
                    if discarded_nesting > 0 {
                        discarded_nesting -= 1;
                    } else if let Some(scope) = close_collected_scope(&mut frames, start) {
                        finish_scope!(scope);
                    }
                    while let Some(next_close) = scope_closer_offset(remaining, escape) {
                        if discarded_nesting == 0 && !discard_line_tail && next_close > 0 {
                            frames
                                .last_mut()
                                .expect("scope collector always retains a root frame")
                                .lines
                                .push(ScopeLine::Text {
                                    start,
                                    end,
                                    bytes: remaining[..next_close].to_vec(),
                                    terminal_inline: false,
                                });
                        }
                        remaining = &remaining[next_close + 2..];
                        if discarded_nesting > 0 {
                            discarded_nesting -= 1;
                            continue;
                        }
                        if let Some(mut scope) = close_collected_scope(&mut frames, start) {
                            if !discard_line_tail && !remaining.is_empty() {
                                scope.lines.push(ScopeLine::Text {
                                    start,
                                    end,
                                    bytes: remaining.to_vec(),
                                    terminal_inline: false,
                                });
                            }
                            finish_scope!(scope);
                        }
                    }
                    if discarded_nesting == 0 && !discard_line_tail && !remaining.is_empty() {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Text {
                                start,
                                end,
                                bytes: remaining.to_vec(),
                                terminal_inline: false,
                            });
                    }
                }
            }
            ScannedLine::Text { start, end, bytes } => {
                let first_close = scope_closer_offset(bytes, escape);
                let has_nested_close = first_close.is_some_and(|close| {
                    scope_closer_offset(&bytes[close + 2..], escape).is_some()
                });
                if has_nested_close && frames.len() > 1 {
                    let text = scope_closer_text(bytes, escape);
                    if discarded_nesting == 0 && !text.is_empty() {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Text {
                                start,
                                end,
                                bytes: text,
                                terminal_inline: false,
                            });
                    }
                    let mut remaining = bytes;
                    while let Some(close) = scope_closer_offset(remaining, escape) {
                        remaining = &remaining[close + 2..];
                        if discarded_nesting > 0 {
                            discarded_nesting -= 1;
                            continue;
                        }
                        if let Some(scope) = close_collected_scope(&mut frames, start) {
                            finish_scope!(scope);
                        }
                    }
                    continue;
                }
                let mut remaining = bytes;
                let mut discard_line_tail = false;
                let mut terminal_inline = false;
                while let Some(close) = scope_closer_offset(remaining, escape) {
                    if discarded_nesting == 0 && !discard_line_tail && frames.len() == 1 {
                        // A suffix after the outermost closer was historically
                        // part of this physical body line (for example
                        // `\\n[count]\\},`).  Keep both visible fragments in
                        // one authored text node before ending the scope.
                        let mut retained = Vec::with_capacity(remaining.len().saturating_sub(2));
                        retained.extend_from_slice(&remaining[..close]);
                        let suffix = &remaining[close + 2..];
                        if !suffix.is_empty() {
                            // The legacy tree puts an invisible `\\&` between
                            // the body and an attached suffix, so punctuation
                            // after an inline closer remains source-visible
                            // rather than being folded into the scope marker.
                            retained.extend_from_slice(&[escape, b'&']);
                            retained.extend_from_slice(suffix);
                        }
                        if !retained.is_empty() {
                            frames
                                .last_mut()
                                .expect("scope collector always retains a root frame")
                                .lines
                                .push(ScopeLine::Text {
                                    start,
                                    end,
                                    bytes: retained,
                                    terminal_inline: true,
                                });
                        }
                        finish_scope!(
                            close_collected_scope(&mut frames, start)
                                .expect("the root scope always closes into a result")
                        );
                    }
                    let closes_inactive_scope = innermost_scope_is_statically_inactive(&frames);
                    if discarded_nesting == 0 && !discard_line_tail && close > 0 {
                        frames
                            .last_mut()
                            .expect("scope collector always retains a root frame")
                            .lines
                            .push(ScopeLine::Text {
                                start,
                                end,
                                bytes: remaining[..close].to_vec(),
                                terminal_inline: false,
                            });
                    }
                    remaining = &remaining[close + 2..];
                    terminal_inline = true;
                    if discarded_nesting > 0 {
                        discarded_nesting -= 1;
                        continue;
                    }
                    discard_line_tail |= closes_inactive_scope;
                    if let Some(scope) = close_collected_scope(&mut frames, start) {
                        // The current scanner API owns a physical line once it
                        // has been read.  Retain suffix text after the outer
                        // closer instead of dropping it; future structural
                        // phases can give that suffix its exact sibling role.
                        finish_scope!(scope);
                    }
                }
                if discarded_nesting == 0 && !discard_line_tail && !remaining.is_empty() {
                    frames
                        .last_mut()
                        .expect("scope collector always retains a root frame")
                        .lines
                        .push(ScopeLine::Text {
                            start,
                            end,
                            bytes: remaining.to_vec(),
                            terminal_inline,
                        });
                }
            }
        }
    }
    *truncated = true;
    let incomplete_start = frames.last().map_or(0, |frame| frame.start);
    let incomplete_end = frames.last().map_or(0, |frame| frame.end);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ROFF_UNTERMINATED_SCOPE,
            Severity::Error,
            source_id,
            incomplete_start,
            incomplete_end,
            unterminated_scope_name.map_or_else(
                || "roff scope reached source end before its `\\}` terminator".to_owned(),
                |name| format!("appending missing end of block: {}", visible_bytes(name)),
            ),
        ),
        truncated,
    );
    let scope = CollectedScope {
        lines: frames
            .into_iter()
            .next()
            .expect("scope collector always retains a root frame")
            .lines,
        terminated: false,
        closer_start: None,
    };
    scanner.restore_character_state(character_state);
    scope
}

/// Remove one brace-delimited body from the active copy-reparsed macro frame.
///
/// The main macro executor already stores deferred lines on an explicit LIFO
/// stack.  A macro-local conditional must consume only entries from that same
/// invocation depth: touching a shallower entry would steal the caller's next
/// request.  The returned lines remain in copy mode until the caller selects
/// and pushes them back onto the execution stack.
pub(in crate::parser) fn collect_pending_macro_scope(
    pending: &mut Vec<PendingMacroLine>,
    macro_depth: usize,
    control: u8,
    escape: u8,
    limits: &Limits,
) -> Option<Vec<PendingMacroLine>> {
    let mut lines = Vec::new();
    let mut nested_scopes = 0_usize;
    while pending
        .last()
        .is_some_and(|(_, _, depth, _, _, _)| *depth == macro_depth)
    {
        let mut line = pending.pop().expect("checked pending macro entry");
        let reparsed = copy_mode_reparse(&line.0, escape);
        let Some((request, arguments)) = split_macro_control(&reparsed, control, escape) else {
            lines.push(line);
            continue;
        };
        if is_scope_closer(request, escape) {
            if nested_scopes == 0 {
                return Some(lines);
            }
            nested_scopes -= 1;
            lines.push(line);
            continue;
        }
        if let Some(retained_request) = request.strip_suffix(&[escape, b'}']) {
            let mut retained_line = Vec::with_capacity(
                1 + retained_request.len() + usize::from(!arguments.is_empty()) + arguments.len(),
            );
            retained_line.push(control);
            retained_line.extend_from_slice(retained_request);
            if !arguments.is_empty() {
                retained_line.push(b' ');
                retained_line.extend_from_slice(arguments);
            }
            line.0 = retained_line;
            if nested_scopes == 0 {
                lines.push(line);
                return Some(lines);
            }
            nested_scopes -= 1;
            lines.push(line);
            continue;
        }
        let opens_scope = match request {
            b"while" => lex_arguments(arguments, escape, limits).is_ok_and(|arguments| {
                arguments
                    .split_first()
                    .is_some_and(|(_, body)| is_scope_opener(&join_arguments(body), escape))
            }),
            b"if" | b"ie" => lex_condition_arguments(arguments, escape, limits)
                .ok()
                .and_then(|arguments| {
                    condition_parts(&arguments)
                        .map(|(_, body_start)| join_arguments(&arguments[body_start..]))
                })
                .is_some_and(|body| is_scope_opener(&body, escape)),
            _ => false,
        };
        if opens_scope {
            nested_scopes = nested_scopes.saturating_add(1);
        }
        lines.push(line);
    }
    None
}

pub(in crate::parser) fn close_collected_scope(
    frames: &mut Vec<PendingScope>,
    closer_start: u32,
) -> Option<CollectedScope> {
    let closed = frames
        .pop()
        .expect("scope collector only closes non-empty frame stacks");
    let Some(kind) = closed.kind else {
        return Some(CollectedScope {
            lines: closed.lines,
            terminated: true,
            closer_start: Some(closer_start),
        });
    };
    let (initial_body, kind) = match kind {
        ScopeKind::Loop {
            predicate,
            initial_body,
        } => (
            initial_body,
            ScopeKind::Loop {
                predicate,
                initial_body: None,
            },
        ),
        ScopeKind::Conditional {
            predicate,
            initial_body,
            else_eligible,
        } => (
            initial_body,
            ScopeKind::Conditional {
                predicate,
                initial_body: None,
                else_eligible,
            },
        ),
        ScopeKind::Else { initial_body } => (initial_body, ScopeKind::Else { initial_body: None }),
    };
    let mut lines = closed.lines;
    if let Some((bytes, start)) = initial_body.filter(|(bytes, _)| !bytes.is_empty()) {
        lines.insert(
            0,
            ScopeLine::Text {
                start,
                end: closed.end,
                bytes,
                terminal_inline: false,
            },
        );
    }
    let line = match kind {
        ScopeKind::Loop { predicate, .. } => ScopeLine::Loop {
            start: closed.start,
            end: closed.end,
            predicate,
            lines,
        },
        ScopeKind::Conditional {
            predicate,
            else_eligible,
            ..
        } => ScopeLine::Conditional {
            start: closed.start,
            end: closed.end,
            predicate,
            lines,
            else_eligible,
        },
        ScopeKind::Else { .. } => ScopeLine::Else {
            start: closed.start,
            end: closed.end,
            lines,
        },
    };
    frames
        .last_mut()
        .expect("nested scope has a parent frame")
        .lines
        .push(line);
    None
}

pub(in crate::parser) fn scoped_request_kind(
    name: &[u8],
    arguments: &[u8],
    argument_start: u32,
    escape: u8,
    limits: &Limits,
) -> Option<ScopeKind> {
    match name {
        b"while" => {
            let arguments = lex_arguments(arguments, escape, limits).ok()?;
            let (predicate, body_arguments) = arguments.split_first()?;
            let body_argument = body_arguments.first()?;
            let body = join_arguments(body_arguments);
            scope_opener_remainder(&body, escape).map(|initial_body| ScopeKind::Loop {
                predicate: predicate.bytes.clone(),
                initial_body: (!initial_body.is_empty()).then(|| {
                    let start = argument_start.saturating_add(
                        u32::try_from(body_argument.offset)
                            .expect("scope argument offsets fit source spans"),
                    );
                    (
                        initial_body.to_vec(),
                        scope_remainder_source_start(&body, start, escape),
                    )
                }),
            })
        }
        b"if" | b"ie" => {
            let arguments = lex_condition_arguments(arguments, escape, limits).ok()?;
            let (predicate, body_start) = condition_parts(&arguments)?;
            let body_arguments = &arguments[body_start..];
            let body_argument = body_arguments.first()?;
            let body = join_arguments(body_arguments);
            scope_opener_remainder(&body, escape).map(|initial_body| ScopeKind::Conditional {
                predicate,
                initial_body: (!initial_body.is_empty()).then(|| {
                    let start = argument_start.saturating_add(
                        u32::try_from(body_argument.offset)
                            .expect("scope argument offsets fit source spans"),
                    );
                    (
                        initial_body.to_vec(),
                        scope_remainder_source_start(&body, start, escape),
                    )
                }),
                else_eligible: name == b"ie",
            })
        }
        b"el" => scope_opener_remainder(arguments, escape).map(|initial_body| ScopeKind::Else {
            initial_body: (!initial_body.is_empty()).then(|| {
                (
                    initial_body.to_vec(),
                    scope_remainder_source_start(arguments, argument_start, escape),
                )
            }),
        }),
        _ => None,
    }
}

/// A same-line conditional body is usually raw text, except that a copy-mode
/// definition must remain a control event so the scope executor can collect
/// and install it before the following physical source resumes.
pub(in crate::parser) fn definition_scope_remainder_line(
    bytes: &[u8],
    start: u32,
    end: u32,
    control: u8,
    escape: u8,
) -> ScopeLine {
    let Some((name, arguments)) = split_macro_control(bytes, control, escape) else {
        return ScopeLine::Text {
            start,
            end,
            bytes: bytes.to_vec(),
            terminal_inline: false,
        };
    };
    if matches!(
        name,
        b"de" | b"de1" | b"am" | b"dei" | b"ami" | b"ds" | b"as"
    ) {
        ScopeLine::Control {
            start,
            end,
            argument_start: start
                .saturating_add(1)
                .saturating_add(
                    u32::try_from(name.len()).expect("scope request names fit source spans"),
                )
                .saturating_add(u32::from(!arguments.is_empty())),
            name: name.to_vec(),
            arguments: arguments.to_vec(),
        }
    } else {
        ScopeLine::Text {
            start,
            end,
            bytes: bytes.to_vec(),
            terminal_inline: false,
        }
    }
}

/// Retain only the names that an inactive scope would otherwise have defined.
/// A subsequent invocation is an upstream error rather than a public unknown
/// element, while unrelated unknown requests keep their existing behavior.
pub(in crate::parser) fn record_suppressed_scope_definitions(
    lines: &[ScopeLine],
    escape: u8,
    environment: &mut Environment,
    limits: &Limits,
) {
    for line in lines {
        let ScopeLine::Control {
            name, arguments, ..
        } = line
        else {
            continue;
        };
        if !matches!(name.as_slice(), b"de" | b"de1" | b"am" | b"dei" | b"ami") {
            continue;
        }
        if let Ok(arguments) = lex_arguments(arguments, escape, limits)
            && let Some(name) = arguments.first()
        {
            environment.suppress_macro_name(&name.bytes);
        }
    }
}
