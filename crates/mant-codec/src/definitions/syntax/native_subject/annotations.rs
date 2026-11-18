//! Resolve supplemental declaration text using accepted native boundaries.
use std::ops::Range;

use super::{offset, operand_at};
use crate::definitions::syntax::scan::{MAX_NESTING, closing, quote_closer};
use crate::definitions::{NativeOperand, NativeOperandRole, NativeSubject};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Boundary<'a> {
    Native(u32, &'a str),
    Literal(Option<u32>, char),
}

struct OpenMark<'a> {
    boundary: Boundary<'a>,
    enclosing: Option<u32>,
    literal_body: Option<u32>,
}

impl OpenMark<'_> {
    fn closes(&self, close: Boundary<'_>, mixed: bool) -> bool {
        match (self.boundary, close) {
            (Boundary::Native(open, left), Boundary::Native(close, right)) => {
                open == close && (!mixed || native_delimiters_match(left, right))
            }
            (Boundary::Literal(open, left), Boundary::Literal(close, right)) => {
                open == close && mate(left) == Some(right)
            }
            // mdoc_macro.c::blk_part_exp and blk_exp_close can leave bare
            // punctuation outside Eo's HEAD or TAIL. Complete visible
            // parentheses still bound an annotation in this HEAD. That
            // spelling alone never proves an argument's native scope.
            (Boundary::Literal(_, left), Boundary::Native(body, right)) if mixed => {
                self.literal_body.is_none_or(|observed| observed == body)
                    && mate(left) == single_glyph(right)
            }
            (Boundary::Native(_, left), Boundary::Literal(owner, right)) if mixed => {
                owner == self.enclosing && single_glyph(left).and_then(mate) == Some(right)
            }
            _ => false,
        }
    }

    fn accepts_literal_body(&mut self, body: u32) -> bool {
        matches!(self.boundary, Boundary::Literal(..))
            && *self.literal_body.get_or_insert(body) == body
    }
}

#[derive(Clone, Copy)]
pub(super) enum Tail {
    Symbol(NativeOperandRole),
    ArgumentGroup,
    SingleAnnotation(Option<NativeOperandRole>),
}

impl Tail {
    fn allows_plain_text(self, bounded: bool) -> bool {
        bounded || matches!(self, Self::ArgumentGroup)
    }

    fn owns(self, role: NativeOperandRole, bounded: bool) -> bool {
        let (Self::Symbol(owner) | Self::SingleAnnotation(Some(owner))) = self else {
            return false;
        };
        role.subject()
            .zip(owner.subject())
            .is_some_and(|(part, owner)| {
                part == owner
                    || bounded
                        && part.kind == owner.kind
                        && owner.invocation.is_some()
                        && part.invocation == owner.invocation
            })
    }

    fn allows_literal_bounds(self) -> bool {
        !matches!(self, Self::ArgumentGroup)
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
    let mut pending: Vec<OpenMark<'_>> = Vec::new();
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
        if let Some((boundary, opening)) = marker(text, start, operand) {
            if opening {
                if pending.len() == MAX_NESTING
                    || matches!(boundary, Boundary::Literal(owner, _) if owner != enclosure)
                {
                    return false;
                }
                pending.push(OpenMark {
                    boundary,
                    enclosing: enclosure,
                    literal_body: None,
                });
                if let Boundary::Native(body, _) = boundary {
                    enclosure = Some(body);
                }
            } else {
                let Some(mark) = pending.pop() else {
                    return false;
                };
                if !mark.closes(boundary, tail.allows_literal_bounds()) {
                    return false;
                }
                enclosure = mark.enclosing;
                if pending.is_empty() && matches!(tail, Tail::SingleAnnotation(_)) {
                    return text[cursor..span.end].trim().is_empty();
                }
            }
        } else if !(is_supplement(operand.role, enclosure, plain_text)
            || tail.owns(operand.role, !pending.is_empty())
            || tail.allows_literal_bounds()
                && supplementary_body(operand.role).is_some_and(|scope| {
                    pending
                        .last_mut()
                        .is_some_and(|mark| mark.accepts_literal_body(scope))
                }))
        {
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
        let Some((boundary, true)) = marker(text, start, operand) else {
            return false;
        };
        if let Boundary::Native(body, _) = boundary {
            scope = Some(body);
        }
        start = operand.bytes.end;
    }
    false
}

fn marker<'a>(
    text: &'a str,
    start: usize,
    operand: &NativeOperand,
) -> Option<(Boundary<'a>, bool)> {
    let word = &text[operand.bytes.clone()];
    if word.contains('\n') || offset(text, word.trim_start()) != start {
        return None;
    }
    let word = word.trim();
    if word.is_empty() {
        return None;
    }
    match operand.role {
        NativeOperandRole::EnclosureStart(scope) => Some((Boundary::Native(scope, word), true)),
        NativeOperandRole::EnclosureEnd(scope) => Some((Boundary::Native(scope, word), false)),
        NativeOperandRole::LiteralDelimiter {
            glyph,
            enclosure: owner,
        } if single_glyph(word) == Some(glyph) => match glyph {
            '(' | '[' => Some((Boundary::Literal(owner, glyph), true)),
            ')' | ']' => Some((Boundary::Literal(owner, glyph), false)),
            _ => None,
        },
        _ => None,
    }
}

fn single_glyph(word: &str) -> Option<char> {
    let mut chars = word.chars();
    let glyph = chars.next()?;
    chars.next().is_none().then_some(glyph)
}

fn native_delimiters_match(left: &str, right: &str) -> bool {
    // Eo/Ec may put ordinary text inside their markers. Scope alone
    // cannot prove that the visible "(more" and "]tail" form a pair.
    let mut expected = left.chars().map_while(mate).peekable();
    expected.peek().is_none()
        || expected.eq(right.chars().rev().take_while(|character| {
            matches!(
                character,
                ')' | ']' | '}' | '>' | '⟩' | '»' | '"' | '\'' | '”' | '’'
            )
        }))
}

fn supplementary_body(role: NativeOperandRole) -> Option<u32> {
    if let NativeOperandRole::EnclosureContent(owner) = role {
        Some(owner)
    } else {
        role.subject()
            .filter(|receipt| matches!(receipt.kind, NativeSubject::Field | NativeSubject::Type))?
            .enclosure
    }
}

pub(in crate::definitions::syntax) fn trailing_parenthesis(
    text: &str,
    span: Range<usize>,
    operands: &[NativeOperand],
    enclosure: Option<u32>,
    owner: Option<NativeOperandRole>,
) -> bool {
    let rest = text[span.clone()].trim();
    let Some(body) = rest
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(')'))
    else {
        return false;
    };
    if body.trim().is_empty() || body.contains(['\r', '\n']) {
        return false;
    }
    let start = offset(text, rest);
    operand_at(operands, start)
        .and_then(|operand| marker(text, start, operand))
        .is_some_and(|(_, opening)| opening)
        && suffix(
            text,
            &span,
            operands,
            enclosure,
            Tail::SingleAnnotation(owner),
        )
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
) -> bool {
    let start = tail.start;
    let Some(rest) = text.get(tail) else {
        return false;
    };
    rest.is_empty() || rest.starts_with(char::is_whitespace)
        || rest.starts_with(|character| mate(character).is_some())
            && operand_at(operands, start).and_then(|operand| marker(text, start, operand)).is_some_and(|(_, opening)| opening)
        // A punctuation-only suffix is independent of where styles split
        // its bytes. `SYS.cache` remains joined even across two receipts.
        || rest.split_whitespace().next().is_some_and(|word| word.chars().all(is_closing_punctuation))
}

fn mate(opening: char) -> Option<char> {
    closing(opening)
        .or_else(|| quote_closer(opening))
        .or(match opening {
            '⟨' => Some('⟩'),
            '«' => Some('»'),
            _ => None,
        })
}

fn is_closing_punctuation(value: char) -> bool {
    // mdoc.c::mdoc_isdelim moves these closing punctuation words outside
    // the inline macro. Parentheses/brackets need both authored marks.
    matches!(value, '.' | ';' | ':' | '?' | '!')
}
