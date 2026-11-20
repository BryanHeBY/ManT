//! Validate the ordered parameter syntax without inventing word boundaries.

use super::parameter_token::{ParameterToken, parameter_character};
use super::{Argument, HeadView, LexicalState, closing, quote_closer};

#[cfg(feature = "roff")]
pub(super) fn proves_native_assignment(view: &HeadView<'_>, argument: &Argument) -> bool {
    use super::NativeOperandRole as Role;

    let span = &argument.bytes;
    if !view.text[span.clone()].starts_with('=') {
        return true;
    }
    // Fl, the exact assignment sign, and a native Ar wrapped by balanced
    // delimiters can be separate term_word() calls. Validate the whole
    // argument slice, including everything after its last Ar.
    let rhs = span.start + 1..span.end;
    let mut cursor = rhs.start;
    let mut lexical = LexicalState::default();
    let mut has_argument = false;
    let mut scopes = Vec::new();
    for operand in view.operands_in(rhs.clone()) {
        let start = operand.bytes.start.max(cursor);
        let end = operand.bytes.end.min(rhs.end);
        if !observe_assignment_syntax(&mut lexical, &view.text[cursor..start]) {
            return false;
        }
        let value = &view.text[start..end];
        let role = operand.role;
        // An attached value of the same Fl word has an accepted spelling
        // receipt even if an overstrike clipped part of the value.
        let own_value = role == Role::ExplicitOption && operand.bytes.start < span.start;
        if own_value || matches!(role, Role::Placeholder { .. } | Role::SearchArgument(_)) {
            for ch in value.chars() {
                lexical.observe(ch);
            }
            has_argument |= !value.trim().is_empty();
        } else {
            match role {
                Role::EnclosureStart { body, parent }
                    if scopes.last().is_none_or(|scope| Some(*scope) == parent) =>
                {
                    scopes.push(body);
                }
                Role::EnclosureEnd(body) if scopes.pop() == Some(body) => {}
                Role::Literal
                | Role::LiteralContinuation(_)
                | Role::LiteralDelimiter { .. }
                | Role::LiteralPunctuation
                | Role::Description
                | Role::SearchSign
                | Role::SearchOpen { .. }
                | Role::SearchClose
                | Role::EnclosureContent(_) => {}
                _ => return false,
            }
            if !observe_assignment_syntax(&mut lexical, value) {
                return false;
            }
        }
        cursor = end;
    }
    has_argument
        && scopes.is_empty()
        && observe_assignment_syntax(&mut lexical, &view.text[cursor..rhs.end])
        && lexical.is_top_level()
}

#[cfg(feature = "roff")]
fn observe_assignment_syntax(lexical: &mut LexicalState, text: &str) -> bool {
    text.chars().all(|ch| {
        lexical.observe(ch);
        ch != '\n'
            && (ch.is_whitespace()
                || closing(ch).is_some()
                || quote_closer(ch).is_some()
                || matches!(
                    ch,
                    ']' | '}' | ')' | '>' | '”' | '’' | ',' | '/' | ':' | '@' | '|' | '.'
                ))
    })
}

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
        let syntax = ParameterToken::from_text(token);
        if structured_token(token, syntax, index == 0 && argument.attached_to_name) {
            continue;
        }
        if syntax.is_bare() {
            bare += 1;
        } else {
            return false;
        }
    }
    lexical.is_top_level() && bare <= 1
}

fn structured_token(token: &str, syntax: ParameterToken, attached_to_name: bool) -> bool {
    if matches!(token, "\0" | "..." | "=")
        || token.starts_with('=')
        || token.starts_with('/')
        || token.split_once('=').is_some_and(|(name, value)| {
            super::super::named::is_variable_term(name) && !value.is_empty()
        })
        || token.contains(':') && token.split(':').all(super::super::named::is_variable_term)
        || syntax.is_uppercase()
    {
        return true;
    }
    // Bracket/quote groups and real parameter runs occupy their original
    // byte position. Comma/colon/@ chains join those placeholders instead of
    // becoming artificial standalone words after a font-run projection.
    let token = token
        .strip_suffix(",...")
        .or_else(|| token.strip_suffix("..."))
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
            !part.is_empty() && part.chars().all(|c| c == '\0' || parameter_character(c))
        })
}
