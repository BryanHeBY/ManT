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
    let start = span.start + 1;
    let spelling = view.text[start..span.end].trim_end();
    let rhs = start..start + spelling.len();
    if view.operand(span.start).is_some_and(|operand| {
        operand.role == Role::ExplicitOption
            && operand.bytes.start < span.start
            && operand.bytes.end >= rhs.end
            && complete_flag_value(view, rhs.clone())
    }) {
        return true;
    }
    let mut cursor = rhs.start;
    let mut syntax = AssignmentSyntax::default();
    let mut scopes = Vec::new();
    let mut operands = view.operands_in(rhs.clone());
    while let Some(operand) = operands.next() {
        let start = operand.bytes.start.max(cursor);
        let mut end = operand.bytes.end.min(rhs.end);
        if matches!(
            operand.role,
            Role::Placeholder { .. } | Role::PathOperand(_)
        ) {
            // The head index exposes an alternative pipe separately.
            // Matching typed node IDs on both sides still prove that
            // the complete spelling belongs to one authored operand.
            loop {
                let mut lookahead = operands.clone();
                let (Some(separator), Some(rest)) = (lookahead.next(), lookahead.next()) else {
                    break;
                };
                if separator.role != Role::Argument
                    || separator.bytes.start != end
                    || rest.bytes.start != separator.bytes.end
                    || rest.role != operand.role
                {
                    break;
                }
                end = rest.bytes.end.min(rhs.end);
                operands = lookahead;
            }
        }
        if syntax
            .literal(&view.text[cursor..start], Literal::Unmarked)
            .is_err()
        {
            return false;
        }
        let value = &view.text[start..end];
        let touches_next = view.text[end..rhs.end].starts_with(|character: char| {
            !character.is_whitespace() && !matches!(character, ']' | ')' | '}' | '>')
        }) && view.operand(end).is_none_or(|next| {
            next.bytes.start == end
                && !matches!(next.role, Role::EnclosureEnd(_) | Role::ClippedEnclosure)
        });
        let piece = OperandPiece {
            role: operand.role,
            value,
            owns_assignment_prefix: operand.bytes.start < span.start,
            tail: if touches_next {
                OperandTail::Joined
            } else if view
                .operand(end)
                .is_some_and(|next| matches!(next.role, Role::EnclosureEnd(_)))
            {
                OperandTail::EnclosureEnd
            } else {
                OperandTail::Separate
            },
            start,
        };
        if syntax.execute_operand(piece, view, &mut scopes).is_err() {
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

/// A complete RHS inside its own Fl operand supplies its literal spelling.
/// Keep grouped suffixes which extend into subsequent operands on the
/// shared, scoped proof path below.
fn complete_flag_value(view: &HeadView<'_>, rhs: std::ops::Range<usize>) -> bool {
    let mut lexical = LexicalState::default();
    for (offset, ch) in view.text[rhs.clone()].char_indices() {
        if !parameter_character(ch) && !"+%*?#~&!^$;/:@=.,|[](){}<>".contains(ch) {
            return false;
        }
        if !matches!(ch, '<' | '>')
            || view.paired_assignment_angle(rhs.start + offset)
            || offset == 0
            || offset + 1 == rhs.len()
        {
            lexical.observe(ch);
        }
    }
    !rhs.is_empty() && lexical.is_top_level()
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
    Assignment,
}

/// A pair contained in one typed operand is its author's spelling. An
/// unmatched bracket can instead continue through other native operands.
fn open_operand_edges(value: &str) -> Result<Vec<(usize, char)>, ()> {
    let mut edges = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    for (offset, character) in value.char_indices() {
        if std::mem::take(&mut escaped) {
            continue;
        }
        if character == '\\' {
            escaped = true;
        } else if let Some(end) = quote {
            if character == end {
                quote = None;
            }
        } else if let Some(end) = quote_closer(character) {
            quote = Some(end);
        } else if edges
            .last()
            .is_some_and(|&(_, open)| closing(open) == Some(character))
        {
            edges.pop();
        } else if closing(character).is_some() || matches!(character, ']' | '}' | ')' | '>') {
            if edges.len() == MAX_NESTING {
                return Err(());
            }
            edges.push((offset, character));
        }
    }
    Ok(edges)
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

#[derive(Clone, Copy)]
struct OperandPiece<'a> {
    role: Role,
    value: &'a str,
    owns_assignment_prefix: bool,
    tail: OperandTail,
    start: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OperandTail {
    Joined,
    EnclosureEnd,
    Separate,
}

impl AssignmentSyntax {
    fn execute_operand(
        &mut self,
        piece: OperandPiece<'_>,
        view: &HeadView<'_>,
        scopes: &mut Vec<(u32, bool)>,
    ) -> Result<(), ()> {
        let OperandPiece {
            role,
            value,
            owns_assignment_prefix,
            ..
        } = piece;
        match role {
            Role::Placeholder { .. } | Role::SearchArgument(_) => {
                self.operand(piece, view, ValueEvidence::Argument, false)
            }
            Role::PathOperand(_) => self.operand(piece, view, ValueEvidence::Argument, true),
            Role::ExplicitOption if owns_assignment_prefix => {
                // Fl can spell the start of its own assignment, including
                // brackets that close in later operands: --skip=[-][sample|time].
                self.literal(value, Literal::Assignment)
            }
            Role::ExplicitOption if self.lexical.within_group() => {
                // An inner Fl proves its own spelling inside the optional
                // group.
                let optional = self.group().is_some_and(|group| group.bracketed);
                self.operand(
                    piece,
                    view,
                    if optional {
                        ValueEvidence::Argument
                    } else {
                        ValueEvidence::Literal
                    },
                    false,
                )
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
                self.literal(value, Literal::Delimiter)
            }
            Role::EnclosureEnd(body) if scopes.pop().is_some_and(|(scope, _)| scope == body) => {
                self.literal(value, Literal::Delimiter)
            }
            Role::EnclosureContent { body, bare }
                if scopes.last().is_some_and(|&(scope, _)| scope == body) =>
            {
                // Actual Op/Oo can supply one bare value such as [host].
                // A Bq with the same terminal glyphs makes no such claim.
                // A No dot can finish a pending choice together with the
                // adjacent part of a Pa in the same generated enclosure.
                let path_prefix = relative_path_prefix(value.trim())
                    && piece.tail == OperandTail::Joined
                    && self
                        .group()
                        .is_some_and(|group| group.join == Some(Join::Word))
                    && view
                        .operand(piece.start + value.len())
                        .is_some_and(|operand| operand.role == Role::PathOperand(Some(body)));
                let literal = if path_prefix {
                    Literal::Explicit
                } else {
                    scoped_literal(scopes.last().filter(|_| bare))
                };
                self.literal(value, literal)
            }
            Role::Command => self.literal(value, scoped_literal(scopes.last())),
            Role::LiteralValue => self.literal(value, Literal::Explicit),
            Role::Literal | Role::LiteralContinuation(_) | Role::Description => {
                self.literal(value, Literal::Unmarked)
            }
            Role::Argument
            | Role::LiteralDelimiter { .. }
            | Role::LiteralPunctuation
            | Role::PipeDelimiter(_)
            | Role::SearchSign
            | Role::SearchOpen { .. }
            | Role::SearchClose => self.literal(value, Literal::Delimiter),
            _ => Err(()),
        }
    }

    fn group(&self) -> Option<&ArgumentGroup> {
        self.groups.last().and_then(Option::as_ref)
    }

    fn group_mut(&mut self) -> Option<&mut ArgumentGroup> {
        self.groups.last_mut().and_then(Option::as_mut)
    }

    fn operand(
        &mut self,
        piece: OperandPiece<'_>,
        view: &HeadView<'_>,
        evidence: ValueEvidence,
        is_path: bool,
    ) -> Result<(), ()> {
        let OperandPiece {
            value,
            tail,
            start: value_start,
            ..
        } = piece;
        let trimmed = value.trim_start();
        self.literal(&value[..value.len() - trimmed.len()], Literal::Delimiter)?;
        let spelling = trimmed.trim_end();
        let spelling_start = value_start + (value.len() - trimmed.len());
        let mut start = 0;
        for (offset, bracket) in open_operand_edges(spelling)? {
            // Comparison signs in a typed operand carry their own spelling.
            // Export an unmatched angle only when it has a cross-operand mate.
            if matches!(bracket, '<' | '>')
                && !view.paired_assignment_angle(spelling_start + offset)
            {
                continue;
            }
            self.operand_fragment(
                &spelling[start..offset],
                evidence,
                is_path,
                OperandTail::Separate,
            )?;
            self.delimiter(bracket)?;
            self.after_space = false;
            start = offset + bracket.len_utf8();
        }
        self.operand_fragment(
            &spelling[start..],
            evidence,
            is_path,
            if tail == OperandTail::Joined && spelling.len() != trimmed.len() {
                OperandTail::Separate
            } else {
                tail
            },
        )?;
        self.literal(&trimmed[spelling.len()..], Literal::Delimiter)
    }

    fn operand_fragment(
        &mut self,
        value: &str,
        evidence: ValueEvidence,
        is_path: bool,
        tail: OperandTail,
    ) -> Result<(), ()> {
        let mut operand = value.trim();
        self.literal(
            &value[..value.len() - value.trim_start().len()],
            Literal::Delimiter,
        )?;
        if !self.after_space
            && !matches!(self.atom, Atom::Expected)
            && let Some(edge) = operand
                .chars()
                .next()
                .filter(|&ch| is_operand_connector(ch))
        {
            self.operator(edge)?;
            operand = &operand[edge.len_utf8()..];
        }
        let trailing_connector = (tail == OperandTail::Joined)
            .then(|| {
                operand
                    .chars()
                    .next_back()
                    .filter(|&ch| is_operand_connector(ch))
            })
            .flatten();
        if let Some(edge) = trailing_connector {
            operand = &operand[..operand.len() - edge.len_utf8()];
        }
        if operand.is_empty() {
            return Ok(());
        }
        let joined_path = is_path
            && !self.after_space
            && matches!(self.atom, Atom::Path)
            && (operand.starts_with('/') || self.lexical.within_group());
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
        self.record_value(evidence, continuation)?;
        self.atom = if operand.ends_with('/') || is_path && operand.ends_with('.') {
            Atom::PathContinuation
        } else if is_path {
            Atom::Path
        } else {
            Atom::Complete
        };
        // A colon at the end of a typed word can introduce an adjacent
        // fragment, but a spaced explanatory label does not prove a form.
        if let Some(edge) = trailing_connector {
            self.operator(edge)?;
        } else if operand.ends_with(':')
            && self.group().is_some()
            && tail != OperandTail::EnclosureEnd
        {
            self.operator(':')?;
        }
        self.literal(&value[value.trim_end().len()..], Literal::Delimiter)
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
            // A single explicit Fl can supply a complete value spelling
            // like c++17, %s or *.conf. Group punctuation still executes.
            let fl_value = literal == Literal::Assignment
                && matches!(
                    ch,
                    '+' | '%' | '*' | '?' | '#' | '~' | '&' | '!' | '^' | '$'
                );
            if ch == '.' && self.after_space && literal == Literal::Unmarked {
                return Err(());
            }
            if !fl_value && self.punctuation(ch)? {
                in_atom = false;
            } else if literal != Literal::Delimiter
                && (self.group().is_none()
                    || matches!(literal, Literal::Explicit | Literal::Assignment))
                && (in_atom || self.atom.accepts_new_fragment())
                && (parameter_character(ch) || fl_value)
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
        } else if ch == '.' {
            if self.after_space {
                self.start_path_value()?;
            } else if !matches!(self.atom, Atom::Expected | Atom::PathContinuation) {
                self.atom = Atom::AfterDot;
            }
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
        if ch == '/' && !(left && adjacent) {
            // A unary slash starts a fresh value. Its suffix cannot prove
            // the earlier literal across a real word separation.
            return self.start_path_value();
        }
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

    fn start_path_value(&mut self) -> Result<(), ()> {
        if let Some(group) = self.group_mut()
            && group.join.is_none()
        {
            group.finish_value()?;
        }
        self.atom = Atom::PathContinuation;
        Ok(())
    }
}

fn is_operand_connector(character: char) -> bool {
    matches!(character, '=' | '|' | ',' | ':' | '@')
}

fn relative_path_prefix(value: &str) -> bool {
    value == "."
        || value
            .strip_suffix('/')
            .is_some_and(|path| path.split('/').all(|part| matches!(part, "." | "..")))
}
