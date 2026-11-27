//! Ordered declaration recognition over accepted text and operand evidence.
//!
//! Font runs describe effective styling, not roff operands. In particular,
//! `man_term.c::pre_alternate()` calls `term_word()` once per TEXT child even when
//! that child changes fonts. Only the private accepted operand receipt can
//! establish that a later literal belongs to a different native operand.

use std::ops::Range;

use mant_ir::Inline;

use crate::definitions::{NativeOperand, NativeOperandRole, RecognizedName};

pub(super) const MAX_NAMES: usize = 256;
pub(super) const MAX_NESTING: usize = 64;

mod lexical;
use lexical::LexicalState;
mod arguments;
use arguments::valid_arguments;
mod parameter_token;
use parameter_token::ParameterToken;

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

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ParameterStyle {
    #[default]
    Empty,
    Literal,
    Styled,
    Mixed,
}

#[derive(Clone, Copy, Default)]
struct Placeholder {
    token: ParameterToken,
    style: ParameterStyle,
    assigned: bool,
}

impl Placeholder {
    fn observe(mut self, character: char, parameter: bool) -> Self {
        if character == '=' && self.token.is_empty() && !self.assigned {
            self.assigned = true;
            return self;
        }
        if !character.is_whitespace() {
            let style = if parameter {
                ParameterStyle::Styled
            } else {
                ParameterStyle::Literal
            };
            self.style = match self.style {
                ParameterStyle::Empty => style,
                current if current == style => current,
                _ => ParameterStyle::Mixed,
            };
        }
        self.token = self.token.observe(character);
        self
    }

    fn is_styled(self) -> bool {
        self.style == ParameterStyle::Styled && self.token.is_word()
    }

    fn is_bare(self) -> bool {
        self.style == ParameterStyle::Literal && self.token.is_bare()
    }
}

mod view;
pub(super) use view::HeadView;
#[cfg(feature = "roff")]
pub(super) use view::paired_option_assignment_ranges;

pub(super) struct OptionHead {
    pub(super) names: Vec<RecognizedName>,
    #[cfg(test)]
    pub(super) complete: bool,
    pub(super) inferred_complete: bool,
    pub(super) unproved_form: bool,
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
            unproved_form: false,
            limit: Some(DeclarationLimit::Names),
        }
    }
}

pub(super) fn option_head(nodes: &[Inline], operands: &[NativeOperand]) -> OptionHead {
    let view = HeadView::new(nodes, operands);
    Scanner::new(&view).scan()
}

/// The executed key can follow a generated optional opener. Only a leading
/// accepted option, and not a later operand in an unrelated label, supplies
/// a stronger role than the section's default.
pub(super) fn has_leading_explicit_option(text: &str, operands: &[NativeOperand]) -> bool {
    let head = text.trim_start_matches(|ch: char| {
        ch.is_whitespace() || closing(ch).is_some() || quote_closer(ch).is_some()
    });
    if !head.starts_with('-') {
        return false;
    }
    let start = text.len() - head.len();
    if operands
        .iter()
        .take_while(|operand| operand.bytes.start < start)
        .any(|operand| {
            operand.role.is_parameter()
                && text
                    .get(operand.bytes.clone())
                    .is_some_and(|prefix| !prefix.trim().is_empty())
        })
    {
        return false;
    }
    operands.iter().any(|operand| {
        operand.role == NativeOperandRole::ExplicitOption && operand.bytes.contains(&start)
    })
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
    form_evidence: FormEvidence,
}

#[derive(Default, PartialEq, Eq)]
enum FormEvidence {
    #[default]
    Proved,
    AttachedPath,
    AttachedValue,
    Unproved,
}

impl FormEvidence {
    fn reject_if(&mut self, reject: bool) {
        if reject {
            *self = Self::Unproved;
        }
    }

    fn attach_path(&mut self) {
        if *self == Self::Proved {
            *self = Self::AttachedPath;
        }
    }

    fn attach_value(&mut self) {
        if *self != Self::Unproved {
            *self = Self::AttachedValue;
        }
    }
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
            placeholder: Placeholder::default(),
            form_evidence: FormEvidence::default(),
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
            if character.is_whitespace()
                || matches!(character, ',' | '|' | '/')
                    && !self.view.literal_punctuation(self.cursor)
            {
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
        let unproved_form = match self.form_evidence {
            FormEvidence::Proved => false,
            // A tight Pa may be an argument in a larger form, but a whole
            // label indistinguishable from one flag cannot prove that flag.
            FormEvidence::AttachedPath => {
                super::options::option_prefix(self.view.text.trim()) == Some(self.view.text.trim())
            }
            // A separately executed literal supplies value bytes, never
            // part of a flag's name. Require a second independently typed
            // option before it can contribute to a complete composite.
            FormEvidence::AttachedValue => {
                !complete
                    || self.names.len() < 2
                    || self.names.iter().any(|name| {
                        name.parts
                            .first()
                            .is_none_or(|part| !self.view.native_option_starts(part.start))
                    })
            }
            FormEvidence::Unproved => true,
        };
        OptionHead {
            names: self.names,
            #[cfg(test)]
            complete,
            inferred_complete: complete && self.admission == OwnerAdmission::Proved,
            unproved_form,
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
        // Whitespace before a separator is still part of the same
        // declaration separator. Do not start an argument until that
        // punctuation has itself been consumed.
        if self.current().is_some_and(|c| {
            (matches!(c, ',' | '|') || c == '/' && self.standalone_slash())
                && !self.view.literal_punctuation(self.cursor)
        }) {
            return;
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

    fn standalone_slash(&self) -> bool {
        let tail_start = self.cursor + 1;
        let name_start = self.view.skip_space(tail_start);
        if !self.view.text[name_start..].starts_with('-')
            || self.view.is_parameter(name_start)
            || self.view.reference_or_environment(name_start)
            || name_start == tail_start
                && !(self.view.independent_slash(self.cursor)
                    && self.view.native_option_starts(name_start))
        {
            return false;
        }

        let native = self.view.explicit_option(name_start);
        let (name, _) = self.option_name_prefix(name_start, native);
        let Some(name) = name else { return false };
        let end = name_start + name.len();
        self.view.permits_option_spelling(name_start..end, true)
            && self.view.text[end..].chars().next().is_none_or(|next| {
                next.is_whitespace()
                    || matches!(next, ',' | '|' | '=' | '[' | '{' | '(' | '<' | '⟨')
                    || native && self.view.native_argument_starts(end)
                    // Consume the independent Fl even if its joined suffix
                    // cannot prove the complete form.
                    || native && self.view.independent_option_suffix(end)
                    || next == '/'
                        && native
                        && self.view.independent_slash(end)
                        && self
                            .view
                            .native_option_starts(self.view.skip_space(end + 1))
                    || self.wrappers.last().is_some_and(|w| w.closer() == next)
            })
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
        let native = self.view.explicit_option(start);
        if self.view.reference_or_environment(start) {
            return false;
        }
        let (name, end) = self.option_name_prefix(start, native);
        if let Some(name) = name {
            if !self
                .view
                .permits_option_spelling(start..start + name.len(), true)
            {
                self.form_evidence = FormEvidence::Unproved;
                return false;
            }
            self.accept_name(name, native, start + name.len() < end);
            return true;
        }
        #[cfg(feature = "roff")]
        {
            self.form_evidence.reject_if(
                native
                    && self
                        .view
                        .operand(end)
                        .is_some_and(|operand| operand.role == NativeOperandRole::ClippedOption),
            );
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

    // Both slash lookahead and actual consumption use the same native key
    // boundary. In particular a special Fl spelling or a tight Ar suffix
    // is not equivalent to a lexical option_prefix() of the whole tail.
    fn option_name_prefix(&self, start: usize, native: bool) -> (Option<&'a str>, usize) {
        if native && let Some(marker) = self.explicit_marker(start) {
            return (Some(marker), start + marker.len());
        }
        let mut end = start + 1;
        for (index, next) in self.view.text[end..].char_indices() {
            let offset = start + 1 + index;
            if !option_character(next)
                || self.view.is_parameter(offset)
                || native && self.view.independent_option_suffix(offset)
            {
                break;
            }
            end = offset + next.len_utf8();
        }
        (
            super::options::option_prefix(&self.view.text[start..end]),
            end,
        )
    }

    fn accept_name(&mut self, name: &str, native: bool, begins_argument: bool) {
        self.names
            .push(RecognizedName::contiguous(name, self.cursor));
        self.cursor += name.len();
        let foreign_tail = native
            && self.current().is_some_and(|ch| !ch.is_whitespace())
            && self.view.independent_option_suffix(self.cursor);
        // Keep the option's own native spelling separate from a joined
        // value. References, clipped values, and prose still fail closed.
        if foreign_tail {
            if self.view.native_argument_starts(self.cursor) {
                self.form_evidence.attach_path();
            } else if self.view.native_composite_value_starts(self.cursor) {
                self.form_evidence.attach_value();
            } else {
                self.form_evidence = FormEvidence::Unproved;
            }
        }
        if begins_argument || foreign_tail {
            self.begin_argument();
        }
    }

    // Keep literal mdoc Fl spellings even through generated wrappers or
    // font escapes. Every emitted byte has to belong to a Fl receipt; one
    // generated dash does not prove a following Ar/template or Cm literal.
    fn explicit_marker(&self, start: usize) -> Option<&'a str> {
        let remaining = &self.view.text[start..];
        let punctuation = remaining
            .get(..2)
            .filter(|flag| flag.as_bytes()[1].is_ascii_punctuation() && flag.as_bytes()[1] != b'-');
        [Some("--%"), Some("--"), punctuation]
            .into_iter()
            .flatten()
            .find(|marker| {
                let end = start + marker.len();
                remaining.strip_prefix(marker).is_some_and(|suffix| {
                    suffix.chars().next().is_none_or(|ch| {
                        ch.is_whitespace()
                            || matches!(ch, ',' | '|' | '/')
                            || self.view.independent_option_suffix(end)
                            || self
                                .wrappers
                                .last()
                                .is_some_and(|wrapper| wrapper.closer() == ch)
                    })
                }) && self.view.permits_option_spelling(start..end, false)
            })
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
        self.finish_argument_at(self.cursor);
    }

    fn finish_argument_at(&mut self, end: usize) {
        if let Some(mut argument) = self.argument_start.take() {
            argument.bytes.end = end;
            #[cfg(feature = "roff")]
            if self
                .names
                .last()
                .and_then(|name| name.parts.first())
                .is_some_and(|part| self.view.explicit_option(part.start))
                && argument.attached_to_name
            {
                self.form_evidence
                    .reject_if(!arguments::proves_native_assignment(self.view, &argument));
            }
            self.arguments.push(argument);
        }
        self.styled_argument = false;
        self.structured_argument = false;
        self.separator = None;
        self.placeholder = Placeholder::default();
    }

    fn complete_parameter_token(&self, delimiter_is_parameter: bool) -> bool {
        self.placeholder.token.is_uppercase_metavariable()
            || self.placeholder.is_styled()
                && !delimiter_is_parameter
                && self.argument_start.as_ref().is_some_and(|argument| {
                    // A styled suffix adjoining the name is only part of a
                    // native word. Author whitespace or an explicit assignment
                    // supplies the left boundary of a complete parameter token.
                    !argument.attached_to_name
                        || self.view.text[argument.bytes.start..].starts_with('=')
                })
    }

    fn is_argument_separator(&self, character: char, operand: Option<&NativeOperand>) -> bool {
        (matches!(character, ',' | '|')
            || character == '/'
                && (self.view.text[..self.cursor].ends_with(char::is_whitespace)
                    || self.view.independent_slash(self.cursor))
                && !self.view.is_parameter(self.cursor)
                && self.standalone_slash())
            && !self.view.literal_punctuation(self.cursor)
            && !operand.is_some_and(|operand| {
                // Commas and pipes inside a single Fl assignment belong
                // to its authored value, even before a dash-looking word.
                operand.role == NativeOperandRole::ExplicitOption
                    && self.argument_start.as_ref().is_some_and(|argument| {
                        operand.bytes.contains(&argument.bytes.start)
                            && self.view.text[argument.bytes.start..].starts_with('=')
                    })
                    && !self.view.native_option_starts(
                        self.view.skip_space(self.cursor + character.len_utf8()),
                    )
            })
    }

    fn consume_argument(&mut self, character: char) {
        let mut next_offset = self.cursor + character.len_utf8();
        let role = self.view.operand(self.cursor);
        let parameter = self.view.is_parameter(self.cursor);
        let separator = self.is_argument_separator(character, role);
        self.styled_argument |= parameter;
        if self.lexical.is_top_level() {
            let mut following = next_offset;
            if separator || character.is_whitespace() {
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
                || separator && self.complete_bare_parameter(following);
            let independent_assignment_alias = separator
                && self.view.native_option_starts(following)
                && self.argument_start.as_ref().is_some_and(|argument| {
                    argument.attached_to_name
                        && self.view.text[argument.bytes.start..].starts_with('=')
                });
            let independent_literal_alias =
                separator && !parameter && self.literal_alias_after_parameter(following);
            let boundary = separator
                && (self.styled_argument || self.structured_argument || complete_parameter)
                || independent_assignment_alias
                || (self.view.literal_operand_starts(self.cursor)
                    || self.view.literal_operand_starts(following))
                    && self.separator.is_some()
                || character.is_whitespace() && self.placeholder.token.is_uppercase_metavariable();
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
                && !independent_assignment_alias
                && !independent_literal_alias
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
                    || independent_assignment_alias
                    || independent_literal_alias
                    || self.view.literal_operand_starts(restart))
            {
                let end = if separator || character.is_whitespace() {
                    self.separator.unwrap_or(self.cursor)
                } else {
                    self.cursor
                };
                self.finish_argument_at(end);
                self.cursor = restart;
                return;
            }
            if character.is_whitespace() {
                // This whole scalar run has one lexical event. Advance it
                // together so an unsuccessful lookahead cannot rescan every
                // suffix of a long argument whitespace run.
                next_offset = following;
            }
            if separator {
                self.separator = Some(self.cursor);
            } else if !character.is_whitespace() {
                self.separator = None;
            }
        }
        let top_level = self.lexical.is_top_level();
        let complete_group = self.observe_argument_character(character);
        if complete_group {
            self.structured_argument = true;
        } else if top_level && !character.is_whitespace() {
            self.structured_argument = false;
        }
        self.placeholder = self.placeholder.observe(
            if internal_apostrophe(&self.view.text, self.cursor) {
                'x'
            } else {
                character
            },
            parameter,
        );
        self.cursor = next_offset;
    }

    // A separator at the start of a distinct literal operand can end the
    // preceding styled parameter even when that parameter is a complex path.
    // Both the separator and the candidate name must belong to that operand.
    fn literal_alias_after_parameter(&self, following: usize) -> bool {
        self.view.literal_operand_starts(self.cursor)
            && self.view.operand(self.cursor).is_some_and(|operand| {
                operand.bytes.contains(&following)
                    && self.argument_start.as_ref().is_some_and(|argument| {
                        operand.bytes.start > argument.bytes.start
                            && self.view.text[argument.bytes.start..self.cursor]
                                .trim_end()
                                .char_indices()
                                .next_back()
                                .is_some_and(|(offset, _)| {
                                    self.view.is_parameter(argument.bytes.start + offset)
                                })
                    })
            })
    }

    fn observe_argument_character(&mut self, character: char) -> bool {
        #[cfg(feature = "roff")]
        {
            // termp_quote_pre/post emit their delimiters from the enclosure
            // node. A visually identical operand glyph cannot close it.
            if let Some(event) = self
                .view
                .operand(self.cursor)
                .filter(|operand| {
                    matches!(
                        operand.role,
                        NativeOperandRole::EnclosureStart { .. }
                            | NativeOperandRole::EnclosureEnd(_)
                    ) && self.view.skip_space(operand.bytes.start) == self.cursor
                })
                .and_then(|operand| self.lexical.observe_enclosure(operand.role))
            {
                return event;
            }
            if self.lexical.inside_native_enclosure() {
                return false;
            }
        }
        #[cfg(feature = "roff")]
        let literal_angle = self.argument_start.as_ref().is_some_and(|argument| {
            self.view
                .literal_assignment_angle(argument, self.cursor, character)
        });
        #[cfg(not(feature = "roff"))]
        let literal_angle = false;
        self.lexical.observe(
            if literal_angle
                || !self.lexical.in_quote() && internal_apostrophe(&self.view.text, self.cursor)
            {
                'x'
            } else {
                character
            },
        )
    }

    fn complete_bare_parameter(&self, following: usize) -> bool {
        if !self.placeholder.is_bare()
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

// A word-internal apostrophe is not an opening quote. An apostrophe at a
// boundary remains a quote, so enclosed dash-looking values stay opaque.
fn internal_apostrophe(text: &str, offset: usize) -> bool {
    text[offset..].starts_with(['\'', '’'])
        && text[..offset]
            .chars()
            .rev()
            .find(|c| !unicode_normalization::char::is_combining_mark(*c))
            .is_some_and(char::is_alphanumeric)
        && text[offset..]
            .chars()
            .nth(1)
            .is_some_and(char::is_alphanumeric)
}

fn option_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '?' | '.' | '+')
}

pub(super) fn closing(character: char) -> Option<char> {
    match character {
        '[' => Some(']'),
        '{' => Some('}'),
        '(' => Some(')'),
        '<' => Some('>'),
        '⟨' => Some('⟩'),
        _ => None,
    }
}

pub(super) fn quote_closer(character: char) -> Option<char> {
    match character {
        '"' | '\'' => Some(character),
        '“' => Some('”'),
        '‘' => Some('’'),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
