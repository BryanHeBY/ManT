//! Validate the ordered parameter syntax without inventing word boundaries.

use super::{Argument, HeadView, LexicalState, closing, quote_closer};

pub(super) fn valid_arguments(view: &HeadView<'_>, argument: &Argument) -> bool {
    let range = &argument.bytes;
    let mut literal = String::new();
    let mut offset = range.start;
    let mut lexical = LexicalState::default();
    let mut opaque = false;
    while offset < range.end {
        let character = view.text[offset..].chars().next().expect("argument scalar");
        let nested = lexical.within_group();
        let opening = lexical.is_top_level()
            && (closing(character).is_some() || quote_closer(character).is_some());
        lexical.observe(character);
        if opening {
            if !opaque {
                literal.push('\0');
            }
            opaque = true;
        } else if nested {
            if lexical.is_top_level() {
                opaque = false;
            }
        } else if view.is_parameter(offset) {
            if character.is_whitespace() {
                literal.push(character);
                opaque = false;
            } else {
                if !opaque {
                    literal.push('\0');
                }
                opaque = true;
            }
        } else {
            literal.push(character);
            opaque = false;
        }
        offset += character.len_utf8();
    }
    let mut bare = 0;
    for (index, token) in literal.split_whitespace().enumerate() {
        if structured_token(token, index == 0 && argument.attached_to_name) {
            continue;
        }
        if !token.starts_with('-')
            && token
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
        {
            bare += 1;
        } else {
            return false;
        }
    }
    lexical.is_top_level() && bare <= 1
}

fn structured_token(token: &str, attached_to_name: bool) -> bool {
    if matches!(token, "\0" | "..." | "=")
        || token.starts_with('=')
        || token.starts_with('/')
        || token.split_once('=').is_some_and(|(name, value)| {
            super::super::named::is_variable_term(name) && !value.is_empty()
        })
        || token.contains(':') && token.split(':').all(super::super::named::is_variable_term)
        || token.chars().any(char::is_uppercase)
            && token
                .chars()
                .all(|c| c.is_uppercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
    {
        return true;
    }
    // Bracket/quote groups and real parameter runs occupy their original
    // byte position. Comma/colon/@ chains join those placeholders instead of
    // becoming artificial standalone words after a font-run projection.
    let token = token
        .strip_suffix(",...")
        .or_else(|| token.strip_suffix(','))
        .unwrap_or(token);
    // A field operator directly adjoining an actual name (for example
    // -L:<funcname>:<file>) begins its parameter attachment. A separated
    // leading colon or an empty internal field supplies no such proof.
    let token = if attached_to_name {
        token
            .strip_prefix(':')
            .or_else(|| token.strip_prefix('@'))
            .unwrap_or(token)
    } else {
        token
    };
    token.contains('\0')
        && token.split([',', ':', '@', '|', '/', '=']).all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c == '\0' || c.is_alphanumeric() || matches!(c, '_' | '-'))
        })
}
