use super::{
    Argument, ArgumentIssue, Diagnostic, DiagnosticCode, DocumentBuilder, Environment,
    EnvironmentError, Limits, MacroSet, Severity, SourcePosition, SourceSpan, decode_visible_bytes,
    join_arguments, lex_arguments, trim_horizontal_space,
};
use num_traits::ToPrimitive;

/// One armed roff input-line trap (`.it`).
///
/// The trap counts only physical text input lines.  It is session-local,
/// intentionally replacing an older arm instead of stacking, as upstream's
/// `roffit_lines`/`roffit_macro` pair does.
#[derive(Default)]
pub(super) struct InputTrap {
    remaining: usize,
    invocation: Vec<u8>,
}

impl InputTrap {
    pub(super) fn consume_text_line(&mut self) -> Option<Vec<u8>> {
        match self.remaining {
            0 => None,
            1 => {
                self.remaining = 0;
                Some(std::mem::take(&mut self.invocation))
            }
            _ => {
                self.remaining -= 1;
                None
            }
        }
    }
}

/// Arm a roff `.it` input-line trap for a bounded scaled-number subset. roff
/// permits both a scale suffix and the macro invocation immediately after the
/// number, so `.it 1vtrap` means one line and `trap`, while `.it 1 trap arg`
/// preserves `arg` for the injected macro invocation.
pub(super) fn arm_input_trap(trap: &mut InputTrap, arguments: &[u8]) -> bool {
    let mut parser = InputTrapNumberParser::new(arguments);
    let Some(count) = parser.parse_expression() else {
        return false;
    };
    let count = if count.is_finite() && count > 0.0 {
        count.to_usize().unwrap_or(usize::MAX)
    } else {
        0
    };
    let mut invocation = trim_horizontal_space(&arguments[parser.cursor..]).to_vec();
    // groff's an-ext macro uses this exact special case to arrange an
    // unconditional break.  Preserve its public effect without exposing the
    // formatter-private trap request itself.
    if count == 1 && invocation == b"an-trap" {
        invocation = b"br".to_vec();
    }
    trap.remaining = count;
    trap.invocation = invocation;
    true
}

/// Small, allocation-free reader for the scaled numeric prefix accepted by
/// `.it`. Its result deliberately ignores unit conversion: upstream parses
/// this request with an integer target and therefore counts `1c + 1i` as two
/// input lines while retaining the suffix syntax.
pub(super) struct InputTrapNumberParser<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> InputTrapNumberParser<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn parse_expression(&mut self) -> Option<f64> {
        self.skip_space();
        let mut total = self.parse_signed_term()?;
        loop {
            self.skip_space();
            let Some(operator) = self.bytes.get(self.cursor).copied() else {
                break;
            };
            if operator == b')' {
                self.cursor += 1;
                break;
            }
            if !matches!(operator, b'+' | b'-') {
                break;
            }
            self.cursor += 1;
            let term = self.parse_term()?;
            total = if operator == b'+' {
                total + term
            } else {
                total - term
            };
        }
        Some(total)
    }

    fn parse_signed_term(&mut self) -> Option<f64> {
        self.skip_space();
        let sign = match self.bytes.get(self.cursor).copied() {
            Some(b'+') => {
                self.cursor += 1;
                1.0
            }
            Some(b'-') => {
                self.cursor += 1;
                -1.0
            }
            _ => 1.0,
        };
        self.parse_term().map(|term| sign * term)
    }

    fn parse_term(&mut self) -> Option<f64> {
        self.skip_space();
        if self.bytes.get(self.cursor) == Some(&b'(') {
            self.cursor += 1;
            return self.parse_expression();
        }
        let start = self.cursor;
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(|byte| byte.is_ascii_digit() || *byte == b'.')
        {
            self.cursor += 1;
        }
        (self.cursor > start).then_some(())?;
        let number = std::str::from_utf8(&self.bytes[start..self.cursor])
            .ok()?
            .parse::<f64>()
            .ok()?;
        // Standard roff scale suffixes are single bytes.  Do not consume an
        // arbitrary letter here: the following bytes are the trap macro name.
        if self.bytes.get(self.cursor).is_some_and(|byte| {
            matches!(
                *byte,
                b'u' | b'i' | b'c' | b'P' | b'p' | b'm' | b'n' | b'v' | b'M'
            )
        }) {
            self.cursor += 1;
        }
        Some(number)
    }

    fn skip_space(&mut self) {
        while self
            .bytes
            .get(self.cursor)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.cursor += 1;
        }
    }
}

/// Scanner-stage subset of man(7)'s `an-margin` register bookkeeping.
///
/// The C parser updates this register while parsing `.RS`/`.RE`; ordinary
/// source text may interpolate it before the later man structural pass runs.
/// Keep the state in the parse session so those source-order expansions retain
/// the same observable values without exposing layout state in the AST.
#[derive(Default)]
pub(super) struct ManIndentState {
    current: i64,
    frames: Vec<i64>,
}

pub(super) fn update_man_indent_register(
    environment: &mut Environment,
    macro_set: MacroSet,
    name: &[u8],
    arguments: &[u8],
    state: &mut ManIndentState,
    limits: &Limits,
) {
    if macro_set != MacroSet::Man {
        return;
    }
    match name {
        b"RS" => {
            // man(7) initializes the internal margin at seven ens (168
            // basic units) when the first reset block opens. Its optional
            // numeric argument is an additive indent measured in ens.
            if state.current == 0 {
                state.current = 7 * 24;
            }
            let indent = man_indent_units(arguments);
            state.current = state.current.saturating_add(indent);
            state.frames.push(indent);
        }
        b"RE" => {
            let levels = trim_horizontal_space(arguments)
                .split(u8::is_ascii_whitespace)
                .next()
                .and_then(|value| std::str::from_utf8(value).ok())
                .and_then(|value| value.parse::<usize>().ok())
                .filter(|levels| *levels > 0)
                .unwrap_or(1);
            for _ in 0..levels {
                let Some(indent) = state.frames.pop() else {
                    break;
                };
                state.current = state.current.saturating_sub(indent);
            }
        }
        _ => return,
    }
    let value = state.current.to_string();
    let _ = environment.define_register(b"an-margin", value.as_bytes(), None, limits);
}

pub(super) fn man_indent_units(arguments: &[u8]) -> i64 {
    let argument = trim_horizontal_space(arguments)
        .split(u8::is_ascii_whitespace)
        .next()
        .unwrap_or_default();
    let numeric_end = argument
        .iter()
        .position(|byte| !matches!(*byte, b'+' | b'-' | b'.' | b'0'..=b'9'))
        .unwrap_or(argument.len());
    let Ok(value) = std::str::from_utf8(&argument[..numeric_end]) else {
        return 0;
    };
    let Ok(value) = value.parse::<f64>() else {
        return 0;
    };
    // `man_macro.c` applies `strtod(argument) * 24.0` and ignores a
    // non-positive result for the stored RS auxiliary value.
    (value * 24.0).max(0.0).to_i64().unwrap_or(i64::MAX)
}

pub(super) fn update_preprocessor_depth(depth: &mut usize, name: &[u8]) {
    match name {
        b"EQ" | b"TS" => *depth = depth.saturating_add(1),
        b"EN" | b"TE" => *depth = depth.saturating_sub(1),
        _ => {}
    }
}

/// Track tbl input separately because its physical-line continuation grammar
/// owns a terminal escape before ordinary roff escape recovery runs.
pub(super) fn update_table_preprocessor_depth(depth: &mut usize, name: &[u8]) {
    match name {
        b"TS" => *depth = depth.saturating_add(1),
        b"TE" => *depth = depth.saturating_sub(1),
        _ => {}
    }
}

/// Update man(7)'s presentation-mode validator independently of AST scopes.
///
/// Nested `.EX` blocks retain nested parser scopes so their source ranges
/// stay recoverable, but mandoc's fill style check is a simple on/off state:
/// a second disable or enable warns and leaves that presentation state intact.
pub(super) fn update_man_example_fill_presentation(
    fill_enabled: &mut bool,
    macro_set: MacroSet,
    name: &[u8],
) -> Option<&'static str> {
    if macro_set != MacroSet::Man {
        return None;
    }
    match name {
        b"nf" => {
            let redundant = !*fill_enabled;
            *fill_enabled = false;
            redundant.then_some("fill mode already disabled, skipping: nf")
        }
        b"fi" => {
            let redundant = *fill_enabled;
            *fill_enabled = true;
            redundant.then_some("fill mode already enabled, skipping: fi")
        }
        b"EX" => {
            let redundant = !*fill_enabled;
            *fill_enabled = false;
            redundant.then_some("fill mode already disabled, skipping: EX")
        }
        b"EE" => {
            let redundant = *fill_enabled;
            *fill_enabled = true;
            redundant.then_some("fill mode already enabled, skipping: EE")
        }
        _ => None,
    }
}

pub(super) fn trailing_whitespace_start(bytes: &[u8]) -> Option<usize> {
    let offset = bytes
        .iter()
        .rposition(|byte| !matches!(*byte, b' ' | b'\t'));
    let Some(offset) = offset else {
        return (!bytes.is_empty()).then_some(0);
    };
    let trailing_start = offset.saturating_add(1);
    (trailing_start < bytes.len()).then_some(trailing_start)
}

/// Emit mandoc's portable-width style finding for ordinary package text.
///
/// tbl and eqn ranges bypass this helper because their fields have independent
/// grammar and are normalized by preprocessing rather than paragraph layout.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_long_input_line(
    bytes: &[u8],
    line_start: u32,
    line_end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    const STYLE_LINE_BYTES: usize = 80;
    const PREVIEW_CHARACTERS: usize = 20;
    if bytes.len() <= STYLE_LINE_BYTES {
        return;
    }
    let preview = decode_visible_bytes(bytes)
        .chars()
        .take(PREVIEW_CHARACTERS)
        .collect::<String>();
    let location = line_start.saturating_add(
        u32::try_from(bytes.len().saturating_sub(1))
            .expect("scanner source lines fit the public offset boundary"),
    );
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::INPUT_LINE_TOO_LONG,
            Severity::Style,
            source_id,
            location,
            line_end,
            format!("input text line longer than 80 bytes: {preview}..."),
        ),
        truncated,
    );
}

pub(super) fn invalid_input_byte_offsets(bytes: &[u8]) -> Vec<(usize, u8)> {
    let mut invalid = bytes
        .iter()
        .enumerate()
        .filter_map(|(offset, byte)| {
            matches!(*byte, 0x00..=0x08 | 0x0b..=0x1f | 0x7f).then_some((offset, *byte))
        })
        .collect::<Vec<_>>();
    let mut cursor = 0;
    while cursor < bytes.len() {
        match std::str::from_utf8(&bytes[cursor..]) {
            Ok(_) => break,
            Err(error) => {
                cursor = cursor.saturating_add(error.valid_up_to());
                let width = error.error_len().unwrap_or(bytes.len() - cursor);
                invalid.extend(
                    bytes[cursor..cursor.saturating_add(width).min(bytes.len())]
                        .iter()
                        .enumerate()
                        .map(|(offset, byte)| (cursor + offset, *byte)),
                );
                cursor = cursor.saturating_add(width);
            }
        }
    }
    invalid.sort_unstable_by_key(|(offset, _)| *offset);
    invalid
}

pub(super) fn push_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    limits: &Limits,
    diagnostic: Diagnostic,
    truncated: &mut bool,
) {
    if diagnostics.len() < limits.max_diagnostics {
        diagnostics.push(diagnostic);
    } else {
        *truncated = true;
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn record_expansion_steps(
    total: &mut usize,
    additional: usize,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> bool {
    let Some(next) = total.checked_add(additional) else {
        *truncated = true;
        return false;
    };
    if next > limits.max_expansion_steps {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::LIMIT_EXPANSION_STEPS,
                Severity::Warning,
                source_id,
                start,
                end,
                "scanner-stage aggregate escape work exceeds max_expansion_steps",
            ),
            truncated,
        );
        return false;
    }
    *total = next;
    true
}

#[allow(clippy::too_many_arguments)] // Keep parser call sites explicit about source-relative limits and recovery.
pub(super) fn expand_environment(
    environment: &mut Environment,
    bytes: &[u8],
    escape: u8,
    arguments: &[Vec<u8>],
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<Vec<u8>> {
    expand_environment_with_missing_reference_policy(
        environment,
        bytes,
        escape,
        arguments,
        limits,
        source_id,
        start,
        end,
        expansion_steps,
        diagnostics,
        truncated,
        true,
        false,
    )
}

/// Report source-level macro argument interpolation where no macro invocation
/// owns an argument frame. The environment normalizer still removes the
/// escaped argument from visible output; this scanner-stage finding retains
/// libmandoc's distinct error rather than treating it as an undefined string.
#[allow(clippy::too_many_arguments)] // Mirrors the other source-relative diagnostic emitters.
pub(super) fn emit_outside_macro_argument_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut offset = 0_usize;
    while offset.saturating_add(2) < bytes.len() {
        if bytes[offset] != escape {
            offset += 1;
            continue;
        }
        if bytes[offset + 1] == escape {
            offset += 2;
            continue;
        }
        if bytes[offset + 1] != b'$' || !matches!(bytes[offset + 2], b'1'..=b'9' | b'*' | b'@') {
            offset += 1;
            continue;
        }
        let finding_start = start.saturating_add(
            u32::try_from(offset).expect("parser bounds physical line offsets before diagnostics"),
        );
        let spelling = visible_bytes(&bytes[offset..offset + 3]);
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ROFF_MACRO_ARGUMENT_OUTSIDE,
                Severity::Error,
                source_id,
                finding_start,
                finding_start.saturating_add(3),
                format!("using macro argument outside macro: {spelling}"),
            ),
            truncated,
        );
        offset += 3;
    }
}

/// Apply the recovery paired with [`emit_outside_macro_argument_escapes`].
/// A top-level `\$1` is diagnosed but cannot become an argument value for a
/// later user macro: retaining it would manufacture a recursive interpolation
/// that does not exist in mandoc's execution state.
pub(super) fn strip_outside_macro_argument_escapes(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let selector = bytes.get(offset + 2).copied();
        if bytes.get(offset) == Some(&escape)
            && bytes.get(offset + 1) == Some(&b'$')
            && matches!(selector, Some(b'1'..=b'9' | b'*' | b'@'))
        {
            offset += 3;
            continue;
        }
        output.push(bytes[offset]);
        offset += 1;
    }
    output
}

/// Validate argument selectors while replaying a user macro body.
///
/// Copy mode leaves `\\$x` dormant in the stored definition and reactivates
/// it at invocation.  At that point mandoc diagnoses a non-numeric selector
/// against the caller's logical line and removes the three-byte escape; it is
/// neither ordinary visible text nor a generic unknown formatter escape.
#[allow(clippy::too_many_arguments)] // Keeps this source-relative rewrite beside its diagnostic policy.
pub(super) fn normalize_macro_argument_number_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    builder: &DocumentBuilder,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Vec<u8> {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut offset = 0_usize;
    while offset < bytes.len() {
        let invalid_selector = bytes.get(offset) == Some(&escape)
            && bytes.get(offset + 1) == Some(&b'$')
            && bytes
                .get(offset + 2)
                .is_some_and(|selector| !matches!(*selector, b'1'..=b'9' | b'*' | b'@'));
        if !invalid_selector {
            normalized.push(bytes[offset]);
            offset += 1;
            continue;
        }
        let spelling = visible_bytes(&bytes[offset..offset + 3]);
        let mut finding = diagnostic(
            DiagnosticCode::ROFF_MACRO_ARGUMENT_OUTSIDE,
            Severity::Error,
            source_id,
            start,
            start,
            format!("argument number is not numeric: {spelling}"),
        );
        if let Some(primary) = finding.primary.as_mut()
            && let Some(position) = builder.source_position(primary)
        {
            primary.logical_start = Some(SourcePosition {
                line: position.line,
                column: u32::try_from(offset + 1)
                    .expect("bounded macro body offsets fit public positions"),
            });
        }
        push_diagnostic(diagnostics, limits, finding, truncated);
        offset += 3;
    }
    normalized
}

/// The roff environment expands bracketed number-register names before the
/// visible-escape normalizer sees them. Preserve the validator diagnostic for
/// a missing closing bracket rather than silently turning the complete tail
/// into an empty register value.
#[allow(clippy::too_many_arguments)] // Mirrors the other source-relative diagnostic emitters.
pub(super) fn emit_unterminated_register_reference_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut offset = 0_usize;
    while offset.saturating_add(2) < bytes.len() {
        if bytes[offset] != escape {
            offset += 1;
            continue;
        }
        if bytes[offset + 1] == escape {
            offset += 2;
            continue;
        }
        if bytes[offset + 1] != b'n' || bytes[offset + 2] != b'[' {
            offset += 1;
            continue;
        }
        if bytes[offset + 3..].contains(&b']') {
            offset += 3;
            continue;
        }
        let finding_start = start.saturating_add(
            u32::try_from(offset).expect("parser bounds physical line offsets before diagnostics"),
        );
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ESCAPE_INVALID,
                Severity::Warning,
                source_id,
                finding_start,
                end,
                format!(
                    "invalid escape sequence: {}",
                    visible_bytes(&bytes[offset..])
                ),
            ),
            truncated,
        );
        if offset > 0 && matches!(bytes[offset - 1], b' ' | b'\t') {
            let whitespace_start = finding_start.saturating_sub(1);
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::INPUT_TRAILING_WHITESPACE,
                    Severity::Style,
                    source_id,
                    whitespace_start,
                    finding_start,
                    "whitespace at end of input line",
                ),
                truncated,
            );
        }
        return;
    }
}

/// Preserve the validator finding for a bracketed string interpolation whose
/// closing bracket is absent.  Environment expansion separately records the
/// remaining name as an undefined string and consumes it to an empty value.
#[allow(clippy::too_many_arguments)] // Mirrors the register-reference validator above.
pub(super) fn emit_unterminated_string_reference_escapes(
    bytes: &[u8],
    escape: u8,
    start: u32,
    end: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut offset = 0_usize;
    while offset.saturating_add(2) < bytes.len() {
        if bytes[offset] != escape {
            offset += 1;
            continue;
        }
        if bytes[offset + 1] == escape {
            offset += 2;
            continue;
        }
        if bytes[offset + 1] != b'*' || bytes[offset + 2] != b'[' {
            offset += 1;
            continue;
        }
        if bytes[offset + 3..].contains(&b']') {
            offset += 3;
            continue;
        }
        let finding_start = start.saturating_add(
            u32::try_from(offset).expect("parser bounds physical line offsets before diagnostics"),
        );
        push_diagnostic(
            diagnostics,
            limits,
            diagnostic(
                DiagnosticCode::ESCAPE_INVALID,
                Severity::Warning,
                source_id,
                finding_start,
                end,
                format!(
                    "invalid escape sequence: {}",
                    visible_bytes(&bytes[offset..])
                ),
            ),
            truncated,
        );
        return;
    }
}

/// Expand a definition while it is copied into session-owned storage.  Roff
/// interpolates ordinary references at definition time, but a doubled escape
/// remains literal until the later macro or string use.  Undefined references
/// have the legacy copy-mode recovery of producing no bytes and no public
/// diagnostic.
#[allow(clippy::too_many_arguments)] // Shares the bounded source-relative expansion boundary above.
pub(super) fn expand_copy_mode_definition(
    environment: &mut Environment,
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<Vec<u8>> {
    expand_environment_with_missing_reference_policy(
        environment,
        bytes,
        escape,
        &[],
        limits,
        source_id,
        start,
        end,
        expansion_steps,
        diagnostics,
        truncated,
        false,
        true,
    )
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // Keep environment output, source-relative recovery, and shared budgets in one auditable boundary.
pub(super) fn expand_environment_with_missing_reference_policy(
    environment: &mut Environment,
    bytes: &[u8],
    escape: u8,
    arguments: &[Vec<u8>],
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
    report_missing_references: bool,
    copy_mode_definition: bool,
) -> Option<Vec<u8>> {
    let remaining_steps = limits.max_expansion_steps.saturating_sub(*expansion_steps);
    let expansion = if copy_mode_definition {
        environment.expand_copy_mode_definition(
            bytes,
            escape,
            remaining_steps,
            limits.max_expanded_line_bytes,
        )
    } else {
        environment.expand(
            bytes,
            escape,
            arguments,
            remaining_steps,
            limits.max_expanded_line_bytes,
        )
    };
    match expansion {
        Ok(result) => {
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
                return None;
            }
            if report_missing_references {
                // Mandoc's string-reference validator drains the findings
                // collected on one physical line in reverse source order.
                // Reset the direct-source matcher for each distinct pending
                // finding so this delayed order does not turn an earlier
                // reference into a line-start fallback.
                for missing in result.missing_references.into_iter().rev() {
                    // Roff installs an implicit empty value after the first
                    // failed interpolation.  It suppresses duplicate
                    // warnings and makes a following `dname` predicate true
                    // until `.rm`, but is not an explicit `.ds` definition
                    // and consequently must not move with `.rn`.
                    if let Err(error) =
                        environment.materialize_implicit_empty_string(&missing, limits)
                    {
                        *truncated = true;
                        push_diagnostic(
                            diagnostics,
                            limits,
                            environment_error_diagnostic(error, source_id, start, end),
                            truncated,
                        );
                        continue;
                    }
                    let mut missing_reference_cursor = 0_usize;
                    let finding_start = next_missing_reference_offset(
                        bytes,
                        escape,
                        &missing,
                        &mut missing_reference_cursor,
                    )
                    .and_then(|offset| u32::try_from(offset).ok())
                    .map_or(start, |offset| {
                        source_offset_or_line_start(start, end, offset)
                    });
                    push_diagnostic(
                        diagnostics,
                        limits,
                        diagnostic(
                            DiagnosticCode::ROFF_UNDEFINED_REFERENCE,
                            Severity::Warning,
                            source_id,
                            finding_start,
                            end,
                            format!("undefined string, using \"\": {}", visible_bytes(&missing)),
                        ),
                        truncated,
                    );
                }
            }
            for offset in result.malformed_escape_offsets {
                let finding_start = source_offset_or_line_start(
                    start,
                    end,
                    u32::try_from(offset).expect("parser bounds every expanded source line"),
                );
                let finding_end = finding_start.saturating_add(2).min(end);
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ESCAPE_UNTERMINATED,
                        Severity::Warning,
                        source_id,
                        finding_start,
                        finding_end,
                        format!(
                            "invalid escape sequence: {}",
                            visible_bytes(bytes.get(offset..).unwrap_or_default())
                        ),
                    ),
                    truncated,
                );
            }
            Some(result.bytes)
        }
        Err(EnvironmentError::ExpansionLimit) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::LIMIT_EXPANSION_STEPS,
                    Severity::Warning,
                    source_id,
                    start,
                    end,
                    "roff environment expansion exceeds max_expansion_steps",
                ),
                truncated,
            );
            None
        }
        Err(EnvironmentError::RecursionLimit) => {
            let reference_offset = bytes
                .windows(2)
                .position(|window| window == [escape, b'*'])
                .unwrap_or(0);
            let finding_start = source_offset_or_line_start(
                start,
                end,
                u32::try_from(reference_offset).expect("parser bounds every expanded source line"),
            );
            let finding_end = finding_start.saturating_add(2).min(end);
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::LIMIT_EXPANSION_STEPS,
                    Severity::Error,
                    source_id,
                    finding_start,
                    finding_end,
                    "input stack limit exceeded, infinite loop?",
                ),
                truncated,
            );
            Some(Vec::new())
        }
        Err(EnvironmentError::OutputLimit) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ESCAPE_OUTPUT_LIMIT,
                    Severity::Warning,
                    source_id,
                    start,
                    end,
                    "roff environment output exceeds max_expanded_line_bytes",
                ),
                truncated,
            );
            None
        }
        Err(error) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                environment_error_diagnostic(error, source_id, start, end),
                truncated,
            );
            Some(bytes.to_vec())
        }
    }
}

/// Translate an offset in expansion input back into its owning physical span.
///
/// Physical source lines preserve their direct escape columns.  A user macro,
/// however, may execute an arbitrary-length stored body while its diagnostic
/// span belongs to the short invocation line.  Those generated offsets have no
/// source location in that invocation; mandoc's compatible recovery is the
/// line start.  Crucially, never let that projection invert a public span.
fn source_offset_or_line_start(start: u32, end: u32, offset: u32) -> u32 {
    start
        .checked_add(offset)
        .filter(|candidate| *candidate <= end)
        .unwrap_or(start)
}

/// Locate the next source-spelled missing string interpolation in one input
/// line. Environment expansion deliberately owns recursive and dynamic names,
/// so it returns only missing names; this pass restores mandoc's direct
/// reference column while a nested definition safely falls back to line start.
pub(super) fn next_missing_reference_offset(
    bytes: &[u8],
    escape: u8,
    name: &[u8],
    cursor: &mut usize,
) -> Option<usize> {
    while *cursor < bytes.len() {
        let offset = bytes[*cursor..]
            .iter()
            .position(|byte| *byte == escape)
            .map(|relative| cursor.saturating_add(relative))?;
        if bytes.get(offset + 1) != Some(&b'*') {
            *cursor = offset.saturating_add(1);
            continue;
        }
        let name_start = offset.saturating_add(2);
        let (candidate, next) = match bytes.get(name_start).copied() {
            Some(b'[') => {
                let content_start = name_start.saturating_add(1);
                match bytes[content_start..].iter().position(|byte| *byte == b']') {
                    Some(relative_end) => {
                        let content_end = content_start.saturating_add(relative_end);
                        (
                            &bytes[content_start..content_end],
                            content_end.saturating_add(1),
                        )
                    }
                    None => (&bytes[content_start..], bytes.len()),
                }
            }
            Some(b'(') if bytes.len() >= name_start.saturating_add(3) => {
                let content_start = name_start.saturating_add(1);
                let content_end = content_start.saturating_add(2);
                (&bytes[content_start..content_end], content_end)
            }
            Some(_) => (
                &bytes[name_start..name_start.saturating_add(1)],
                name_start.saturating_add(1),
            ),
            None => return None,
        };
        *cursor = next;
        if candidate == name {
            return Some(offset);
        }
    }
    None
}

pub(super) fn environment_error_diagnostic(
    error: EnvironmentError,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
) -> Diagnostic {
    let (code, message) = match error {
        EnvironmentError::DefinitionLimit => (
            DiagnosticCode::ROFF_DEFINITION_LIMIT,
            "roff environment definition count exceeds max_definitions",
        ),
        EnvironmentError::DefinitionBytesLimit => (
            DiagnosticCode::ROFF_DEFINITION_BYTES_LIMIT,
            "roff environment definition bytes exceed max_definition_bytes",
        ),
        EnvironmentError::RegisterExpression => (
            DiagnosticCode::ROFF_REGISTER_EXPRESSION,
            "number-register expression is not an integral basic-unit value",
        ),
        EnvironmentError::ExpansionLimit => (
            DiagnosticCode::LIMIT_EXPANSION_STEPS,
            "roff environment expansion exceeds max_expansion_steps",
        ),
        EnvironmentError::RecursionLimit => (
            DiagnosticCode::LIMIT_EXPANSION_STEPS,
            "input stack limit exceeded, infinite loop?",
        ),
        EnvironmentError::OutputLimit => (
            DiagnosticCode::ESCAPE_OUTPUT_LIMIT,
            "roff environment output exceeds max_expanded_line_bytes",
        ),
    };
    diagnostic(code, Severity::Warning, source_id, start, end, message)
}

#[allow(clippy::too_many_arguments)] // Translation shares the established source-aware recovery boundary.
pub(super) fn translate_visible(
    environment: &Environment,
    bytes: &[u8],
    escape: u8,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Option<Vec<u8>> {
    match environment.translate_text(bytes, escape, limits.max_expanded_line_bytes) {
        Ok(bytes) => Some(bytes),
        Err(error) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                environment_error_diagnostic(error, source_id, start, end),
                truncated,
            );
            None
        }
    }
}

pub(super) fn diagnostic(
    code: &'static str,
    severity: Severity,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    message: impl Into<Box<str>>,
) -> Diagnostic {
    let code = DiagnosticCode::new(code).expect("static diagnostic code is valid");
    let span = SourceSpan::new(source_id, start, end).expect("scanner spans are monotonic");
    Diagnostic::new(code, severity, message).with_primary(span)
}

/// Keep the one byte accepted by roff's `.cc`, `.c2`, and `.ec` requests.
///
/// The scanner has already applied that first byte to the subsequent physical
/// input stream. This public-AST projection additionally mirrors mandoc's
/// validator: attached or later operands are discarded and produce one
/// source-precise excess-argument diagnostic.
pub(super) fn normalize_character_request_arguments(
    request: &[u8],
    arguments: &mut Vec<Argument>,
    source_id: crate::SourceId,
    argument_start: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let Some(first) = arguments.first() else {
        return;
    };
    let (excess_offset, excess_bytes) = if first.bytes.len() > 1 {
        (first.offset.saturating_add(1), &first.bytes[1..])
    } else if let Some(second) = arguments.get(1) {
        (second.offset, second.bytes.as_slice())
    } else {
        return;
    };
    let excess = visible_bytes(excess_bytes);
    let start = argument_start
        .checked_add(
            u32::try_from(excess_offset).expect("argument offsets are bounded by line length"),
        )
        .expect("parser checks public span offsets first");
    let end = start
        .checked_add(
            u32::try_from(excess_bytes.len()).expect("argument bytes are bounded by line length"),
        )
        .expect("parser checks public span offsets first");
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
                "skipping excess arguments: {} ... {excess}",
                visible_bytes(request)
            ),
        ),
        truncated,
    );
    if let Some(first) = arguments.first_mut() {
        first.bytes.truncate(1);
    }
    arguments.truncate(1);
}

/// Validate and retain the declared-name state for a roff `.char` request.
///
/// libmandoc excludes these formatter definitions from the package AST. Its
/// old parser nevertheless validates the left operand independently of the
/// replacement string and carries unknown bracketed names into later escape
/// recovery, which is the observable contract preserved here.
#[allow(clippy::too_many_arguments)]
pub(super) fn validate_character_request(
    raw_arguments: &[u8],
    escape: u8,
    environment: &mut Environment,
    source_id: crate::SourceId,
    argument_start: u32,
    line_end: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let arguments = match lex_arguments(raw_arguments, escape, limits) {
        Ok(arguments) => arguments,
        Err(ArgumentIssue::UnterminatedQuote) => {
            push_diagnostic(
                diagnostics,
                limits,
                diagnostic(
                    DiagnosticCode::ARGUMENT_UNTERMINATED_QUOTE,
                    Severity::Warning,
                    source_id,
                    line_end,
                    line_end,
                    "roff char arguments contain an unterminated quote",
                ),
                truncated,
            );
            return;
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
                    line_end,
                    line_end,
                    "roff char arguments exceed configured parser limits",
                ),
                truncated,
            );
            return;
        }
    };
    let Some(first) = arguments.first() else {
        emit_invalid_character_argument(
            raw_arguments,
            source_id,
            line_end,
            line_end,
            limits,
            diagnostics,
            truncated,
        );
        return;
    };
    let start = argument_start
        .checked_add(
            u32::try_from(first.offset).expect("argument offsets are bounded by line length"),
        )
        .expect("parser checks public span offsets first");
    if let Some(name) = bracketed_character_name(&first.bytes, escape) {
        environment.declare_character(name);
        emit_invalid_declared_character_warning(
            name,
            escape,
            source_id,
            start,
            limits,
            diagnostics,
            truncated,
        );
        if first.bytes.len() == name.len().saturating_add(3) {
            environment.define_character(name, join_arguments(&arguments[1..]));
            return;
        }
    }
    if first.bytes.len() == 1 {
        environment.define_character(&first.bytes, join_arguments(&arguments[1..]));
        return;
    }
    emit_invalid_character_argument(
        raw_arguments,
        source_id,
        start,
        line_end,
        limits,
        diagnostics,
        truncated,
    );
}

/// Return the leading `\\[name]` spelling, even when an invalid request
/// attaches trailing bytes that must separately produce an argument error.
pub(super) fn bracketed_character_name(bytes: &[u8], escape: u8) -> Option<&[u8]> {
    let remainder = bytes.strip_prefix(&[escape, b'['])?;
    let close = remainder.iter().position(|byte| *byte == b']')?;
    (!remainder[..close].is_empty()).then_some(&remainder[..close])
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_declared_character_escape_warnings(
    bytes: &[u8],
    escape: u8,
    environment: &Environment,
    source_id: crate::SourceId,
    line_start: u32,
    line_end: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let mut occurrences = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != escape {
            cursor += 1;
            continue;
        }
        let Some(name) = bracketed_character_name(&bytes[cursor..], escape) else {
            cursor += 1;
            continue;
        };
        if environment.has_declared_character(name) {
            occurrences.push((cursor, name));
        }
        cursor = cursor.saturating_add(name.len()).saturating_add(3);
    }
    // libmandoc emits multiple unknown special-character findings from one
    // source line in reverse encounter order.
    for (offset, name) in occurrences.into_iter().rev() {
        let start = line_start
            .checked_add(u32::try_from(offset).expect("line bytes fit source offsets"))
            .expect("parser checks public span offsets first");
        let _ = line_end;
        emit_invalid_declared_character_warning(
            name,
            escape,
            source_id,
            start,
            limits,
            diagnostics,
            truncated,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_invalid_declared_character_warning(
    name: &[u8],
    escape: u8,
    source_id: crate::SourceId,
    start: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let spelling = format!("{}[{}]", char::from(escape), visible_bytes(name));
    let end = start
        .checked_add(
            u32::try_from(name.len().saturating_add(3)).expect("name bytes fit source offsets"),
        )
        .expect("parser checks public span offsets first");
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ESCAPE_UNKNOWN_SPECIAL_CHARACTER,
            Severity::Warning,
            source_id,
            start,
            end,
            format!("invalid escape sequence: {spelling}"),
        ),
        truncated,
    );
}

/// Replace only names previously accepted by `.char`, retaining the original
/// source bytes for diagnostics. Formatter font escapes in a character value
/// receive mandoc's synthetic reset before following literal source flow.
pub(super) fn expand_declared_character_escapes(
    bytes: &[u8],
    escape: u8,
    environment: &Environment,
) -> Vec<u8> {
    let mut expanded = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != escape {
            if let Some(value) = environment.character_definition(&bytes[cursor..=cursor]) {
                expanded.extend_from_slice(value);
            } else {
                expanded.push(bytes[cursor]);
            }
            cursor += 1;
            continue;
        }
        let Some(name) = bracketed_character_name(&bytes[cursor..], escape) else {
            expanded.push(bytes[cursor]);
            cursor += 1;
            continue;
        };
        let consumed = name.len().saturating_add(3);
        if let Some(value) = environment.character_definition(name) {
            expanded.extend_from_slice(value);
            if value.starts_with(&[escape, b'f']) {
                expanded.extend_from_slice(&[escape, b'f', b'P']);
            }
        } else {
            expanded.extend_from_slice(&bytes[cursor..cursor.saturating_add(consumed)]);
        }
        cursor = cursor.saturating_add(consumed);
    }
    expanded
}

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_invalid_character_argument(
    raw_arguments: &[u8],
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    let display = visible_bytes(raw_arguments);
    push_diagnostic(
        diagnostics,
        limits,
        diagnostic(
            DiagnosticCode::ROFF_INVALID_CHARACTER_ARGUMENT,
            Severity::Error,
            source_id,
            start,
            end,
            format!("argument is not a character: char {display}"),
        ),
        truncated,
    );
}

pub(super) fn visible_bytes(bytes: &[u8]) -> String {
    decode_visible_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::source_offset_or_line_start;

    #[test]
    fn expansion_offsets_outside_the_invocation_fall_back_to_line_start() {
        assert_eq!(source_offset_or_line_start(40, 80, 12), 52);
        assert_eq!(source_offset_or_line_start(40, 80, 41), 40);
        assert_eq!(
            source_offset_or_line_start(u32::MAX - 2, u32::MAX, 4),
            u32::MAX - 2
        );
    }
}
