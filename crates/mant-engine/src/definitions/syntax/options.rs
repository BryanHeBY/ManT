//! options recognition; complete forms retain their role-specific grammar.
use super::forms;
use crate::definitions::RecognizedName;
use crate::inline::plain_text;
use mant_ir::{DefinitionItem, EntryKind, Inline, NameCase};

pub(in crate::definitions) fn parameter_identity(
    item: &DefinitionItem,
    first_term: &str,
) -> (EntryKind, NameCase, Vec<String>) {
    if first_term == "--" || first_term == "--%" {
        return (
            EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Marker,
            },
            NameCase::Sensitive,
            vec![first_term.to_owned()],
        );
    }
    if first_term == "-" {
        return (
            EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Operand,
            },
            NameCase::Sensitive,
            vec![first_term.to_owned()],
        );
    }
    let names = parameter_names(item);
    if names.is_empty() {
        (EntryKind::Term, NameCase::Sensitive, Vec::new())
    } else {
        (
            EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            NameCase::Sensitive,
            names,
        )
    }
}

pub(in crate::definitions) fn parameter_names(item: &DefinitionItem) -> Vec<String> {
    let mut names = option_names(item);
    for found in parameter_occurrences(&item.terms).into_iter().flatten() {
        if !names.contains(&found.name) {
            names.push(found.name);
        }
    }
    names
}

pub(in crate::definitions) fn option_names(item: &DefinitionItem) -> Vec<String> {
    option_names_from_terms(&item.terms)
}

pub(crate) fn option_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    let mut names = Vec::new();
    for found in option_occurrences_from_terms(terms).into_iter().flatten() {
        if !names.contains(&found.name) {
            names.push(found.name);
        }
    }
    names
}

pub(crate) fn option_occurrences_from_terms(terms: &[Vec<Inline>]) -> Vec<Vec<RecognizedName>> {
    terms
        .iter()
        .map(|term| {
            forms::AuthoredForm::new(term)
                .option_candidates()
                .filter_map(|candidate| {
                    let (token, start) = candidate.invocation_token()?;
                    Some(RecognizedName::contiguous(option_prefix(&token)?, start))
                })
                .collect()
        })
        .collect()
}

pub(in crate::definitions) fn parameter_occurrences(
    terms: &[Vec<Inline>],
) -> Vec<Vec<RecognizedName>> {
    let mut found = option_occurrences_from_terms(terms);
    for (term, names) in terms.iter().zip(&mut found) {
        let text = plain_text(term);
        let Some(token) = text.split_whitespace().next() else {
            continue;
        };
        let start = token.as_ptr() as usize - text.as_ptr() as usize;
        if let Some(body) = token.strip_prefix("[-+]")
            && is_option_name_body(body)
        {
            for (sign, offset) in [('-', 1), ('+', 2)] {
                names.push(RecognizedName {
                    name: format!("{sign}{body}"),
                    parts: vec![
                        start + offset..start + offset + 1,
                        start + 4..start + token.len(),
                    ],
                });
            }
        } else if token.strip_prefix('+').is_some_and(is_option_name_body)
            || matches!(token, "--" | "--%" | "-")
        {
            names.push(RecognizedName::contiguous(token, start));
        }
    }
    found
}

/// Recognize legacy slash-separated dash options, not general alias syntax.
/// Every non-final segment must be a complete option name: a slash inside an
/// argument path or assignment RHS must never start a new invocation. Only
/// the final invocation may carry an argument (validated by the caller).
pub(crate) fn slash_option_forms(value: &str) -> Option<Vec<&str>> {
    if !value.contains('/') {
        return None;
    }
    let parts: Vec<_> = value.split('/').collect();
    let (last, preceding) = parts.split_last()?;
    if !preceding
        .iter()
        .all(|part| option_prefix(part) == Some(*part))
    {
        return None;
    }
    let token = last.split_whitespace().next()?;
    let name = option_prefix(token)?;
    (token == name || token[name.len()..].starts_with('=')).then_some(parts)
}

pub(crate) fn option_prefix(token: &str) -> Option<&str> {
    if !token.starts_with('-') || token == "-" {
        return None;
    }
    let end = token
        .char_indices()
        .skip(1)
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?' | '.' | '+')
        })
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let candidate = &token[..end];
    let body = candidate.trim_start_matches('-');
    is_option_name_body(body).then_some(candidate)
}

pub(in crate::definitions) fn is_option_name_body(value: &str) -> bool {
    value.split('.').all(|segment| {
        !segment.is_empty()
            && segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?' | '+')
            })
            && segment
                .chars()
                .any(|character| character.is_ascii_alphanumeric() || character == '?')
    })
}
