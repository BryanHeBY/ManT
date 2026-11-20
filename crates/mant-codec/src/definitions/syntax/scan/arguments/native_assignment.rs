//! Prove the complete right-hand side over accepted native operands.

use super::super::MAX_NESTING;
use super::super::NativeOperandRole as Role;
use super::{Argument, HeadView, LexicalState, closing, parameter_character, quote_closer};

pub(in crate::definitions::syntax::scan) fn proves_native_assignment(
    view: &HeadView<'_>,
    argument: &Argument,
) -> bool {
    let span = &argument.bytes;
    if !view.text[span.clone()].starts_with('=') {
        return true;
    }
    // term_word() can execute the operator, value and delimiters in
    // separate operands. The accepted character ranges give adjacency.
    let rhs = span.start + 1..span.end;
    let mut cursor = rhs.start;
    let mut syntax = AssignmentSyntax::default();
    let mut scopes = Vec::new();
    for operand in view.operands_in(rhs.clone()) {
        let start = operand.bytes.start.max(cursor);
        let end = operand.bytes.end.min(rhs.end);
        if syntax
            .literal(&view.text[cursor..start], true, false)
            .is_err()
        {
            return false;
        }
        let value = &view.text[start..end];
        let accepted = match operand.role {
            Role::Placeholder { .. } | Role::SearchArgument(_) => syntax.parameter(value, true),
            Role::PathOperand(_) if syntax.atom.accepts_new_fragment() => syntax.path(value),
            Role::ExplicitOption
                if operand.bytes.start < span.start || syntax.lexical.within_group() =>
            {
                // An enclosing Fl can own its own value. An inner Fl
                // proves its spelling inside the group, not the group.
                syntax.parameter(value, false)
            }
            Role::EnclosureStart { body, parent }
                if scopes.len() < MAX_NESTING
                    && scopes.last().is_none_or(|scope| Some(*scope) == parent) =>
            {
                scopes.push(body);
                syntax.literal(value, false, true)
            }
            Role::EnclosureEnd(body) if scopes.pop() == Some(body) => {
                syntax.literal(value, false, false)
            }
            Role::EnclosureContent(body) if scopes.last() == Some(&body) => {
                syntax.literal(value, true, false)
            }
            Role::Literal | Role::LiteralContinuation(_) | Role::Description | Role::Command => {
                syntax.literal(value, true, false)
            }
            Role::Argument
            | Role::LiteralDelimiter { .. }
            | Role::LiteralPunctuation
            | Role::PipeDelimiter(_)
            | Role::SearchSign
            | Role::SearchOpen { .. }
            | Role::SearchClose => syntax.literal(value, false, false),
            _ => return false,
        };
        if accepted.is_err() {
            return false;
        }
        cursor = end;
    }
    scopes.is_empty()
        && syntax
            .literal(&view.text[cursor..rhs.end], true, false)
            .is_ok()
        && syntax.has_value
        && syntax.lexical.is_top_level()
}

#[derive(Clone, Copy, Default)]
enum Atom {
    #[default]
    Expected,
    Complete,
    AfterDot,
}

impl Atom {
    fn accepts_new_fragment(self) -> bool {
        !matches!(self, Self::Complete)
    }
}

#[derive(Default)]
struct AssignmentSyntax {
    lexical: LexicalState,
    atom: Atom,
    has_value: bool,
    after_space: bool,
    /// A detached author group can be prose. Every content atom needs
    /// independent argument evidence, and punctuation alone is insufficient.
    detached_argument: Option<bool>,
}

impl AssignmentSyntax {
    fn parameter(&mut self, value: &str, proves_argument: bool) -> Result<(), ()> {
        for ch in value.chars() {
            if !self.punctuation(ch, false)? {
                self.lexical.observe(ch);
                self.record_value(proves_argument);
            }
        }
        Ok(())
    }

    fn path(&mut self, value: &str) -> Result<(), ()> {
        if value.is_empty() {
            return Ok(());
        }
        // mdoc_html.c::mdoc_pa_pre() preserves a pathname even when it
        // consists entirely of punctuation, as with /, . and ../.
        let punctuation_only = value.chars().all(|ch| matches!(ch, '/' | '.'));
        if punctuation_only {
            self.record_value(true);
            Ok(())
        } else {
            self.parameter(value, true)
        }
    }

    fn record_value(&mut self, proves_argument: bool) {
        self.atom = Atom::Complete;
        self.has_value = true;
        self.after_space = false;
        if let Some(proved) = &mut self.detached_argument {
            *proved |= proves_argument;
        }
    }

    fn literal(&mut self, value: &str, permits_atom: bool, native_group: bool) -> Result<(), ()> {
        let mut in_atom = false;
        for ch in value.chars() {
            if ch == '\n' {
                return Err(());
            }
            if ch == '.' && in_atom {
                self.atom = Atom::AfterDot;
                continue;
            }
            if self.punctuation(ch, native_group)? {
                in_atom = false;
            } else if self.detached_argument.is_none()
                && permits_atom
                && (in_atom || self.atom.accepts_new_fragment())
                && parameter_character(ch)
            {
                self.lexical.observe(ch);
                self.record_value(false);
                in_atom = true;
            } else {
                return Err(());
            }
        }
        Ok(())
    }

    fn punctuation(&mut self, ch: char, native_group: bool) -> Result<bool, ()> {
        if ch.is_whitespace() {
            self.after_space = true;
            if matches!(self.atom, Atom::AfterDot) {
                self.atom = Atom::Complete;
            }
            return Ok(true);
        }
        let opener = closing(ch).is_some() || quote_closer(ch).is_some();
        let separator = matches!(ch, '=' | ',' | '/' | ':' | '@' | '|');
        if opener || separator || matches!(ch, ']' | '}' | ')' | '>' | '”' | '’') {
            if opener
                && self.after_space
                && self.lexical.is_top_level()
                && (!self.has_value || !self.atom.accepts_new_fragment())
                && !(native_group && ch == '[')
            {
                self.detached_argument = Some(false);
            }
            let closed = self.lexical.observe(ch);
            if closed && self.detached_argument.take() == Some(false) {
                return Err(());
            }
            self.atom = if !closed && (opener || separator) {
                Atom::Expected
            } else {
                Atom::Complete
            };
        } else if ch == '.' && !self.after_space && self.has_value {
            // A dot can bridge two touching literal fragments. Its
            // permission expires at a word separator.
            self.atom = Atom::AfterDot;
        } else if !(matches!(ch, '.' | '…') || ch == '+' && self.lexical.within_group()) {
            return Ok(false);
        }
        self.after_space = false;
        Ok(true)
    }
}
