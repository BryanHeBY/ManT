//! Resolve supplemental declaration text using accepted native boundaries.
use std::ops::Range;

use super::{offset, operand_at};
use crate::definitions::syntax::scan::MAX_NESTING;
use crate::definitions::{NativeOperand, NativeOperandRole, NativeSubject};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Boundary {
    Native(u32),
    Literal(char),
}

#[derive(Clone, Copy)]
pub(super) enum Tail {
    Symbol(NativeOperandRole),
    ArgumentGroup,
}

impl Tail {
    fn allows_plain_text(self, bounded: bool) -> bool {
        bounded || matches!(self, Self::ArgumentGroup)
    }

    fn owns(self, role: NativeOperandRole) -> bool {
        matches!(self, Self::Symbol(owner) if role.subject() == owner.subject())
    }
}

/// A field or symbol may have punctuation, descriptive text, parameters,
/// and bounded annotations after its subject. Within a proved enclosure,
/// ordinary text and semantic styles (for example `Ic config`) also belong
/// to the annotation. A separate native declaration is never supplemental.
pub(super) fn suffix(
    text: &str,
    span: &Range<usize>,
    operands: &[NativeOperand],
    mut enclosure: Option<u32>,
    tail: Tail,
) -> bool {
    let mut pending = Vec::new();
    let mut cursor = span.start;
    while let Some(rest) = text.get(cursor..span.end).map(str::trim_start) {
        let Some(first) = rest.chars().next() else {
            return pending.is_empty();
        };
        let start = offset(text, rest);
        let index = operands.partition_point(|operand| operand.bytes.end <= start);
        let receipt = operands.get(index);
        let plain_text = tail.allows_plain_text(!pending.is_empty());
        let Some(operand) = receipt.filter(|operand| operand.bytes.contains(&start)) else {
            cursor = if plain_text {
                receipt.map_or(span.end, |operand| operand.bytes.start.min(span.end))
            } else if is_closing_punctuation(first) {
                start + first.len_utf8()
            } else {
                return false;
            };
            continue;
        };
        cursor = operand.bytes.end.min(span.end);
        if !text[cursor..operand.bytes.end].trim().is_empty() {
            return false;
        }
        if let Some((boundary, opening)) = marker(text, start, operand, enclosure) {
            if opening {
                if pending.len() == MAX_NESTING {
                    return false;
                }
                pending.push((boundary, enclosure));
                if let Boundary::Native(body) = boundary {
                    enclosure = Some(body);
                }
            } else if let Some((expected, enclosing)) = pending.pop()
                && expected == boundary
            {
                enclosure = enclosing;
            } else {
                return false;
            }
        } else if !is_supplement(operand.role, enclosure, plain_text) && !tail.owns(operand.role) {
            return false;
        }
    }
    false
}

pub(super) fn argument_group(
    text: &str,
    span: &Range<usize>,
    operands: &[NativeOperand],
    enclosure: Option<u32>,
) -> bool {
    let mut start = span.start;
    let mut scope = enclosure;
    for _ in 0..=MAX_NESTING {
        let Some(rest) = text.get(start..span.end).map(str::trim_start) else {
            return false;
        };
        start = offset(text, rest);
        let Some(operand) = operand_at(operands, start) else {
            return false;
        };
        if operand.role.subject().is_some_and(|receipt| {
            receipt.kind == NativeSubject::Field && receipt.enclosure == scope
        }) {
            // Native and literal opening marks can both precede the field.
            // The suffix pass proves their closing marks in the same HEAD.
            return suffix(text, span, operands, enclosure, Tail::ArgumentGroup);
        }
        let Some((boundary, true)) = marker(text, start, operand, scope) else {
            return false;
        };
        if let Boundary::Native(body) = boundary {
            scope = Some(body);
        }
        start = operand.bytes.end;
    }
    false
}

fn marker(
    text: &str,
    start: usize,
    operand: &NativeOperand,
    enclosure: Option<u32>,
) -> Option<(Boundary, bool)> {
    let word = &text[operand.bytes.clone()];
    if word.contains('\n') || offset(text, word.trim_start()) != start {
        return None;
    }
    match operand.role {
        NativeOperandRole::EnclosureStart(scope) => Some((Boundary::Native(scope), true)),
        NativeOperandRole::EnclosureEnd(scope) => Some((Boundary::Native(scope), false)),
        NativeOperandRole::LiteralDelimiter {
            glyph,
            enclosure: owner,
        } if owner == enclosure && word.trim().chars().eq([glyph]) => match glyph {
            '(' => Some((Boundary::Literal(')'), true)),
            '[' => Some((Boundary::Literal(']'), true)),
            ')' | ']' => Some((Boundary::Literal(glyph), false)),
            _ => None,
        },
        _ => None,
    }
}

fn is_supplement(role: NativeOperandRole, enclosure: Option<u32>, plain_text: bool) -> bool {
    role == NativeOperandRole::Description
        || role.is_operand_label()
        || role.subject().is_some_and(|receipt| {
            matches!(receipt.kind, NativeSubject::Field | NativeSubject::Type)
                && receipt.enclosure == enclosure
        })
        || plain_text
            && match role {
                NativeOperandRole::EnclosureContent(owner) => enclosure == Some(owner),
                NativeOperandRole::Literal => true,
                _ => false,
            }
}

pub(super) fn has_word_boundary(
    text: &str,
    tail: Range<usize>,
    operands: &[NativeOperand],
    enclosure: Option<u32>,
) -> bool {
    let start = tail.start;
    let Some(rest) = text.get(tail) else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
        || operand_at(operands, start).and_then(|operand| marker(text, start, operand, enclosure)).is_some_and(|(_, opening)| opening)
        // A punctuation-only suffix is independent of where styles split
        // its bytes. `SYS.cache` remains joined even across two receipts.
        || rest.split_whitespace().next().is_some_and(|word| word.chars().all(is_closing_punctuation))
}

fn is_closing_punctuation(value: char) -> bool {
    // mdoc.c::mdoc_isdelim moves these closing punctuation words outside
    // the inline macro. Parentheses/brackets need both authored marks.
    matches!(value, '.' | ';' | ':' | '?' | '!')
}
