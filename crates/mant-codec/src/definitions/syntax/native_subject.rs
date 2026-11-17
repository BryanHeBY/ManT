//! Name only the accepted operands of a typed mdoc definition head.
use super::eligibility::{NameSource, OperandUses};
use super::scan::{MAX_NAMES, MAX_NESTING, closing, quote_closer};
use super::{DeclarationLimit, named, occurrences::HeadNames};
use crate::definitions::{
    NativeOperand, NativeOperandRole, NativeSubject, RecognizedName, evidence::HeadOperands,
};
use mant_ir::{DefinitionItem, EntryKind, inline_plain_text};
use std::ops::Range;

mod declaration_parts;

pub(super) fn recognize_head(
    item: &DefinitionItem,
    first: &str,
    operands: HeadOperands<'_>,
) -> Option<(EntryKind, HeadNames)> {
    let (kind, first) = recognize(first, operands.for_term(0)?)?;
    let mut limit = first.limit;
    let mut occurrences = vec![first.names];
    for (index, term) in item.terms.iter().enumerate().skip(1) {
        let mut names = Vec::new();
        if let Some(ranges) = operands.for_term(index)
            && let Some((_, term)) = recognize(&inline_plain_text(term), ranges)
        {
            names = term.names;
            limit = limit.or(term.limit);
        }
        occurrences.push(names);
    }
    Some((kind, HeadNames { occurrences, limit }))
}

#[derive(Default)]
struct Recognition {
    names: Vec<RecognizedName>,
    limit: Option<DeclarationLimit>,
}

impl Recognition {
    fn one(name: Option<RecognizedName>) -> Self {
        Self {
            names: name.into_iter().collect(),
            limit: None,
        }
    }
}

fn recognize(text: &str, operands: &[NativeOperand]) -> Option<(EntryKind, Recognition)> {
    let (head, _) = unwrapped(text, text, operands);
    let start = offset(text, head);
    let first = operands.partition_point(|operand| operand.bytes.end <= start);
    let leading = operands
        .get(first)
        .filter(|operand| operand.bytes.contains(&start));
    let subject = leading
        .and_then(|operand| operand.role.subject())
        .map(|(subject, _)| subject);
    if matches!(
        subject,
        Some(NativeSubject::Constant | NativeSubject::Error)
    ) {
        return recognize_part(text, head, operands);
    }
    let typed = matches!(
        subject,
        Some(
            NativeSubject::Variable
                | NativeSubject::ReturnType
                | NativeSubject::Function
                | NativeSubject::FunctionBlock
        )
    );
    let protected = operands.iter().any(|operand| operand.role.is_search())
        || leading.is_some_and(|operand| operand.role.is_operand_label())
            && operands[first..]
                .iter()
                .any(|operand| operand.role == NativeOperandRole::ExplicitOption);
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
    recognize_part(text, head, operands)
}

fn recognize_part(
    text: &str,
    head: &str,
    operands: &[NativeOperand],
) -> Option<(EntryKind, Recognition)> {
    let mut start = offset(text, head);
    let end = start + head.len();
    let mut first = operands.partition_point(|operand| operand.bytes.end <= start);
    let lead = operands
        .get(first)
        .filter(|operand| operand.bytes.contains(&start))?;
    if lead.role.is_operand_label()
        || matches!(lead.role, NativeOperandRole::SearchArgument(_))
            && head.len() < text.trim().len()
    {
        // Ar can label a real definition or a placeholder in a syntax
        // expression. Its spelling alone proves neither a command nor an
        // environment variable. A surviving generated enclosure gives an
        // enclosed search form the same scope for independent declarations.
        // The role check still prevents a search argument from naming one.
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
        return Some((EntryKind::Term, Recognition { names, limit: None }));
    }
    let (mut subject, _) = lead.role.subject()?;
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
            let symbols = symbol_names(text, start..end, &operands[first..], subject);
            return Some((EntryKind::Term, symbols));
        }
        NativeSubject::Type | NativeSubject::Field => {
            // A type/field can be the subject of a term, but the type
            // spelling never supplies a variable or command identity.
            subject_span(text, &operands[first..], start..end)
                .and_then(|span| simple_binding(text, span, plain_identifier))
        }
        NativeSubject::Variable => subject_span(text, &operands[first..], start..end)
            .and_then(|span| simple_binding(text, span, declaration_name)),
        NativeSubject::Function | NativeSubject::FunctionBlock => {
            let span = matching_subject_span(text, &operands[first..], start..end);
            span.and_then(|span| {
                if operands[first..]
                    .iter()
                    .skip_while(|operand| operand.bytes.start < span.end)
                    .take_while(|operand| operand.bytes.start < end)
                    .any(|operand| {
                        operand.role.subject().is_some_and(|(role, _)| {
                            matches!(
                                role,
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

fn following_function(
    text: &str,
    operands: &[NativeOperand],
    head: Range<usize>,
) -> Option<(usize, NativeSubject)> {
    let descriptor = matching_subject_span(text, operands, head)?;
    let index = operands.partition_point(|operand| operand.bytes.end <= descriptor.end);
    let next = operands.get(index)?;
    let NativeOperandRole::Subject(
        subject @ (NativeSubject::Function | NativeSubject::FunctionBlock),
        _,
    ) = next.role
    else {
        return None;
    };
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

fn symbol_names(
    text: &str,
    head: Range<usize>,
    operands: &[NativeOperand],
    subject: NativeSubject,
) -> Recognition {
    let uses = OperandUses::new(operands).shifted(head.start);
    let Some(groups) = named::named_groups(&text[head], uses) else {
        return Recognition::default();
    };
    let mut result = Recognition::default();
    for group in groups {
        let group = group.trim();
        let start = offset(text, group);
        let first = operands.partition_point(|operand| operand.bytes.end <= start);
        // An attached Fa can annotate a symbol. A separate Fa part starts
        // an invocation's argument tail, including any later Dv sentinel.
        if subject == NativeSubject::Constant
            && !result.names.is_empty()
            && operands.get(first).is_some_and(|operand| {
                operand.bytes.contains(&start)
                    && operand
                        .role
                        .subject()
                        .is_some_and(|(role, _)| role == NativeSubject::Field)
            })
        {
            break;
        }
        let recognized = symbol_group(
            text,
            start..start + group.len(),
            &operands[first..],
            subject,
        );
        if recognized.limit.is_some() || result.names.len() + recognized.names.len() > MAX_NAMES {
            return Recognition {
                limit: Some(DeclarationLimit::Names),
                ..Recognition::default()
            };
        }
        result.names.extend(recognized.names);
    }
    result
}

fn symbol_group(
    text: &str,
    head: Range<usize>,
    operands: &[NativeOperand],
    subject: NativeSubject,
) -> Recognition {
    let mut names = Vec::new();
    let mut cursor = head.start;
    let mut previous_scope = None;
    let mut limit = None;
    for operand in operands
        .iter()
        .take_while(|operand| operand.bytes.end <= head.end)
    {
        let Some((role, scope)) = operand.role.subject() else {
            continue;
        };
        let start = operand.bytes.start.max(head.start);
        let Some(gap) = text.get(cursor..start).map(str::trim) else {
            return Recognition::default();
        };
        if role == NativeSubject::Field
            && subject == NativeSubject::Constant
            && !names.is_empty()
            && gap.is_empty()
        {
            cursor = operand.bytes.end;
            continue;
        }
        if role != subject || !matches!(operand.role, NativeOperandRole::Subject(..)) {
            continue;
        }
        let Some(name) = text
            .get(start..operand.bytes.end)
            .and_then(|word| plain_identifier(word.trim()))
        else {
            continue;
        };
        if !gap.is_empty() || !names.is_empty() && previous_scope != Some(scope) {
            return Recognition::default();
        }
        let name = RecognizedName::contiguous(name, offset(text, name));
        if !OperandUses::new(operands).permits(&name, NameSource::Operand(operand.role)) {
            continue;
        }
        if names.len() == MAX_NAMES {
            limit = Some(DeclarationLimit::Names);
        } else {
            names.push(name);
        }
        cursor = operand.bytes.end;
        previous_scope = Some(scope);
    }
    if text
        .get(cursor..head.end)
        .is_some_and(|suffix| suffix.trim().is_empty())
    {
        if limit.is_some() {
            names.clear();
        }
        Recognition { names, limit }
    } else {
        Recognition::default()
    }
}

fn unwrapped<'a>(
    text: &'a str,
    part: &'a str,
    operands: &[NativeOperand],
) -> (&'a str, Option<u32>) {
    let mut head = part.trim();
    let mut enclosure = None;
    for _ in 0..MAX_NESTING {
        let Some(front) = head.chars().next() else {
            break;
        };
        let Some(pair) = closing(front)
            .or_else(|| quote_closer(front))
            .or_else(|| (front == '⟨').then_some('⟩'))
        else {
            break;
        };
        let start = offset(text, head);
        let Some(inner) = head[front.len_utf8()..].strip_suffix(pair) else {
            break;
        };
        // Both glyphs must survive from the same enclosure's native
        // pre/post. Sibling scopes can emit the same paired punctuation.
        let Some(opening) = operand_at(operands, start)
            .filter(|operand| text[operand.bytes.clone()].trim().len() == front.len_utf8())
        else {
            break;
        };
        let NativeOperandRole::EnclosureStart(scope) = opening.role else {
            break;
        };
        if !operand_at(operands, start + head.len() - pair.len_utf8()).is_some_and(|operand| {
            operand.role == NativeOperandRole::EnclosureEnd(scope)
                && text[operand.bytes.clone()].trim().len() == pair.len_utf8()
        }) {
            break;
        }
        head = inner.trim();
        enclosure = Some(scope);
    }
    (head, enclosure)
}

fn operand_at(operands: &[NativeOperand], offset: usize) -> Option<&NativeOperand> {
    operands
        .get(operands.partition_point(|operand| operand.bytes.end <= offset))
        .filter(|operand| operand.bytes.contains(&offset))
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
