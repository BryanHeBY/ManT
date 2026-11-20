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
            .literal(&view.text[cursor..start], Literal::Unmarked)
            .is_err()
        {
            return false;
        }
        let value = &view.text[start..end];
        let accepted = match operand.role {
            Role::Placeholder { .. } | Role::SearchArgument(_) => syntax.parameter(value, true),
            Role::PathOperand(_) => syntax.path(value),
            Role::ExplicitOption
                if operand.bytes.start < span.start || syntax.lexical.within_group() =>
            {
                // An enclosing Fl can own its own value. An inner Fl
                // proves its spelling inside the group, not the group.
                let optional = syntax
                    .detached
                    .is_some_and(|group| group.kind == GroupKind::Optional);
                syntax.parameter(value, operand.bytes.start < span.start || optional)
            }
            Role::EnclosureStart {
                body,
                parent,
                optional,
            } if scopes.len() < MAX_NESTING
                && scopes
                    .last()
                    .is_none_or(|&(scope, _)| Some(scope) == parent) =>
            {
                scopes.push((body, optional));
                syntax.literal(value, Literal::Delimiter)
            }
            Role::EnclosureEnd(body) if scopes.pop().is_some_and(|(scope, _)| scope == body) => {
                syntax.literal(value, Literal::Delimiter)
            }
            Role::EnclosureContent { body, bare }
                if scopes.last().is_some_and(|&(scope, _)| scope == body) =>
            {
                // Actual Op/Oo can supply one bare value such as [host].
                // A Bq with the same terminal glyphs makes no such claim.
                syntax.literal(value, scoped_literal(scopes.last().filter(|_| bare)))
            }
            Role::Command => syntax.literal(value, scoped_literal(scopes.last())),
            Role::LiteralValue => syntax.literal(value, Literal::Explicit),
            Role::Literal | Role::LiteralContinuation(_) | Role::Description => {
                syntax.literal(value, Literal::Unmarked)
            }
            Role::Argument
            | Role::LiteralDelimiter { .. }
            | Role::LiteralPunctuation
            | Role::PipeDelimiter(_)
            | Role::SearchSign
            | Role::SearchOpen { .. }
            | Role::SearchClose => syntax.literal(value, Literal::Delimiter),
            _ => return false,
        };
        if accepted.is_err() {
            return false;
        }
        cursor = end;
    }
    scopes.is_empty()
        && syntax
            .literal(&view.text[cursor..rhs.end], Literal::Unmarked)
            .is_ok()
        && syntax.has_value
        && syntax.lexical.is_top_level()
}

fn scoped_literal(scope: Option<&(u32, bool)>) -> Literal {
    if scope.is_some_and(|&(_, optional)| optional) {
        Literal::Explicit
    } else {
        Literal::Unmarked
    }
}

#[derive(Clone, Copy, Default)]
enum Atom {
    #[default]
    Expected,
    Complete,
    AfterDot,
    PathContinuation,
}

impl Atom {
    fn accepts_new_fragment(self) -> bool {
        !matches!(self, Self::Complete)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Literal {
    Delimiter,
    Unmarked,
    Explicit,
}

#[derive(Clone, Copy, Default)]
struct DetachedGroup {
    kind: GroupKind,
    proved_argument: bool,
    literal: bool,
    connector: bool,
    awaiting: Option<Join>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum GroupKind {
    #[default]
    Ordinary,
    Optional,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Word,
    Adjacent,
}

impl DetachedGroup {
    fn complete(self) -> bool {
        self.awaiting.is_none()
            && if self.proved_argument {
                !self.literal || self.connector
            } else {
                self.kind == GroupKind::Optional && self.literal
            }
    }
}

#[derive(Default)]
struct AssignmentSyntax {
    lexical: LexicalState,
    atom: Atom,
    has_value: bool,
    after_space: bool,
    /// Detached groups need native argument evidence. Li can supply the
    /// literal side of an assignment or alternative inside that group.
    detached: Option<DetachedGroup>,
}

impl AssignmentSyntax {
    fn parameter(&mut self, value: &str, proves_argument: bool) -> Result<(), ()> {
        for ch in value.chars() {
            if !self.punctuation(ch)? {
                self.lexical.observe(ch);
                self.record_value(proves_argument);
            }
        }
        Ok(())
    }

    fn path(&mut self, value: &str) -> Result<(), ()> {
        let path = value.trim_start();
        self.literal(&value[..value.len() - path.len()], Literal::Delimiter)?;
        if path.is_empty() {
            return Ok(());
        }
        if !self.atom.accepts_new_fragment() {
            return Err(());
        }
        let trimmed = path.trim_end();
        // mdoc_html.c::mdoc_pa_pre() preserves a pathname even when it
        // consists entirely of punctuation, as with /, . and ../.
        let punctuation_only = trimmed.chars().all(|ch| matches!(ch, '/' | '.'));
        if punctuation_only {
            self.record_value(true);
        } else {
            self.parameter(trimmed, true)?;
            // A typed pathname can itself end at a slash.
            if trimmed.ends_with('/') {
                self.record_value(true);
            }
        }
        if trimmed.ends_with(['/', '.']) {
            self.atom = Atom::PathContinuation;
        }
        self.literal(&path[trimmed.len()..], Literal::Delimiter)
    }

    fn record_value(&mut self, proves_argument: bool) {
        self.atom = Atom::Complete;
        self.has_value = true;
        self.after_space = false;
        if let Some(group) = &mut self.detached {
            group.proved_argument |= proves_argument;
            group.awaiting = None;
        }
    }

    fn literal(&mut self, value: &str, literal: Literal) -> Result<(), ()> {
        let mut in_atom = false;
        for ch in value.chars() {
            if ch == '\n' {
                return Err(());
            }
            if ch == '.' && in_atom {
                self.atom = Atom::AfterDot;
                continue;
            }
            if self.punctuation(ch)? {
                in_atom = false;
            } else if literal != Literal::Delimiter
                && (self.detached.is_none() || literal == Literal::Explicit)
                && (in_atom || self.atom.accepts_new_fragment())
                && parameter_character(ch)
            {
                self.lexical.observe(ch);
                if let Some(group) = &mut self.detached {
                    group.literal = true;
                    // A real glyph after a touching path separator or
                    // dot proves the continuation. A terminal dot or a
                    // separator followed by whitespace cannot do that.
                    group.connector |=
                        !in_atom && matches!(self.atom, Atom::AfterDot | Atom::PathContinuation);
                }
                self.record_value(false);
                in_atom = true;
            } else {
                return Err(());
            }
        }
        Ok(())
    }

    fn punctuation(&mut self, ch: char) -> Result<bool, ()> {
        if ch.is_whitespace() {
            if self
                .detached
                .is_some_and(|group| group.awaiting == Some(Join::Adjacent))
            {
                // A detached `default: value` note can be styled as Ar
                // on both sides. A colon joins atoms only without a gap.
                return Err(());
            }
            self.after_space = true;
            if matches!(self.atom, Atom::AfterDot | Atom::PathContinuation) {
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
                && (!self.has_value || !matches!(self.atom, Atom::Expected))
            {
                self.detached = Some(DetachedGroup {
                    kind: if ch == '[' {
                        GroupKind::Optional
                    } else {
                        GroupKind::Ordinary
                    },
                    ..DetachedGroup::default()
                });
            }
            let join = match ch {
                '|' | '=' | ',' => Some(Join::Word),
                ':' => Some(Join::Adjacent),
                '/' if !matches!(self.atom, Atom::Expected) && !self.after_space => {
                    Some(Join::Adjacent)
                }
                _ => None,
            };
            if let (Some(group), Some(join)) = (&mut self.detached, join) {
                let adjacent = join == Join::Adjacent;
                if matches!(self.atom, Atom::Expected) || adjacent && self.after_space {
                    return Err(());
                }
                group.connector = true;
                group.awaiting = Some(join);
            }
            let closed = self.lexical.observe(ch);
            if closed && self.detached.take().is_some_and(|group| !group.complete()) {
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
