//! Small source-neutral spelling rules shared by producers and IR validation.
//!
//! These rules recognize names, not roff syntax or terminal geometry. A
//! producer still needs independent owner/markup evidence and a checked
//! binding to visible content before it may publish an entry.

use std::ops::Range;

mod literal_names;
pub(crate) use literal_names::{
    DeclarationScan, StyledBoundaryRule, literal_declaration_scan_with_starts,
};
pub use literal_names::{is_complete_hanging_option_head, literal_option_names};

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

/// A complete ordinary option token in an independently established
/// declaration head. Unlike an authored `Fl` role, spelling alone does not
/// license a negative number or a token with an attached argument.
#[must_use]
pub fn lexical_option_token(token: &str) -> bool {
    if token.starts_with('-')
        && !token.starts_with("--")
        && token
            .chars()
            .nth(1)
            .is_some_and(|character| character.is_ascii_digit())
    {
        return false;
    }
    option_prefix(token) == Some(token)
}

/// Exact option aliases in one complete, independently established literal
/// declaration. The returned ranges refer to the original visible UTF-8
/// text; punctuation and spacing remain in the sole display form.
///
/// An unpunctuated group must start with one short spelling followed by one
/// or more long spellings. This covers a single literal head such as gzip's
/// `-c --stdout --to-stdout` without treating another short option, an
/// assignment value, or an arbitrary operand as an alias. Equal spellings
/// retain distinct ranges; the caller groups them into one name binding.
#[must_use]
pub fn literal_option_aliases(form: &str) -> Option<Vec<(String, Range<usize>)>> {
    let mut cursor = 0;
    let mut names = Vec::new();
    let mut whitespace_group = false;
    let mut punctuation_group = false;
    let mut word_connector = false;
    loop {
        while let Some(character) = form[cursor..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        let start = cursor;
        while let Some(character) = form[cursor..].chars().next() {
            if character.is_whitespace() || matches!(character, ',' | '|' | '/') {
                break;
            }
            cursor += character.len_utf8();
        }
        let token = form.get(start..cursor)?;
        if !lexical_option_token(token) || names.len() == 64 {
            return None;
        }
        names.push((token.to_owned(), start..cursor));
        let separator_start = cursor;
        while let Some(character) = form[cursor..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        if cursor == form.len() {
            break;
        }
        if form[cursor..]
            .chars()
            .next()
            .is_some_and(|character| matches!(character, ',' | '|' | '/'))
        {
            // A slash after whitespace can introduce a path operand. Only
            // an adjacent slash separates aliases within one invocation.
            if form.as_bytes()[cursor] == b'/' && cursor > separator_start {
                return None;
            }
            punctuation_group = true;
            cursor += 1;
        } else if cursor > separator_start {
            whitespace_group = true;
            // The Flow literal declaration grammar admits exactly this
            // textual connector between a short and a long option.
            if names.len() == 1
                && form[cursor..].starts_with("or")
                && form[cursor + 2..].starts_with(char::is_whitespace)
            {
                word_connector = true;
                cursor += 2;
                while let Some(character) = form[cursor..].chars().next() {
                    if !character.is_whitespace() {
                        break;
                    }
                    cursor += character.len_utf8();
                }
            }
        } else {
            return None;
        }
        if cursor == form.len() {
            return None;
        }
    }
    if names.len() < 2 {
        return None;
    }
    if whitespace_group {
        let short_then_long = names[0].0.len() == 2
            && names[0].0.starts_with('-')
            && names.iter().skip(1).all(|(name, _)| name.starts_with("--"));
        if !short_then_long || (word_connector || punctuation_group) && names.len() != 2 {
            return None;
        }
    }
    Some(names)
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

/// One complete command word proved by an authored native `Ic`/`Cm` head.
/// A suffix in the same visible token must not be promoted as another name.
#[must_use]
pub fn native_command_token(token: &str) -> bool {
    !token.is_empty()
        && !token.chars().any(char::is_whitespace)
        && !token.chars().any(char::is_control)
        && !token.starts_with(['-', '+', '/'])
        && !token
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        && !token.contains(['[', ']', '{', '}', '<', '>', '|', ','])
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
        assert!(native_command_token("attach-session"));
        assert!(!native_command_token("-bad"));
        assert!(!native_command_token("run tail"));
        assert_eq!(
            environment_variable_alias("DEMO_HOME=foo"),
            Some("DEMO_HOME".to_owned())
        );
        assert_eq!(environment_variable_alias("a!"), None);
    }

    #[test]
    fn complete_literal_aliases_preserve_one_form_and_exact_ranges() {
        // The matching TP/B heads first ran through pinned CVS -Ttree.
        // man_macro.c::blk_imp retains one HEAD, and man_term.c::pre_B prints
        // its one operand without splitting punctuation into AST nodes.
        let aliases = literal_option_aliases("-a, --all").unwrap();
        assert_eq!(
            aliases,
            [("-a".to_owned(), 0..2), ("--all".to_owned(), 4..9)]
        );
        assert_eq!(
            literal_option_aliases("-a --all"),
            Some(vec![("-a".to_owned(), 0..2), ("--all".to_owned(), 3..8)])
        );
        assert_eq!(
            literal_option_aliases("-q or --quiet"),
            Some(vec![("-q".to_owned(), 0..2), ("--quiet".to_owned(), 6..13)])
        );
        // Pinned CVS man_macro.c::blk_imp keeps gzip's escaped-dash TP/B
        // source `\-c \-\-stdout \-\-to-stdout` in one HEAD. term.c::
        // term_word prints the three visible spellings with ASCII spaces;
        // source-neutral alias grammar operates on that final form.
        assert_eq!(
            literal_option_aliases("-c --stdout --to-stdout"),
            Some(vec![
                ("-c".to_owned(), 0..2),
                ("--stdout".to_owned(), 3..11),
                ("--to-stdout".to_owned(), 12..23),
            ])
        );
        // Pinned CVS chars.c maps \~ to U+00A0; man_term.c::pre_alternate
        // joins operands and term.c::term_word emits that visible glyph.
        // It remains two UTF-8 bytes in
        // the form, and must be syntax whitespace without losing offsets.
        assert_eq!(
            literal_option_aliases("-o\u{a0}--output"),
            Some(vec![
                ("-o".to_owned(), 0..2),
                ("--output".to_owned(), 4..12),
            ])
        );
        // Pinned CVS keeps all three BI operands in one head. Equal option
        // spellings are distinct visible occurrences, not a corrupt group.
        assert_eq!(
            literal_option_aliases("-o --output --output"),
            Some(vec![
                ("-o".to_owned(), 0..2),
                ("--output".to_owned(), 3..11),
                ("--output".to_owned(), 12..20),
            ])
        );
        assert_eq!(
            literal_option_aliases("-a, -a"),
            Some(vec![("-a".to_owned(), 0..2), ("-a".to_owned(), 4..6)])
        );
        for not_aliases in [
            "--set=KEY,VALUE",
            "-a, text",
            "--first --second",
            "-a -b --all",
            "-a --all -b",
            "-a --all operand",
            "-a or --all --other",
            "-a, --all --other",
            "-a,, --all",
        ] {
            assert!(
                literal_option_aliases(not_aliases).is_none(),
                "{not_aliases}"
            );
        }
    }
}
