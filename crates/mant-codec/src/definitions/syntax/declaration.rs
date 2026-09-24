//! Bounded syntax state shared across transparent inline wrappers.
//!
//! A separator in an argument is not a new declaration. An explicit literal
//! separator following a completed argument may introduce another full option;
//! it never gives a parameter fragment a fresh chance to become a name.

use mant_ir::{ContentContext, Inline, InlineView, lexical_option_token, option_prefix};
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
    argument_starts: HashSet<usize>,
    validated_token: bool,
}

impl DeclarationState {
    pub(super) fn new(content: ContentContext<'_>, text: String, inlines: &[Inline]) -> Self {
        let mut literal_starts = HashSet::new();
        collect_literal_starts(content, inlines, false, false, &mut 0, &mut literal_starts);
        let mut ranges = Vec::new();
        literal_ranges(content, inlines, false, false, &mut 0, &mut ranges);
        let argument_starts = ranges
            .iter()
            .filter_map(|&(_, end)| {
                let rest = &text[end..];
                let next = rest.trim_start();
                let offset = text.len() - next.len();
                let token = first_declaration_token(next);
                // A leading dash alone is not declaration evidence: -10 is
                // an ordinary parameter whose commas remain inside it.
                let starts_declaration = next.starts_with([',', '|', '/', '+'])
                    || next.starts_with('-') && !negative_number_parameter(token);
                (rest.starts_with(char::is_whitespace)
                    && !next.is_empty()
                    && !starts_declaration
                    && !literal_starts.contains(&offset))
                .then_some(offset)
            })
            .collect();
        Self {
            text,
            offset: 0,
            closers: Vec::new(),
            phase: Phase::Name,
            uncertain: false,
            literal_starts,
            argument_starts,
            validated_token: false,
        }
    }

    /// Construct the equivalent state for one complete literal Markdown leaf.
    ///
    /// This is the same grammar as a single `Inline::Code` node: the whole
    /// value is literal, contains no styled-argument boundary, and has no
    /// independently strong declaration restart.
    pub(super) fn literal(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            offset: 0,
            closers: Vec::new(),
            phase: Phase::Name,
            uncertain: false,
            literal_starts: HashSet::new(),
            argument_starts: HashSet::new(),
            validated_token: false,
        }
    }

    pub(super) fn within_validated_token(mut self) -> Self {
        self.validated_token = true;
        self
    }

    pub(super) fn separator(&mut self, character: char, eligible: bool) -> bool {
        if self.argument_starts.contains(&self.offset) {
            self.begin_argument();
        }
        let next_offset = self.offset + character.len_utf8();
        // This method sees every visible scalar, not just separators. Avoid
        // rescanning the remaining head for every character in a long form.
        if !eligible {
            self.observe(character, false);
            self.offset = next_offset;
            return false;
        }
        let remainder = &self.text[next_offset..];
        let following = remainder.trim_start();
        let token = first_declaration_token(following);
        let negative_argument = negative_number_parameter(token);
        let fresh_option = remainder.starts_with(char::is_whitespace)
            && following.starts_with(['-', '+'])
            && !negative_argument;
        let fresh_literal = self.phase != Phase::Name
            && remainder.starts_with(char::is_whitespace)
            && self
                .literal_starts
                .contains(&(self.text.len() - following.len()))
            && following
                .split_whitespace()
                .next()
                .is_some_and(super::commands::is_command_name);
        let split = !negative_argument
            && (self.validated_token
                || !self.uncertain
                    && self.closers.is_empty()
                    && (self.phase == Phase::Name || fresh_option || fresh_literal));
        if split {
            self.phase = Phase::Name;
        } else {
            if negative_argument {
                // Keep the entire signed parameter, including its commas,
                // in the preceding declaration's argument interval.
                self.begin_argument();
            }
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

fn first_declaration_token(value: &str) -> &str {
    value
        .split(|character: char| character.is_whitespace() || matches!(character, ',' | '|'))
        .next()
        .unwrap_or_default()
}

fn negative_number_parameter(token: &str) -> bool {
    // Keep the Flow grammar's non-ASCII and attached-value spellings. Only a
    // complete dash token that the shared lexical rule specifically rejects
    // is a signed numeric parameter rather than a declaration candidate.
    option_prefix(token) == Some(token) && !lexical_option_token(token)
}

/// Preserve literal coverage through transparent wrappers. Only a whitespace-
/// separated unstyled suffix begins an ordinary argument; a multiword literal
/// command or a new independently styled declaration keeps its full spelling.
fn literal_ranges(
    content: ContentContext<'_>,
    nodes: &[Inline],
    literal: bool,
    parameter: bool,
    offset: &mut usize,
    ranges: &mut Vec<(usize, usize)>,
) {
    for node in nodes {
        match content.inline(node).expect("definition content resolves") {
            InlineView::Text(value) | InlineView::Code(value) => {
                let end = *offset + value.len();
                if (literal || matches!(node, Inline::Code { .. })) && !parameter {
                    ranges.push((*offset, end));
                }
                *offset = end;
            }
            InlineView::Strong(children) => {
                literal_ranges(content, children, true, parameter, offset, ranges);
            }
            InlineView::Emphasis(children) => {
                literal_ranges(content, children, literal, true, offset, ranges);
            }
            InlineView::Link(link) => {
                literal_ranges(content, link.children(), literal, parameter, offset, ranges);
            }
            InlineView::LineBreak => *offset += 1,
            InlineView::Anchor(_) => {}
            _ => unreachable!("all inline views are handled"),
        }
    }
}

fn collect_literal_starts(
    content: ContentContext<'_>,
    inlines: &[Inline],
    strong: bool,
    parameter: bool,
    offset: &mut usize,
    starts: &mut HashSet<usize>,
) {
    for inline in inlines {
        match content.inline(inline).expect("definition content resolves") {
            InlineView::Text(value) | InlineView::Code(value) => {
                if strong && !parameter && !value.trim_start().is_empty() {
                    starts.insert(*offset + value.len() - value.trim_start().len());
                }
                *offset += value.len();
            }
            InlineView::Strong(children) => {
                collect_literal_starts(content, children, true, parameter, offset, starts);
            }
            InlineView::Emphasis(children) => {
                collect_literal_starts(content, children, strong, true, offset, starts);
            }
            InlineView::Link(link) => {
                collect_literal_starts(content, link.children(), strong, parameter, offset, starts);
            }
            InlineView::LineBreak => *offset += 1,
            InlineView::Anchor(_) => {}
            _ => unreachable!("all inline views are handled"),
        }
    }
}
