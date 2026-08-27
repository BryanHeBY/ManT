use super::{
    Argument, ArgumentIssue, Diagnostic, DocumentBuilder, Environment, EnvironmentError, Limits,
    MacroSet, Scanner, ScopeLine, expand_copy_mode_definition, is_builtin_package_macro,
    join_arguments, lex_arguments, strip_inline_comment,
};

pub(super) fn is_environment_request(name: &[u8]) -> bool {
    // `.ad`, `.ftr`, `.na`, `.ne`, `.nh`, `.pl`, and `.ps` are formatter-side state in libmandoc.
    // Classifying them here consumes the requests without exposing AST nodes;
    // the shared no-op fallback in `apply_environment_request` is their
    // intentional semantic implementation.
    matches!(
        name,
        b"ds"
            | b"as"
            | b"nr"
            | b"rr"
            | b"rm"
            | b"rn"
            | b"als"
            | b"ad"
            | b"ftr"
            | b"na"
            | b"ne"
            | b"nh"
            | b"pl"
            | b"ps"
    )
}

/// Split a copy-reparsed roff request with scanner-equivalent input-comment tails.
///
/// Macro replay bypasses the physical scanner, so request-local input comments
/// must be removed before a package macro or roff state request observes them.
pub(super) fn split_macro_control(bytes: &[u8], control: u8, escape: u8) -> Option<(&[u8], &[u8])> {
    let remainder = trim_horizontal_space(bytes.strip_prefix(&[control])?);
    let name_end = remainder
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(remainder.len());
    let name = &remainder[..name_end];
    let arguments = trim_horizontal_space(strip_inline_comment(&remainder[name_end..], escape));
    (!name.is_empty()).then_some((name, arguments))
}

/// Recognize comments after a copy-mode macro body has been re-dispatched.
///
/// Physical input reaches `Scanner::next_line`, which already handles both
/// the standard `."` spelling and the active escape-character variant. Macro
/// bodies bypass that scanner path, so they need the equivalent guard before
/// treating a copied comment as a normal roff request.
pub(super) fn is_macro_comment_request(name: &[u8], escape: u8) -> bool {
    name == b"\"" || name == [escape, b'"']
}

/// Return the one-based source column of a control stored in a copy-mode
/// macro body.  The body is replayed at its caller's physical span, so this
/// is used only as logical provenance on generated public nodes.
pub(super) fn macro_body_control_column(bytes: &[u8], control: u8) -> u32 {
    let Some(remainder) = bytes.strip_prefix(&[control]) else {
        return 1;
    };
    let leading = remainder
        .iter()
        .take_while(|byte| matches!(**byte, b' ' | b'\t'))
        .count();
    u32::try_from(leading)
        .expect("bounded macro body whitespace fits public source columns")
        .saturating_add(2)
}

/// Whether a macro begins with the unconditional self-call that mandoc treats
/// as an input-stack recursion rather than ordinary nested macro depth.
pub(super) fn macro_definition_directly_invokes(
    definition: &crate::roff::MacroDefinition,
    name: &[u8],
    control: u8,
) -> bool {
    definition.lines.first().is_some_and(|line| {
        let line = copy_mode_reparse(line, b'\\');
        split_macro_control(&line, control, b'\\')
            .is_some_and(|(request, arguments)| request == name && arguments.is_empty())
    })
}

pub(super) fn is_macro_terminator(bytes: &[u8], control: u8) -> bool {
    bytes.starts_with(&[control, b'.'])
        && bytes
            .get(2..)
            .is_none_or(|remaining| remaining.is_empty() || remaining[0].is_ascii_whitespace())
}

/// Whether a copy-mode macro definition ends at the selected request name.
///
/// Traditional `..` always remains a valid closer, even when a custom
/// delimiter was supplied. A custom delimiter is also a request name and may
/// carry trailing argument text, as in `.end-marker explanatory words`.
pub(super) fn is_definition_terminator(bytes: &[u8], control: u8, marker: &[u8]) -> bool {
    if is_macro_terminator(bytes, control) {
        return true;
    }
    if marker == b"." {
        return false;
    }
    let Some(remainder) = bytes.strip_prefix(&[control]) else {
        return false;
    };
    let remainder = trim_horizontal_space(remainder);
    let name_end = remainder
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(remainder.len());
    remainder[..name_end] == *marker
}

pub(super) fn ignore_marker(
    raw_arguments: &[u8],
    escape: u8,
    limits: &Limits,
) -> Result<Vec<u8>, ArgumentIssue> {
    let arguments = lex_arguments(raw_arguments, escape, limits)?;
    Ok(arguments
        .first()
        .map_or_else(|| vec![b'.'], |argument| argument.bytes.clone()))
}

pub(super) fn consume_ignore_block(scanner: &mut Scanner<'_>, marker: &[u8]) {
    while let Some(ignored) = scanner.next_raw_line() {
        if is_ignore_terminator(ignored.bytes, scanner.control_character(), marker) {
            break;
        }
    }
}

pub(super) fn is_scope_ignore_terminator(line: &ScopeLine, marker: &[u8]) -> bool {
    let ScopeLine::Control {
        name, arguments, ..
    } = line
    else {
        return false;
    };
    name == marker && arguments.iter().all(u8::is_ascii_whitespace)
}

/// Whether one physical line closes a roff `.ig` block.
///
/// The default marker is the traditional `..`; an explicit marker is a
/// request name following the active control character.  Both forms accept
/// trailing horizontal whitespace but no trailing argument text.
pub(super) fn is_ignore_terminator(bytes: &[u8], control: u8, marker: &[u8]) -> bool {
    if marker == b"." {
        return is_macro_terminator(bytes, control);
    }
    let Some(remainder) = bytes.strip_prefix(&[control]) else {
        return false;
    };
    let remainder = trim_horizontal_space(remainder);
    let name_end = remainder
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(remainder.len());
    remainder[..name_end] == *marker
        && remainder[name_end..]
            .iter()
            .all(|byte| matches!(*byte, b' ' | b'\t'))
}

pub(super) fn trim_horizontal_space(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|byte| !matches!(*byte, b' ' | b'\t'))
        .unwrap_or(bytes.len());
    &bytes[start..]
}

pub(super) fn copy_mode_reparse(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut reparsed = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        // A bracketed string or register name remains copy-mode opaque until
        // the environment resolves that reference.  In particular, in
        // `\\*[std\\\\esc]`, the inner doubled delimiter denotes a literal
        // delimiter in the *name*; collapsing it here would turn the result
        // into `\\e` and make the later name resolver consume the `e`.
        if matches!(bytes.get(cursor + 1), Some(b'*' | b'n'))
            && bytes.get(cursor) == Some(&escape)
            && bytes.get(cursor + 2) == Some(&b'[')
        {
            let end = bracketed_reference_name_end(bytes, cursor + 3).unwrap_or(bytes.len());
            reparsed.extend_from_slice(&bytes[cursor..end]);
            cursor = end;
            continue;
        }
        if bytes[cursor] == escape && bytes.get(cursor + 1) == Some(&escape) {
            // A doubled outer delimiter does become active in copy mode, but
            // retain a following bracketed reference name verbatim for the
            // same reason as the active form above.
            if matches!(bytes.get(cursor + 2), Some(b'*' | b'n'))
                && bytes.get(cursor + 3) == Some(&b'[')
            {
                let end = bracketed_reference_name_end(bytes, cursor + 4).unwrap_or(bytes.len());
                reparsed.push(escape);
                reparsed.extend_from_slice(&bytes[cursor + 2..end]);
                cursor = end;
                continue;
            }
            reparsed.push(escape);
            cursor += 2;
        } else {
            reparsed.push(bytes[cursor]);
            cursor += 1;
        }
    }
    reparsed
}

/// Reparse a user-macro argument for delayed `\$` substitution.
///
/// Roff's argument reader treats an embedded literal quote as data when it
/// appears inside an unquoted argument.  If that value is later substituted
/// into a quoted macro-body argument, leaving the byte literal would instead
/// terminate the surrounding quote during the second parse.  mandoc rewrites
/// that injected byte as its standard `\(dq` spelling; escaped source quotes
/// are already explicit roff controls and remain untouched.
pub(super) fn macro_argument_copy_mode_reparse(bytes: &[u8], escape: u8) -> Vec<u8> {
    let mut bytes = copy_mode_reparse(bytes, escape);
    // An active delimiter at physical end of a macro invocation is roff's
    // line-continuation marker.  Keeping it would escape the first literal
    // byte after `\$n` in the macro body (commonly the closing `)`), whereas
    // mandoc consumes it before delayed argument substitution.
    if bytes.last() == Some(&escape) {
        bytes.pop();
    }
    let mut reparsed = Vec::with_capacity(bytes.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] == escape && cursor + 1 < bytes.len() {
            reparsed.extend_from_slice(&bytes[cursor..cursor + 2]);
            cursor += 2;
            continue;
        }
        if bytes[cursor] == b'"' {
            reparsed.extend_from_slice(&[escape, b'(', b'd', b'q']);
        } else {
            reparsed.push(bytes[cursor]);
        }
        cursor += 1;
    }
    reparsed
}

/// Return the exclusive end of the bracketed name whose first byte follows
/// the opening `[`; an unterminated name is left to the normal escape recovery
/// path rather than being partially reparsed here.
pub(super) fn bracketed_reference_name_end(bytes: &[u8], name_start: usize) -> Option<usize> {
    bytes
        .get(name_start..)?
        .iter()
        .position(|byte| *byte == b']')
        .map(|offset| name_start + offset + 1)
}

/// Detect the copy-mode spelling whose provenance cannot be recovered from
/// the public text alone: both `\t` and `\\t` can project as `\t` after
/// reparsing, but only the latter is authored literal text.
pub(super) fn has_protected_tabulation_escape(bytes: &[u8], escape: u8) -> bool {
    bytes
        .windows(3)
        .any(|window| window == [escape, escape, b't'])
}

pub(super) fn apply_environment_request(
    environment: &mut Environment,
    builder: &mut DocumentBuilder,
    request: &[u8],
    escape: u8,
    arguments: &[Argument],
    limits: &Limits,
) -> Result<(), EnvironmentError> {
    let result = match request {
        b"ds" | b"as" => {
            if let Some((name, value)) = arguments.split_first() {
                environment.define_string(
                    &name.bytes,
                    &join_arguments(value),
                    request == b"as",
                    limits,
                )
            } else {
                Ok(())
            }
        }
        b"nr" => {
            // mandoc's `.nr` accepts only a literal space after the register
            // name.  A tab terminates the name but makes the whole request a
            // no-op; preserve that request-specific distinction from the
            // scanner rather than weakening generic argument lexing.
            if arguments.first().and_then(|name| name.separator_after) == Some(b'\t') {
                Ok(())
            } else {
                let Some((name, expression, increment)) = number_register_arguments(arguments)
                else {
                    return Ok(());
                };
                environment.define_register(&name.bytes, &expression, increment, limits)
            }
        }
        b"rr" => {
            // Unlike `.rm`, legacy `.rr` accepts exactly one register name.
            // Additional tokens (including one separated by a tab) are not
            // independent removals.
            if let Some(name) = arguments.first() {
                // A non-literal escape in a register name is diagnosed, then
                // mandoc removes the valid name prefix.  In contrast `.nr`
                // itself leaves such a definition untouched.
                let name = malformed_register_name_prefix(&name.bytes).unwrap_or(&name.bytes);
                environment.remove_register(name);
            }
            Ok(())
        }
        b"rm" => {
            for argument in arguments {
                let normalized = normalize_roff_name_prefix(&argument.bytes, escape);
                // A later invocation of a user macro that `.rm` removed is
                // not ordinary unknown roff syntax: mandoc drops it and
                // reports the deleted callable spelling.  Strings and
                // registers do not receive that request-level treatment.
                let removed_macro = environment.macro_removal_is_diagnosable(&normalized.name);
                environment.remove(&normalized.name);
                if removed_macro {
                    environment.suppress_macro_name(&normalized.name);
                }
                // A prohibited escape is diagnosed by the request dispatcher.
                // Mandoc still removes the valid prefix, but abandons the
                // remaining names in the same `.rm` request.
                if normalized.invalid_escape_preview.is_some() {
                    break;
                }
                // `.rm` accepts a space-delimited name list. A tab ends the
                // first name but leaves the remaining tail outside that list.
                if argument.separator_after == Some(b'\t') {
                    break;
                }
            }
            Ok(())
        }
        b"rn" => {
            // roff's rename request requires a literal space after the old
            // name. A tab there makes the whole request a no-op; tabs after
            // the new name are merely ignored tail input.
            if arguments.first().and_then(|old| old.separator_after) == Some(b'\t') {
                if let Some(new) = arguments.get(1) {
                    // The request is rejected, but mandoc still remembers
                    // the attempted target as a user macro spelling and
                    // diagnoses a later call instead of retaining it as an
                    // arbitrary roff element.
                    environment.suppress_macro_name(&new.bytes);
                }
                return Ok(());
            }
            if let [old, new, ..] = arguments {
                environment.rename(&old.bytes, &new.bytes);
                environment.suppress_macro_name(&old.bytes);
                environment.clear_suppressed_macro_name(&new.bytes);
                if is_builtin_package_macro(builder.macro_set(), &old.bytes) {
                    environment.rename_package_macro(&old.bytes, &new.bytes);
                }
            }
            Ok(())
        }
        b"als" => {
            if let [target, alias, ..] = arguments {
                environment.alias_macro(&target.bytes, &alias.bytes, limits)?;
            }
            Ok(())
        }
        _ => Ok(()),
    };
    if result.is_ok() {
        record_mdoc_synopsis_register_state(builder, environment, request, arguments);
    }
    result
}

/// Return the valid prefix preceding a prohibited escape in a register name.
/// A doubled delimiter is the one literal form and remains part of the name.
pub(super) fn malformed_register_name_prefix(name: &[u8]) -> Option<&[u8]> {
    let mut offset = 0_usize;
    while offset < name.len() {
        if name[offset] != b'\\' {
            offset += 1;
            continue;
        }
        if name.get(offset + 1) == Some(&b'\\') {
            offset += 2;
            continue;
        }
        return Some(&name[..offset]);
    }
    None
}

/// Reconstruct the parenthesized `.nr` value grammar from generic roff
/// argument tokens.  Whitespace terminates ordinary request arguments, but
/// mandoc accepts it inside a parenthesized numeric expression, where the
/// token after the matching close parenthesis becomes the optional increment.
pub(super) type NumberRegisterArguments<'a> = (&'a Argument, Vec<u8>, Option<&'a [u8]>);

pub(super) fn number_register_arguments(
    arguments: &[Argument],
) -> Option<NumberRegisterArguments<'_>> {
    let (name, remainder) = arguments.split_first()?;
    let expression = remainder.first()?;
    if !expression.bytes.contains(&b'(') {
        return Some((
            name,
            expression.bytes.clone(),
            arguments.get(2).map(|increment| increment.bytes.as_slice()),
        ));
    }
    let mut depth = 0_usize;
    for (index, argument) in arguments[1..].iter().enumerate() {
        for byte in &argument.bytes {
            match byte {
                b'(' => depth = depth.saturating_add(1),
                b')' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        if depth == 0 {
            let last = index + 1;
            return Some((
                name,
                join_arguments(&arguments[1..=last]),
                arguments
                    .get(last + 1)
                    .map(|increment| increment.bytes.as_slice()),
            ));
        }
    }
    // An unclosed parenthesis remains a permissive numeric prefix in the
    // legacy evaluator. Its whitespace-separated tail still belongs to that
    // value rather than becoming an accidental increment argument.
    Some((name, join_arguments(&arguments[1..]), None))
}

/// Return the `.nr` expression when its parsed arithmetic contains a `/ 0`
/// or `% 0` operand.  The numeric evaluator deliberately recovers that form
/// to zero so subsequent interpolation remains deterministic; the scanner
/// owns the source-addressable legacy error finding.
pub(super) fn register_division_by_zero(arguments: &[Argument]) -> Option<&Argument> {
    let [name, expression, ..] = arguments else {
        return None;
    };
    if name.separator_after == Some(b'\t') || !has_zero_divisor(&expression.bytes) {
        return None;
    }
    Some(expression)
}

pub(super) fn has_zero_divisor(expression: &[u8]) -> bool {
    expression.iter().enumerate().any(|(index, operator)| {
        if !matches!(operator, b'/' | b'%') {
            return false;
        }
        let mut cursor = index + 1;
        while expression.get(cursor).is_some_and(u8::is_ascii_whitespace) {
            cursor += 1;
        }
        if expression
            .get(cursor)
            .is_some_and(|byte| matches!(byte, b'+' | b'-'))
        {
            cursor += 1;
        }
        let start = cursor;
        while expression.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        cursor > start && expression[start..cursor].iter().all(|digit| *digit == b'0')
    })
}

/// Preserve mdoc's private `nS` execution state across the scanner/semantic
/// boundary.  The `.nr` request itself remains transparent in the public AST;
/// its effect is consumed in source order by the mdoc structural pass.
pub(super) fn record_mdoc_synopsis_register_state(
    builder: &mut DocumentBuilder,
    environment: &Environment,
    request: &[u8],
    arguments: &[Argument],
) {
    if builder.macro_set() != MacroSet::Mdoc
        || request != b"nr"
        || arguments
            .first()
            .is_none_or(|argument| argument.bytes != b"nS")
    {
        return;
    }
    builder.record_mdoc_synopsis_state(
        environment
            .register_value(b"nS")
            .is_some_and(|value| value != 0),
    );
}

/// Apply roff's string-definition syntax without treating data quotes as
/// generic macro-argument delimiters.
///
/// In `.ds name value` and `.as name value`, the first double quote of the
/// value is a copy-mode control character.  It is removed even if no closing
/// quote appears; later quotes are retained literally.  This differs from the
/// argument grammar used by ordinary control macros (and is why this logic is
/// kept at the request boundary).
#[allow(clippy::too_many_arguments)] // Definition-time interpolation shares the session-wide budget and source-relative diagnostics.
pub(super) fn apply_string_request(
    environment: &mut Environment,
    raw_arguments: &[u8],
    escape: u8,
    append: bool,
    limits: &Limits,
    source_id: crate::SourceId,
    start: u32,
    end: u32,
    expansion_steps: &mut usize,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) -> Result<(), EnvironmentError> {
    let raw_arguments = trim_horizontal_space(raw_arguments);
    let mut name_end = 0;
    while name_end < raw_arguments.len() && !raw_arguments[name_end].is_ascii_whitespace() {
        name_end += if raw_arguments[name_end] == escape && name_end + 1 < raw_arguments.len() {
            2
        } else {
            1
        };
    }
    let Some(name) = raw_arguments
        .get(..name_end)
        .filter(|name| !name.is_empty())
    else {
        return Ok(());
    };
    let Some(name) = normalize_roff_definition_name(name, escape) else {
        // The scanner-stage caller emits the source-precise invalid-name
        // diagnostic. A definition with a prohibited escape has no state
        // effect, while a doubled delimiter is retained as one literal byte.
        return Ok(());
    };
    // Roff consumes separating spaces after a definition name, but a tab at
    // this boundary belongs to the copied value.  That distinction is
    // observable later when the string is expanded into filled text.
    let value = &raw_arguments[name_end..];
    let value = &value[value.iter().take_while(|byte| **byte == b' ').count()..];
    let value = value.strip_prefix(b"\"").unwrap_or(value);
    let Some(value) = expand_copy_mode_definition(
        environment,
        value,
        escape,
        limits,
        source_id,
        start,
        end,
        expansion_steps,
        diagnostics,
        truncated,
    ) else {
        return Ok(());
    };
    let value = copy_mode_reparse(&value, escape);
    environment.define_string(&name, &value, append, limits)
}

/// Normalize a roff definition name after its request-specific validation.
/// A doubled delimiter denotes one literal delimiter; every other escape is
/// prohibited in string-definition names.
pub(super) fn normalize_roff_definition_name(name: &[u8], escape: u8) -> Option<Vec<u8>> {
    let normalized = normalize_roff_name_prefix(name, escape);
    normalized
        .invalid_escape_preview
        .is_none()
        .then_some(normalized.name)
}

/// Roff name recovery shared by macro definitions, removals, and control-line
/// dispatch.  A doubled delimiter is one literal byte.  Any other escape is
/// illegal in a name; mandoc keeps the prefix before it for the request's
/// state change and stops inspecting the rest of that name.
#[derive(Debug)]
pub(super) struct NormalizedRoffName {
    pub(super) name: Vec<u8>,
    pub(super) invalid_escape_preview: Option<Vec<u8>>,
}

pub(super) fn normalize_roff_name_prefix(name: &[u8], escape: u8) -> NormalizedRoffName {
    let mut normalized = Vec::with_capacity(name.len());
    let mut cursor = 0_usize;
    while let Some(byte) = name.get(cursor).copied() {
        if byte != escape {
            normalized.push(byte);
            cursor += 1;
            continue;
        }
        if name.get(cursor + 1) == Some(&escape) {
            normalized.push(escape);
            cursor += 2;
            continue;
        }
        // The escaped byte remains part of mandoc's visible diagnostic
        // spelling, including an escaped space or tab.  The recovered name
        // still ends before the escape itself.
        let preview_end = cursor.saturating_add(2).min(name.len());
        return NormalizedRoffName {
            name: normalized,
            invalid_escape_preview: Some(name[..preview_end].to_vec()),
        };
    }
    NormalizedRoffName {
        name: normalized,
        invalid_escape_preview: None,
    }
}

/// A physical control name is normally cut before an adjacent escape by the
/// scanner.  Retain that split for scope delimiters, while recovering the
/// roff-name cases that need it: literal `\\` names and a known macro or
/// definition request followed by a prohibited escape.
#[derive(Debug)]
pub(super) struct AttachedControlName {
    pub(super) name: Vec<u8>,
    pub(super) display_name: Vec<u8>,
    pub(super) arguments: Vec<u8>,
    pub(super) invalid_escape_preview: Option<Vec<u8>>,
}

pub(super) fn recover_attached_control_name(
    name: &[u8],
    raw_arguments: &[u8],
    escape: u8,
    recover_prohibited_escape: bool,
) -> Option<AttachedControlName> {
    let escaped = raw_arguments.strip_prefix(&[escape])?;
    if escaped.first() == Some(&escape) {
        let name_tail_end = raw_arguments
            .iter()
            .position(u8::is_ascii_whitespace)
            .unwrap_or(raw_arguments.len());
        let mut raw_name = Vec::with_capacity(name.len() + name_tail_end);
        raw_name.extend_from_slice(name);
        raw_name.extend_from_slice(&raw_arguments[..name_tail_end]);
        let normalized = normalize_roff_name_prefix(&raw_name, escape);
        return Some(AttachedControlName {
            name: normalized.name,
            display_name: raw_name,
            arguments: trim_horizontal_space(&raw_arguments[name_tail_end..]).to_vec(),
            invalid_escape_preview: normalized.invalid_escape_preview,
        });
    }
    // `\{` opens a roff scope and belongs to the conditional grammar, not a
    // request name.  All other illegal escapes recover the valid prefix.
    if !recover_prohibited_escape || escaped.first() == Some(&b'{') {
        return None;
    }
    let escape_width = roff_escape_name_width(raw_arguments, escape);
    let preview_end = 2.min(raw_arguments.len());
    let mut preview = Vec::with_capacity(name.len() + preview_end);
    preview.extend_from_slice(name);
    preview.extend_from_slice(&raw_arguments[..preview_end]);
    Some(AttachedControlName {
        name: name.to_vec(),
        display_name: name.to_vec(),
        arguments: trim_horizontal_space(&raw_arguments[escape_width..]).to_vec(),
        invalid_escape_preview: Some(preview),
    })
}

pub(super) fn roff_escape_name_width(bytes: &[u8], escape: u8) -> usize {
    debug_assert_eq!(bytes.first(), Some(&escape));
    match bytes.get(1).copied() {
        Some(b'(') => 4.min(bytes.len()),
        Some(b'[') => bytes
            .get(2..)
            .and_then(|tail| tail.iter().position(|byte| *byte == b']'))
            .map_or(bytes.len(), |offset| offset + 3),
        _ => 2.min(bytes.len()),
    }
}
