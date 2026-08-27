use super::{
    Argument, ArgumentIssue, Diagnostic, DiagnosticCode, Environment, Limits, MacroSet, Severity,
    diagnostic, evaluate_sum, is_builtin_package_macro, lex_arguments, macro_body_control_column,
    push_diagnostic, trim_horizontal_space, visible_bytes,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BranchOutcome {
    Taken,
    Skipped,
}

impl BranchOutcome {
    pub(super) const fn is_taken(self) -> bool {
        matches!(self, Self::Taken)
    }

    pub(super) const fn is_skipped(self) -> bool {
        matches!(self, Self::Skipped)
    }

    pub(super) const fn inverse(self) -> Self {
        match self {
            Self::Taken => Self::Skipped,
            Self::Skipped => Self::Taken,
        }
    }
}

impl From<bool> for BranchOutcome {
    fn from(value: bool) -> Self {
        if value { Self::Taken } else { Self::Skipped }
    }
}

pub(super) fn condition_parts(arguments: &[Argument]) -> Option<(Vec<u8>, usize)> {
    let first = arguments.first()?;
    if matches!(first.bytes.as_slice(), b"d" | b"r" | b"!d" | b"!r") {
        let name = arguments.get(1)?;
        let mut predicate = first.bytes.clone();
        predicate.extend_from_slice(&name.bytes);
        Some((predicate, 2))
    } else {
        Some((first.bytes.clone(), 1))
    }
}

/// Validate the register and name-defined condition forms before expansion.
///
/// Their operand is an identifier, so an escape is not a deferred expansion:
/// mandoc diagnoses it at the beginning of the name and continues with the
/// unexpanded predicate. The lexer deliberately keeps the authored spelling
/// here, which also preserves the location of a two-token `r name` form.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_escaped_condition_name(
    arguments: &[Argument],
    escape: u8,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(first) = arguments.first() else {
        return;
    };
    let (negation_width, predicate) = first
        .bytes
        .strip_prefix(b"!")
        .map_or((0_usize, first.bytes.as_slice()), |predicate| {
            (1, predicate)
        });
    let Some(name) = predicate
        .strip_prefix(b"r")
        .or_else(|| predicate.strip_prefix(b"d"))
    else {
        return;
    };
    let (name, source_offset) = if name.is_empty() {
        let Some(name) = arguments.get(1) else {
            return;
        };
        (name.bytes.as_slice(), name.offset)
    } else {
        (name, first.offset.saturating_add(negation_width + 1))
    };
    let Some(escape_offset) = name.iter().position(|byte| *byte == escape) else {
        return;
    };
    // Retain the escape's one-byte delimiter in the message: this is the
    // short spelling reported by mandoc for both `\\(` and `\\[` names.
    let preview_end = escape_offset.saturating_add(2).min(name.len());
    let start = argument_start.saturating_add(
        u32::try_from(source_offset).expect("argument offsets are bounded by line length"),
    );
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
                visible_bytes(&name[..preview_end])
            ),
        ),
        truncated,
    );
}

/// Reject an escape in the first name argument of requests such as `.nr` and
/// `.rr`. A doubled delimiter is a literal backslash in a roff name, but any
/// other escape is rejected before the request's existing recovery executes.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_escaped_request_name(
    arguments: &[Argument],
    escape: u8,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(name) = arguments.first() else {
        return;
    };
    let mut cursor = 0;
    let escape_offset = loop {
        let Some(offset) = name.bytes[cursor..]
            .iter()
            .position(|byte| *byte == escape)
            .map(|offset| cursor + offset)
        else {
            return;
        };
        if name.bytes.get(offset + 1) == Some(&escape) {
            cursor = offset + 2;
            continue;
        }
        break offset;
    };
    let preview_end = if matches!(name.bytes.get(escape_offset + 1), Some(b' ' | b'\t')) {
        escape_offset.saturating_add(1)
    } else {
        escape_offset.saturating_add(2).min(name.bytes.len())
    };
    let start = argument_start.saturating_add(
        u32::try_from(name.offset).expect("argument offsets are bounded by line length"),
    );
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
                visible_bytes(&name.bytes[..preview_end])
            ),
        ),
        truncated,
    );
}

/// Preserve the source spelling of an inline conditional body.
///
/// The condition predicate needs roff-aware tokenization, but its body is a
/// request or text fragment in its own right.  Rejoining lexer tokens would
/// discard a `.ds`/`.as` value's leading copy-mode quote before that request
/// can interpret it.  Argument offsets let us parse only the predicate while
/// slicing the body from the original bytes.
pub(super) fn condition_body_template(
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
) -> Vec<u8> {
    condition_body_template_from_offset(raw_arguments, arguments, body_start, None)
}

/// Return the copied-input cursor for a one-predicate inline macro body.
///
/// A user macro is first reparsed in copy mode.  If its condition's predicate
/// shrinks while expanding a `\$n` argument, mandoc continues at the reduced
/// cursor for the inline body, not at the original definition byte offset.
/// Keep this deliberately narrow: the two-token `r`/`d` forms have distinct
/// condition grammar and are not needed for the macro-body recovery path.
pub(super) fn macro_conditional_body_origin(
    body_line: &[u8],
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
    predicate_width: Option<usize>,
) -> Option<u32> {
    if body_start != 1 {
        return None;
    }
    let predicate = arguments.first()?;
    let body = arguments.get(body_start)?;
    let predicate_width = predicate_width?;
    if predicate_width == predicate.bytes.len() {
        return None;
    }
    let control_width = body_line.len().checked_sub(raw_arguments.len())?;
    let separator_width = body
        .offset
        .checked_sub(predicate.offset.checked_add(predicate.bytes.len())?)?;
    u32::try_from(
        control_width
            .saturating_add(predicate_width)
            .saturating_add(separator_width),
    )
    .ok()
}

/// Return the copied-input cursor inherited by the first line of a braced
/// macro conditional.  `roff_cond()` reruns from immediately after the
/// compacted predicate and opening `\{`; the following retained line is not
/// a new macro invocation at column one.
pub(super) fn macro_scope_body_origin(
    body_line: &[u8],
    control: u8,
    predicate_width: Option<usize>,
) -> Option<u32> {
    let predicate_width = u32::try_from(predicate_width?).ok()?;
    Some(macro_body_control_column(body_line, control).saturating_add(predicate_width))
}

pub(super) fn condition_body_template_from_offset(
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
    escaped_name_body_offset: Option<usize>,
) -> Vec<u8> {
    if let Some(body_offset) = escaped_name_body_offset {
        return raw_arguments
            .get(body_offset..)
            .unwrap_or_default()
            .to_vec();
    }
    let Some(body) = arguments.get(body_start) else {
        return Vec::new();
    };
    let separator_start = body_start
        .checked_sub(1)
        .and_then(|index| arguments.get(index))
        .and_then(|predicate| predicate.offset.checked_add(predicate.bytes.len()))
        .unwrap_or(body.offset);
    let body_offset = raw_arguments
        .get(separator_start..body.offset)
        .and_then(|separator| separator.iter().rposition(|byte| *byte == b'\t'))
        .and_then(|offset| separator_start.checked_add(offset))
        .unwrap_or(body.offset);
    raw_arguments
        .get(body_offset..)
        .unwrap_or_default()
        .to_vec()
}

/// Split an escaped name condition into its accepted identifier and retained
/// inline body. `roff_cond()` stops the identifier at the invalid escape, then
/// reparses that escape as the beginning of visible body text.
pub(super) fn split_escaped_condition_body(
    arguments: &[Argument],
    escape: u8,
    fallback_predicate: &[u8],
) -> Option<(Vec<u8>, usize)> {
    let first = arguments.first()?;
    let (negation_width, predicate) = first
        .bytes
        .strip_prefix(b"!")
        .map_or((0_usize, first.bytes.as_slice()), |predicate| {
            (1, predicate)
        });
    let kind = *predicate.first()?;
    if !matches!(kind, b'r' | b'd') {
        return None;
    }
    let (name, name_offset, prefix) = if predicate.len() == 1 {
        let name = arguments.get(1)?;
        (name.bytes.as_slice(), name.offset, first.bytes.clone())
    } else {
        (
            &predicate[1..],
            first.offset.saturating_add(negation_width + 1),
            first.bytes[..=negation_width].to_vec(),
        )
    };
    let escape_offset = name.iter().position(|byte| *byte == escape)?;
    let mut predicate = prefix;
    predicate.extend_from_slice(&name[..escape_offset]);
    if predicate == fallback_predicate {
        return None;
    }
    Some((predicate, name_offset.saturating_add(escape_offset)))
}

/// Select the public source start for an inline conditional body.
///
/// A literal tab separating a register/name predicate from visible text is
/// consumed by the argument lexer, but mandoc anchors the visible text at
/// that tab. This preserves both its diagnostic position and the source-aware
/// renderer's distinction from an ordinary separating space.
pub(super) fn condition_body_source_start_from_offset(
    raw_arguments: &[u8],
    arguments: &[Argument],
    body_start: usize,
    argument_start: u32,
    fallback: u32,
    escaped_name_body_offset: Option<usize>,
) -> u32 {
    if let Some(source_offset) = escaped_name_body_offset {
        return u32::try_from(source_offset)
            .ok()
            .and_then(|offset| argument_start.checked_add(offset))
            .unwrap_or(fallback);
    }
    let Some(body) = arguments.get(body_start) else {
        return fallback;
    };
    let separator_start = body_start
        .checked_sub(1)
        .and_then(|index| arguments.get(index))
        .and_then(|predicate| predicate.offset.checked_add(predicate.bytes.len()))
        .unwrap_or(body.offset);
    let source_offset = raw_arguments
        .get(separator_start..body.offset)
        .and_then(|separator| separator.iter().rposition(|byte| *byte == b'\t'))
        .and_then(|offset| separator_start.checked_add(offset))
        .unwrap_or(body.offset);
    u32::try_from(source_offset)
        .ok()
        .and_then(|offset| argument_start.checked_add(offset))
        .unwrap_or(fallback)
}

pub(super) fn lex_condition_arguments(
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<Argument>, ArgumentIssue> {
    let leading = bytes
        .len()
        .saturating_sub(trim_horizontal_space(bytes).len());
    let bytes = &bytes[leading..];
    if bytes.first() != Some(&b'"') {
        return lex_arguments(bytes, escape, limits);
    }
    let mut delimiters = 0_usize;
    let mut end = None;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'"' {
            delimiters += 1;
            if delimiters == 3 {
                end = Some(index + 1);
                break;
            }
        }
    }
    let Some(end) = end else {
        return lex_arguments(bytes, escape, limits);
    };
    let mut arguments = vec![Argument {
        offset: leading,
        quoted: true,
        separator_after: bytes.get(end).copied().filter(u8::is_ascii_whitespace),
        separator_contains_tab: bytes[end..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .any(|byte| *byte == b'\t'),
        embedded_tab_count: memchr::memchr_iter(b'\t', &bytes[..end]).count(),
        separator_width: bytes[end..]
            .iter()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count(),
        bytes: bytes[..end].to_vec(),
    }];
    let mut tail = lex_arguments(&bytes[end..], escape, limits)?;
    for argument in &mut tail {
        argument.offset += leading + end;
    }
    if arguments.len() + tail.len() > limits.max_arguments {
        return Err(ArgumentIssue::Limit);
    }
    let bytes_used = arguments
        .iter()
        .chain(&tail)
        .map(|argument| argument.bytes.len())
        .sum::<usize>();
    if bytes_used > limits.max_argument_bytes {
        return Err(ArgumentIssue::Limit);
    }
    arguments.append(&mut tail);
    Ok(arguments)
}

pub(super) fn evaluate_condition(environment: &mut Environment, bytes: &[u8]) -> Option<bool> {
    let (negated, bytes) = bytes
        .strip_prefix(b"!")
        .map_or((false, bytes), |remaining| (true, remaining));
    // A bare opening parenthesis has started numeric parsing in mandoc, then
    // failed before an operand.  That is an invalid condition rather than an
    // ordinary false string comparison, so a preceding `!` does not turn it
    // true (`roff_evalcond()` returns false directly once its cursor moved).
    if bytes == b"(" {
        return Some(false);
    }
    let value = if let Some(name) = bytes.strip_prefix(b"r").filter(|name| !name.is_empty()) {
        Some(environment.is_register_defined(name))
    } else if let Some(name) = bytes.strip_prefix(b"d").filter(|name| !name.is_empty()) {
        let defined = environment.is_name_defined(name)
            || is_builtin_request(name)
            // `.if dBR` and peers are evaluated after the man parser has
            // selected its package, even though the generic roff condition
            // evaluator does not receive that selection explicitly.
            || is_builtin_package_macro(MacroSet::Man, name)
            || is_builtin_package_macro(MacroSet::Mdoc, name);
        if !defined {
            environment.observe_undefined_name_condition(name);
        }
        Some(defined)
    } else {
        match bytes {
            b"n" => Some(true),
            b"t" => Some(false),
            _ => evaluate_numeric_condition(bytes).or_else(|| evaluate_string_condition(bytes)),
        }
    }?;
    Some(if negated { !value } else { value })
}

pub(super) fn is_builtin_request(name: &[u8]) -> bool {
    matches!(
        name,
        b"br"
            | b"ce"
            | b"ft"
            | b"ll"
            | b"ps"
            | b"na"
            | b"nf"
            | b"fi"
            | b"PP"
            | b"LP"
            | b"P"
            | b"TH"
            | b"SH"
            | b"SS"
            | b"TP"
            | b"TQ"
    )
}

pub(super) fn evaluate_string_condition(bytes: &[u8]) -> Option<bool> {
    let (&delimiter, remainder) = bytes.split_first()?;
    if delimiter.is_ascii_digit() || matches!(delimiter, b'+' | b'-' | b'<' | b'>' | b'=') {
        return None;
    }
    let Some(middle) = remainder.iter().position(|byte| *byte == delimiter) else {
        return Some(false);
    };
    let right = &remainder[middle + 1..];
    let Some(end) = right.iter().position(|byte| *byte == delimiter) else {
        return Some(false);
    };
    Some(remainder[..middle] == right[..end])
}

pub(super) fn evaluate_numeric_condition(bytes: &[u8]) -> Option<bool> {
    // A leading unmatched opening parenthesis groups as much numeric input as
    // follows it in groff/mandoc condition syntax.  A bare `(` is instead the
    // false, malformed-string form handled below.
    let bytes = bytes.strip_prefix(b"(").unwrap_or(bytes);
    if bytes.is_empty() {
        return None;
    }
    if let Some(operator) = bytes
        .iter()
        .enumerate()
        .find_map(|(index, byte)| matches!(*byte, b'&' | b':').then_some(index))
    {
        let left = evaluate_sum(&bytes[..operator]).ok()?;
        let right = evaluate_sum(&bytes[operator + 1..]).ok()?;
        return Some(match bytes[operator] {
            b'&' => left.magnitude != 0 && right.magnitude != 0,
            b':' => left.magnitude != 0 || right.magnitude != 0,
            _ => unreachable!("boolean condition operators are exhaustive"),
        });
    }
    let operator = bytes
        .iter()
        .enumerate()
        .find_map(|(index, byte)| matches!(*byte, b'<' | b'>' | b'=' | b'!').then_some(index));
    let Some(operator) = operator else {
        // `roff_evalcond()` selects the true branch only for a *positive*
        // numeric result.  A negative scaled value is well-formed but false;
        // this differs from the usual Rust/C nonzero truthiness.
        return evaluate_sum(bytes).ok().map(|value| value.magnitude > 0);
    };
    let left = evaluate_sum(&bytes[..operator]).ok()?;
    let (operation, right_start): (&[u8], usize) = match bytes.get(operator..)? {
        [b'<', b'=', ..] => (b"<=", operator + 2),
        [b'>', b'=', ..] => (b">=", operator + 2),
        [b'!', b'=', ..] | [b'<', b'>', ..] => (b"!=", operator + 2),
        [b'=', b'=', ..] => (b"==", operator + 2),
        [b'<', ..] => (b"<", operator + 1),
        [b'>', ..] => (b">", operator + 1),
        [b'=', ..] => (b"==", operator + 1),
        _ => return None,
    };
    let Some(right) = evaluate_sum(&bytes[right_start..]).ok() else {
        // mandoc selects the false branch for an incomplete/malformed numeric
        // comparison (for example `42=bad`), rather than treating it as an
        // unsupported extension.
        return Some(false);
    };
    let ordering = left.compare(right)?;
    Some(match operation {
        b"<" => ordering.is_lt(),
        b"<=" => ordering.is_le(),
        b">" => ordering.is_gt(),
        b">=" => ordering.is_ge(),
        b"==" => ordering.is_eq(),
        b"!=" => ordering.is_ne(),
        _ => unreachable!("condition operators are exhaustive"),
    })
}
