//! Bind the symbol of each accepted Dv or Er group.
use super::{Recognition, enclosure_inner, offset, operand_at, plain_identifier};
use crate::definitions::syntax::scan::MAX_NESTING;
use crate::definitions::{NativeOperand, NativeOperandRole, NativeSubject, RecognizedName};
use std::ops::Range;

pub(super) fn recognize(
    text: &str,
    head: &Range<usize>,
    operands: &[NativeOperand],
) -> Recognition {
    Recognition::one(symbol(text, head, operands))
}

fn symbol(text: &str, head: &Range<usize>, operands: &[NativeOperand]) -> Option<RecognizedName> {
    let (first, rest) = operands.split_first()?;
    let NativeOperandRole::Subject(declaration) = first.role else {
        return None;
    };
    let word = complete_word(text, first, head)?;
    // CVS mdoc_macro.c::in_line keeps the errno number and symbol as
    // separate TEXT operands of one Er. Dv has no numeric prefix grammar.
    let (name, last) = if declaration.kind == NativeSubject::Error && is_decimal_number(word) {
        let second = rest.first().filter(|second| second.role == first.role)?;
        (
            complete_word(text, second, &(first.bytes.end..head.end))?,
            second,
        )
    } else {
        (word, first)
    };
    let name = plain_identifier(name)?;
    let name_end = offset(text, name) + name.len();
    if !has_symbol_boundary(text, name_end..head.end, operands) {
        // termp_ns_pre and Sm off can join authored operands into one
        // visible word. A typed suffix does not certify a name prefix.
        return None;
    }
    let annotation = |role: NativeOperandRole, scope| {
        role == NativeOperandRole::Description
            || role.is_operand_label()
            || role.subject().is_some_and(|receipt| {
                receipt == declaration
                    || matches!(receipt.kind, NativeSubject::Field | NativeSubject::Type)
                        && receipt.enclosure == scope
            })
    };
    let tail = text.get(last.bytes.end..head.end)?;
    suffix_is_annotation(
        text,
        tail,
        operands,
        declaration.enclosure,
        MAX_NESTING,
        annotation,
    )
    .then(|| RecognizedName::contiguous(name, offset(text, name)))
}

fn is_decimal_number(word: &str) -> bool {
    let digits = word.strip_prefix(['+', '-', '\u{2212}']).unwrap_or(word);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn has_symbol_boundary(text: &str, tail: Range<usize>, operands: &[NativeOperand]) -> bool {
    let start = tail.start;
    let Some(next) = text.get(tail).and_then(|tail| tail.chars().next()) else {
        return true;
    };
    match operand_at(operands, start) {
        _ if next.is_whitespace() => true,
        Some(NativeOperand {
            role: NativeOperandRole::EnclosureStart(_),
            ..
        }) => true,
        Some(operand) => {
            is_closing_punctuation(next)
                && text[operand.bytes.clone()].trim().len() == next.len_utf8()
        }
        None => is_closing_punctuation(next),
    }
}

fn complete_word<'a>(
    text: &'a str,
    operand: &NativeOperand,
    span: &Range<usize>,
) -> Option<&'a str> {
    let word = text.get(operand.bytes.clone())?.trim();
    let start = offset(text, word);
    (operand.bytes.end <= span.end && text.get(span.start..start)?.trim().is_empty())
        .then_some(word)
}

fn suffix_is_annotation<'text>(
    text: &'text str,
    mut tail: &'text str,
    operands: &[NativeOperand],
    scope: Option<u32>,
    remaining_depth: usize,
    annotation: impl Fn(NativeOperandRole, Option<u32>) -> bool + Copy,
) -> bool {
    let end = offset(text, tail) + tail.len();
    loop {
        tail = tail.trim_start();
        let Some(first) = tail.chars().next() else {
            return true;
        };
        let start = offset(text, tail);
        let next = match operand_at(operands, start) {
            Some(NativeOperand {
                role: NativeOperandRole::EnclosureStart(scope),
                ..
            }) if remaining_depth > 0 => {
                let first = operands.partition_point(|operand| operand.bytes.end <= start);
                let Some(closing) = operands[first..]
                    .iter()
                    .take_while(|operand| operand.bytes.end <= end)
                    .find(|operand| operand.role == NativeOperandRole::EnclosureEnd(*scope))
                else {
                    return false;
                };
                let Some((inner, _)) =
                    enclosure_inner(text, text[start..closing.bytes.end].trim_end(), operands)
                else {
                    return false;
                };
                if !suffix_is_annotation(
                    text,
                    inner,
                    operands,
                    Some(*scope),
                    remaining_depth - 1,
                    annotation,
                ) {
                    return false;
                }
                closing.bytes.end
            }
            Some(operand) if annotation(operand.role, scope) => operand.bytes.end,
            // mdoc.c::mdoc_isdelim moves closing punctuation outside the
            // inline macro. Standalone parens/brackets need enclosure proof.
            _ if is_closing_punctuation(first) => start + first.len_utf8(),
            _ => return false,
        };
        let Some(rest) = text.get(next..end) else {
            return false;
        };
        tail = rest;
    }
}

fn is_closing_punctuation(value: char) -> bool {
    matches!(value, '.' | ';' | ':' | '?' | '!')
}
