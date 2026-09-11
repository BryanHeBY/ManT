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
    if let Some(name) = optional_parameter_name(value, validate) {
        return Some(name);
    }
    let (name, annotation) = value.split_once(char::is_whitespace)?;
    (validate(name) && annotations(annotation)).then_some(name)
}

/// A bracketed optional assignment suffix is authored invocation syntax, not
/// part of a generic term's selector.  Keep this intentionally narrower than
/// an array subscript: `name[=<type>]` names `name`, whereas `name[index]`
/// remains one complete variable spelling.
fn optional_parameter_name<'a>(value: &'a str, validate: fn(&str) -> bool) -> Option<&'a str> {
    let (name, suffix) = value.split_once('[')?;
    let parameter = suffix.strip_suffix(']')?.strip_prefix('=')?;
    let parameter = parameter.strip_prefix('<')?.strip_suffix('>')?;
    (validate(name) && is_variable_term(parameter)).then_some(name)
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
            let trimmed = part.trim();
            let name = if let Some(name) = named_term_name(trimmed, validate) {
                name
            } else if let Some((name, value)) = trimmed.split_once('=') {
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

/// Generic terms normally use their complete identifier grammar.  A narrow
/// invocation form makes source-defined callable names addressable without
/// treating prose as a declaration: its name must contain lowercase technical
/// spelling and every remaining token must be an all-uppercase placeholder.
pub(in crate::definitions) fn term_occurrences(text: &str) -> Option<Vec<RecognizedName>> {
    named_occurrences(text, is_variable_term).or_else(|| {
        let name = invocation_name(text.trim())?;
        let offset = name.as_ptr() as usize - text.as_ptr() as usize;
        Some(vec![RecognizedName::contiguous(name, offset)])
    })
}

fn invocation_name(value: &str) -> Option<&str> {
    let (name, parameters) = value.split_once(char::is_whitespace)?;
    (is_variable_term(name)
        && name.chars().any(char::is_lowercase)
        && parameters.split_whitespace().all(invocation_placeholder))
    .then_some(name)
}

fn invocation_placeholder(token: &str) -> bool {
    !token.is_empty()
        && token.split(',').all(|part| {
            !part.is_empty()
                && part.chars().all(|character| {
                    character.is_ascii_uppercase()
                        || character.is_ascii_digit()
                        || matches!(character, '_' | '-')
                })
        })
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
    is_term_component_path(head)
        && index.is_none_or(|index| {
            !index.is_empty()
                && index
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
}

/// A generic semantic term can be an ordinary identifier or a qualified
/// technical name such as Perl's `Class::ISA`. Single-colon spellings remain
/// excluded: they commonly denote prose, URLs, or provider syntax rather than
/// a name. Every `::` component independently follows the existing identifier
/// grammar, preventing empty pieces and URI-like `://` forms.
fn is_term_component_path(value: &str) -> bool {
    value.split("::").all(is_term_component)
}

fn is_term_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        && value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
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

#[cfg(test)]
mod tests {
    use super::{is_variable_term, term_occurrences};

    #[test]
    fn qualified_technical_terms_require_complete_double_colon_components() {
        for value in ["Class::ISA", "Pod::Plainer", "std::path::Path", "$Foo::bar"] {
            assert!(is_variable_term(value), "accepted spelling: {value}");
        }
        for value in [
            "Class:",
            "Class:::ISA",
            "Class::",
            "::ISA",
            "https://example",
            "name: value",
        ] {
            assert!(!is_variable_term(value), "rejected spelling: {value}");
        }
    }

    #[test]
    fn generic_terms_keep_optional_parameters_and_uppercase_invocations_out_of_names() {
        let names = |value| {
            term_occurrences(value)
                .unwrap_or_default()
                .into_iter()
                .map(|found| found.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("istrip[=<bool>]"), ["istrip"]);
        assert_eq!(names("getservbyname NAME,PROTO"), ["getservbyname"]);
        assert_eq!(names("array[index]"), ["array[index]"]);
        assert!(names("Using References").is_empty());
        assert!(names("name[=literal]").is_empty());
    }
}
