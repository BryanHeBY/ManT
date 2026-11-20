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
    // term_word() can write the operator, each parameter component and
    // the enclosing delimiters separately. Advance through their accepted
    // ranges, including punctuation and literal choices between Ar words.
    let rhs = span.start + 1..span.end;
    let mut cursor = rhs.start;
    let mut syntax = AssignmentSyntax::new();
    let mut scopes: Vec<u32> = Vec::new();
    let mut typed_suffix = None;
    for operand in view.operands_in(rhs.clone()) {
        let start = operand.bytes.start.max(cursor);
        let end = operand.bytes.end.min(rhs.end);
        if !syntax.literal(&view.text[cursor..start], true, false) {
            return false;
        }
        let value = &view.text[start..end];
        let literal_atom = match operand.role {
            Role::Placeholder { .. } | Role::SearchArgument(_) => {
                typed_suffix = None;
                syntax.parameter(value);
                cursor = end;
                continue;
            }
            Role::PathOperand(_) if syntax.expects_atom => {
                typed_suffix = None;
                syntax.parameter(value);
                cursor = end;
                continue;
            }
            Role::ExplicitOption
                if operand.bytes.start < span.start || syntax.lexical.within_group() =>
            {
                // The same Fl can own its value. An enclosed Fl can also
                // be argument syntax, as in an optional [-p path] operand.
                syntax.parameter(value);
                cursor = end;
                continue;
            }
            Role::EnclosureStart { body, parent }
                if scopes.len() < super::MAX_NESTING
                    && scopes.last().is_none_or(|scope| Some(*scope) == parent) =>
            {
                if syntax.separated_group(value) && value.trim_start().starts_with('(') {
                    // A detached Po may be a comment. An executed argument
                    // within it proves the optional suffix of the option.
                    typed_suffix = Some(body);
                }
                scopes.push(body);
                false
            }
            Role::EnclosureEnd(body)
                if scopes.pop() == Some(body) && typed_suffix != Some(body) =>
            {
                false
            }
            Role::EnclosureContent(body) if scopes.last() == Some(&body) => true,
            Role::Literal | Role::LiteralContinuation(_) | Role::Description | Role::Command => {
                true
            }
            Role::Argument
            | Role::LiteralDelimiter { .. }
            | Role::LiteralPunctuation
            | Role::PipeDelimiter(_)
            | Role::SearchSign
            | Role::SearchOpen { .. }
            | Role::SearchClose => false,
            _ => return false,
        };
        if !syntax.literal(
            value,
            literal_atom,
            matches!(operand.role, Role::EnclosureStart { .. }),
        ) {
            return false;
        }
        cursor = end;
    }
    scopes.is_empty()
        && syntax.literal(&view.text[cursor..rhs.end], true, false)
        && syntax.has_value
        && syntax.lexical.is_top_level()
}

#[cfg(feature = "roff")]
struct AssignmentSyntax {
    lexical: LexicalState,
    expects_atom: bool,
    has_value: bool,
    after_space: bool,
}

#[cfg(feature = "roff")]
impl AssignmentSyntax {
    fn new() -> Self {
        Self {
            lexical: LexicalState::default(),
            expects_atom: true,
            has_value: false,
            after_space: false,
        }
    }

    fn parameter(&mut self, value: &str) {
        for ch in value.chars() {
            if !self.punctuation(ch) {
                self.lexical.observe(ch);
                self.expects_atom = false;
                self.has_value = true;
                self.after_space = false;
            }
        }
    }

    fn separated_group(&self, value: &str) -> bool {
        (!self.has_value || !self.expects_atom)
            && self.lexical.is_top_level()
            && (self.after_space || value.starts_with(char::is_whitespace))
    }

    fn literal(&mut self, value: &str, permits_atom: bool, native_group: bool) -> bool {
        let mut in_atom = false;
        // A standalone dot can join adjacent executed words, but does
        // not join a free-standing sentence to a completed RHS.
        if value == "." && self.has_value && !self.after_space {
            self.expects_atom = true;
        }
        for ch in value.chars() {
            if ch == '\n' {
                return false;
            }
            if ch == '.' && in_atom {
                // A dot within a literal is part of the same parameter.
                continue;
            }
            if !native_group
                && self.separated_group("")
                && (closing(ch).is_some() || quote_closer(ch).is_some())
            {
                return false;
            }
            if self.punctuation(ch) {
                in_atom = false;
            } else if permits_atom && (in_atom || self.expects_atom) && parameter_character(ch) {
                self.lexical.observe(ch);
                self.expects_atom = false;
                self.has_value = true;
                in_atom = true;
                self.after_space = false;
            } else {
                return false;
            }
        }
        true
    }

    fn punctuation(&mut self, ch: char) -> bool {
        self.after_space = ch.is_whitespace();
        let opener = closing(ch).is_some() || quote_closer(ch).is_some();
        let separator = matches!(ch, '=' | ',' | '/' | ':' | '@' | '|');
        if opener || separator || matches!(ch, ']' | '}' | ')' | '>' | '”' | '’') {
            let closed = self.lexical.observe(ch);
            self.expects_atom = !closed && (opener || separator);
        } else if !(ch.is_whitespace()
            || matches!(ch, '.' | '…')
            || ch == '+' && self.lexical.within_group())
        {
            return false;
        }
        true
    }
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
