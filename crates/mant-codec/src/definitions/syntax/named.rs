//! named recognition; complete forms retain their role-specific grammar.
use crate::definitions::RecognizedName;
use crate::definitions::context::DefinitionContext;
use mant_ir::contains_additional_environment_assignment;
use mant_ir::inline_plain_text as plain_text;
pub(in crate::definitions) use mant_ir::is_ordinal_marker;
use mant_ir::{ContentContext, EnvironmentNameLimit, Inline};
pub(crate) use mant_ir::{environment_variable_alias, environment_variable_body};

/// Strong local configuration spelling for a weak hanging candidate. The
/// section context must already be configuration-specific, and the whole
/// visible head must be complete; a dotted or assignment prefix alone cannot
/// turn an arbitrary nested option value into a configuration key.
pub(super) fn local_configuration_head(text: &str, context: DefinitionContext) -> bool {
    let text = text.trim();
    match context {
        DefinitionContext::ConfigurationKeys => {
            (text.contains('.') || text.contains('='))
                && mant_ir::configuration_key_declaration_range(text).is_some()
        }
        DefinitionContext::RootConfigurationKeys => {
            mant_ir::root_configuration_assignment_range(text).is_some()
        }
        _ => false,
    }
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
fn optional_parameter_name(value: &str, validate: fn(&str) -> bool) -> Option<&str> {
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
    // A TP/It term is one subject. Comma and pipe punctuation in its visible
    // label do not create several names without independent authored roles.
    named_occurrences(text, is_variable_term)
        .filter(|names| names.len() == 1)
        .or_else(|| {
            let range = mant_ir::generic_callable_name_range(text)?;
            Some(vec![RecognizedName::contiguous(
                &text[range.clone()],
                range.start,
            )])
        })
}

/// A declaration group is atomic: accepting a word after rejected prose does
/// not prove that either the paragraph or that word declares a variable.
pub(super) fn environment_occurrences(text: &str) -> Option<Vec<RecognizedName>> {
    mant_ir::environment_declaration_names(text).map(|names| {
        names
            .into_iter()
            .map(|(name, range)| RecognizedName::contiguous(&name, range.start))
            .collect()
    })
}

/// Count complete environment-name occurrences across one Flow owner, not
/// merely within each TP/TQ term. Stop before allocating a 65th binding.
pub(in crate::definitions) fn environment_owner_occurrences(
    content: ContentContext<'_>,
    terms: &[Vec<Inline>],
    native_prefix: bool,
) -> Result<Vec<Vec<RecognizedName>>, EnvironmentNameLimit> {
    let mut all = Vec::with_capacity(terms.len().min(64));
    let mut total = 0usize;
    for term in terms {
        let text = if native_prefix {
            super::forms::environment_prefix(content, term)
        } else {
            Some(plain_text(content, term))
        };
        let names = match text {
            Some(text) => mant_ir::scan_environment_declaration_names(&text)?.unwrap_or_default(),
            None => Vec::new(),
        };
        total = total
            .checked_add(names.len())
            .ok_or(EnvironmentNameLimit::TooManyMembers)?;
        if total > 64 {
            return Err(EnvironmentNameLimit::TooManyMembers);
        }
        all.push(
            names
                .into_iter()
                .map(|(name, range)| RecognizedName::contiguous(&name, range.start))
                .collect(),
        );
    }
    Ok(all)
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
        assert_eq!(names("run_filter($cmd,$src)"), ["run_filter"]);
        assert_eq!(
            names("install_rooted_file( $file )"),
            ["install_rooted_file"]
        );
        assert_eq!(names("array[index]"), ["array[index]"]);
        assert!(names("Using References").is_empty());
        assert!(names("name[=literal]").is_empty());
        assert!(names("Using ($example) text").is_empty());
        assert!(names("function(argument)").is_empty());
        assert!(names("zle -I").is_empty());
    }
}
