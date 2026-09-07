//! options recognition; complete forms retain their role-specific grammar.
use super::forms;
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
    for term in &item.terms {
        let text = plain_text(term);
        let token = text.split_whitespace().next().unwrap_or_default();
        if let Some(body) = token.strip_prefix("[-+]")
            && is_option_name_body(body)
        {
            for prefix in ['-', '+'] {
                let name = format!("{prefix}{body}");
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        } else if let Some(body) = token.strip_prefix('+')
            && is_option_name_body(body)
        {
            let name = format!("+{body}");
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

pub(in crate::definitions) fn option_names(item: &DefinitionItem) -> Vec<String> {
    option_names_from_terms(&item.terms)
}

pub(crate) fn option_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    let mut names = Vec::new();
    for term in terms {
        for candidate in forms::AuthoredForm::new(term).option_candidates() {
            let Some(token) = candidate.invocation_token() else {
                continue;
            };
            let Some(name) = option_prefix(&token) else {
                continue;
            };
            if !names.iter().any(|existing| existing == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
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
