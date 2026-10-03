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
mod arguments;
use arguments::valid_arguments;

#[derive(Clone, Copy)]
enum Wrapper {
    Bracket(char),
    Quote(char),
}

impl Wrapper {
    fn closer(self) -> char {
        match self {
            Self::Bracket(closer) | Self::Quote(closer) => closer,
        }
    }
}

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
    Styled,
    StyledGap,
    Bare,
    BareGap,
    Invalid,
}

impl Placeholder {
    fn observe(self, character: char, parameter: bool) -> Self {
        match (self, character) {
            (Self::Start, '=') => Self::Assigned,
            (Self::Start | Self::Assigned, 'A'..='Z')
            | (Self::Uppercase, 'A'..='Z' | '0'..='9' | '_' | '-') => Self::Uppercase,
            (Self::Uppercase, c) if c.is_whitespace() => Self::Uppercase,
            (Self::Start | Self::Assigned | Self::Styled, c)
                if parameter && (c.is_alphanumeric() || matches!(c, '_' | '-')) =>
            {
                Self::Styled
            }
            (Self::Styled | Self::StyledGap, c) if c.is_whitespace() => Self::StyledGap,
            (Self::Start | Self::Assigned, c) if !parameter && c.is_alphanumeric() => Self::Bare,
            (Self::Bare, c) if !parameter && (c.is_alphanumeric() || matches!(c, '_' | '-')) => {
                Self::Bare
            }
            (Self::Bare | Self::BareGap, c) if c.is_whitespace() => Self::BareGap,
            _ => Self::Invalid,
        }
    }
}

mod view;
use view::HeadView;

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
    argument_start: Option<Argument>,
    arguments: Vec<Argument>,
    styled_argument: bool,
    structured_argument: bool,
    lexical: LexicalState,
    wrappers: Vec<Wrapper>,
    admission: OwnerAdmission,
    separator: Option<usize>,
    placeholder: Placeholder,
}

struct Argument {
    bytes: Range<usize>,
    /// Recorded when the argument begins at the previous name's exact end,
    /// before any parameter token projection can alter its punctuation.
    attached_to_name: bool,
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
            wrappers: Vec::new(),
            admission: OwnerAdmission::Proved,
            separator: None,
            placeholder: Placeholder::Start,
        }
    }

    fn scan(mut self) -> OptionHead {
        self.open_wrappers();
        while let Some(character) = self.current() {
            if self.names.len() > MAX_NAMES {
                return OptionHead::name_limit();
            }
            if self.close_wrapper(character) {
                continue;
            }
            if self.argument_start.is_some() {
                self.consume_argument(character);
                continue;
            }
            if character.is_whitespace() || matches!(character, ',' | '|' | '/') {
                self.consume_separator(character);
                continue;
            }
            if self.view.is_parameter(self.cursor) {
                self.begin_argument();
                continue;
            }
            if character == '-' && self.consume_name() {
                continue;
            }
            self.begin_argument();
        }
        self.finish_argument();
        if self.names.len() > MAX_NAMES {
            return OptionHead::name_limit();
        }
        // A quoted value with no closing quote cannot prove that its dash
        // spelling is an entire declaration. Native incomplete bracket tags
        // retain their existing partial-name contract, but never infer owners.
        if self
            .wrappers
            .iter()
            .any(|wrapper| matches!(wrapper, Wrapper::Quote(_)))
        {
            self.names.clear();
        }
        let complete = self.lexical.is_top_level()
            && self.wrappers.is_empty()
            && !self.names.is_empty()
            && self
                .arguments
                .iter()
                .all(|argument| valid_arguments(self.view, argument));
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

    fn open_wrappers(&mut self) {
        let start = self.cursor;
        let mut inherited_parameter_font = false;
        while let Some(character) = self.current() {
            let wrapper = quote_closer(character)
                .map(Wrapper::Quote)
                .or_else(|| closing(character).map(Wrapper::Bracket));
            let Some(wrapper) = wrapper else { break };
            if self.wrappers.len() == MAX_NESTING || self.view.explicit_argument(self.cursor) {
                self.wrappers.clear();
                self.cursor = start;
                return;
            }
            // quote_pre keeps the outer font, while a real Fl supplies its
            // own accepted ExplicitOption receipt. That proof may override
            // presentation alone, never an actual Ar opening operand.
            inherited_parameter_font |= self.view.is_parameter(self.cursor);
            self.wrappers.push(wrapper);
            self.cursor += character.len_utf8();
            self.skip_whitespace();
        }
        if !self.view.text[self.cursor..].starts_with('-')
            || self.view.is_parameter(self.cursor)
            || inherited_parameter_font && !self.view.explicit_option(self.cursor)
        {
            self.wrappers.clear();
            self.cursor = start;
        }
    }

    fn close_wrapper(&mut self, character: char) -> bool {
        if !self.lexical.is_top_level()
            || self
                .wrappers
                .last()
                .is_none_or(|wrapper| wrapper.closer() != character)
        {
            return false;
        }
        self.finish_argument();
        self.wrappers.pop();
        self.cursor += character.len_utf8();
        true
    }

    fn consume_separator(&mut self, separator: char) {
        self.cursor += separator.len_utf8();
        self.skip_whitespace();
        if let Some(rest) = self.view.text[self.cursor..].strip_prefix("or")
            && !self.view.is_parameter(self.cursor)
            && !self.view.is_parameter(self.cursor + 1)
            && rest.starts_with(char::is_whitespace)
            && rest.trim_start().starts_with('-')
        {
            self.cursor += 2;
            self.skip_whitespace();
        }
        if self.current().is_some_and(|c| c != '-')
            && self
                .current()
                .is_none_or(|c| self.wrappers.last().is_none_or(|w| w.closer() != c))
            || self.view.is_parameter(self.cursor)
            || separator.is_whitespace()
                && self.next_is_parameter_token()
                && !self.view.explicit_option(self.cursor)
                && !self.names.is_empty()
                && !self.numeric_enumeration()
        {
            self.begin_argument();
        }
    }

    fn numeric_enumeration(&self) -> bool {
        !self.wrappers.is_empty()
            && self.names.iter().all(|name| {
                name.name
                    .strip_prefix('-')
                    .is_some_and(|value| value.chars().all(|c| c.is_ascii_digit()))
            })
    }

    fn consume_name(&mut self) -> bool {
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
            return true;
        }
        let pattern_end = self.view.text[start..]
            .find(|c: char| !matches!(c, '-' | '#'))
            .map_or(self.view.text.len(), |end| start + end);
        if pattern_end > start + 1 && self.view.text[start..pattern_end].contains('#') {
            self.cursor = pattern_end;
            return true;
        }
        let mut suffix = self.view.text[start + 1..].chars();
        if let Some(character) = suffix.next()
            && character.is_ascii_punctuation()
            && !matches!(
                character,
                '-' | '=' | '[' | ']' | '{' | '}' | '(' | ')' | '<' | '>' | ',' | '|'
            )
            && suffix
                .next()
                .is_none_or(|next| next.is_whitespace() || matches!(next, ',' | '|' | '/'))
        {
            // Complete punctuation flags are declaration syntax even when
            // the selectable-name grammar intentionally does not name them.
            self.cursor += 2;
            return true;
        }
        false
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
        self.argument_start.get_or_insert_with(|| Argument {
            bytes: self.cursor..self.cursor,
            attached_to_name: self.names.last().is_some_and(|name| {
                name.parts
                    .last()
                    .is_some_and(|part| part.end == self.cursor)
            }),
        });
        self.styled_argument |= self.view.is_parameter(self.cursor);
    }

    fn finish_argument(&mut self) {
        if let Some(mut argument) = self.argument_start.take() {
            argument.bytes.end = self.cursor;
            self.arguments.push(argument);
        }
        self.styled_argument = false;
        self.structured_argument = false;
        self.separator = None;
        self.placeholder = Placeholder::Start;
    }

    fn complete_parameter_token(&self, delimiter_is_parameter: bool) -> bool {
        match self.placeholder {
            Placeholder::Uppercase => true,
            Placeholder::Styled | Placeholder::StyledGap if !delimiter_is_parameter => {
                self.argument_start.as_ref().is_some_and(|argument| {
                    // A styled suffix adjoining the name is only part of a
                    // native word. Author whitespace or an explicit assignment
                    // supplies the left boundary of a complete parameter token.
                    !argument.attached_to_name
                        || self.view.text[argument.bytes.start..].starts_with('=')
                })
            }
            _ => false,
        }
    }

    fn consume_argument(&mut self, character: char) {
        let mut next_offset = self.cursor + character.len_utf8();
        let role = self.view.operand(self.cursor);
        let parameter = self.view.is_parameter(self.cursor);
        self.styled_argument |= parameter;
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
            // term_word() may change fonts within one native TEXT operand.
            // A complete parameter token followed by literal punctuation is
            // declaration syntax, without claiming that the font change was
            // a new native word. Punctuation still inside a parameter cannot
            // supply this proof, nor can a multiword styled prose suffix.
            let complete_parameter = self.complete_parameter_token(parameter)
                || matches!(character, ',' | '|') && self.complete_bare_parameter(following);
            let boundary = matches!(character, ',' | '|')
                && (self.styled_argument || self.structured_argument || complete_parameter)
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
                && (!self.view.is_parameter(restart)
                    || self.argument_start.as_ref().is_some_and(|argument| {
                        argument.attached_to_name
                            && !self.view.text[argument.bytes.start..].starts_with('=')
                            && self.view.style(restart).strong
                    }))
                && self.styled_argument
                && !self.structured_argument
                && !complete_parameter
                && !self.view.literal_operand_starts(restart)
            {
                // A literal receipt prevents a false name inside one word;
                // it cannot make that unfinished parameter prove an owner.
                self.admission = OwnerAdmission::UnprovedStyleRestart;
            }
            if boundary
                && self.view.text[restart..].starts_with('-')
                && !self.view.is_parameter(restart)
                && (role.is_none()
                    || self.structured_argument
                    || complete_parameter
                    || self.view.literal_operand_starts(restart))
            {
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
        self.placeholder = self.placeholder.observe(character, parameter);
        self.cursor = next_offset;
    }

    fn complete_bare_parameter(&self, following: usize) -> bool {
        if !matches!(self.placeholder, Placeholder::Bare | Placeholder::BareGap)
            || self.styled_argument
            || self.argument_start.as_ref().is_none_or(|argument| {
                argument.attached_to_name
                    && !self.view.text[argument.bytes.start..].starts_with('=')
            })
            || self.view.is_parameter(following)
            || !(self.view.style(following).strong
                || self.view.explicit_option(following)
                || self.view.literal_operand_starts(following))
        {
            return false;
        }
        // A single bare parameter is accepted by valid_arguments, but a
        // comma alone does not separate declarations: first,--fake,last and
        // temporary fonts remain one parameter. Require an independently
        // styled/owned, complete option on its right, with its own ending.
        let suffix = &self.view.text[following..];
        let Some(name) = super::options::option_prefix(suffix) else {
            return false;
        };
        let end = following + name.len();
        if (following..end)
            .any(|offset| self.view.text.is_char_boundary(offset) && self.view.is_parameter(offset))
        {
            return false;
        }
        suffix[name.len()..].chars().next().is_none_or(|next| {
            next.is_whitespace()
                || next == '='
                || matches!(next, ',' | '|')
                    && suffix[name.len() + next.len_utf8()..].starts_with(char::is_whitespace)
                || self
                    .wrappers
                    .last()
                    .is_some_and(|wrapper| wrapper.closer() == next)
        })
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

#[cfg(test)]
mod tests;
