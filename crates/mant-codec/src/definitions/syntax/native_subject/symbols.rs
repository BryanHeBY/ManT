//! Bind the symbol of each accepted Dv or Er group.
use super::{Recognition, annotations, offset, plain_identifier};
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
        let name = complete_word(text, second, &(first.bytes.end..head.end))?;
        let ordinal_end = offset(text, word) + word.len();
        if text.get(ordinal_end..offset(text, name))?.is_empty() {
            // Sm can join the ordinal and the next operand. The vanished
            // space would leave an identifier suffix inside one word.
            return None;
        }
        (name, second)
    } else {
        (word, first)
    };
    let name = plain_identifier(name)?;
    let name_end = offset(text, name) + name.len();
    if !annotations::has_word_boundary(text, name_end..head.end, operands, declaration.enclosure) {
        // termp_ns_pre and Sm off can join authored operands into one
        // visible word. A typed suffix does not certify a name prefix.
        return None;
    }
    annotations::suffix(
        text,
        &(last.bytes.end..head.end),
        operands,
        declaration.enclosure,
        annotations::Tail::Symbol(first.role),
    )
    .then(|| RecognizedName::contiguous(name, offset(text, name)))
}

fn is_decimal_number(word: &str) -> bool {
    let digits = word.strip_prefix(['+', '-', '\u{2212}']).unwrap_or(word);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
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
