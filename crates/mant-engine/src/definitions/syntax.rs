//! Source-neutral grammars for semantic definition names.
//!
//! These recognizers deliberately consume complete authored forms. Section
//! context decides which grammar to try; this module only decides whether a
//! spelling is trustworthy enough to expose as an addressable entry.

use mant_ir::{DefinitionCase, DefinitionItem, DefinitionRole, Inline};

use crate::inline::plain_text;

use super::{DefinitionContext, key_binding_command_form};

pub(super) fn infer_identity(
    item: &DefinitionItem,
    context: DefinitionContext,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    let first = item
        .terms
        .first()
        .map_or_else(String::new, |term| plain_text(term));
    let trimmed = first.trim();
    match context {
        DefinitionContext::Commands
            if trimmed.starts_with(['-', '+']) || trimmed.starts_with("[-+]") =>
        {
            parameter_identity(item, trimmed)
        }
        DefinitionContext::Commands => {
            let names = command_names(item);
            if names.is_empty() {
                (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new())
            } else {
                (DefinitionRole::Command, DefinitionCase::Sensitive, names)
            }
        }
        DefinitionContext::EnvironmentVariables => named_identity(
            DefinitionRole::EnvironmentVariable,
            DefinitionCase::Sensitive,
            environment_names(item),
        ),
        DefinitionContext::Variables => named_identity(
            DefinitionRole::Variable,
            DefinitionCase::Sensitive,
            named_term(item, is_variable_term),
        ),
        DefinitionContext::ConfigurationKeys => named_identity(
            DefinitionRole::ConfigurationKey,
            DefinitionCase::Insensitive,
            named_term(item, is_configuration_key),
        ),
        DefinitionContext::Values => named_identity(
            DefinitionRole::Value,
            DefinitionCase::Sensitive,
            named_term(item, is_value_name),
        ),
        DefinitionContext::Parameters => parameter_identity(item, trimmed),
        DefinitionContext::Generic if trimmed.starts_with('-') => parameter_identity(item, trimmed),
        DefinitionContext::Generic => (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new()),
    }
}

fn named_identity(
    role: DefinitionRole,
    case: DefinitionCase,
    names: Vec<String>,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    if names.is_empty() {
        (DefinitionRole::Term, DefinitionCase::Sensitive, names)
    } else {
        (role, case, names)
    }
}

pub(super) fn is_value_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !is_ordinal_marker(value)
        && value.chars().all(|character| {
            character.is_alphanumeric()
                || matches!(character, '-' | '_' | '.' | '/' | ':' | '+' | '?')
        })
}

fn is_ordinal_marker(value: &str) -> bool {
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

fn parameter_identity(
    item: &DefinitionItem,
    first_term: &str,
) -> (DefinitionRole, DefinitionCase, Vec<String>) {
    if first_term == "--" || first_term == "--%" {
        return (
            DefinitionRole::Marker,
            DefinitionCase::Sensitive,
            vec![first_term.to_owned()],
        );
    }
    if first_term == "-" {
        return (
            DefinitionRole::Operand,
            DefinitionCase::Sensitive,
            vec![first_term.to_owned()],
        );
    }
    let names = parameter_names(item);
    if names.is_empty() {
        (DefinitionRole::Term, DefinitionCase::Sensitive, Vec::new())
    } else {
        (DefinitionRole::Option, DefinitionCase::Sensitive, names)
    }
}

fn parameter_names(item: &DefinitionItem) -> Vec<String> {
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

fn command_names(item: &DefinitionItem) -> Vec<String> {
    item.terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
            if let Some((name, _)) = key_binding_command_form(&text) {
                return vec![name.to_owned()];
            }
            if let Some(name) = leading_styled_command_name(term) {
                return vec![name];
            }
            text.split([',', '|'])
                .filter_map(command_name_from_authored_form)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

/// Extract the command token from an unstyled authored form.
fn command_name_from_authored_form(value: &str) -> Option<&str> {
    let value = value.trim();
    let Some((first, suffix)) = value.split_once(char::is_whitespace) else {
        return is_command_name(value).then_some(value);
    };
    let suffix = suffix.trim_start();
    (suffix.starts_with(['-', '+', '/', '[', '<', '{']) && is_command_name(first)).then_some(first)
}

/// Read a formatter-emphasized command name without adjacent placeholders.
fn leading_styled_command_name(term: &[Inline]) -> Option<String> {
    let first = term.iter().find(|inline| match inline {
        Inline::Anchor { .. } => false,
        Inline::Text { value } => !value.trim().is_empty(),
        _ => true,
    })?;
    let Inline::Strong { children } = first else {
        return None;
    };
    let name = plain_text(children);
    let name = name.trim();
    is_command_name(name).then(|| name.to_owned())
}

fn named_term(item: &DefinitionItem, validate: fn(&str) -> bool) -> Vec<String> {
    item.terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
            text.split(',')
                .filter_map(|part| named_term_name(part, validate).map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .fold(Vec::new(), |mut names, name| {
            if !names.contains(&name) {
                names.push(name);
            }
            names
        })
}

/// Accept a complete semantic name and one delimited trailing annotation.
fn named_term_name(value: &str, validate: fn(&str) -> bool) -> Option<&str> {
    let value = value.trim();
    if validate(value) {
        return Some(value);
    }
    let (name, annotation) = value.rsplit_once(" (")?;
    (annotation.ends_with(')') && validate(name)).then_some(name)
}

fn environment_names(item: &DefinitionItem) -> Vec<String> {
    environment_names_from_terms(&item.terms)
}

pub(super) fn environment_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    terms
        .iter()
        .flat_map(|term| {
            let text = plain_text(term);
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

fn contains_additional_environment_assignment(value: &str) -> bool {
    value.split_whitespace().any(|token| {
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

fn is_variable_term(value: &str) -> bool {
    let value = value.strip_prefix('$').unwrap_or(value);
    let (head, index) = value
        .split_once('[')
        .map_or((value, None), |(head, tail)| (head, tail.strip_suffix(']')));
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

fn is_configuration_key(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
}

fn is_command_name(value: &str) -> bool {
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && !value.starts_with(['-', '+', '/'])
        && !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
}

pub(super) fn option_names(item: &DefinitionItem) -> Vec<String> {
    option_names_from_terms(&item.terms)
}

pub(crate) fn option_names_from_terms(terms: &[Vec<Inline>]) -> Vec<String> {
    let mut names = Vec::new();
    for term in terms {
        let text = plain_text(term);
        for token in text.split(|character: char| {
            character.is_whitespace() || matches!(character, ',' | '|' | '/' | ';')
        }) {
            let token = token.trim_matches(|character: char| {
                matches!(
                    character,
                    '[' | ']' | '(' | ')' | '{' | '}' | '“' | '”' | '‘' | '’'
                )
            });
            let Some(name) = option_prefix(token) else {
                continue;
            };
            if !names.iter().any(|existing| existing == name) {
                names.push(name.to_owned());
            }
        }
    }
    names
}

pub(crate) fn option_prefix(token: &str) -> Option<&str> {
    if !token.starts_with('-') || token == "-" {
        return None;
    }
    let end = token
        .char_indices()
        .skip(1)
        .take_while(|(_, character)| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?' | '.')
        })
        .map(|(index, character)| index + character.len_utf8())
        .last()?;
    let candidate = &token[..end];
    let body = candidate.trim_start_matches('-');
    is_option_name_body(body).then_some(candidate)
}

fn is_option_name_body(value: &str) -> bool {
    value.split('.').all(|segment| {
        !segment.is_empty()
            && segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?')
            })
            && segment
                .chars()
                .any(|character| character.is_ascii_alphanumeric() || character == '?')
    })
}
