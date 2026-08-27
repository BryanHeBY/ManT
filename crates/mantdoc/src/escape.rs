//! Left-to-right visible roff escape normalization for scanner-stage text.

use crate::Limits;

/// A recoverable escape finding with a byte offset relative to its input line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EscapeIssue {
    /// Zero-based byte offset of the escape introducer.
    pub(crate) offset: u32,
    /// Number of source bytes involved in the malformed or deferred construct.
    pub(crate) length: u32,
    /// Stable scanner-stage category.
    pub(crate) kind: EscapeIssueKind,
    /// 原始拼写；仅用于必须逐字匹配的兼容诊断。
    pub(crate) spelling: Option<Box<str>>,
}

/// Stable categories emitted while normalizing scanner-stage escapes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EscapeIssueKind {
    /// An escape opener did not contain the required following bytes.
    Unterminated,
    /// A named special character was not in the M2 normalization subset.
    UnknownSpecialCharacter,
    /// An escape is syntactically unknown at this scanner stage.
    UnknownEscape,
    /// A recognized legacy escape is deliberately unsupported by mandoc.
    UnsupportedEscape,
    /// A recognized escape used a delimiter that its grammar forbids.
    InvalidSyntax,
    /// A bracketed spelling was used for an ignored one-byte escape.
    InvalidBracketIgnoredEscape(u8),
    /// A terminal signed size escape has no size operand.
    InvalidTerminalSize,
    /// Legacy `\\U` Unicode escape retained for compatibility with mandoc's AST.
    LegacyUnicodeEscape,
    /// A numeric Unicode character name cannot represent a valid scalar value.
    UnsupportedUnicode,
    /// Legacy bracket spelling for an acute accent is syntactically invalid.
    InvalidBracketAcuteAccent,
    /// Legacy bracket spelling for a grave accent is syntactically invalid.
    InvalidBracketGraveAccent,
    /// A bracketed spelling was used for a single-byte whitespace control.
    InvalidBracketWhitespaceControl(u8),
    /// String/register expansion is deferred to the M3 roff execution environment.
    DeferredExpansion,
    /// Visible expansion exceeded a deterministic per-line work bound.
    ExpansionLimit,
    /// Visible output exceeded the deterministic per-line output bound.
    OutputLimit,
}

/// Visible text and recovery details for one physical source line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct EscapeResult {
    /// Normalized visible text with invalid source bytes preserved without replacement.
    pub(crate) text: String,
    /// Whether an unescaped final `\c` suppresses the normal line break.
    pub(crate) line_continuation: bool,
    /// Recoverable scanner-stage issues in source order.
    pub(crate) issues: Vec<EscapeIssue>,
    /// Number of byte/escape dispatch steps consumed before completion or a limit.
    pub(crate) steps: usize,
    /// Whether a configured deterministic bound cut this line's visible result short.
    pub(crate) truncated: bool,
}

/// A known named-character result before its source spelling is normalized.
///
/// This deliberately distinguishes an absent catalog entry from a known
/// formatter-only entry, without encoding that distinction as nested options.
enum SpecialCharacterLookup {
    Visible(char),
    ZeroWidth,
}

/// Normalize the scanner-stage visible subset of roff escapes.
#[allow(clippy::too_many_lines)] // Keep all byte-consumption cases adjacent for auditability.
pub(crate) fn normalize_escapes(bytes: &[u8], escape: u8, limits: &Limits) -> EscapeResult {
    normalize_escapes_with_projection(bytes, escape, limits, false)
}

/// Normalize escapes while retaining formatter spellings observable in a
/// package AST.  The raw roff execution view intentionally remains narrower:
/// it removes zero-width controls before its M3 counters are recorded.
pub(crate) fn normalize_ast_escapes(bytes: &[u8], escape: u8, limits: &Limits) -> EscapeResult {
    normalize_escapes_with_projection(bytes, escape, limits, true)
}

#[allow(clippy::too_many_lines)] // Keep all byte-consumption cases adjacent for auditability.
fn normalize_escapes_with_projection(
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
    retain_formatter_spelling: bool,
) -> EscapeResult {
    let mut visible = Vec::with_capacity(bytes.len());
    let mut issues = Vec::new();
    let mut cursor = 0;
    let mut steps = 0_usize;
    let mut line_continuation = false;
    let mut truncated = false;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if retain_formatter_spelling
            && byte == 0xc2
            && let Some(control) = bytes.get(cursor + 1).copied().filter(|byte| *byte <= 0x9f)
        {
            let spelling = format!("\\[u00{control:02X}]");
            if !push_bytes(
                &mut visible,
                spelling.as_bytes(),
                limits.max_expanded_line_bytes,
            ) {
                push_issue(&mut issues, cursor, 2, EscapeIssueKind::OutputLimit);
                truncated = true;
                break;
            }
            cursor += 2;
            continue;
        }
        if byte != escape {
            if !push_bytes(&mut visible, &[byte], limits.max_expanded_line_bytes) {
                push_issue(&mut issues, cursor, 1, EscapeIssueKind::OutputLimit);
                truncated = true;
                break;
            }
            cursor += 1;
            continue;
        }
        steps = steps.saturating_add(1);
        if steps > limits.max_line_expansion_steps {
            push_issue(&mut issues, cursor, 0, EscapeIssueKind::ExpansionLimit);
            truncated = true;
            break;
        }
        let escape_start = cursor;
        cursor += 1;
        let Some(code) = bytes.get(cursor).copied() else {
            push_issue(&mut issues, escape_start, 1, EscapeIssueKind::Unterminated);
            if !push_bytes(&mut visible, &[escape], limits.max_expanded_line_bytes) {
                truncated = true;
            }
            break;
        };
        cursor += 1;
        let status = match code {
            // `\\e` represents a backslash to the execution engine, but the
            // package AST is expected to preserve the authored formatter
            // spelling just like the other source-visible escapes.
            // Mandoc's no-argument, ignored escapes stay observable in the
            // package AST as source spelling.  They are formatter controls,
            // not unknown escapes.  The raw roff execution view consumes
            // them before it records semantic text.
            b' ' | b'\'' | b'-' | b'.' | b'0' | b':' | b'_' | b'`' | b'~' | b'\\' | b'e' | b'p'
            | b'%' | b'&' | b')' | b',' | b'/' | b'^' | b'a' | b'd' | b'r' | b't' | b'u' | b'{'
            | b'|' | b'}' | b'c'
                if retain_formatter_spelling =>
            {
                line_continuation |= code == b'c' && cursor == bytes.len();
                push_bytes(
                    &mut visible,
                    &[escape, code],
                    limits.max_expanded_line_bytes,
                )
            }
            b'\\' | b'e' => push_bytes(&mut visible, &[escape], limits.max_expanded_line_bytes),
            // mandoc accepts these single-byte spellings as special
            // characters.  Their package-AST spelling was retained above;
            // the execution projection can resolve the same full catalog as
            // `\\(...)` and `\\[...]` forms.
            b' ' | b'\'' | b'.' | b'0' | b':' | b'_' | b'`' | b'~' => normalize_special(
                &[code],
                &mut visible,
                &mut issues,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            // Conditional-scope delimiters and formatter-only controls are
            // removed only from the raw roff execution view.
            b'p' | b'%' | b'&' | b')' | b',' | b'/' | b'^' | b'a' | b'd' | b'r' | b't' | b'u'
            | b'{' | b'|' | b'}' => true,
            b'-' | b'N' => push_bytes(
                &mut visible,
                &[escape, code],
                limits.max_expanded_line_bytes,
            ),
            b'c' => {
                line_continuation |= cursor == bytes.len();
                true
            }
            // `\z` is a zero-width formatter escape.  man/mdoc retain both
            // its authored spelling and the one atom it applies to; notably,
            // a following `\c` is data inside the zero-width construct, not
            // a physical-line continuation of its own.
            b'z' if retain_formatter_spelling => {
                let trailing_no_space = bytes.get(cursor) == Some(&escape)
                    && bytes.get(cursor.saturating_add(1)) == Some(&b'c')
                    && cursor.saturating_add(2) == bytes.len();
                let status = retain_zero_width_escape(
                    bytes,
                    &mut cursor,
                    &mut visible,
                    escape,
                    escape_start,
                    limits.max_expanded_line_bytes,
                );
                line_continuation |= trailing_no_space;
                status
            }
            b'"' => break,
            b'(' if retain_formatter_spelling => retain_two_character_special(
                bytes,
                &mut cursor,
                &mut visible,
                &mut issues,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            b'(' => normalize_two_character_special(
                bytes,
                &mut cursor,
                &mut visible,
                &mut issues,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            b'[' if retain_formatter_spelling => retain_bracket_special(
                bytes,
                &mut cursor,
                &mut visible,
                &mut issues,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            b'[' => normalize_bracket_special(
                bytes,
                &mut cursor,
                &mut visible,
                &mut issues,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            // `man` and `mdoc` expose their source-level inline formatting in
            // text nodes.  Keep it in the public AST projection, while the
            // raw roff execution view still consumes it as a formatter-only
            // control.  This covers both the one-character and bracketed
            // argument spellings accepted by groff and mandoc.
            b's' if retain_formatter_spelling
                && bytes.get(cursor) == Some(&b'-')
                && cursor.saturating_add(1) == bytes.len() =>
            {
                retain_invalid_terminal_size(
                    bytes,
                    &mut cursor,
                    &mut visible,
                    &mut issues,
                    escape_start,
                    limits.max_expanded_line_bytes,
                )
            }
            b'f' | b's' if retain_formatter_spelling => retain_format_argument(
                bytes,
                &mut cursor,
                &mut visible,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            b'f' | b's' => {
                consume_format_argument(bytes, &mut cursor);
                true
            }
            // These register-formatting and horizontal-position escapes have
            // no visible AST effect, but their argument grammars must remain
            // opaque so nested formatter spelling is not misdiagnosed as an
            // unrelated unknown escape.
            b'k' if retain_formatter_spelling => retain_name_argument(
                bytes,
                &mut cursor,
                &mut visible,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            b'k' => {
                consume_name_argument(bytes, &mut cursor);
                true
            }
            b'R' | b'C' | b'o' if retain_formatter_spelling => retain_delimited_escape(
                bytes,
                &mut cursor,
                &mut visible,
                escape_start,
                limits.max_expanded_line_bytes,
            ),
            // These formatter escapes take delimiter-bounded arguments.  The
            // package AST retains their spelling, but their grammar must be
            // consumed as one escape so valid nested text never falls through
            // to the unknown-escape recovery path.
            b'B' | b'w' if retain_formatter_spelling => retain_checked_delimited_escape(
                bytes,
                &mut cursor,
                &mut visible,
                &mut issues,
                escape_start,
                limits.max_expanded_line_bytes,
                false,
            ),
            b'h' | b'H' | b'L' | b'l' | b'S' | b'v' | b'x' if retain_formatter_spelling => {
                retain_checked_delimited_escape(
                    bytes,
                    &mut cursor,
                    &mut visible,
                    &mut issues,
                    escape_start,
                    limits.max_expanded_line_bytes,
                    true,
                )
            }
            // Overstriking changes renderer presentation but has no separate
            // scanner-stage text representation.  Consume its arbitrary
            // delimiter-bounded argument so it never becomes an unknown
            // escape in the raw execution view.
            b'R' | b'o' => {
                consume_delimited_escape(bytes, &mut cursor);
                true
            }
            // Mandoc's legacy Unicode escape requires its single-quote
            // delimiter. Without it, `\\U` is an ordinary undefined escape
            // and must not consume a following bracketed spelling.
            b'U' if retain_formatter_spelling && bytes.get(cursor) == Some(&b'\'') => {
                let status = retain_delimited_escape(
                    bytes,
                    &mut cursor,
                    &mut visible,
                    escape_start,
                    limits.max_expanded_line_bytes,
                );
                push_issue(
                    &mut issues,
                    escape_start,
                    cursor.saturating_sub(escape_start),
                    EscapeIssueKind::LegacyUnicodeEscape,
                );
                status
            }
            b'*' if retain_formatter_spelling && is_default_device_string(bytes, cursor) => {
                consume_name_argument(bytes, &mut cursor);
                push_bytes(
                    &mut visible,
                    &bytes[escape_start..cursor],
                    limits.max_expanded_line_bytes,
                )
            }
            b'*' | b'n' => {
                consume_name_argument(bytes, &mut cursor);
                push_issue(
                    &mut issues,
                    escape_start,
                    cursor.saturating_sub(escape_start),
                    EscapeIssueKind::DeferredExpansion,
                );
                true
            }
            b'O' => {
                let argument_start = cursor;
                let (kind, end) = match bytes.get(cursor).copied() {
                    Some(b'1'..=b'4') => (None, cursor.saturating_add(1)),
                    Some(b'0') => (
                        Some(EscapeIssueKind::UnsupportedEscape),
                        cursor.saturating_add(1),
                    ),
                    Some(b'5' | b'6') => (
                        Some(EscapeIssueKind::InvalidSyntax),
                        cursor.saturating_add(1),
                    ),
                    Some(b'(') => (
                        Some(EscapeIssueKind::InvalidSyntax),
                        cursor.saturating_add(3).min(bytes.len()),
                    ),
                    Some(b'[') => {
                        let end = bytes[cursor.saturating_add(1)..]
                            .iter()
                            .position(|byte| *byte == b']')
                            .map_or(bytes.len(), |relative| {
                                cursor.saturating_add(relative).saturating_add(2)
                            });
                        (Some(EscapeIssueKind::UnsupportedEscape), end)
                    }
                    _ => (Some(EscapeIssueKind::UnknownEscape), argument_start),
                };
                cursor = end;
                if let Some(kind) = kind {
                    push_issue_with_spelling(
                        &mut issues,
                        escape_start,
                        cursor.saturating_sub(escape_start),
                        kind,
                        &bytes[escape_start..cursor],
                    );
                }
                push_bytes(
                    &mut visible,
                    &bytes[escape_start..cursor],
                    limits.max_expanded_line_bytes,
                )
            }
            b'!' | b'?' => {
                push_issue_with_spelling(
                    &mut issues,
                    escape_start,
                    cursor.saturating_sub(escape_start),
                    EscapeIssueKind::UnsupportedEscape,
                    &bytes[escape_start..cursor],
                );
                push_bytes(
                    &mut visible,
                    &[escape, code],
                    limits.max_expanded_line_bytes,
                )
            }
            _ => {
                push_issue_with_spelling(
                    &mut issues,
                    escape_start,
                    cursor.saturating_sub(escape_start),
                    EscapeIssueKind::UnknownEscape,
                    &bytes[escape_start..cursor],
                );
                push_bytes(
                    &mut visible,
                    &[escape, code],
                    limits.max_expanded_line_bytes,
                )
            }
        };
        if !status {
            push_issue(
                &mut issues,
                escape_start,
                cursor.saturating_sub(escape_start),
                EscapeIssueKind::OutputLimit,
            );
            truncated = true;
            break;
        }
    }
    // Package ASTs preserve source-significant trailing horizontal space.
    // Besides allowing diagnostics to point at the original byte range, this
    // is observable in man(7) automatic-tag priority: `plain` is strong,
    // while `plain ` is deliberately weak.  The renderer decides separately
    // whether that space affects output layout.
    EscapeResult {
        text: decode_visible_bytes(&visible),
        line_continuation,
        issues,
        steps,
        truncated,
    }
}

fn retain_two_character_special(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    issues: &mut Vec<EscapeIssue>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    let end = cursor.saturating_add(2).min(bytes.len());
    if end - *cursor != 2 {
        push_issue(
            issues,
            escape_start,
            end.saturating_sub(escape_start),
            EscapeIssueKind::Unterminated,
        );
    }
    *cursor = end;
    push_bytes(visible, &bytes[escape_start..end], maximum)
}

fn retain_bracket_special(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    issues: &mut Vec<EscapeIssue>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    let name_start = *cursor;
    let Some(close_offset) = bytes[name_start..].iter().position(|byte| *byte == b']') else {
        push_issue(
            issues,
            escape_start,
            bytes.len().saturating_sub(escape_start),
            EscapeIssueKind::Unterminated,
        );
        *cursor = bytes.len();
        return push_bytes(visible, &bytes[escape_start..], maximum);
    };
    let end = name_start + close_offset + 1;
    *cursor = end;
    let name = &bytes[name_start..end - 1];
    let issue = if invalid_unicode_character_name(name) {
        Some(EscapeIssueKind::UnsupportedUnicode)
    } else if name == b"'" {
        Some(EscapeIssueKind::InvalidBracketAcuteAccent)
    } else if name == b"`" {
        Some(EscapeIssueKind::InvalidBracketGraveAccent)
    } else {
        invalid_bracket_whitespace_control(name)
            .map(EscapeIssueKind::InvalidBracketWhitespaceControl)
            .or_else(|| {
                if matches!(
                    name,
                    b")" | b","
                        | b"!"
                        | b"?"
                        | b"/"
                        | b"+"
                        | b";"
                        | b"<"
                        | b"="
                        | b">"
                        | b"@"
                        | b"{"
                        | b"}"
                        | b"1"
                        | b"G"
                        | b"I"
                        | b"i"
                        | b"J"
                        | b"j"
                        | b"K"
                        | b"P"
                        | b"Q"
                        | b"q"
                        | b"T"
                        | b"U"
                        | b"W"
                        | b"y"
                ) {
                    name.first()
                        .copied()
                        .map(EscapeIssueKind::InvalidBracketIgnoredEscape)
                } else {
                    None
                }
            })
    };
    if let Some(issue) = issue {
        let length = end.saturating_sub(escape_start);
        if issue == EscapeIssueKind::UnsupportedUnicode {
            push_issue_with_spelling(
                issues,
                escape_start,
                length,
                issue,
                &bytes[escape_start..end],
            );
        } else {
            push_issue(issues, escape_start, length, issue);
        }
    }
    push_bytes(visible, &bytes[escape_start..end], maximum)
}

/// 方括号形式不能用于这些单字节空白控制；首字符为空格时，上游只报告 `\[`。
fn invalid_bracket_whitespace_control(name: &[u8]) -> Option<u8> {
    if name.first() == Some(&b' ') {
        return Some(b' ');
    }
    name.first()
        .copied()
        .filter(|_| matches!(name, b"%" | b"&" | b":" | b"^" | b"_" | b"|" | b"~" | b"0"))
}

fn invalid_unicode_character_name(name: &[u8]) -> bool {
    let Some(hex) = name.strip_prefix(b"u") else {
        return false;
    };
    let hexadecimal_length = hex
        .iter()
        .take_while(|byte| byte.is_ascii_hexdigit())
        .count();
    if hexadecimal_length == 0 {
        return false;
    }
    // mandoc accepts a canonical Unicode scalar spelling: four digits for
    // BMP values, then the shortest possible hexadecimal form. A non-hex
    // suffix is invalid only after a numeric Unicode prefix was established.
    let has_suffix = hexadecimal_length != hex.len();
    let hex = &hex[..hexadecimal_length];
    if hexadecimal_length < 4 {
        return hex.first().is_some_and(u8::is_ascii_digit);
    }
    let Ok(value) = u32::from_str_radix(std::str::from_utf8(hex).unwrap_or_default(), 16) else {
        return true;
    };
    let canonical_length = if value <= 0xffff {
        4
    } else {
        format!("{value:X}").len()
    };
    has_suffix || hexadecimal_length != canonical_length || char::from_u32(value).is_none()
}

fn retain_format_argument(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    consume_format_argument(bytes, cursor);
    push_bytes(visible, &bytes[escape_start..*cursor], maximum)
}

/// Preserve a source-spelled terminal `\\s-` but retain its exact malformed
/// shape for the parser's legacy diagnostic projection.
fn retain_invalid_terminal_size(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    issues: &mut Vec<EscapeIssue>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    *cursor = cursor.saturating_add(1).min(bytes.len());
    push_issue_with_spelling(
        issues,
        escape_start,
        cursor.saturating_sub(escape_start),
        EscapeIssueKind::InvalidTerminalSize,
        &bytes[escape_start..*cursor],
    );
    push_bytes(visible, &bytes[escape_start..*cursor], maximum)
}

/// Preserve a formatter-only name escape in package AST text while consuming
/// all of its name grammar as one scanner atom.
fn retain_name_argument(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    consume_name_argument(bytes, cursor);
    push_bytes(visible, &bytes[escape_start..*cursor], maximum)
}

/// Retain a package-AST `\z` plus the one roff atom it governs.  The governed
/// atom may itself be an escape with a bounded argument spelling, in which
/// case it must be copied as one unit rather than re-entering normal escape
/// execution.
fn retain_zero_width_escape(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    escape: u8,
    escape_start: usize,
    maximum: usize,
) -> bool {
    if !push_bytes(visible, &bytes[escape_start..*cursor], maximum) {
        return false;
    }
    let atom_start = *cursor;
    if bytes.get(*cursor) != Some(&escape) {
        return true;
    }
    *cursor += 1;
    let Some(code) = bytes.get(*cursor).copied() else {
        return push_bytes(visible, &bytes[atom_start..*cursor], maximum);
    };
    *cursor += 1;
    match code {
        b'(' => *cursor = cursor.saturating_add(2).min(bytes.len()),
        b'[' | b'f' | b's' => consume_format_argument(bytes, cursor),
        b'*' | b'n' => consume_name_argument(bytes, cursor),
        b'B' | b'C' | b'h' | b'H' | b'L' | b'l' | b'o' | b'S' | b'v' | b'w' | b'x' => {
            consume_delimited_escape(bytes, cursor);
        }
        _ => {}
    }
    push_bytes(visible, &bytes[atom_start..*cursor], maximum)
}

fn retain_delimited_escape(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    consume_delimited_escape(bytes, cursor);
    push_bytes(visible, &bytes[escape_start..*cursor], maximum)
}

/// Retain a formatter escape with a required delimiter while reporting only
/// the malformed shape libmandoc itself diagnoses.  The public AST remains a
/// source projection, so even malformed input is copied verbatim.
#[allow(clippy::too_many_arguments)]
fn retain_checked_delimited_escape(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    issues: &mut Vec<EscapeIssue>,
    escape_start: usize,
    maximum: usize,
    movement_escape: bool,
) -> bool {
    let Some(delimiter) = bytes.get(*cursor).copied() else {
        push_issue_with_spelling(
            issues,
            escape_start,
            cursor.saturating_sub(escape_start),
            EscapeIssueKind::Unterminated,
            &bytes[escape_start..*cursor],
        );
        return push_bytes(visible, &bytes[escape_start..*cursor], maximum);
    };
    if movement_escape && b" %&()*+-./0123456789:<=>".contains(&delimiter) {
        let end = cursor.saturating_add(1).min(bytes.len());
        push_issue_with_spelling(
            issues,
            escape_start,
            end.saturating_sub(escape_start),
            EscapeIssueKind::InvalidSyntax,
            &bytes[escape_start..end],
        );
        return push_bytes(visible, &bytes[escape_start..*cursor], maximum);
    }

    *cursor += 1;
    while *cursor < bytes.len() && bytes[*cursor] != delimiter {
        *cursor += 1;
    }
    if *cursor == bytes.len() {
        push_issue_with_spelling(
            issues,
            escape_start,
            cursor.saturating_sub(escape_start),
            EscapeIssueKind::Unterminated,
            &bytes[escape_start..*cursor],
        );
    } else {
        *cursor += 1;
    }
    push_bytes(visible, &bytes[escape_start..*cursor], maximum)
}

fn consume_delimited_escape(bytes: &[u8], cursor: &mut usize) {
    let Some(delimiter) = bytes.get(*cursor).copied() else {
        return;
    };
    *cursor += 1;
    while *cursor < bytes.len() && bytes[*cursor] != delimiter {
        *cursor += 1;
    }
    if *cursor < bytes.len() {
        *cursor += 1;
    }
}

fn normalize_two_character_special(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    issues: &mut Vec<EscapeIssue>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    let Some(name) = bytes.get(*cursor..cursor.saturating_add(2)) else {
        push_issue(
            issues,
            escape_start,
            cursor.saturating_sub(escape_start),
            EscapeIssueKind::Unterminated,
        );
        return true;
    };
    *cursor += 2;
    normalize_special(name, visible, issues, escape_start, maximum)
}

fn normalize_bracket_special(
    bytes: &[u8],
    cursor: &mut usize,
    visible: &mut Vec<u8>,
    issues: &mut Vec<EscapeIssue>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    let name_start = *cursor;
    let Some(close_offset) = bytes[name_start..].iter().position(|byte| *byte == b']') else {
        push_issue(
            issues,
            escape_start,
            bytes.len().saturating_sub(escape_start),
            EscapeIssueKind::Unterminated,
        );
        *cursor = bytes.len();
        return true;
    };
    let close = name_start + close_offset;
    let name = &bytes[name_start..close];
    *cursor = close + 1;
    // The execution and rendering projection drops malformed Unicode scalar
    // names entirely.  The package-AST path above retains their spelling for
    // diagnostics, but accepting a non-canonical form such as `u0002B` here
    // would incorrectly render it as `+`.
    if invalid_unicode_character_name(name) {
        // Keep the execution-view finding in the existing generic category:
        // source-package parsing emits the typed Unicode diagnostic through
        // `retain_bracket_special`, while raw roff consumers historically
        // receive an unknown-special recovery here.
        push_issue(
            issues,
            escape_start,
            close.saturating_add(1).saturating_sub(escape_start),
            EscapeIssueKind::UnknownSpecialCharacter,
        );
        return true;
    }
    normalize_special(name, visible, issues, escape_start, maximum)
}

fn normalize_special(
    name: &[u8],
    visible: &mut Vec<u8>,
    issues: &mut Vec<EscapeIssue>,
    escape_start: usize,
    maximum: usize,
) -> bool {
    match numeric_special_character(name)
        .map(SpecialCharacterLookup::Visible)
        .or_else(|| catalog_special_character(name))
    {
        Some(SpecialCharacterLookup::Visible(character)) => {
            let mut encoded = [0_u8; 4];
            push_bytes(
                visible,
                character.encode_utf8(&mut encoded).as_bytes(),
                maximum,
            )
        }
        Some(SpecialCharacterLookup::ZeroWidth) => true,
        None => {
            push_issue(
                issues,
                escape_start,
                name.len().saturating_add(3),
                EscapeIssueKind::UnknownSpecialCharacter,
            );
            true
        }
    }
}

/// Decode roff's explicitly scalar special names without any host encoding
/// dependency. `u` accepts the groff/mandoc 4--6 hexadecimal-digit spelling;
/// `char` accepts the portable printable Latin-1 decimal form.
fn numeric_special_character(name: &[u8]) -> Option<char> {
    if let Some(hexadecimal) = name.strip_prefix(b"u") {
        if !(4..=6).contains(&hexadecimal.len()) {
            return None;
        }
        let value = hexadecimal.iter().try_fold(0_u32, |value, digit| {
            let digit = match digit {
                b'0'..=b'9' => u32::from(digit - b'0'),
                b'a'..=b'f' => u32::from(digit - b'a') + 10,
                b'A'..=b'F' => u32::from(digit - b'A') + 10,
                _ => return None,
            };
            value.checked_mul(16)?.checked_add(digit)
        })?;
        return char::from_u32(value);
    }
    let decimal = name.strip_prefix(b"char")?;
    if !(2..=3).contains(&decimal.len()) || !decimal.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let value = decimal
        .iter()
        .fold(0_u32, |value, digit| value * 10 + u32::from(digit - b'0'));
    ((0x21..=0x7e).contains(&value) || (0xa0..=0xff).contains(&value))
        .then(|| char::from_u32(value))
        .flatten()
}

fn catalog_special_character(name: &[u8]) -> Option<SpecialCharacterLookup> {
    let name = std::str::from_utf8(name).ok()?;
    match crate::special_character(name) {
        Some(crate::SpecialCharacter::Visible(character)) => {
            Some(SpecialCharacterLookup::Visible(character))
        }
        Some(crate::SpecialCharacter::ZeroWidth) => Some(SpecialCharacterLookup::ZeroWidth),
        None => None,
    }
}

fn consume_format_argument(bytes: &[u8], cursor: &mut usize) {
    if bytes.get(*cursor) == Some(&b'[') {
        *cursor += 1;
        while *cursor < bytes.len() && bytes[*cursor] != b']' {
            *cursor += 1;
        }
        if *cursor < bytes.len() {
            *cursor += 1;
        }
    } else if *cursor < bytes.len() {
        *cursor += 1;
    }
}

fn consume_name_argument(bytes: &[u8], cursor: &mut usize) {
    if bytes.get(*cursor) == Some(&b'[') {
        consume_format_argument(bytes, cursor);
    } else {
        *cursor = cursor.saturating_add(2).min(bytes.len());
    }
}

/// The formatter-default device string is semantically defined but remains
/// source-visible in mandoc's package AST when no user `.ds .T` overrides it.
fn is_default_device_string(bytes: &[u8], cursor: usize) -> bool {
    bytes[cursor..].starts_with(b"(.T") || bytes[cursor..].starts_with(b"[.T]")
}

fn push_bytes(visible: &mut Vec<u8>, bytes: &[u8], maximum: usize) -> bool {
    let Some(length) = visible.len().checked_add(bytes.len()) else {
        return false;
    };
    if length > maximum {
        return false;
    }
    visible.extend_from_slice(bytes);
    true
}

fn push_issue(issues: &mut Vec<EscapeIssue>, offset: usize, length: usize, kind: EscapeIssueKind) {
    let offset = u32::try_from(offset).expect("source offsets are checked before scanning");
    let length = u32::try_from(length).expect("line length is bounded by public u32 spans");
    issues.push(EscapeIssue {
        offset,
        length,
        kind,
        spelling: None,
    });
}

fn push_issue_with_spelling(
    issues: &mut Vec<EscapeIssue>,
    offset: usize,
    length: usize,
    kind: EscapeIssueKind,
    spelling: &[u8],
) {
    let offset = u32::try_from(offset).expect("source offsets are checked before scanning");
    let length = u32::try_from(length).expect("line length is bounded by public u32 spans");
    issues.push(EscapeIssue {
        offset,
        length,
        kind,
        spelling: Some(
            String::from_utf8_lossy(spelling)
                .into_owned()
                .into_boxed_str(),
        ),
    });
}

/// Decode valid UTF-8 as UTF-8 and map isolated invalid bytes to Latin-1 code
/// points. This avoids replacement characters and retains an inspectable value
/// for every input byte until a later encoding policy has more context.
pub(crate) fn decode_visible_bytes(bytes: &[u8]) -> String {
    let mut text = String::new();
    let mut remaining = bytes;
    loop {
        match std::str::from_utf8(remaining) {
            Ok(valid) => {
                text.push_str(valid);
                return text;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                if valid > 0 {
                    text.push_str(
                        std::str::from_utf8(&remaining[..valid])
                            .expect("valid_up_to identifies valid UTF-8 prefix"),
                    );
                }
                let invalid_length = error.error_len().unwrap_or(remaining.len() - valid);
                for byte in &remaining[valid..valid + invalid_length] {
                    text.push(char::from(*byte));
                }
                remaining = &remaining[valid + invalid_length..];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Limits;

    use super::{
        EscapeIssue, EscapeIssueKind, invalid_unicode_character_name, normalize_ast_escapes,
        normalize_escapes,
    };

    #[test]
    fn unicode_character_names_require_canonical_scalar_spelling() {
        for spelling in [
            b"u2B".as_slice(),
            b"u02B",
            b"u0002B",
            b"u00002B",
            b"u000002B",
            b"u110000",
            b"u1234g",
        ] {
            assert!(invalid_unicode_character_name(spelling), "{spelling:?}");
        }
        for spelling in [b"u002B".as_slice(), b"u10000", b"u10FFFF", b"ul", b"ua"] {
            assert!(!invalid_unicode_character_name(spelling), "{spelling:?}");
        }
    }

    #[test]
    fn escapes_are_normalized_left_to_right_without_utf8_replacement() {
        let result = normalize_escapes(b"a\\(em b\\[bu]\\&\xff", b'\\', &Limits::default());
        assert_eq!(result.text, "a— b•ÿ");
        assert!(result.issues.is_empty());
    }

    #[test]
    fn final_no_space_escape_marks_line_continuation() {
        let result = normalize_escapes(b"join\\c", b'\\', &Limits::default());
        assert_eq!(result.text, "join");
        assert!(result.line_continuation);

        let ast = normalize_ast_escapes(b"join\\c", b'\\', &Limits::default());
        assert_eq!(ast.text, "join\\c");
        assert!(ast.line_continuation);
    }

    #[test]
    fn package_ast_projection_retains_trailing_horizontal_space() {
        let result = normalize_ast_escapes(b"plain \\fIterm\\fP\t", b'\\', &Limits::default());
        assert_eq!(result.text, "plain \\fIterm\\fP\t");
    }

    #[test]
    fn package_ast_retains_the_default_device_string_spelling() {
        let result = normalize_ast_escapes(b"\\*(.T \\*[.T]", b'\\', &Limits::default());
        assert_eq!(result.text, "\\*(.T \\*[.T]");
        assert!(result.issues.is_empty());
    }

    #[test]
    fn conditional_scope_delimiters_are_zero_width_controls() {
        let result = normalize_escapes(b"\\{visible\\}", b'\\', &Limits::default());
        assert_eq!(result.text, "visible");
        assert!(result.issues.is_empty());
    }

    #[test]
    fn malformed_and_deferred_escapes_are_recoverable() {
        let unterminated = normalize_escapes(b"\\[missing", b'\\', &Limits::default());
        assert_eq!(unterminated.issues[0].kind, EscapeIssueKind::Unterminated);
        let deferred = normalize_escapes(b"\\*[name]", b'\\', &Limits::default());
        assert!(
            deferred
                .issues
                .iter()
                .any(|issue| issue.kind == EscapeIssueKind::DeferredExpansion)
        );
        let unknown = normalize_escapes(b"\\Q", b'\\', &Limits::default());
        assert!(
            unknown
                .issues
                .iter()
                .any(|issue| issue.kind == EscapeIssueKind::UnknownEscape)
        );
    }

    #[test]
    fn empty_bracket_escape_is_retained_without_panicking() {
        let ast = normalize_ast_escapes(b"before\\[]after", b'\\', &Limits::default());
        assert_eq!(ast.text, r"before\[]after");
        assert!(ast.issues.is_empty());
    }

    #[test]
    fn visible_output_limit_truncates_deterministically() {
        let limits = Limits {
            max_expanded_line_bytes: 2,
            ..Limits::default()
        };
        let result = normalize_escapes(b"abc", b'\\', &limits);
        assert_eq!(result.text, "ab");
        assert!(result.truncated);
        assert_eq!(
            result.issues.last().map(|issue| issue.kind),
            Some(EscapeIssueKind::OutputLimit)
        );
    }

    #[test]
    fn numeric_specials_accept_only_valid_scalar_or_printable_latin1_values() {
        let result = normalize_escapes(
            b"\\[u2014]\\[u10FFFF]\\[char169]",
            b'\\',
            &Limits::default(),
        );
        assert_eq!(result.text, "—\u{10ffff}©");
        assert!(result.issues.is_empty());

        let invalid = normalize_escapes(b"\\[uD800]\\[char31]", b'\\', &Limits::default());
        assert_eq!(invalid.issues.len(), 2);
        assert!(
            invalid
                .issues
                .iter()
                .all(|issue| issue.kind == EscapeIssueKind::UnknownSpecialCharacter)
        );

        let noncanonical = normalize_escapes(b"before\\[u0002B]after", b'\\', &Limits::default());
        assert_eq!(noncanonical.text, "beforeafter");
        assert_eq!(
            noncanonical.issues.as_slice(),
            [EscapeIssue {
                offset: 6,
                length: 9,
                kind: EscapeIssueKind::UnknownSpecialCharacter,
                spelling: None,
            }]
        );
    }

    #[test]
    fn portable_catalog_covers_roff_and_char_regression_families() {
        let result = normalize_escapes(
            b"\\(*A \\(Eu \\[product] \\(lA \\(Bq \\(c+ \\(la",
            b'\\',
            &Limits::default(),
        );
        assert_eq!(result.text, "Α € ∏ ⇐ „ ⊕ ⟨");
        assert!(result.issues.is_empty());
    }

    #[test]
    fn complete_catalog_keeps_known_zero_width_controls_distinct_from_unknown_names() {
        let known = normalize_escapes(b"before\\[:]after", b'\\', &Limits::default());
        assert_eq!(known.text, "beforeafter");
        assert!(known.issues.is_empty());

        let unknown = normalize_escapes(
            b"before\\[not-a-mandoc-character]after",
            b'\\',
            &Limits::default(),
        );
        assert!(
            unknown
                .issues
                .iter()
                .any(|issue| issue.kind == EscapeIssueKind::UnknownSpecialCharacter)
        );
    }

    #[test]
    fn ignored_mandoc_escapes_are_retained_in_ast_without_scanner_warnings() {
        let ast = normalize_ast_escapes(
            b"A\\ B\\'C\\-D\\.E\\0F\\:G\\_H\\`I\\~J\\eK\\pL\\%M\\^N\\|O\\&P\\)Q\\,R\\/S\\aT\\dU\\rV\\tW\\uX\\{Y\\}Z",
            b'\\',
            &Limits::default(),
        );
        assert_eq!(
            ast.text,
            "A\\ B\\'C\\-D\\.E\\0F\\:G\\_H\\`I\\~J\\eK\\pL\\%M\\^N\\|O\\&P\\)Q\\,R\\/S\\aT\\dU\\rV\\tW\\uX\\{Y\\}Z"
        );
        assert!(ast.issues.is_empty());

        let escaped_backslash = normalize_ast_escapes(b"A\\eB", b'\\', &Limits::default());
        assert_eq!(escaped_backslash.text, "A\\eB");
        assert!(escaped_backslash.issues.is_empty());

        let doubled_backslash = normalize_ast_escapes(b"A\\\\B", b'\\', &Limits::default());
        assert_eq!(doubled_backslash.text, r"A\\B");
        assert!(doubled_backslash.issues.is_empty());

        let execution = normalize_escapes(b"A\\pB\\%C\\^D\\|E", b'\\', &Limits::default());
        assert_eq!(execution.text, "ABCDE");
        assert!(execution.issues.is_empty());

        let single_character_execution =
            normalize_escapes(b"\\.\\`\\_\\0\\:\\~", b'\\', &Limits::default());
        assert_eq!(single_character_execution.text, ".`_\u{a0}\u{a0}");
        assert!(single_character_execution.issues.is_empty());
    }

    #[test]
    fn overstrike_escape_is_source_visible_but_not_an_unknown_escape() {
        let ast = normalize_ast_escapes(b"x\\o'|-O'x", b'\\', &Limits::default());
        assert_eq!(ast.text, "x\\o'|-O'x");
        assert!(ast.issues.is_empty());

        let execution = normalize_escapes(b"x\\o'|-O'x", b'\\', &Limits::default());
        assert_eq!(execution.text, "xx");
        assert!(execution.issues.is_empty());
    }

    #[test]
    fn delimited_formatter_escapes_report_only_their_real_syntax_failures() {
        let valid =
            normalize_ast_escapes(b"\\B'1+1' \\w'text' \\h'0.16i'", b'\\', &Limits::default());
        assert_eq!(valid.text, "\\B'1+1' \\w'text' \\h'0.16i'");
        assert!(valid.issues.is_empty());

        let unclosed = normalize_ast_escapes(b"\\w'foo", b'\\', &Limits::default());
        assert_eq!(unclosed.text, "\\w'foo");
        assert_eq!(unclosed.issues[0].kind, EscapeIssueKind::Unterminated);
        assert_eq!(unclosed.issues[0].spelling.as_deref(), Some("\\w'foo"));

        let invalid = normalize_ast_escapes(b"\\h-", b'\\', &Limits::default());
        assert_eq!(invalid.text, "\\h-");
        assert_eq!(invalid.issues[0].kind, EscapeIssueKind::InvalidSyntax);
        assert_eq!(invalid.issues[0].spelling.as_deref(), Some("\\h-"));
    }
}
