//! options recognition; complete forms retain their role-specific grammar.
use super::forms;
use crate::definitions::{RecognizedName, evidence::HeadOperands};
#[cfg(test)]
use mant_ir::DefinitionItem;
use mant_ir::Inline;
use mant_ir::inline_plain_text as plain_text;

#[cfg(test)]
pub(in crate::definitions) fn option_names(item: &DefinitionItem) -> Vec<String> {
    option_names_from_terms(&item.terms)
}

pub(crate) fn option_names_from_terms(terms: &[impl AsRef<[Inline]>]) -> Vec<String> {
    let mut names = Vec::new();
    for found in option_occurrences_from_terms(terms).into_iter().flatten() {
        if !names.contains(&found.name) {
            names.push(found.name);
        }
    }
    names
}

pub(crate) fn option_occurrences_from_terms(
    terms: &[impl AsRef<[Inline]>],
) -> Vec<Vec<RecognizedName>> {
    option_occurrences_with_operands(terms, HeadOperands::Absent)
}

fn option_occurrences_with_operands(
    terms: &[impl AsRef<[Inline]>],
    operands: HeadOperands<'_>,
) -> Vec<Vec<RecognizedName>> {
    scan_option_occurrences(terms, operands).occurrences
}

pub(super) struct OptionOccurrences {
    pub(super) occurrences: Vec<Vec<RecognizedName>>,
    pub(super) limit: Option<super::DeclarationLimit>,
}

fn scan_option_occurrences(
    terms: &[impl AsRef<[Inline]>],
    operands: HeadOperands<'_>,
) -> OptionOccurrences {
    let mut result = OptionOccurrences {
        occurrences: Vec::with_capacity(terms.len()),
        limit: None,
    };
    for (index, term) in terms.iter().enumerate() {
        let Some(operands) = operands.for_term(index) else {
            result.occurrences.push(Vec::new());
            continue;
        };
        let scanned = super::scan::option_head(term.as_ref(), operands);
        result.limit = result.limit.or(scanned.limit);
        result.occurrences.push(scanned.names);
    }
    result
}

/// A validated native Fl head proves punctuation is invocation spelling.
/// Read it before generic separator grouping can treat the comma in `-,` as
/// alias punctuation; styled arguments still stop the literal prefix.
pub(super) fn native_option_occurrences(
    terms: &[impl AsRef<[Inline]>],
    operands: HeadOperands<'_>,
) -> OptionOccurrences {
    let mut result = scan_option_occurrences(terms, operands);
    if result.limit.is_some() {
        return result;
    }
    for (term, names) in terms.iter().zip(&mut result.occurrences) {
        let prefix = forms::literal_prefix(term.as_ref());
        let Some(token) = prefix.split_whitespace().next() else {
            continue;
        };
        retain_native_short_option(token, prefix.len() - prefix.trim_start().len(), names);
    }
    result
}

fn retain_native_short_option(token: &str, start: usize, names: &mut Vec<RecognizedName>) {
    let mut chars = token.chars();
    if chars.next() == Some('-')
        && chars
            .next()
            .is_some_and(|c| !c.is_whitespace() && !c.is_control())
        && chars.next().is_none()
        && !names.iter().any(|found| found.name == token)
    {
        names.insert(0, RecognizedName::contiguous(token, start));
    }
}

/// Run the option grammar on exactly one accepted Fl scope. The complete
/// scope may contain a key, an attached value or explicit legacy markers.
#[cfg(feature = "roff")]
pub(super) fn native_option_key(word: &str) -> Option<RecognizedName> {
    super::scan::native_option_key(word).or_else(|| {
        let mut names = Vec::new();
        let token = word.split_whitespace().next()?;
        retain_native_short_option(token, word.len() - word.trim_start().len(), &mut names);
        names.pop()
    })
}

/// A TQ term's accepted native key is independent of the original HEAD's
/// role. Its exact ranges survive alongside an independent lexical term.
pub(super) fn retain_continued_option_names(
    terms: &[impl AsRef<[Inline]>],
    operands: HeadOperands<'_>,
    occurrences: &mut [Vec<RecognizedName>],
    limit: &mut Option<super::DeclarationLimit>,
) -> bool {
    let mut found = false;
    for (index, (term, names)) in terms.iter().zip(occurrences).enumerate().skip(1) {
        let Some(ranges) = operands.for_term(index) else {
            continue;
        };
        if !ranges
            .iter()
            .any(|operand| operand.role == crate::definitions::NativeOperandRole::ExplicitOption)
            || !super::scan::has_leading_explicit_option(&plain_text(term.as_ref()), ranges)
        {
            continue;
        }
        let recognized = super::scan::option_head(term.as_ref(), ranges);
        found |= !recognized.names.is_empty();
        *limit = limit.or(recognized.limit);
        *names = recognized.names;
    }
    found
}

pub(in crate::definitions) fn parameter_occurrences(
    terms: &[impl AsRef<[Inline]>],
    operands: HeadOperands<'_>,
) -> Vec<Vec<RecognizedName>> {
    let mut found = option_occurrences_with_operands(terms, operands);
    for (term, names) in terms.iter().zip(&mut found) {
        let text = plain_text(term.as_ref());
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
