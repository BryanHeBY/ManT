//! Bounded syntax state shared across transparent inline wrappers.
//!
//! A separator in an argument is not a new declaration. An explicit literal
//! separator following a completed argument may introduce another full option;
//! it never gives a parameter fragment a fresh chance to become a name.

use mant_ir::Inline;
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Name,
    Argument,
    StyledArgument,
}

pub(super) struct DeclarationState {
    text: String,
    offset: usize,
    closers: Vec<char>,
    phase: Phase,
    uncertain: bool,
    literal_starts: HashSet<usize>,
    validated_token: bool,
}

impl DeclarationState {
    pub(super) fn new(text: String, inlines: &[Inline]) -> Self {
        let mut literal_starts = HashSet::new();
        collect_literal_starts(inlines, false, false, &mut 0, &mut literal_starts);
        Self {
            text,
            offset: 0,
            closers: Vec::new(),
            phase: Phase::Name,
            uncertain: false,
            literal_starts,
            validated_token: false,
        }
    }

    pub(super) fn within_validated_token(mut self) -> Self {
        self.validated_token = true;
        self
    }

    pub(super) fn separator(&mut self, character: char, eligible: bool) -> bool {
        let next_offset = self.offset + character.len_utf8();
        // This method sees every visible scalar, not just separators. Avoid
        // rescanning the remaining head for every character in a long form.
        if !eligible {
            self.observe(character, false);
            self.offset = next_offset;
            return false;
        }
        let remainder = &self.text[next_offset..];
        let fresh_option = remainder.starts_with(char::is_whitespace)
            && remainder.trim_start().starts_with(['-', '+']);
        let following = remainder.trim_start();
        let fresh_literal = self.phase == Phase::StyledArgument
            && self
                .literal_starts
                .contains(&(self.text.len() - following.len()))
            && following
                .split_whitespace()
                .next()
                .is_some_and(super::commands::is_command_name);
        let split = self.validated_token
            || !self.uncertain
                && self.closers.is_empty()
                && (self.phase == Phase::Name || fresh_option || fresh_literal);
        if split {
            self.phase = Phase::Name;
        } else {
            self.observe(character, false);
        }
        self.offset = next_offset;
        split
    }

    pub(super) fn opaque(&mut self, text: &str) {
        for character in text.chars() {
            self.observe(character, true);
            self.offset += character.len_utf8();
        }
    }

    fn observe(&mut self, character: char, opaque: bool) {
        if opaque && !character.is_whitespace() {
            self.phase = Phase::StyledArgument;
        }
        let closer = match character {
            '[' => Some(']'),
            '{' => Some('}'),
            '(' => Some(')'),
            '<' => Some('>'),
            _ => None,
        };
        if let Some(closer) = closer {
            self.begin_argument();
            if self.closers.len() < 64 {
                self.closers.push(closer);
            } else {
                self.uncertain = true;
            }
        } else if Some(&character) == self.closers.last() {
            self.closers.pop();
        } else if matches!(character, ']' | '}' | ')') {
            self.uncertain = true;
        } else if character == '=' {
            self.begin_argument();
        }
    }

    fn begin_argument(&mut self) {
        if self.phase == Phase::Name {
            self.phase = Phase::Argument;
        }
    }
}

fn collect_literal_starts(
    inlines: &[Inline],
    strong: bool,
    parameter: bool,
    offset: &mut usize,
    starts: &mut HashSet<usize>,
) {
    for inline in inlines {
        match inline {
            Inline::Text { value } | Inline::Code { value } => {
                if strong && !parameter && !value.trim_start().is_empty() {
                    starts.insert(*offset + value.len() - value.trim_start().len());
                }
                *offset += value.len();
            }
            Inline::Strong { children } => {
                collect_literal_starts(children, true, parameter, offset, starts);
            }
            Inline::Emphasis { children } => {
                collect_literal_starts(children, strong, true, offset, starts);
            }
            Inline::Link { children, .. } => {
                collect_literal_starts(children, strong, parameter, offset, starts);
            }
            Inline::LineBreak => *offset += 1,
            Inline::Anchor { .. } => {}
        }
    }
}
