use super::{
    Argument, ArgumentIssue, Diagnostic, DiagnosticCode, DocumentBuilder, Environment, EscapeIssue,
    EscapeIssueKind, Limits, MacroSet, NodeFlags, NodeId, NodeKind, PackageFillScope, Severity,
    SourceSpan, TranslationRequestIssue, decode_visible_bytes, diagnostic,
    invalid_input_byte_offsets, lex_arguments, normalize_ast_escapes, normalize_escapes,
    push_diagnostic, trailing_whitespace_start, translation_request_issue, visible_bytes,
};

pub(super) fn normalize_document_escapes(
    builder: &DocumentBuilder,
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
) -> crate::escape::EscapeResult {
    if builder.macro_set() == MacroSet::None {
        normalize_escapes(bytes, escape, limits)
    } else {
        normalize_ast_escapes(bytes, escape, limits)
    }
}

/// `\\.` is not a comment request: it remains visible text.  libmandoc still
/// flags the following quote as the historical "bad comment style" while
/// retaining that text in the public tree.  Diagnose from raw scanner bytes so
/// escape normalization cannot erase the distinction.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_bad_comment_style(
    bytes: &[u8],
    escape: u8,
    control: u8,
    start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    debug_assert!(is_bad_comment_style(bytes, escape, control));
    let quote_start = start.saturating_add(2);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_BAD_COMMENT_STYLE,
            Severity::Style,
            source_id,
            quote_start,
            quote_start.saturating_add(1),
            "bad comment style",
        ),
        truncated,
    );
}

pub(super) fn is_bad_comment_style(bytes: &[u8], escape: u8, control: u8) -> bool {
    bytes.starts_with(&[escape, control, b'"'])
}

/// Preserve mandoc's diagnostics for exceptional `.tr` request shapes while
/// leaving the executor's pair-to-space recovery unchanged.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_translation_request_diagnostics(
    glyphs: &[u8],
    escape: u8,
    control_start: u32,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    match translation_request_issue(glyphs, escape) {
        Some(TranslationRequestIssue::Empty) => push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_EMPTY_REQUEST,
                Severity::Warning,
                source_id,
                control_start,
                control_start.saturating_add(2),
                "skipping empty request: tr",
            ),
            truncated,
        ),
        Some(TranslationRequestIssue::Odd { start, end }) => {
            let glyph = visible_bytes(&glyphs[start..end]);
            let start = argument_start.saturating_add(u32::try_from(start).unwrap_or(u32::MAX));
            let end = argument_start.saturating_add(u32::try_from(end).unwrap_or(u32::MAX));
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ROFF_ODD_TRANSLATION,
                    Severity::Warning,
                    source_id,
                    start,
                    end,
                    format!("odd number of characters in request: tr {glyph}"),
                ),
                truncated,
            );
        }
        None => {}
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_text_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    flags: NodeFlags,
    text: String,
    limits: &Limits,
    text_bytes: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    append_textual_node(
        builder,
        parent,
        NodeKind::Text,
        source_id,
        start,
        end,
        flags,
        text,
        limits,
        text_bytes,
        diagnostics,
        truncated,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_textual_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    kind: NodeKind,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    flags: NodeFlags,
    text: String,
    limits: &Limits,
    text_bytes: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    let Some(total) = text_bytes.checked_add(text.len()) else {
        *truncated = true;
        return false;
    };
    if total > limits.max_text_bytes {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::LIMIT_TEXT_BYTES,
                Severity::Warning,
                source_id,
                start,
                end,
                "scanner-stage visible text exceeds max_text_bytes and was skipped",
            ),
            truncated,
        );
        return false;
    }
    let Some(node) = append_node(
        builder,
        parent,
        kind,
        source_id,
        start,
        end,
        flags,
        limits,
        diagnostics,
        truncated,
    ) else {
        return false;
    };
    if !builder.text(node, text) {
        *truncated = true;
        return false;
    }
    *text_bytes = total;
    true
}

#[allow(clippy::too_many_arguments)]
pub(super) fn append_node(
    builder: &mut DocumentBuilder,
    parent: NodeId,
    kind: NodeKind,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    flags: NodeFlags,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<NodeId> {
    if builder.node_count() >= limits.max_nodes {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::LIMIT_NODES,
                Severity::Warning,
                source_id,
                start,
                end,
                "scanner-stage AST node count exceeds max_nodes and was truncated",
            ),
            truncated,
        );
        return None;
    }
    let node = builder.push(parent, kind)?;
    let span = SourceSpan::new(source_id, start, end).expect("scanner spans are monotonic");
    if !builder.location(node, span) || !builder.flags(node, flags) {
        *truncated = true;
        return None;
    }
    Some(node)
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_lines)] // Ordered diagnostics need one exhaustive escape taxonomy.
pub(super) fn emit_escape_issues(
    issues: &[EscapeIssue],
    line_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let reverse_unicode_issues = issues.len() > 1
        && issues
            .iter()
            .all(|issue| issue.kind == EscapeIssueKind::UnsupportedUnicode);
    let has_bracket_validation_issues = issues.iter().any(|issue| {
        matches!(
            issue.kind,
            EscapeIssueKind::InvalidBracketAcuteAccent
                | EscapeIssueKind::InvalidBracketGraveAccent
                | EscapeIssueKind::InvalidBracketWhitespaceControl(_)
                | EscapeIssueKind::InvalidBracketIgnoredEscape(_)
        )
    });
    let ordered_issues = if has_bracket_validation_issues {
        issues
            .iter()
            .filter(|issue| {
                matches!(
                    issue.kind,
                    EscapeIssueKind::InvalidBracketAcuteAccent
                        | EscapeIssueKind::InvalidBracketGraveAccent
                        | EscapeIssueKind::InvalidBracketWhitespaceControl(_)
                        | EscapeIssueKind::InvalidBracketIgnoredEscape(_)
                )
            })
            .rev()
            .chain(
                issues
                    .iter()
                    .filter(|issue| {
                        !matches!(
                            issue.kind,
                            EscapeIssueKind::InvalidBracketAcuteAccent
                                | EscapeIssueKind::InvalidBracketGraveAccent
                                | EscapeIssueKind::InvalidBracketWhitespaceControl(_)
                                | EscapeIssueKind::InvalidBracketIgnoredEscape(_)
                        )
                    })
                    .rev(),
            )
            .collect::<Vec<_>>()
    } else if reverse_unicode_issues {
        issues.iter().rev().collect()
    } else {
        issues.iter().collect()
    };
    for issue in ordered_issues {
        // mandoc emits malformed `\[u…]` diagnostics in reverse encounter
        // order for one physical line, while retaining the normal
        // escape-start source anchor for each individual spelling.
        // Environment expansion may consume earlier formatter-size controls
        // before the AST normalizer sees a terminal `\\s-`.  That malformed
        // form is necessarily at the end of its physical source line, so use
        // the retained physical end rather than the post-expansion offset.
        let start = if issue.kind == EscapeIssueKind::InvalidTerminalSize {
            line_end.saturating_sub(issue.length)
        } else {
            line_start.saturating_add(issue.offset).min(line_end)
        };
        let end = start.saturating_add(issue.length).min(line_end).max(start);
        let (code, message) = match issue.kind {
            EscapeIssueKind::Unterminated => (
                DiagnosticCode::ESCAPE_UNTERMINATED,
                issue.spelling.as_deref().map_or_else(
                    || "roff escape is missing required bytes".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::UnknownSpecialCharacter => (
                DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
                "named roff special character is not known by the scanner-stage catalog".to_owned(),
            ),
            EscapeIssueKind::UnknownEscape => (
                DiagnosticCode::ESCAPE_UNKNOWN,
                issue.spelling.as_deref().map_or_else(
                    || "roff escape is not known by the scanner stage".to_owned(),
                    |spelling| format!("undefined escape, printing literally: {spelling}"),
                ),
            ),
            EscapeIssueKind::UnsupportedEscape => {
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ESCAPE_UNKNOWN,
                        Severity::Unsupported,
                        source_id,
                        start,
                        end,
                        issue.spelling.as_deref().map_or_else(
                            || "unsupported roff escape sequence".to_owned(),
                            |spelling| format!("unsupported escape sequence: {spelling}"),
                        ),
                    ),
                    truncated,
                );
                continue;
            }
            EscapeIssueKind::InvalidSyntax => (
                DiagnosticCode::ESCAPE_INVALID,
                issue.spelling.as_deref().map_or_else(
                    || "roff escape uses an invalid syntax shape".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::InvalidBracketIgnoredEscape(control) => (
                DiagnosticCode::ESCAPE_INVALID,
                format!("invalid escape sequence: \\[{}]", char::from(control)),
            ),
            EscapeIssueKind::InvalidTerminalSize => (
                DiagnosticCode::ESCAPE_INVALID,
                issue.spelling.as_deref().map_or_else(
                    || "invalid escape sequence: \\s-".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::LegacyUnicodeEscape => (
                DiagnosticCode::ESCAPE_UNSUPPORTED_UNICODE,
                "undefined escape, printing literally: \\U".to_owned(),
            ),
            EscapeIssueKind::UnsupportedUnicode => (
                DiagnosticCode::ESCAPE_UNSUPPORTED_UNICODE,
                issue.spelling.as_deref().map_or_else(
                    || "legacy Unicode escape is retained but unsupported by mandoc".to_owned(),
                    |spelling| format!("invalid escape sequence: {spelling}"),
                ),
            ),
            EscapeIssueKind::InvalidBracketAcuteAccent => (
                DiagnosticCode::ESCAPE_INVALID,
                "invalid escape sequence: \\[']".to_owned(),
            ),
            EscapeIssueKind::InvalidBracketGraveAccent => (
                DiagnosticCode::ESCAPE_INVALID,
                "invalid escape sequence: \\[`]".to_owned(),
            ),
            EscapeIssueKind::InvalidBracketWhitespaceControl(control) => (
                DiagnosticCode::ESCAPE_INVALID,
                if control == b' ' {
                    "invalid escape sequence: \\[".to_owned()
                } else {
                    format!("invalid escape sequence: \\[{}]", char::from(control))
                },
            ),
            // An escaped string/register reference is deliberate literal
            // input after the execution pass.  Keep the event available to
            // the low-level escape normalizer, but do not invent a public
            // diagnostic that mandoc does not emit for it.
            EscapeIssueKind::DeferredExpansion => continue,
            EscapeIssueKind::ExpansionLimit => (
                DiagnosticCode::ESCAPE_EXPANSION_LIMIT,
                "scanner-stage escape work exceeds max_line_expansion_steps".to_owned(),
            ),
            EscapeIssueKind::OutputLimit => (
                DiagnosticCode::ESCAPE_OUTPUT_LIMIT,
                "scanner-stage visible output exceeds max_expanded_line_bytes".to_owned(),
            ),
        };
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(code, Severity::Warning, source_id, start, end, message),
            truncated,
        );
    }
}

pub(super) fn emit_invalid_input_bytes(
    bytes: &[u8],
    line_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    let offsets = invalid_input_byte_offsets(bytes);
    let has_invalid_input_bytes = !offsets.is_empty();
    for (offset, byte) in offsets {
        let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
        let start = line_start.saturating_add(offset);
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_INVALID_BYTE,
                Severity::Error,
                source_id,
                start,
                start.saturating_add(1),
                format!("skipping bad character: 0x{byte:x}"),
            ),
            truncated,
        );
    }
    has_invalid_input_bytes
}

/// Locate valid UTF-8 runs without treating one malformed span as evidence
/// that the entire physical line was non-Unicode input.
pub(super) fn contains_valid_utf8_non_ascii(mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(text) => return !text.is_ascii(),
            Err(error) => {
                let valid = &bytes[..error.valid_up_to()];
                let text = std::str::from_utf8(valid)
                    .expect("the valid prefix reported by UTF-8 validation is UTF-8");
                if !text.is_ascii() {
                    return true;
                }
                let consumed = error
                    .valid_up_to()
                    .saturating_add(error.error_len().unwrap_or(1));
                bytes = bytes.get(consumed..).unwrap_or_default();
            }
        }
    }
    false
}

/// Reproduce tbl's byte-facing input projection before generic escape
/// normalization can merge a malformed byte with a following ASCII byte.
pub(super) fn legacy_table_input_text(bytes: &[u8]) -> String {
    let mut projected = String::with_capacity(bytes.len());
    let mut remaining = bytes;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                append_legacy_table_utf8(&mut projected, valid);
                return projected;
            }
            Err(error) => {
                let valid = &remaining[..error.valid_up_to()];
                append_legacy_table_utf8(
                    &mut projected,
                    std::str::from_utf8(valid)
                        .expect("the valid prefix reported by UTF-8 validation is UTF-8"),
                );
                let invalid_length = error.error_len().unwrap_or(remaining.len() - valid.len());
                projected.extend(std::iter::repeat_n('?', invalid_length));
                remaining = &remaining[valid.len() + invalid_length..];
            }
        }
    }
}

pub(super) fn append_legacy_table_utf8(projected: &mut String, text: &str) {
    use std::fmt::Write as _;

    for character in text.chars() {
        if character == '\t' {
            projected.push(character);
        } else if character.is_ascii_control() {
            projected.push('?');
        } else if character.is_ascii() {
            projected.push(character);
        } else {
            write!(projected, r"\[u{:04X}]", u32::from(character))
                .expect("writing to a String cannot fail");
        }
    }
}

pub(super) fn emit_trailing_whitespace(
    bytes: &[u8],
    line_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(trailing_start) = trailing_whitespace_start(bytes) else {
        return;
    };
    let offset = if bytes[..trailing_start].ends_with(b"\\\"") {
        bytes.len().saturating_sub(1)
    } else {
        trailing_start
    };
    let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
    let start = line_start.saturating_add(offset);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
            Severity::Style,
            source_id,
            start,
            start.saturating_add(1),
            "whitespace at end of input line",
        ),
        truncated,
    );
}

/// Emit mandoc's style finding for an incomplete control-line quote and keep
/// its recovered token available to package or user-macro execution.
#[allow(clippy::too_many_arguments)] // Keeps ordered quote and tail recovery together.
pub(super) fn emit_unterminated_quoted_argument(
    arguments: &[u8],
    argument_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let quote_offset = arguments
        .iter()
        .enumerate()
        .find_map(|(offset, byte)| {
            (*byte == b'"' && (offset == 0 || arguments[offset - 1].is_ascii_whitespace()))
                .then_some(offset)
        })
        .unwrap_or(0);
    let quote_start = argument_start.saturating_add(
        u32::try_from(quote_offset).expect("bounded control-line offsets fit public spans"),
    );
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
            Severity::Style,
            source_id,
            quote_start,
            line_end,
            "unterminated quoted argument",
        ),
        truncated,
    );
    if trailing_whitespace_start(arguments).is_some() {
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                Severity::Style,
                source_id,
                line_end,
                line_end,
                "whitespace at end of input line",
            ),
            truncated,
        );
    }
}

/// Complete the one malformed quoted token locally after its public finding
/// has been emitted.  The synthetic delimiter is never published in text;
/// it only lets the normal lexer retain the same recovery argument as mandoc.
pub(super) fn recover_unterminated_quoted_arguments(
    arguments: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<Argument>, ArgumentIssue> {
    let mut recovered = Vec::with_capacity(arguments.len().saturating_add(1));
    recovered.extend_from_slice(arguments);
    recovered.push(b'"');
    lex_arguments(&recovered, escape, limits)
}

#[allow(clippy::too_many_arguments)] // Reuses the parser's shared bounded diagnostic context.
pub(super) fn emit_mdoc_control_trailing_whitespace(
    name: &[u8],
    raw_arguments: &[u8],
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    // 空 D1/Dl 的定位由其专用恢复路径处理，避免重复诊断。
    // `.It` uses terminal tabs as a column-cell boundary.  mandoc's mdoc
    // argument grammar consumes that separator rather than issuing the
    // generic end-of-line style warning, including outside a column list.
    if matches!(name, b"D1" | b"Dl" | b"It") || trailing_whitespace_start(raw_arguments).is_none() {
        return;
    }
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
            Severity::Style,
            source_id,
            line_end,
            line_end,
            "whitespace at end of input line",
        ),
        truncated,
    );
}

/// Flag an unseparated trailing delimiter on the mdoc macros that validate it.
///
/// This mirrors the narrow `post_delim_nb()` validator rather than treating
/// every implicit enclosure punctuation mark as a style error.  In particular,
/// a multi-word Pq sentence ending is ordinary prose, not an attached
/// delimiter error.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_mdoc_implicit_trailing_delimiter_spacing(
    name: &[u8],
    raw_arguments: &[u8],
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    const DELIMITER_VALIDATORS: [&[u8]; 10] = [
        b"Aq", b"Ar", b"Brq", b"Bx", b"No", b"Op", b"Pq", b"Ql", b"Qq", b"Sq",
    ];
    if !DELIMITER_VALIDATORS.contains(&name) {
        return;
    }
    let arguments = raw_arguments.trim_ascii();
    let Some((&delimiter, prefix)) = arguments.split_last() else {
        return;
    };
    if !matches!(
        delimiter,
        b',' | b'.' | b';' | b':' | b'!' | b'?' | b')' | b']' | b'|'
    ) || prefix.last().is_none_or(u8::is_ascii_whitespace)
        || mdoc_trailing_delimiter_is_allowed(
            name,
            arguments,
            mdoc_final_argument(arguments),
            delimiter,
        )
    {
        return;
    }
    let (display, has_prior_argument) = mdoc_trailing_delimiter_display(arguments);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::MDOC_TRAILING_DELIMITER_SPACING,
            Severity::Style,
            source_id,
            line_end.saturating_sub(1),
            line_end,
            format!(
                "no blank before trailing delimiter: {}{} {display}",
                decode_visible_bytes(name),
                if has_prior_argument { " ..." } else { "" },
            ),
        ),
        truncated,
    );
}

/// Extract the final mdoc argument as `post_delim_nb()` displays it. Quoted
/// phrases remain one argument; an earlier argument is represented by `...`.
pub(super) fn mdoc_trailing_delimiter_display(arguments: &[u8]) -> (String, bool) {
    let mut index = 0_usize;
    let mut count = 0_usize;
    let mut last = &[][..];
    while index < arguments.len() {
        while arguments.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == arguments.len() {
            break;
        }
        let start = index;
        if arguments[index] == b'"' {
            index += 1;
            let content_start = index;
            while index < arguments.len() {
                if arguments[index] == b'\\' {
                    index = index.saturating_add(2).min(arguments.len());
                    continue;
                }
                index += 1;
                if arguments[index - 1] == b'"' {
                    break;
                }
            }
            let content_end = index.min(arguments.len()).saturating_sub(usize::from(
                arguments.get(index.saturating_sub(1)) == Some(&b'"'),
            ));
            last = &arguments[content_start..content_end];
        } else {
            while index < arguments.len() && !arguments[index].is_ascii_whitespace() {
                index += 1;
            }
            last = &arguments[start..index];
        }
        if !matches!(last, b"(" | b"[") {
            count += 1;
        }
    }
    (String::from_utf8_lossy(last).into_owned(), count > 1)
}

/// Return whether `post_delim_nb()` accepts an otherwise attached delimiter.
pub(super) fn mdoc_trailing_delimiter_is_allowed(
    name: &[u8],
    arguments: &[u8],
    final_argument: &[u8],
    delimiter: u8,
) -> bool {
    let Some((&last, prefix)) = final_argument.split_last() else {
        return true;
    };
    debug_assert_eq!(last, delimiter);

    // A zero-width escape deliberately turns punctuation into authored text.
    if prefix.len() >= 2
        && prefix[prefix.len() - 2] == b'\\'
        && matches!(prefix[prefix.len() - 1], b'&' | b'e')
    {
        return true;
    }

    match delimiter {
        b')' if prefix.contains(&b'(') => return true,
        b'.' if prefix.ends_with(b"..") || prefix.last() == Some(&b'.') => return true,
        b';' if name == b"Vt" => return true,
        b'?' if prefix.last() == Some(&b'?') => return true,
        b']' if prefix.contains(&b'[') => return true,
        b'|' if prefix.len() == 1 && prefix[0] == b'|' => return true,
        _ => {}
    }

    // A two-byte non-word pair has no meaningful delimiter attachment.
    if prefix.len() == 1 && !prefix[0].is_ascii_alphanumeric() {
        return true;
    }

    // The upstream false-positive filter treats a complete multi-word
    // sentence as prose for these four macro families.
    matches!(name, b"Em" | b"Li" | b"Pq" | b"Sy")
        && matches!(delimiter, b'!' | b'.' | b':' | b'?')
        && has_three_trailing_ascii_words(&arguments[..arguments.len().saturating_sub(1)])
}

/// Return the last source argument without applying package-level joining.
/// Quotes stay present because callers only inspect delimiter byte spelling.
pub(super) fn mdoc_final_argument(arguments: &[u8]) -> &[u8] {
    let mut index = 0_usize;
    let mut last = &[][..];
    while index < arguments.len() {
        while arguments.get(index).is_some_and(u8::is_ascii_whitespace) {
            index += 1;
        }
        if index == arguments.len() {
            break;
        }
        let start = index;
        if arguments[index] == b'"' {
            index += 1;
            while index < arguments.len() {
                if arguments[index] == b'\\' {
                    index = index.saturating_add(2);
                    continue;
                }
                index += 1;
                if arguments[index - 1] == b'"' {
                    break;
                }
            }
        } else {
            while index < arguments.len() && !arguments[index].is_ascii_whitespace() {
                index += 1;
            }
        }
        last = &arguments[start..index.min(arguments.len())];
    }
    last
}

/// Match the backwards word walk used by mandoc's delimiter validator.
pub(super) fn has_three_trailing_ascii_words(prefix: &[u8]) -> bool {
    let mut index = prefix.len();
    let mut spaces = 0_usize;
    while index > 0 {
        index -= 1;
        match prefix[index] {
            b' ' => {
                spaces += 1;
                if index > 0 && prefix[index - 1] == b',' {
                    index -= 1;
                }
            }
            byte if byte.is_ascii_alphabetic() => {
                if spaces > 1 {
                    return true;
                }
            }
            _ => return false,
        }
    }
    false
}

#[allow(clippy::too_many_arguments)] // Reuses the parser's shared bounded diagnostic context.
pub(super) fn emit_mdoc_empty_display(
    name: &[u8],
    arguments: &[u8],
    raw_arguments: &[u8],
    control_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    if !matches!(name, b"D1" | b"Dl") || !arguments.is_empty() {
        return;
    }
    if trailing_whitespace_start(raw_arguments).is_some() {
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                Severity::Style,
                source_id,
                line_end.saturating_sub(1),
                line_end,
                "whitespace at end of input line",
            ),
            truncated,
        );
    }
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::MDOC_EMPTY_BLOCK,
            Severity::Warning,
            source_id,
            control_start,
            control_start.saturating_add(2),
            if name == b"D1" {
                "empty block: D1"
            } else {
                "empty block: Dl"
            },
        ),
        truncated,
    );
}

#[allow(clippy::too_many_arguments)] // Reuses the parser's shared bounded diagnostic context.
pub(super) fn emit_man_alternating_font_trailing_whitespace(
    name: &[u8],
    raw_arguments: &[u8],
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    const ALTERNATING_FONT_MACROS: [&[u8]; 6] = [b"BI", b"BR", b"IB", b"IR", b"RB", b"RI"];
    if !ALTERNATING_FONT_MACROS.contains(&name)
        || trailing_whitespace_start(raw_arguments).is_none()
    {
        return;
    }
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TRAILING_WHITESPACE,
            Severity::Style,
            source_id,
            // libmandoc's alternating-font argument parser reports the
            // post-argument cursor (one column after the final space), unlike
            // an empty mdoc `.Dl`, whose finding points at the final byte.
            line_end,
            line_end,
            "whitespace at end of input line",
        ),
        truncated,
    );
}

pub(super) fn emit_filled_text_tabs(
    bytes: &[u8],
    line_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    for (offset, _) in bytes.iter().enumerate().filter(|(_, byte)| **byte == b'\t') {
        let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
        let start = line_start.saturating_add(offset);
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
                Severity::Warning,
                source_id,
                start,
                start.saturating_add(1),
                "tab in filled text",
            ),
            truncated,
        );
    }
}

/// Validate the parser-visible portion of a roff `.ft` request.
///
/// Font selection affects rendering rather than the owned syntax tree, but
/// mandoc still emits request diagnostics.  Keep this at scanner scope so a
/// rejected selection has no accidental AST effect.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_font_request_diagnostics(
    bytes: &[u8],
    escape: u8,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Ok(arguments) = lex_arguments(bytes, escape, limits) else {
        return;
    };
    if arguments.is_empty() {
        return;
    }
    if let Some(excess) = arguments.get(1) {
        let start = argument_start.saturating_add(
            u32::try_from(excess.offset).expect("argument offsets are bounded by line length"),
        );
        let end = start.saturating_add(
            u32::try_from(excess.bytes.len()).expect("argument bytes are bounded by line length"),
        );
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_EXCESS_ARGUMENTS,
                Severity::Error,
                source_id,
                start,
                end,
                format!(
                    "skipping excess arguments: ft ... {}",
                    visible_bytes(&excess.bytes)
                ),
            ),
            truncated,
        );
    }
}

/// The finite font selector catalogue accepted by mandoc's roff validator.
///
/// mdoc applies its copy during structural validation to retain its established
/// recovery ordering; man has no equivalent pass, so scanner recovery uses the
/// same catalogue directly.
pub(super) fn is_legacy_roff_font_selector(font: &[u8]) -> bool {
    matches!(
        font,
        b"C" | b"V"
            | b"B"
            | b"3"
            | b"I"
            | b"2"
            | b"P"
            | b"R"
            | b"1"
            | b"4"
            | b"BI"
            | b"CB"
            | b"CI"
            | b"CR"
            | b"CW"
            | b"VB"
            | b"VI"
    )
}

/// man(7) validates tab input inside visible macro arguments separately from
/// ordinary roff text.  Both a literal tab and the single-byte `\t` escape
/// are layout tabulation in this context; an escaped backslash remains
/// authored text and must not manufacture a warning.
pub(super) fn emit_filled_macro_argument_tabs(
    bytes: &[u8],
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut cursor = 0;
    // The man argument lexer consumes a tabulation escape as one logical
    // argument character, although it occupies two source bytes. Findings
    // for later tabs follow that parser cursor rather than raw byte columns.
    let mut prior_tab_escapes = 0_u32;
    while let Some(byte) = bytes.get(cursor) {
        let offset = match *byte {
            b'\t' => Some(cursor),
            b'\\' if bytes.get(cursor + 1) == Some(&b'\\') => {
                cursor += 2;
                continue;
            }
            b'\\' if bytes.get(cursor + 1) == Some(&b't') => Some(cursor),
            _ => None,
        };
        if let Some(offset) = offset {
            let offset = u32::try_from(offset).expect("scanned line offsets fit public u32 spans");
            let start = argument_start.saturating_add(offset.saturating_sub(prior_tab_escapes));
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
                    Severity::Warning,
                    source_id,
                    start,
                    start.saturating_add(1),
                    "tab in filled text",
                ),
                truncated,
            );
            if *byte == b'\\' {
                prior_tab_escapes = prior_tab_escapes.saturating_add(1);
            }
            cursor += usize::from(*byte == b'\\') + 1;
        } else {
            cursor += 1;
        }
    }
}

/// A direct user-macro invocation treats its first tab as the request
/// separator. A second adjacent tab is filled-text tabulation, and mandoc
/// reports it at the shared post-separator cursor. Package macros own richer
/// argument grammars and are handled by their package-specific validators.
#[allow(clippy::too_many_arguments)] // Shares the parser's source-relative diagnostic boundary.
pub(super) fn emit_user_macro_leading_tabs(
    raw_arguments: &[u8],
    control_start: u32,
    name_len: usize,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    if !raw_arguments.starts_with(b"\t\t") {
        return;
    }
    let name_len = u32::try_from(name_len).expect("scanned request names fit source offsets");
    let start = control_start.saturating_add(name_len).saturating_add(2);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_TAB_IN_FILLED_TEXT,
            Severity::Warning,
            source_id,
            start,
            start.saturating_add(1),
            "tab in filled text",
        ),
        truncated,
    );
}

/// Roff consumes the first tab after a user-macro name as the request
/// separator. Exactly one adjacent tab before visible text remains the first
/// macro argument's prefix; a third tab (or a later horizontal-space run)
/// instead follows the normal argument separator recovery.
pub(super) fn retain_user_macro_tab_argument_prefix(
    arguments: &mut Vec<Argument>,
    raw_arguments: &[u8],
) {
    if !raw_arguments.starts_with(b"\t\t") {
        return;
    }
    let tab_argument = || Argument {
        offset: 1,
        quoted: false,
        separator_after: Some(b'\t'),
        separator_contains_tab: true,
        embedded_tab_count: 1,
        separator_width: 1,
        bytes: vec![b'\t'],
    };
    if raw_arguments.get(2).is_some_and(u8::is_ascii_whitespace) {
        arguments.insert(0, tab_argument());
    } else if let Some(first) = arguments.first_mut() {
        first.bytes.insert(0, b'\t');
    } else {
        arguments.push(tab_argument());
    }
}

/// Implemented man macros whose arguments are visible text rather than pure
/// layout state. Their parser path applies the special tabulation warning.
pub(super) fn is_man_visible_argument_macro(macro_set: MacroSet, name: &[u8]) -> bool {
    macro_set == MacroSet::Man
        && matches!(
            name,
            b"B" | b"I"
                | b"R"
                | b"SM"
                | b"SB"
                | b"BR"
                | b"BI"
                | b"IB"
                | b"IR"
                | b"RB"
                | b"RI"
                | b"IP"
                | b"HP"
                | b"TP"
                | b"TQ"
        )
}

/// A trailing odd escape consumes the physical newline in roff input.  The
/// caller joins only the immediately following text line, leaving any control
/// line available for the ordinary scanner path.
pub(super) fn has_physical_line_continuation(bytes: &[u8], escape: u8) -> bool {
    let trailing_escapes = bytes
        .iter()
        .rev()
        .take_while(|byte| **byte == escape)
        .count();
    trailing_escapes % 2 == 1
}

pub(super) fn update_fill_mode(
    environment: &mut Environment,
    macro_set: MacroSet,
    name: &[u8],
    arguments: &[u8],
) {
    match name {
        b"nf" => environment.no_fill(true),
        b"fi" => environment.no_fill(false),
        b"EX" if macro_set == MacroSet::Man => {
            environment.push_package_fill_scope(PackageFillScope::ManExample, true);
        }
        b"EE" if macro_set == MacroSet::Man => {
            environment.pop_package_fill_scope(PackageFillScope::ManExample);
        }
        b"Bd" if macro_set == MacroSet::Mdoc => {
            let no_fill = arguments
                .split(u8::is_ascii_whitespace)
                .any(|argument| matches!(argument, b"-literal" | b"-unfilled"));
            environment.push_package_fill_scope(PackageFillScope::MdocDisplay, no_fill);
        }
        b"Ed" if macro_set == MacroSet::Mdoc => {
            environment.pop_package_fill_scope(PackageFillScope::MdocDisplay);
        }
        b"Bl" if macro_set == MacroSet::Mdoc => {
            let no_fill = arguments
                .split(u8::is_ascii_whitespace)
                .any(|argument| argument == b"-column");
            environment.push_package_fill_scope(PackageFillScope::MdocList, no_fill);
        }
        b"El" if macro_set == MacroSet::Mdoc => {
            environment.pop_package_fill_scope(PackageFillScope::MdocList);
        }
        _ => {}
    }
}

/// Implemented package request names remain package syntax even when roff
/// copy mode has a user definition with the same name. `mandoc` dispatches
/// these requests to the package validator after syntax selection; it does
/// not let a preceding `.de BI` replace the alternating-font macro. The
/// scanner must make that choice before executing a user macro, otherwise the
/// structural pass sees the generated request instead of the authored node.
///
/// Keep this deliberately limited to implemented package semantics. Unknown
/// names still take the ordinary roff macro path, so document-local helpers
/// remain executable.
pub(super) fn is_builtin_package_macro(macro_set: MacroSet, name: &[u8]) -> bool {
    macro_set == MacroSet::Man
        && matches!(
            name,
            b"TH"
                | b"SH"
                | b"SS"
                | b"TP"
                | b"TQ"
                | b"LP"
                | b"PP"
                | b"P"
                | b"IP"
                | b"HP"
                | b"RS"
                | b"RE"
                | b"UR"
                | b"UE"
                | b"MT"
                | b"ME"
                | b"SY"
                | b"YS"
                | b"SM"
                | b"SB"
                | b"R"
                | b"B"
                | b"I"
                | b"BR"
                | b"BI"
                | b"IB"
                | b"IR"
                | b"RB"
                | b"RI"
                | b"EX"
                | b"EE"
                | b"nf"
                | b"fi"
                | b"ce"
                | b"rj"
                | b"PD"
                | b"in"
                | b"br"
                | b"sp"
                | b"na"
                | b"ad"
                | b"nh"
                | b"hy"
        )
        // `At` already has a validator-defined default word and `Bc` closes
        // an mdoc `Bo` block. Preserve their package dispatch even when roff
        // has a same-named user definition; otherwise `.am Bc` turns the
        // authored closer into macro output and leaves the `Bo` unclosed.
        || (macro_set == MacroSet::Mdoc && matches!(name, b"At" | b"Bc"))
}
