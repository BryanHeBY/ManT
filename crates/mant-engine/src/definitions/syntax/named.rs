//! named recognition; complete forms retain their role-specific grammar.
use crate::inline::plain_text;
use mant_ir::Inline;

pub(in crate::definitions) fn is_value_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !is_ordinal_marker(value)
        && value.chars().all(|character| {
            character.is_alphanumeric()
                || matches!(character, '-' | '_' | '.' | '/' | ':' | '+' | '?')
        })
}

pub(in crate::definitions) fn is_ordinal_marker(value: &str) -> bool {
    let value = value.trim();
    let digits = if let Some(digits) = value.strip_suffix('.') {
        Some(digits)
    } else if let Some(digits) = value.strip_suffix(')') {
        Some(digits.strip_prefix('(').unwrap_or(digits))
    } else {
        value
            .strip_prefix('[')
            .and_then(|digits| digits.strip_suffix(']'))
    };
    digits.is_some_and(|digits| {
        !digits.is_empty() && digits.chars().all(|character| character.is_ascii_digit())
    })
}

/// Accept a complete semantic name and one delimited trailing annotation.
pub(in crate::definitions) fn named_term_name(
    value: &str,
    validate: fn(&str) -> bool,
) -> Option<&str> {
    let value = value.trim();
    if validate(value) {
        return Some(value);
    }
    let (name, annotation) = value.rsplit_once(" (")?;
    (annotation.ends_with(')') && validate(name)).then_some(name)
}

pub(in crate::definitions) fn environment_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
            // Assignment values may contain alias punctuation. Recognize the
            // complete assignment before considering any alias separators.
            if text.contains('=') {
                return environment_variable_alias(&text)
                    .into_iter()
                    .collect::<Vec<_>>();
            }
            text.split([',', '|'])
                .filter_map(environment_variable_alias)
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

/// Return one exact environment-variable spelling without an authored value.
pub(crate) fn environment_variable_alias(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() || value.contains(['\r', '\n']) {
        return None;
    }
    let (spelling, assignment) = value
        .split_once('=')
        .map_or((value, None), |(name, assignment)| {
            (name.trim_end(), Some(assignment.trim_start()))
        });
    if spelling.is_empty() || spelling.chars().any(char::is_whitespace) {
        return None;
    }
    if assignment.is_some_and(contains_additional_environment_assignment) {
        return None;
    }
    let body = environment_variable_body(spelling)?;
    is_environment_variable_body(body).then(|| spelling.to_owned())
}

pub(in crate::definitions) fn contains_additional_environment_assignment(value: &str) -> bool {
    value
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '|'))
        .any(|token| {
            token.split_once('=').is_some_and(|(name, _)| {
                environment_variable_body(name).is_some_and(is_environment_variable_body)
            })
        })
}

/// Remove a recognized shell/provider wrapper from an environment selector.
pub(crate) fn environment_variable_body(value: &str) -> Option<&str> {
    if let Some(body) = value
        .strip_prefix('%')
        .and_then(|body| body.strip_suffix('%'))
    {
        return Some(body);
    }
    if let Some(body) = value
        .strip_prefix("${")
        .and_then(|body| body.strip_suffix('}'))
    {
        return strip_environment_provider(body);
    }
    if let Some(body) = strip_environment_provider(value) {
        return Some(body);
    }
    Some(value.strip_prefix('$').unwrap_or(value))
}

pub(in crate::definitions) fn strip_environment_provider(value: &str) -> Option<&str> {
    let (provider, body) = value.split_once(':')?;
    provider
        .trim_start_matches('$')
        .eq_ignore_ascii_case("env")
        .then_some(body)
}

pub(in crate::definitions) fn is_environment_variable_body(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '(' | ')')
        })
}

pub(in crate::definitions) fn is_variable_term(value: &str) -> bool {
    let value = value.strip_prefix('$').unwrap_or(value);
    let (head, index) = if let Some((head, tail)) = value.split_once('[') {
        let Some(index) = tail.strip_suffix(']') else {
            return false;
        };
        (head, Some(index))
    } else {
        (value, None)
    };
    !head.is_empty()
        && head
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        && head
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && index.is_none_or(|index| {
            !index.is_empty()
                && index
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}

pub(in crate::definitions) fn is_configuration_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
}
