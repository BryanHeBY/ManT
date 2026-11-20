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
            Role::ExplicitOption if operand.bytes.start < span.start => {
                // Fl can spell the start of its own assignment, including
                // brackets that close in later operands: --skip=[-][sample|time].
                syntax.literal(value, Literal::Explicit)
            }
            Role::ExplicitOption if syntax.lexical.within_group() => {
                // An inner Fl proves its own spelling inside the optional
                // group.
                let optional = syntax.group().is_some_and(|group| group.bracketed);
                syntax.parameter(value, optional)
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
    Path,
    PathContinuation,
}

impl Atom {
    fn accepts_new_fragment(self) -> bool {
        !matches!(self, Self::Complete | Self::Path)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Literal {
    Delimiter,
    Unmarked,
    Explicit,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ValueEvidence {
    Literal,
    Argument,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Join {
    Word,
    Adjacent,
}

/// Only an operator at this level can connect its two neighboring
/// operands. A child group returns one value after its own validation.
struct ArgumentGroup {
    bracketed: bool,
    completed: Option<ValueEvidence>,
    current: Option<ValueEvidence>,
    join: Option<Join>,
}

impl ArgumentGroup {
    fn new(bracketed: bool) -> Self {
        Self {
            bracketed,
            completed: None,
            current: None,
            join: None,
        }
    }

    fn begin_value(&mut self, evidence: ValueEvidence, adjacent: bool) -> Result<(), ()> {
        if self.join.take().is_some() || adjacent {
            self.current = Some(self.current.map_or(evidence, |word| word.max(evidence)));
        } else {
            self.finish_value()?;
            self.current = Some(evidence);
        }
        Ok(())
    }

    fn finish_value(&mut self) -> Result<(), ()> {
        if let Some(value) = self.current.take() {
            // Brackets can describe a selection of explicit literals.
            // A disconnected literal and an argument are separate words,
            // even when one has a delimiter in its own spelling.
            if self.completed == Some(ValueEvidence::Literal)
                || value == ValueEvidence::Literal && (!self.bracketed || self.completed.is_some())
            {
                return Err(());
            }
            self.completed = Some(value);
        }
        Ok(())
    }

    fn close(mut self) -> Result<(), ()> {
        self.finish_value()?;
        (self.completed.is_some() && self.join.is_none())
            .then_some(())
            .ok_or(())
    }
}

#[derive(Default)]
struct AssignmentSyntax {
    lexical: LexicalState,
    atom: Atom,
    has_value: bool,
    after_space: bool,
    // Untracked frames are attached to the assignment directly. A
    // detached group and each of its subexpressions need local proof.
    groups: Vec<Option<ArgumentGroup>>,
}

impl AssignmentSyntax {
    fn group(&self) -> Option<&ArgumentGroup> {
        self.groups.last().and_then(Option::as_ref)
    }

    fn group_mut(&mut self) -> Option<&mut ArgumentGroup> {
        self.groups.last_mut().and_then(Option::as_mut)
    }

    fn parameter(&mut self, value: &str, proves_argument: bool) -> Result<(), ()> {
        self.operand(value, proves_argument, false)
    }

    fn path(&mut self, value: &str) -> Result<(), ()> {
        self.operand(value, true, true)
    }

    fn operand(&mut self, value: &str, proves_argument: bool, is_path: bool) -> Result<(), ()> {
        let trimmed = value.trim_start();
        self.literal(&value[..value.len() - trimmed.len()], Literal::Delimiter)?;
        let operand = trimmed.trim_end();
        if operand.is_empty() {
            return Ok(());
        }
        let joined_path = is_path
            && !self.after_space
            && matches!(self.atom, Atom::Path)
            && operand.starts_with('/');
        if is_path && !self.atom.accepts_new_fragment() && !joined_path {
            return Err(());
        }
        if operand.contains('\n') {
            return Err(());
        }
        if self.group().is_some()
            && operand
                .split_whitespace()
                .rev()
                .skip(1)
                .any(|word| word.ends_with(':'))
        {
            return Err(());
        }
        // A typed operand can end in '/', and delimiters inside its
        // spelling cannot connect unrelated operands at this level.
        let continuation = !self.after_space
            && (joined_path || matches!(self.atom, Atom::PathContinuation | Atom::AfterDot));
        self.record_value(
            if proves_argument {
                ValueEvidence::Argument
            } else {
                ValueEvidence::Literal
            },
            continuation,
        )?;
        self.atom = if operand.ends_with('/') || is_path && operand.ends_with('.') {
            Atom::PathContinuation
        } else if is_path {
            Atom::Path
        } else {
            Atom::Complete
        };
        // A colon at the end of a typed word can introduce an adjacent
        // fragment, but a spaced explanatory label does not prove a form.
        if operand.ends_with(':')
            && let Some(group) = self.group_mut()
        {
            group.join = Some(Join::Adjacent);
            self.atom = Atom::Expected;
        }
        self.literal(&trimmed[operand.len()..], Literal::Delimiter)
    }

    fn record_value(&mut self, evidence: ValueEvidence, adjacent: bool) -> Result<(), ()> {
        if let Some(group) = self.group_mut() {
            group.begin_value(evidence, adjacent)?;
        }
        self.atom = Atom::Complete;
        self.has_value = true;
        self.after_space = false;
        Ok(())
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
                && (self.group().is_none() || literal == Literal::Explicit)
                && (in_atom || self.atom.accepts_new_fragment())
                && parameter_character(ch)
            {
                self.lexical.observe(ch);
                if in_atom {
                    self.atom = Atom::Complete;
                } else {
                    let continuation = matches!(self.atom, Atom::AfterDot | Atom::PathContinuation);
                    self.record_value(ValueEvidence::Literal, continuation)?;
                }
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
                .group()
                .is_some_and(|group| group.join == Some(Join::Adjacent))
            {
                return Err(());
            }
            self.after_space = true;
            self.atom = match self.atom {
                Atom::AfterDot => Atom::Complete,
                Atom::PathContinuation => Atom::Path,
                atom => atom,
            };
            return Ok(true);
        }
        let delimiter = closing(ch).is_some()
            || quote_closer(ch).is_some()
            || matches!(ch, ']' | '}' | ')' | '>' | '”' | '’');
        if delimiter {
            self.delimiter(ch)?;
        } else if matches!(ch, '=' | ',' | '/' | ':' | '@' | '|') {
            self.operator(ch)?;
        } else if ch == '.' && !self.after_space && self.has_value {
            self.atom = Atom::AfterDot;
        } else if !(matches!(ch, '.' | '…') || ch == '+' && self.lexical.within_group()) {
            return Ok(false);
        }
        self.after_space = false;
        Ok(true)
    }

    fn delimiter(&mut self, ch: char) -> Result<(), ()> {
        let depth = self.lexical.group_depth();
        self.lexical.observe(ch);
        match self.lexical.group_depth().cmp(&depth) {
            std::cmp::Ordering::Greater => {
                let tracked = self.group().is_some()
                    || depth == 0
                        && self.after_space
                        && (!self.has_value || !matches!(self.atom, Atom::Expected));
                self.groups
                    .push(tracked.then(|| ArgumentGroup::new(ch == '[')));
                self.atom = Atom::Expected;
            }
            std::cmp::Ordering::Less => {
                if let Some(Some(group)) = self.groups.pop() {
                    group.close()?;
                    self.record_value(ValueEvidence::Argument, false)?;
                }
                self.atom = Atom::Complete;
            }
            std::cmp::Ordering::Equal => {}
        }
        Ok(())
    }

    fn operator(&mut self, ch: char) -> Result<(), ()> {
        if ch == '/' && self.after_space && matches!(self.atom, Atom::Path) {
            return Err(());
        }
        let left = !matches!(self.atom, Atom::Expected);
        let adjacent = !self.after_space;
        let join = match ch {
            '|' | '=' | ',' => Some(Join::Word),
            ':' => Some(Join::Adjacent),
            '/' | '@' if left && adjacent => Some(Join::Adjacent),
            _ => None,
        };
        if let (Some(group), Some(join)) = (self.group_mut(), join) {
            if !left || join == Join::Adjacent && !adjacent {
                return Err(());
            }
            group.join = Some(join);
        }
        self.atom = Atom::Expected;
        Ok(())
    }
}
