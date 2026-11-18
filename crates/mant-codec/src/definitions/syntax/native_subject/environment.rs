//! Recognize Ev names from accepted operands and their own annotation.
use super::{annotations, offset, operand_at};
use crate::definitions::syntax::{eligibility::OperandUses, named};
use crate::definitions::{NativeOperand, NativeOperandRole, RecognizedName};
use mant_ir::{Inline, inline_plain_text};
use std::ops::Range;

pub(in crate::definitions::syntax) fn environment_names(
    term: &[Inline],
    operands: &[NativeOperand],
) -> Option<Vec<RecognizedName>> {
    if !operands.iter().any(|operand| is_environment(operand.role)) {
        return None;
    }
    let text = inline_plain_text(term);
    let uses = OperandUses::new(operands);
    Some(recognize(&text, operands, uses).unwrap_or_default())
}

fn recognize(
    text: &str,
    operands: &[NativeOperand],
    uses: OperandUses<'_>,
) -> Option<Vec<RecognizedName>> {
    // in_line() retains punctuation inside one Ev word, but emits
    // standalone delimiters as their own nodes. An Env word may have
    // literal parentheses in its name or punctuation in its value.
    let mut environment_word = None;
    let mut parts = named::split_named_groups(text, |at| {
        if uses.protects_syntax(at) {
            return true;
        }
        let Some(operand) = operand_at(operands, at).filter(|operand| is_environment(operand.role))
        else {
            return false;
        };
        if !matches!(text.as_bytes()[at], b',' | b'|') {
            return true;
        }
        let start = operand.bytes.start;
        // The delimiter walk is in byte order. Scan each word once
        // even if a long assignment contains many separators.
        if environment_word.is_none_or(|(previous, _)| previous != start) {
            environment_word = Some((start, text[operand.bytes.clone()].find(['=', '('])));
        }
        environment_word
            .and_then(|(_, marker)| marker)
            .is_some_and(|marker| start + marker < at)
    })?
    .into_iter()
    .peekable();
    let mut names = Vec::new();
    while let Some(part) = parts.next() {
        let first = part.trim();
        let start = offset(text, first);
        let mut end = start + first.len();
        let leading = operand_at(operands, start).map(|operand| operand.role);
        let annotation = annotation_start(text, &(start..end), operands);
        if text[start..annotation.unwrap_or(end)].contains('=') {
            // A lexical separator in the RHS keeps belonging to the
            // assignment until a distinct executed Ev starts a new atom.
            // The parser's invocation ID is independent of source coords.
            while let Some(next) = parts.peek()
                && !independent_environment(text, next, operands, leading, start)
            {
                let value = parts.next()?.trim_end();
                end = offset(text, value) + value.len();
            }
        }
        let range = start..end;
        if let Some(name) = name_in_atom(text, &range, operands, leading, uses) {
            names.push(RecognizedName::contiguous(&name, start));
        } else if !leading.is_some_and(is_environment)
            && !named::environment_template(&text[range.clone()])
            && !uses.protects(&range)
        {
            // Unproved ordinary prose still vetoes lexical aliases.
            return None;
        }
    }
    Some(names)
}

fn independent_environment(
    text: &str,
    next: &str,
    operands: &[NativeOperand],
    previous: Option<NativeOperandRole>,
    assignment_start: usize,
) -> bool {
    let Some(NativeOperandRole::Environment { enclosure, .. }) = previous else {
        return false;
    };
    let offset_of_next = offset(text, next.trim_start());
    let index = operands.partition_point(|operand| operand.bytes.end <= offset_of_next);
    let Some(operand) = operands.get(index) else {
        return false;
    };
    let NativeOperandRole::Environment {
        invocation: Some(current),
        enclosure: current_enclosure,
    } = operand.role
    else {
        return false;
    };
    if current_enclosure != enclosure
        || offset(text, text[operand.bytes.clone()].trim_start()) != offset_of_next
    {
        return false;
    }
    // If the next word is a continuation of an Ev already supplying the
    // RHS, its invocation can differ from the original assignment macro.
    // Punctuation only closes the parser's element, not that invocation.
    operands[..index]
        .iter()
        .rev()
        .take_while(|operand| operand.bytes.end > assignment_start)
        .find_map(|operand| match operand.role {
            NativeOperandRole::Environment {
                invocation,
                enclosure: owner,
            }
            | NativeOperandRole::ClippedEnvironment {
                invocation,
                enclosure: owner,
            } if owner == enclosure => Some(invocation),
            _ => None,
        })
        .is_some_and(|previous| previous.is_some_and(|previous| previous != current))
}

fn name_in_atom(
    text: &str,
    range: &Range<usize>,
    operands: &[NativeOperand],
    leading: Option<NativeOperandRole>,
    uses: OperandUses<'_>,
) -> Option<String> {
    if matches!(leading, Some(NativeOperandRole::ClippedEnvironment { .. })) {
        return None;
    }
    let part = if let Some(start) = annotation_start(text, range, operands) {
        // Start the check at the first real delimiter, not at an opening
        // glyph that belongs to an Ev spelling such as BASH_FUNC_home().
        let Some(NativeOperandRole::Environment { enclosure, .. }) = leading else {
            return None;
        };
        if !annotations::trailing_parenthesis(text, start..range.end, operands, enclosure, None) {
            return None;
        }
        &text[range.start..start]
    } else {
        &text[range.clone()]
    };
    named::environment_atom(part.trim_end())
        .or_else(|| environment_with_operand_suffix(text, part, uses))
}

fn annotation_start(text: &str, range: &Range<usize>, operands: &[NativeOperand]) -> Option<usize> {
    let start = operands.partition_point(|operand| operand.bytes.end <= range.start);
    operands[start..]
        .iter()
        .take_while(|operand| operand.bytes.start < range.end)
        .find(|operand| {
            matches!(
                operand.role,
                NativeOperandRole::EnclosureStart { .. }
                    | NativeOperandRole::EnclosureEnd(_)
                    | NativeOperandRole::LiteralDelimiter { .. }
                    | NativeOperandRole::ClippedEnclosure
            )
        })
        .map(|operand| offset(text, text[operand.bytes.clone()].trim_start()))
}

fn environment_with_operand_suffix(
    text: &str,
    part: &str,
    uses: OperandUses<'_>,
) -> Option<String> {
    let (name, suffix) = part.trim_end().split_once(char::is_whitespace)?;
    // A separate Ar operand remains supplemental even when an earlier
    // alias uses a styled annotation. Ns would have removed this split.
    suffix
        .split_whitespace()
        .all(|word| {
            let start = offset(text, word);
            uses.covers(
                &(start..start + word.len()),
                NativeOperandRole::is_operand_label,
            )
        })
        .then(|| named::environment_variable_alias(name))
        .flatten()
}

fn is_environment(role: NativeOperandRole) -> bool {
    matches!(
        role,
        NativeOperandRole::Environment { .. } | NativeOperandRole::ClippedEnvironment { .. }
    )
}
