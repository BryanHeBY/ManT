//! Name only the accepted operands of a typed mdoc definition head.
use super::eligibility::{NameSource, OperandUses, operand_at, operand_cursor};
use super::scan::MAX_NESTING;
use super::{DeclarationLimit, named, occurrences::HeadNames};
use crate::definitions::{
    NativeOperand, NativeOperandRole, NativeSubject, RecognizedName, evidence::HeadOperands,
};
use mant_ir::{DefinitionTerm, EntryKind, inline_plain_text};
use std::ops::Range;

mod annotations;
mod declaration_parts;
mod display;
pub(super) use display::infer_display_head;
mod environment;
pub(super) use environment::environment_names;
mod symbols;

pub(super) fn recognize_head(
    terms: &[DefinitionTerm],
    first: &str,
    operands: HeadOperands<'_>,
) -> Option<(EntryKind, HeadNames)> {
    recognize_terms(terms, first, operands, recognize)
}

pub(super) fn recognize_after_separator(
    terms: &[DefinitionTerm],
    first: &str,
    operands: HeadOperands<'_>,
) -> Option<(EntryKind, HeadNames)> {
    recognize_terms(terms, first, operands, |text, ranges| {
        ranges
            .iter()
            .any(|operand| operand.role.subject().is_some())
            .then_some(())?;
        declaration_parts::recognize(text, ranges, declaration_parts::Admission::AfterSeparator)
    })
    .filter(|(_, names)| !names.is_empty())
}

type TermRecognizer = fn(&str, &[NativeOperand]) -> Option<(EntryKind, Recognition)>;

fn recognize_terms(
    terms: &[DefinitionTerm],
    first: &str,
    operands: HeadOperands<'_>,
    recognize: TermRecognizer,
) -> Option<(EntryKind, HeadNames)> {
    let (kind, first) = recognize(first, operands.for_term(0)?)?;
    let mut limit = first.limit;
    let mut semantic = !first.unproved_field;
    let mut occurrences = vec![first.names];
    for (index, term) in terms.iter().enumerate().skip(1) {
        let mut names = Vec::new();
        if let Some(ranges) = operands.for_term(index)
            && let Some((_, term)) = recognize(&inline_plain_text(term), ranges)
        {
            names = term.names;
            limit = limit.or(term.limit);
            semantic |= !term.unproved_field;
        } else {
            semantic = true;
        }
        occurrences.push(names);
    }
    Some((
        kind,
        HeadNames {
            occurrences,
            excluded_forms: Vec::new(),
            limit,
            semantic,
        },
    ))
}

#[derive(Default)]
struct Recognition {
    names: Vec<RecognizedName>,
    limit: Option<DeclarationLimit>,
    unproved_field: bool,
}

impl Recognition {
    fn one(name: Option<RecognizedName>) -> Self {
        Self {
            names: name.into_iter().collect(),
            ..Self::default()
        }
    }
}

fn recognize(text: &str, operands: &[NativeOperand]) -> Option<(EntryKind, Recognition)> {
    let (head, enclosure) = unwrapped(text, text, operands);
    let start = offset(text, head);
    let first = operand_cursor(operands, start);
    let leading = operands
        .get(first)
        .filter(|operand| operand.bytes.contains(&start));
    let protected = operands.iter().any(|operand| operand.role.is_search())
        || leading.is_some_and(|operand| operand.role.is_operand_label())
            && operands[first..]
                .iter()
                .any(|operand| operand.role == NativeOperandRole::ExplicitOption);
    let typed = leading.is_some_and(|operand| {
        operand.role.subject().is_some()
            || !protected
                && matches!(
                    operand.role,
                    NativeOperandRole::EnclosureStart { .. } | NativeOperandRole::ClippedEnclosure
                )
                && operands[first..]
                    .iter()
                    .any(|operand| operand.role.subject().is_some())
    });
    if (typed || protected)
        && let Some(identity) = declaration_parts::recognize(
            text,
            operands,
            if typed {
                declaration_parts::Admission::TypedSubjects
            } else {
                declaration_parts::Admission::ProtectedSyntax
            },
        )
    {
        return Some(identity);
    }
    recognize_part(text, head, operands, enclosure)
}

fn recognize_part(
    text: &str,
    head: &str,
    operands: &[NativeOperand],
    enclosure: Option<u32>,
) -> Option<(EntryKind, Recognition)> {
    let mut start = offset(text, head);
    let end = start + head.len();
    let mut first = operand_cursor(operands, start);
    let lead = operands
        .get(first)
        .filter(|operand| operand.bytes.contains(&start))?;
    if lead.role.is_operand_label()
        || matches!(lead.role, NativeOperandRole::SearchArgument(_))
            && head.len() < text.trim().len()
    {
        return Some((
            EntryKind::Term,
            recognize_operand_label(head, start, operands),
        ));
    }
    let receipt = lead.role.subject()?;
    let mut subject = receipt.kind;
    if subject == NativeSubject::ReturnType {
        // Ft supplies a type before the following function's own name.
        // Keep an isolated or unsupported descriptor as an unnamed term.
        let Some((next, function)) = following_function(text, &operands[first..], start..end)
        else {
            return Some((EntryKind::Term, Recognition::default()));
        };
        first += next;
        start = operands[first].bytes.start;
        subject = function;
    }
    let name = match subject {
        NativeSubject::Constant | NativeSubject::Error => {
            let symbols = symbols::recognize(text, &(start..end), &operands[first..]);
            return Some((EntryKind::Term, symbols));
        }
        NativeSubject::Field if receipt.enclosure != enclosure => {
            // An exact form is independently queryable even without names.
            // Keep this label and its BODY outside the semantic entry index
            // until both bounds prove that its Fa can act as a subject.
            return Some((
                EntryKind::Term,
                Recognition {
                    unproved_field: true,
                    ..Recognition::default()
                },
            ));
        }
        NativeSubject::Type | NativeSubject::Field => {
            // A type/field can be the subject of a term, but the type
            // spelling never supplies a variable or command identity.
            subject_span(text, &operands[first..], start..end)
                .and_then(|span| simple_binding(text, span, plain_identifier))
        }
        NativeSubject::Variable => recognize_variable(text, operands, first, start..end),
        NativeSubject::Function | NativeSubject::FunctionBlock => {
            let span = matching_subject_span(text, &operands[first..], start..end);
            span.and_then(|span| {
                if operands[first..]
                    .iter()
                    .skip_while(|operand| operand.bytes.start < span.end)
                    .take_while(|operand| operand.bytes.start < end)
                    .any(|operand| {
                        operand.role.subject().is_some_and(|receipt| {
                            matches!(
                                receipt.kind,
                                NativeSubject::Function
                                    | NativeSubject::FunctionBlock
                                    | NativeSubject::ReturnType
                            )
                        })
                    })
                {
                    return None;
                }
                let suffix = text[span.end..end].trim().trim_end_matches(';');
                if !suffix.starts_with('(') || !suffix.ends_with(')') {
                    return None;
                }
                let grammar = if subject == NativeSubject::Function {
                    declaration_name
                } else {
                    untyped_name
                };
                simple_binding(text, span, grammar)
            })
        }
        NativeSubject::ReturnType => None,
    };
    let kind = if subject == NativeSubject::Variable {
        EntryKind::Variable
    } else {
        EntryKind::Term
    };
    let uses = OperandUses::new(operands);
    let scope = NameSource::Operand(operands[first].role);
    Some((
        kind,
        Recognition::one(name.filter(|name| uses.permits(name, scope))),
    ))
}

fn recognize_variable(
    text: &str,
    operands: &[NativeOperand],
    first: usize,
    head: Range<usize>,
) -> Option<RecognizedName> {
    let end = head.end;
    let span = matching_subject_span(text, &operands[first..], head)?;
    let role = operands[first].role;
    if !text[span.end..end].trim().is_empty()
        && !annotations::trailing_parenthesis(
            text,
            span.end..end,
            operands,
            role.subject()?.enclosure,
            Some(role),
        )
    {
        return None;
    }
    simple_binding(text, span, declaration_name)
}

fn recognize_operand_label(head: &str, start: usize, operands: &[NativeOperand]) -> Recognition {
    // Ar can label a definition or supply a parameter in a search form.
    // In a proved enclosure only actual labels and names qualify.
    let uses = OperandUses::new(operands).shifted(start);
    let names = named::term_occurrences(head, uses)
        .unwrap_or_default()
        .into_iter()
        .filter(|name| {
            uses.permits(name, NameSource::Operand(NativeOperandRole::Placeholder))
                || uses.permits(name, NameSource::Lexical)
        })
        .map(|mut name| {
            for part in &mut name.parts {
                part.start += start;
                part.end += start;
            }
            name
        })
        .collect();
    Recognition {
        names,
        ..Recognition::default()
    }
}

fn following_function(
    text: &str,
    operands: &[NativeOperand],
    head: Range<usize>,
) -> Option<(usize, NativeSubject)> {
    let descriptor = matching_subject_span(text, operands, head)?;
    let index = operand_cursor(operands, descriptor.end);
    let next = operands.get(index)?;
    let NativeOperandRole::Subject(receipt) = next.role else {
        return None;
    };
    let subject = receipt.kind;
    if !matches!(
        subject,
        NativeSubject::Function | NativeSubject::FunctionBlock
    ) {
        return None;
    }
    text.get(descriptor.end..next.bytes.start)?
        .trim()
        .is_empty()
        .then_some((index, subject))
}

fn subject_span(
    text: &str,
    operands: &[NativeOperand],
    head: Range<usize>,
) -> Option<Range<usize>> {
    let span = matching_subject_span(text, operands, head.clone())?;
    text[span.end..head.end].trim().is_empty().then_some(span)
}

fn matching_subject_span(
    text: &str,
    operands: &[NativeOperand],
    head: Range<usize>,
) -> Option<Range<usize>> {
    let first = operands.first()?;
    if first.bytes.end > head.end
        || first.bytes.start < head.start
            && !text.get(first.bytes.start..head.start)?.trim().is_empty()
    {
        return None;
    }
    let mut span = first.bytes.start.max(head.start)..first.bytes.end;
    for operand in &operands[1..] {
        if operand.role != first.role || operand.bytes.end > head.end {
            break;
        }
        let gap = text.get(span.end..operand.bytes.start)?;
        if !gap.trim().is_empty() {
            break;
        }
        span.end = operand.bytes.end;
    }
    Some(span)
}

fn unwrapped<'a>(
    text: &'a str,
    part: &'a str,
    operands: &[NativeOperand],
) -> (&'a str, Option<u32>) {
    let mut head = part.trim();
    let mut enclosure = None;
    for _ in 0..MAX_NESTING {
        let Some((inner, scope)) = enclosure_inner(text, head, operands) else {
            break;
        };
        head = inner.trim();
        enclosure = Some(scope);
    }
    (head, enclosure)
}

fn enclosure_inner<'a>(
    text: &'a str,
    head: &str,
    operands: &[NativeOperand],
) -> Option<(&'a str, u32)> {
    let start = offset(text, head);
    let end = start + head.len();
    let opening = operand_at(operands, start)?;
    let closing = operand_at(operands, end.checked_sub(1)?)?;
    let NativeOperandRole::EnclosureStart { body: scope, .. } = opening.role else {
        return None;
    };
    if closing.role != NativeOperandRole::EnclosureEnd(scope) {
        return None;
    }
    // Eo/Ec print their authored HEAD/TAIL operands. Match their full
    // accepted ranges and native BODY identity, including delimiters with
    // multiple glyphs, without interpreting their spelling as punctuation.
    let left = text.get(opening.bytes.clone())?;
    let right = text.get(closing.bytes.clone())?;
    if left.contains('\n')
        || right.contains('\n')
        || offset(text, left.trim_start()) != start
        || closing.bytes.start + right.trim_end().len() != end
    {
        return None;
    }
    Some((text.get(opening.bytes.end..closing.bytes.start)?, scope))
}

fn simple_binding(
    text: &str,
    range: Range<usize>,
    grammar: fn(&str) -> Option<&str>,
) -> Option<RecognizedName> {
    let operand = text.get(range)?;
    let name = grammar(operand.trim())?;
    Some(RecognizedName::contiguous(name, offset(text, name)))
}

fn offset(text: &str, part: &str) -> usize {
    part.as_ptr() as usize - text.as_ptr() as usize
}

fn identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn plain_identifier(text: &str) -> Option<&str> {
    identifier(text).then_some(text)
}

fn untyped_name(text: &str) -> Option<&str> {
    (identifier(text)
        && !type_word(text)
        && !qualifier(text)
        && !matches!(text, "struct" | "union" | "enum"))
    .then_some(text)
}

/// Recognize a simple C identifier with a complete type and pointer prefix.
fn declaration_name(text: &str) -> Option<&str> {
    if text.contains('\n') {
        return None;
    }
    let (start, _) = text
        .char_indices()
        .rev()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_')
        .last()?;
    let name = untyped_name(&text[start..])?;
    if start == 0 {
        return Some(name);
    }
    let mut levels = text[..start].split('*');
    let base = levels.next()?.trim();
    let mut types = base.split_whitespace().filter(|word| !qualifier(word));
    let first = types.next()?;
    let valid_base = if matches!(first, "struct" | "union" | "enum") {
        types.next().is_some_and(identifier) && types.next().is_none()
    } else if type_word(first) {
        builtin_type(first, types)
    } else {
        first.ends_with("_t") && identifier(first) && types.next().is_none()
    };
    (valid_base && levels.all(|level| level.split_whitespace().all(qualifier))).then_some(name)
}

fn builtin_type<'a>(mut head: &'a str, mut tail: impl Iterator<Item = &'a str>) -> bool {
    let signed = matches!(head, "signed" | "unsigned");
    if signed {
        let Some(base) = tail.next() else {
            return true;
        };
        head = base;
    }
    match head {
        "char" | "int" => tail.next().is_none(),
        "short" => tail
            .next()
            .is_none_or(|word| word == "int" && tail.next().is_none()),
        "long" => match tail.next() {
            None => true,
            Some("int") => tail.next().is_none(),
            Some("long") => tail
                .next()
                .is_none_or(|word| word == "int" && tail.next().is_none()),
            Some("double") if !signed => tail.next().is_none(),
            _ => false,
        },
        "float" | "double" if !signed => tail
            .next()
            .is_none_or(|word| word == "_Complex" && tail.next().is_none()),
        "void" | "bool" | "_Bool" | "_Complex" if !signed => tail.next().is_none(),
        _ => false,
    }
}

fn qualifier(text: &str) -> bool {
    matches!(
        text,
        "const" | "restrict" | "volatile" | "_Atomic" | "static" | "extern"
    )
}

fn type_word(text: &str) -> bool {
    matches!(
        text,
        "void"
            | "bool"
            | "_Bool"
            | "char"
            | "short"
            | "int"
            | "long"
            | "float"
            | "double"
            | "signed"
            | "unsigned"
            | "_Complex"
    )
}
