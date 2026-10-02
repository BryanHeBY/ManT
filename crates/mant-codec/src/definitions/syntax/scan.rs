//! Ordered declaration recognition over accepted text and operand evidence.
//!
//! Font runs describe effective styling, not roff operands. In particular,
//! `man_term.c::pre_alternate()` calls `term_word()` once per TEXT child even when
//! that child changes fonts. Only the private accepted operand receipt can
//! establish that a later literal belongs to a different native operand.

use std::ops::Range;

use mant_ir::Inline;

use crate::definitions::{NativeOperand, NativeOperandRole, RecognizedName};

const MAX_NAMES: usize = 256;
const MAX_NESTING: usize = 64;

mod lexical;
use lexical::LexicalState;

#[derive(PartialEq, Eq)]
enum OwnerAdmission {
    Proved,
    UnprovedStyleRestart,
}

#[derive(Clone, Copy)]
enum Placeholder {
    Start,
    Assigned,
    Uppercase,
    Invalid,
}

impl Placeholder {
    fn observe(self, character: char) -> Self {
        match (self, character) {
            (Self::Start, '=') => Self::Assigned,
            (Self::Assigned, 'A'..='Z') | (Self::Uppercase, 'A'..='Z' | '0'..='9' | '_' | '-') => {
                Self::Uppercase
            }
            (Self::Uppercase, c) if c.is_whitespace() => Self::Uppercase,
            _ => Self::Invalid,
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Style {
    strong: bool,
    emphasis: bool,
}

struct Run {
    bytes: Range<usize>,
    style: Style,
}

struct HeadView<'a> {
    text: String,
    runs: Vec<Run>,
    operands: &'a [NativeOperand],
    literal_starts: Vec<usize>,
}

impl<'a> HeadView<'a> {
    fn new(nodes: &[Inline], operands: &'a [NativeOperand]) -> Self {
        let mut view = Self {
            text: String::new(),
            runs: Vec::new(),
            operands,
            literal_starts: Vec::new(),
        };
        view.append(nodes, Style::default());
        // An authored leading blank belongs to the operand; its first name
        // starts later. Compute that point once instead of rescanning prefixes.
        view.literal_starts = operands
            .iter()
            .filter_map(|operand| {
                if operand.role == NativeOperandRole::Argument {
                    return None;
                }
                let text = view.text.get(operand.bytes.clone())?;
                let name = text.trim_start();
                (!name.is_empty()).then_some(operand.bytes.start + text.len() - name.len())
            })
            .collect();
        view
    }

    fn append(&mut self, nodes: &[Inline], style: Style) {
        for node in nodes {
            match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    let start = self.text.len();
                    self.text.push_str(value);
                    self.runs.push(Run {
                        bytes: start..self.text.len(),
                        style,
                    });
                }
                Inline::Strong { children } => self.append(
                    children,
                    Style {
                        strong: true,
                        ..style
                    },
                ),
                Inline::Emphasis { children } => {
                    self.append(
                        children,
                        Style {
                            emphasis: true,
                            ..style
                        },
                    );
                }
                Inline::Link { children, .. } => self.append(children, style),
                Inline::LineBreak { .. } => {
                    let start = self.text.len();
                    self.text.push('\n');
                    self.runs.push(Run {
                        bytes: start..start + 1,
                        style,
                    });
                }
                Inline::Anchor { .. } => {}
            }
        }
    }

    fn style(&self, offset: usize) -> Style {
        let index = self.runs.partition_point(|run| run.bytes.end <= offset);
        self.runs
            .get(index)
            .map_or(Style::default(), |run| run.style)
    }

    fn operand(&self, offset: usize) -> Option<&NativeOperand> {
        let index = self
            .operands
            .partition_point(|operand| operand.bytes.end <= offset);
        self.operands
            .get(index)
            .filter(|operand| operand.bytes.contains(&offset))
    }

    fn is_parameter(&self, offset: usize) -> bool {
        if let Some(operand) = self.operand(offset) {
            match operand.role {
                NativeOperandRole::Argument => return true,
                NativeOperandRole::ExplicitOption => return false,
                NativeOperandRole::Literal => {}
            }
        }
        let style = self.style(offset);
        style.emphasis && !style.strong
    }

    fn literal_operand_starts(&self, offset: usize) -> bool {
        self.literal_starts.binary_search(&offset).is_ok()
    }

    fn explicit_option(&self, offset: usize) -> bool {
        self.operand(offset)
            .is_some_and(|operand| operand.role == NativeOperandRole::ExplicitOption)
    }
}

pub(super) struct OptionHead {
    pub(super) names: Vec<RecognizedName>,
    #[cfg(test)]
    pub(super) complete: bool,
    pub(super) inferred_complete: bool,
    pub(super) limit: Option<DeclarationLimit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::definitions) enum DeclarationLimit {
    Names,
}

impl OptionHead {
    fn name_limit() -> Self {
        Self {
            names: Vec::new(),
            #[cfg(test)]
            complete: false,
            inferred_complete: false,
            limit: Some(DeclarationLimit::Names),
        }
    }
}

pub(super) fn option_head(nodes: &[Inline], operands: &[NativeOperand]) -> OptionHead {
    let view = HeadView::new(nodes, operands);
    Scanner::new(&view).scan()
}

struct Scanner<'a> {
    view: &'a HeadView<'a>,
    cursor: usize,
    names: Vec<RecognizedName>,
    argument_start: Option<usize>,
    arguments: Vec<Range<usize>>,
    styled_argument: bool,
    structured_argument: bool,
    lexical: LexicalState,
    wrapper: Option<char>,
    admission: OwnerAdmission,
    separator: Option<usize>,
    placeholder: Placeholder,
}

impl<'a> Scanner<'a> {
    fn new(view: &'a HeadView<'a>) -> Self {
        Self {
            view,
            cursor: view.text.len() - view.text.trim_start().len(),
            names: Vec::new(),
            argument_start: None,
            arguments: Vec::new(),
            styled_argument: false,
            structured_argument: false,
            lexical: LexicalState::default(),
            wrapper: None,
            admission: OwnerAdmission::Proved,
            separator: None,
            placeholder: Placeholder::Start,
        }
    }

    fn scan(mut self) -> OptionHead {
        if let Some(character) = self.current()
            && let Some(closer) = closing(character)
            && self.view.text[self.cursor + character.len_utf8()..]
                .trim_start()
                .starts_with('-')
        {
            self.wrapper = Some(closer);
            self.cursor += character.len_utf8();
        }
        while let Some(character) = self.current() {
            if self.names.len() > MAX_NAMES {
                return OptionHead::name_limit();
            }
            if self.argument_start.is_some() {
                self.consume_argument(character);
                continue;
            }
            if character.is_whitespace() || matches!(character, ',' | '|' | '/') {
                let separator = character;
                self.cursor += character.len_utf8();
                self.skip_whitespace();
                if self.view.text[self.cursor..].starts_with("or ") {
                    self.cursor += 3;
                    self.skip_whitespace();
                }
                if self.current().is_some_and(|c| c != '-')
                    || self.view.is_parameter(self.cursor)
                    || (separator.is_whitespace()
                        && self.next_is_parameter_token()
                        && !self.view.explicit_option(self.cursor)
                        && !self.names.is_empty()
                        && self.wrapper.is_none())
                {
                    self.begin_argument();
                }
                continue;
            }
            if Some(character) == self.wrapper {
                self.wrapper = None;
                self.cursor += character.len_utf8();
                continue;
            }
            if self.view.is_parameter(self.cursor) {
                self.begin_argument();
                continue;
            }
            if character == '-' {
                let start = self.cursor;
                let mut end = start + 1;
                for (index, next) in self.view.text[end..].char_indices() {
                    let offset = start + 1 + index;
                    if !option_character(next) || self.view.is_parameter(offset) {
                        break;
                    }
                    end = offset + next.len_utf8();
                }
                if let Some(name) = super::options::option_prefix(&self.view.text[start..end]) {
                    self.names.push(RecognizedName::contiguous(name, start));
                    self.cursor = start + name.len();
                    if self.cursor < end {
                        self.begin_argument();
                    }
                    continue;
                }
                let pattern_end = self.view.text[start..]
                    .find(|c: char| !matches!(c, '-' | '#'))
                    .map_or(self.view.text.len(), |end| start + end);
                if pattern_end > start + 1 && self.view.text[start..pattern_end].contains('#') {
                    self.cursor = pattern_end;
                    continue;
                }
            }
            self.begin_argument();
        }
        self.finish_argument();
        if self.names.len() > MAX_NAMES {
            return OptionHead::name_limit();
        }
        let complete = self.lexical.is_top_level()
            && self.wrapper.is_none()
            && !self.names.is_empty()
            && self
                .arguments
                .iter()
                .all(|range| valid_arguments(self.view, range.clone()));
        OptionHead {
            names: self.names,
            #[cfg(test)]
            complete,
            inferred_complete: complete && self.admission == OwnerAdmission::Proved,
            limit: None,
        }
    }

    fn current(&self) -> Option<char> {
        self.view.text[self.cursor..].chars().next()
    }

    fn skip_whitespace(&mut self) {
        while self.current().is_some_and(char::is_whitespace) {
            self.cursor += self.current().expect("observed a scalar").len_utf8();
        }
    }

    fn next_is_parameter_token(&self) -> bool {
        let token = self.view.text[self.cursor..]
            .split_whitespace()
            .next()
            .unwrap_or_default();
        token.strip_prefix('-').is_some_and(|suffix| {
            suffix.chars().next().is_some_and(|c| c.is_ascii_digit())
                || suffix.len() > 1 && suffix.chars().all(|c| c.is_ascii_uppercase() || c == '_')
        })
    }

    fn begin_argument(&mut self) {
        self.argument_start.get_or_insert(self.cursor);
        self.styled_argument |= self.view.is_parameter(self.cursor);
    }

    fn finish_argument(&mut self) {
        if let Some(start) = self.argument_start.take() {
            self.arguments.push(start..self.cursor);
        }
        self.styled_argument = false;
        self.structured_argument = false;
        self.separator = None;
        self.placeholder = Placeholder::Start;
    }

    fn consume_argument(&mut self, character: char) {
        let mut next_offset = self.cursor + character.len_utf8();
        let role = self.view.operand(self.cursor);
        self.styled_argument |= self.view.is_parameter(self.cursor);
        if self.lexical.is_top_level() {
            let mut following = next_offset;
            if matches!(character, ',' | '|') || character.is_whitespace() {
                while self.view.text[following..].starts_with(char::is_whitespace) {
                    following += self.view.text[following..]
                        .chars()
                        .next()
                        .expect("whitespace scalar")
                        .len_utf8();
                }
            }
            let boundary = matches!(character, ',' | '|')
                && (self.styled_argument
                    || self.structured_argument
                    || matches!(self.placeholder, Placeholder::Uppercase))
                || (self.view.literal_operand_starts(self.cursor)
                    || self.view.literal_operand_starts(following))
                    && self.separator.is_some()
                || character.is_whitespace() && matches!(self.placeholder, Placeholder::Uppercase);
            let restart =
                if following == next_offset && self.view.literal_operand_starts(self.cursor) {
                    self.cursor
                } else {
                    following
                };
            if boundary
                && self.view.text[restart..].starts_with('-')
                && !self.view.is_parameter(restart)
                && (role.is_none()
                    || self.structured_argument
                    || matches!(self.placeholder, Placeholder::Uppercase)
                    || self.view.literal_operand_starts(restart))
            {
                if role.is_none() && self.styled_argument && !self.structured_argument {
                    self.admission = OwnerAdmission::UnprovedStyleRestart;
                }
                self.finish_argument();
                self.cursor = restart;
                return;
            }
            if character.is_whitespace() {
                // This whole scalar run has one lexical event. Advance it
                // together so an unsuccessful lookahead cannot rescan every
                // suffix of a long argument whitespace run.
                next_offset = following;
            }
            if matches!(character, ',' | '|') {
                self.separator = Some(self.cursor);
            } else if !character.is_whitespace() {
                self.separator = None;
            }
        }
        let top_level = self.lexical.is_top_level();
        if self.lexical.observe(character) {
            self.structured_argument = true;
        } else if top_level && !character.is_whitespace() {
            self.structured_argument = false;
        }
        self.placeholder = self.placeholder.observe(character);
        self.cursor = next_offset;
    }
}

fn option_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?' | '.' | '+')
}

fn closing(character: char) -> Option<char> {
    match character {
        '[' => Some(']'),
        '{' => Some('}'),
        '(' => Some(')'),
        '<' => Some('>'),
        _ => None,
    }
}

fn quote_closer(character: char) -> Option<char> {
    match character {
        '"' | '\'' => Some(character),
        '“' => Some('”'),
        '‘' => Some('’'),
        _ => None,
    }
}

fn valid_arguments(view: &HeadView<'_>, range: Range<usize>) -> bool {
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
        if view.is_parameter(offset) {
            if !opaque {
                literal.push_str(" \0 ");
            }
            opaque = true;
        } else if opening {
            literal.push_str(" \0 ");
            opaque = false;
        } else if nested {
            if lexical.is_top_level() {
                literal.push(' ');
            }
        } else {
            literal.push(character);
            opaque = false;
        }
        offset += character.len_utf8();
    }
    let mut bare = 0;
    for token in literal.split_whitespace() {
        if token == "\0" || token == "..." || token == "=" {
            continue;
        }
        if token.starts_with('=')
            || token.starts_with('/')
            || token.split_once('=').is_some_and(|(name, value)| {
                super::named::is_variable_term(name) && !value.is_empty()
            })
            || token.contains(':')
                && token
                    .split(':')
                    .all(|part| part == "\0" || super::named::is_variable_term(part))
            || token.chars().any(char::is_uppercase)
                && token
                    .chars()
                    .all(|c| c.is_uppercase() || c.is_ascii_digit() || matches!(c, '_' | '-'))
        {
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

#[cfg(test)]
mod tests;
