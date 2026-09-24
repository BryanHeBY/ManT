//! Small source-neutral spelling rules shared by producers and IR validation.
//!
//! These rules recognize names, not roff syntax or terminal geometry. A
//! producer still needs independent owner/markup evidence and a checked
//! binding to visible content before it may publish an entry.

/// Leading ordinary dash-option spelling within one visible token.
#[must_use]
pub fn option_prefix(token: &str) -> Option<&str> {
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

/// Complete spelling licensed by an independently proved native option head.
/// The extra two-character case preserves a single punctuation operand.
#[must_use]
pub fn native_option_token(token: &str) -> bool {
    option_prefix(token) == Some(token)
        || token.starts_with('-')
            && token.chars().count() == 2
            && token
                .chars()
                .nth(1)
                .is_some_and(|character| !character.is_whitespace() && !character.is_control())
}

/// Whether a dash-option body is a finite technical spelling.
#[must_use]
pub fn is_option_name_body(value: &str) -> bool {
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

/// Exact environment-variable spelling without an authored value.
#[must_use]
pub fn environment_variable_alias(value: &str) -> Option<String> {
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

/// Detect a second environment assignment inside a candidate value.
#[must_use]
pub fn contains_additional_environment_assignment(value: &str) -> bool {
    value
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '|'))
        .any(|token| {
            token.split_once('=').is_some_and(|(name, _)| {
                environment_variable_body(name).is_some_and(is_environment_variable_body)
            })
        })
}

/// Remove an accepted shell or provider wrapper from an environment selector.
#[must_use]
pub fn environment_variable_body(value: &str) -> Option<&str> {
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

fn strip_environment_provider(value: &str) -> Option<&str> {
    let (provider, body) = value.split_once(':')?;
    provider
        .trim_start_matches('$')
        .eq_ignore_ascii_case("env")
        .then_some(body)
}

fn is_environment_variable_body(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '(' | ')')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_spelling_rules_reject_unproved_or_malformed_names() {
        assert!(native_option_token("-a"));
        assert!(native_option_token("-,"));
        assert!(!native_option_token("foo"));
        assert_eq!(
            environment_variable_alias("DEMO_HOME=foo"),
            Some("DEMO_HOME".to_owned())
        );
        assert_eq!(environment_variable_alias("a!"), None);
    }
}
