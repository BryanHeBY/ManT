//! named recognition; complete forms retain their role-specific grammar.
use crate::definitions::RecognizedName;

/// Strong local configuration spelling, used only on complete declaration
/// heads. A dotted key needs a configuration/variable context; a mixed-case
/// assignment label is useful even under topical headings such as PATHS.
pub(super) fn local_configuration_head(text: &str, variable_context: bool) -> bool {
    let text = text.trim();
    if let Some(key) = text.strip_suffix('=') {
        return is_configuration_key(key)
            && key.chars().any(char::is_lowercase)
            && key.chars().any(char::is_uppercase)
            && !key.contains('_');
    }
    variable_context
        && text.contains('.')
        && named_occurrences(text, is_configuration_key).is_some()
}

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

/// Accept a complete semantic name and bounded trailing annotations. The
/// suffix is retained in the form, never expanded into additional names.
pub(in crate::definitions) fn named_term_name(
    value: &str,
    validate: fn(&str) -> bool,
) -> Option<&str> {
    let value = value.trim();
    if validate(value) {
        return Some(value);
    }
    let (name, annotation) = value.split_once(char::is_whitespace)?;
    (validate(name) && annotations(annotation)).then_some(name)
}

fn annotations(value: &str) -> bool {
    let mut rest = value.trim();
    let mut count = 0;
    while !rest.is_empty() {
        count += 1;
        if count > 64 {
            return false;
        }
        let Some(end) = annotation_end(rest) else {
            return false;
        };
        let body = &rest[1..end];
        if body.trim().is_empty() || body.contains(['\r', '\n']) {
            return false;
        }
        rest = rest[end + 1..].trim_start();
    }
    count > 0
}

fn annotation_end(value: &str) -> Option<usize> {
    let opener = value.chars().next()?;
    let closer = match opener {
        '(' => ')',
        '<' => '>',
        _ => return None,
    };
    let mut depth = 1;
    for (index, character) in value.char_indices().skip(1) {
        if character == opener {
            depth += 1;
            if depth > 64 {
                return None;
            }
        } else if character == closer {
            depth -= 1;
            if depth == 0 {
                return Some(index);
            }
        }
    }
    None
}

/// Delimiters inside a bounded annotation or placeholder are not declaration
/// separators. Every outer part still has to pass its entire name grammar.
fn named_groups(text: &str) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut closers = Vec::new();
    for (index, character) in text.char_indices() {
        // Parenthesized annotations contain literal defaults/key bindings:
        // the '[' in C-[ is not a fresh syntax group. Only their own delimiter
        // pair nests; commas and pipes remain inside the annotation.
        if matches!(closers.last(), Some(')')) && !matches!(character, '(' | ')')
            || matches!(closers.last(), Some('>')) && !matches!(character, '<' | '>')
        {
            continue;
        }
        match character {
            '(' | '<' | '[' | '{' => {
                if closers.len() >= 64 {
                    return None;
                }
                closers.push(match character {
                    '(' => ')',
                    '<' => '>',
                    '[' => ']',
                    _ => '}',
                });
            }
            ')' | '>' | ']' | '}' if closers.pop() != Some(character) => return None,
            ',' | '|' if closers.is_empty() => {
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if !closers.is_empty() {
        return None;
    }
    parts.push(&text[start..]);
    Some(parts)
}

pub(super) fn named_occurrences(
    text: &str,
    validate: fn(&str) -> bool,
) -> Option<Vec<RecognizedName>> {
    let parts = if text.contains('=') {
        vec![text]
    } else {
        named_groups(text)?
    };
    parts
        .into_iter()
        .map(|part| {
            let name = if let Some((name, value)) = part.trim().split_once('=') {
                let name = name.trim_end();
                if !validate(name) || contains_additional_environment_assignment(value) {
                    return None;
                }
                name
            } else {
                named_term_name(part, validate)?
            };
            Some(RecognizedName::contiguous(
                name,
                name.as_ptr() as usize - text.as_ptr() as usize,
            ))
        })
        .collect()
}

/// A declaration group is atomic: accepting a word after rejected prose does
/// not prove that either the paragraph or that word declares a variable.
pub(super) fn environment_occurrences(text: &str) -> Option<Vec<RecognizedName>> {
    let parts = if text.contains('=') {
        vec![text]
    } else {
        named_groups(text)?
    };
    let mut names = Vec::new();
    for part in parts {
        let name = environment_variable_alias(part).or_else(|| {
            let (name, suffix) = part.trim().split_once(char::is_whitespace)?;
            annotations(suffix)
                .then(|| environment_variable_alias(name))
                .flatten()
        });
        let Some(name) = name else {
            if environment_template(part.trim()) {
                continue;
            }
            return None;
        };
        let start =
            part.as_ptr() as usize - text.as_ptr() as usize + part.len() - part.trim_start().len();
        names.push(RecognizedName::contiguous(&name, start));
    }
    Some(names)
}

// A bounded literal/placeholder environment head is a declaration, but not
// an exact environment-variable name. Keep it opaque alongside concrete names;
// do not salvage words from prose or expand template instances.
fn environment_template(value: &str) -> bool {
    if value.len() > 512 || value.chars().any(char::is_whitespace) {
        return false;
    }
    let mut rest = value;
    let mut literal = String::new();
    let mut templates = 0;
    while let Some((prefix, tail)) = rest.split_once('<') {
        if templates == 0 && prefix.is_empty() {
            return false;
        }
        let Some((parameter, suffix)) = tail.split_once('>') else {
            return false;
        };
        if !is_variable_term(parameter) {
            return false;
        }
        literal.push_str(prefix);
        literal.push('X');
        templates += 1;
        if templates > 16 {
            return false;
        }
        rest = suffix;
    }
    literal.push_str(rest);
    templates > 0
        && literal
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        && environment_variable_alias(&literal).is_some()
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
    value.split('.').all(|component| {
        !component.is_empty()
            && component.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_')
            })
    }) && value
        .chars()
        .next()
        .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
}
