//! Complete, source-neutral environment declaration names in one visible HEAD.
//!
//! This is the former Flow environment-group grammar, moved so Flow inference
//! and Fixed production/read-time proof use the same spelling and byte ranges.
//! Punctuation separates declarations only outside bounded annotations; it
//! never proves an alias relation or an authored macro role.

use std::ops::Range;

use super::environment_variable_alias;

/// A complete declaration that exceeds the bounded semantic occurrence
/// budget. The visible HEAD remains valid; callers must report the omitted
/// semantic recognition rather than treating it as a syntax rejection.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EnvironmentNameLimit {
    TooManyMembers,
}

const MAX_ENVIRONMENT_MEMBERS: usize = 64;

/// Exact spellings and their UTF-8 byte ranges within one complete HEAD.
#[doc(hidden)]
pub type EnvironmentNameOccurrences = Vec<(String, Range<usize>)>;

/// Recognize every exact environment name in a complete visible declaration.
/// `Some([])` means a bounded template-only label, not an exact selector.
/// A rejected member invalidates the group; no first-name salvage is allowed.
#[doc(hidden)]
#[must_use]
pub fn environment_declaration_names(text: &str) -> Option<EnvironmentNameOccurrences> {
    scan_environment_declaration_names(text).ok().flatten()
}

/// The same grammar with a distinct limit result for annotation coverage.
///
/// # Errors
///
/// Returns `TooManyMembers` before allocating a 65th declaration member.
#[doc(hidden)]
pub fn scan_environment_declaration_names(
    text: &str,
) -> Result<Option<EnvironmentNameOccurrences>, EnvironmentNameLimit> {
    let parts = if text.contains('=') {
        vec![text]
    } else {
        match named_groups(text)? {
            Some(parts) => parts,
            None => return Ok(None),
        }
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
            if is_environment_template_label(part.trim()) {
                continue;
            }
            return Ok(None);
        };
        let start =
            part.as_ptr() as usize - text.as_ptr() as usize + part.len() - part.trim_start().len();
        let Some(end) = start.checked_add(name.len()) else {
            return Ok(None);
        };
        if text.get(start..end) != Some(name.as_str()) {
            return Ok(None);
        }
        names.push((name, start..end));
    }
    Ok(Some(names))
}

pub(super) fn annotations(value: &str) -> bool {
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

fn named_groups(text: &str) -> Result<Option<Vec<&str>>, EnvironmentNameLimit> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut closers = Vec::new();
    for (index, character) in text.char_indices() {
        if matches!(closers.last(), Some(')')) && !matches!(character, '(' | ')')
            || matches!(closers.last(), Some('>')) && !matches!(character, '<' | '>')
        {
            continue;
        }
        match character {
            '(' | '<' | '[' | '{' => {
                if closers.len() >= 64 {
                    return Ok(None);
                }
                closers.push(match character {
                    '(' => ')',
                    '<' => '>',
                    '[' => ']',
                    _ => '}',
                });
            }
            ')' | '>' | ']' | '}' if closers.pop() != Some(character) => return Ok(None),
            ',' | '|' if closers.is_empty() => {
                if parts.len() + 1 >= MAX_ENVIRONMENT_MEMBERS {
                    return Err(EnvironmentNameLimit::TooManyMembers);
                }
                parts.push(&text[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }
    if !closers.is_empty() {
        return Ok(None);
    }
    parts.push(&text[start..]);
    Ok(Some(parts))
}

/// A complete environment-name family label, never an exact selector.
/// The trailing glob is a documented family spelling, not permission to
/// expand it into guessed instance names.
#[doc(hidden)]
#[must_use]
pub fn is_environment_template_label(value: &str) -> bool {
    let value = value.trim();
    if value.len() > 512 {
        return false;
    }
    if let Some(prefix) = value.strip_suffix('*')
        && !prefix.is_empty()
        && prefix
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        let mut sample = String::from(prefix);
        sample.push('X');
        return environment_variable_alias(&sample).is_some();
    }
    environment_template(value)
}

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

/// A complete variable-like token, never a type classification by spelling.
/// Va/Dv still need an authored native role and a surviving display binding.
pub(crate) fn is_variable_term(value: &str) -> bool {
    let value = value.strip_prefix('$').unwrap_or(value);
    let (head, index) = if let Some((head, tail)) = value.split_once('[') {
        let Some(index) = tail.strip_suffix(']') else {
            return false;
        };
        (head, Some(index))
    } else {
        (value, None)
    };
    head.split("::").all(|component| {
        !component.is_empty()
            && component.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '_' | '-')
            })
            && component
                .chars()
                .next()
                .is_some_and(|character| character.is_ascii_alphabetic() || character == '_')
    }) && index.is_none_or(|index| {
        !index.is_empty()
            && index
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::{
        EnvironmentNameLimit, environment_declaration_names, scan_environment_declaration_names,
    };

    #[test]
    fn complete_environment_group_has_exact_independent_ranges() {
        // E01 was run unchanged through the pinned CVS reference before this
        // assertion. man_term.c::pre_TP/pre_B keep one visible HEAD; splitting
        // names is ManT's separate complete-declaration policy.
        assert_eq!(
            environment_declaration_names("TMPDIR, TEMP, TMP"),
            Some(vec![
                ("TMPDIR".to_owned(), 0..6),
                ("TEMP".to_owned(), 8..12),
                ("TMP".to_owned(), 14..17),
            ])
        );
        assert_eq!(
            environment_declaration_names("CPATH"),
            Some(vec![("CPATH".to_owned(), 0..5)])
        );
        assert!(environment_declaration_names("Otherwise the default directory").is_none());
    }

    #[test]
    fn member_limit_stops_before_allocating_an_unbounded_group() {
        // The corresponding 65-name .B HEAD ran pinned CVS -Tutf8 first;
        // this is ManT's semantic occurrence budget, not a formatter rule.
        let over_limit = (1..=65)
            .map(|index| format!("A{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        assert_eq!(
            scan_environment_declaration_names(&over_limit),
            Err(EnvironmentNameLimit::TooManyMembers)
        );
    }
}
