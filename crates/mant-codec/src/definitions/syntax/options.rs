//! options recognition; complete forms retain their role-specific grammar.
use super::forms;
use crate::definitions::RecognizedName;
#[cfg(test)]
use mant_ir::DefinitionItem;
use mant_ir::inline_plain_text as plain_text;
use mant_ir::{ContentContext, Inline};

#[cfg(test)]
pub(in crate::definitions) fn option_names(
    content: ContentContext<'_>,
    item: &DefinitionItem,
) -> Vec<String> {
    option_names_from_terms(content, &item.terms)
}

#[cfg(test)]
pub(crate) fn option_names_from_terms(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Vec<String> {
    let mut names = Vec::new();
    for found in option_occurrences_from_terms(content, terms)
        .into_iter()
        .flatten()
    {
        if !names.contains(&found.name) {
            names.push(found.name);
        }
    }
    names
}

fn recognize_option_occurrences_from_terms(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Vec<Vec<RecognizedName>> {
    terms
        .iter()
        .map(|term| {
            forms::AuthoredForm::new(content, term)
                .option_candidates()
                .filter_map(|candidate| {
                    let (token, start) = candidate.invocation_token()?;
                    Some(RecognizedName::contiguous(option_prefix(&token)?, start))
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn option_occurrences_from_terms(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Vec<Vec<RecognizedName>> {
    recognize_option_occurrences_from_terms(content, terms)
}

/// Recognize the option declarations in one complete literal leaf.
///
/// Markdown entry discovery uses this instead of manufacturing a detached
/// `Inline::Code` node with its own copy of the visible text.
pub(crate) fn option_occurrences_from_literal(value: &str) -> Vec<RecognizedName> {
    forms::literal_option_tokens(value)
        .into_iter()
        .filter_map(|(token, start)| {
            Some(RecognizedName::contiguous(option_prefix(&token)?, start))
        })
        .collect()
}

pub(crate) fn option_names_from_literal(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    for found in option_occurrences_from_literal(value) {
        if !names.contains(&found.name) {
            names.push(found.name);
        }
    }
    names
}

/// A validated native Fl head proves punctuation is invocation spelling.
/// Read it before generic separator grouping can treat the comma in `-,` as
/// alias punctuation; styled arguments still stop the literal prefix.
pub(super) fn native_option_occurrences(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Vec<Vec<RecognizedName>> {
    let mut result = recognize_option_occurrences_from_terms(content, terms);
    for (term, names) in terms.iter().zip(&mut result) {
        let prefix = forms::literal_prefix(content, term);
        let Some(token) = prefix.split_whitespace().next() else {
            continue;
        };
        if token.starts_with('-')
            && token.chars().count() == 2
            && token
                .chars()
                .nth(1)
                .is_some_and(|c| !c.is_whitespace() && !c.is_control())
            && !names.iter().any(|found| found.name == token)
        {
            names.insert(
                0,
                RecognizedName::contiguous(token, prefix.len() - prefix.trim_start().len()),
            );
        }
    }
    result
}

pub(in crate::definitions) fn parameter_occurrences(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
) -> Vec<Vec<RecognizedName>> {
    let mut found = recognize_option_occurrences_from_terms(content, terms);
    for (term, names) in terms.iter().zip(&mut found) {
        let text = plain_text(content, term);
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
        } else if token.strip_prefix('+').is_some_and(is_option_name_body) {
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

#[cfg(test)]
mod literal_tests {
    use super::{option_names_from_literal, option_occurrences_from_literal};

    #[test]
    fn literal_entry_api_preserves_alias_argument_and_pair_rules() {
        assert_eq!(option_names_from_literal("-h, --help"), ["-h", "--help"]);
        assert_eq!(option_names_from_literal("--set=KEY,VALUE"), ["--set"]);
        assert_eq!(
            option_names_from_literal("-q or --quiet"),
            ["-q", "--quiet"]
        );
        assert!(option_occurrences_from_literal("ordinary prose").is_empty());
    }
}
