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
    name_end: Option<usize>,
    after_name_space: bool,
    quote: Option<char>,
    uncertain: bool,
    literal_starts: HashSet<usize>,
    argument_starts: HashSet<usize>,
    validated_token: bool,
    suffix: SuffixCursor,
    #[cfg(test)]
    prepass_scan_bytes: usize,
}

impl DeclarationState {
    pub(super) fn new(content: ContentContext<'_>, text: String, inlines: &[Inline]) -> Self {
        let mut literal_starts = HashSet::new();
        collect_literal_starts(content, inlines, false, false, &mut 0, &mut literal_starts);
        let mut ranges = Vec::new();
        literal_ranges(content, inlines, false, false, &mut 0, &mut ranges);
        let mut suffix = SuffixCursor::default();
        let argument_starts = ranges
            .iter()
            .filter_map(|&(_, end)| {
                let rest = &text[end..];
                if !rest.starts_with(char::is_whitespace) {
                    return None;
                }
                let next = suffix.probe(&text, end);
                // A leading dash alone is not declaration evidence: -10 is
                // an ordinary parameter whose commas remain inside it.
                let starts_declaration = next.token.starts_with([',', '|', '/', '+'])
                    || next.token.starts_with('-') && !next.negative_number;
                (!next.token.is_empty()
                    && !starts_declaration
                    && !literal_starts.contains(&next.start))
                .then_some(next.start)
            })
            .collect();
        #[cfg(test)]
        let prepass_scan_bytes = suffix.scanned_bytes;
        let name_end = option_name_end(&text, 0);
        Self {
            text,
            offset: 0,
            closers: Vec::new(),
            phase: Phase::Name,
            name_end,
            after_name_space: false,
            quote: None,
            uncertain: false,
            literal_starts,
            argument_starts,
            validated_token: false,
            suffix: SuffixCursor::default(),
            #[cfg(test)]
            prepass_scan_bytes,
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
            name_end: option_name_end(text, 0),
            after_name_space: false,
            quote: None,
            uncertain: false,
            literal_starts: HashSet::new(),
            argument_starts: HashSet::new(),
            validated_token: false,
            suffix: SuffixCursor::default(),
            #[cfg(test)]
            prepass_scan_bytes: 0,
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
        let following = self.suffix.probe(&self.text, next_offset);
        let negative_argument = following.negative_number;
        let fresh_option = remainder.starts_with(char::is_whitespace)
            && following.token.starts_with(['-', '+'])
            && !negative_argument;
        let fresh_literal = self.phase != Phase::Name
            && remainder.starts_with(char::is_whitespace)
            && self.literal_starts.contains(&following.start)
            && following.command_name;
        let split = self.quote.is_none()
            && !negative_argument
            && (self.validated_token
                || !self.uncertain
                    && self.closers.is_empty()
                    && (self.phase == Phase::Name || fresh_option || fresh_literal));
        if split {
            self.phase = Phase::Name;
            self.name_end = option_name_end(&self.text, following.start);
            self.after_name_space = false;
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

    /// An emphasized parameter stays opaque except for a terminal separator
    /// followed by an independently styled declaration. A comma inside the
    /// parameter, even before option-looking text, cannot create a name.
    pub(super) fn styled_character(&mut self, character: char, eligible: bool) -> bool {
        if eligible && self.quote.is_none() && self.closers.is_empty() {
            let next = self.offset + character.len_utf8();
            let following = self.suffix.probe(&self.text, next);
            if self.literal_starts.contains(&following.start)
                && lexical_option_token(following.token)
            {
                return self.separator(character, true);
            }
        }
        self.observe(character, true);
        self.offset += character.len_utf8();
        false
    }

    pub(super) fn opaque(&mut self, text: &str) {
        for character in text.chars() {
            self.observe(character, true);
            self.offset += character.len_utf8();
        }
    }

    fn observe(&mut self, character: char, opaque: bool) {
        if self.phase == Phase::Name && self.name_end.is_some_and(|end| self.offset >= end) {
            if character.is_whitespace() {
                self.after_name_space = true;
            } else if self.after_name_space {
                if option_name_end(&self.text, self.offset).is_none() {
                    self.begin_argument();
                }
                self.after_name_space = false;
            }
        }
        if opaque && !character.is_whitespace() {
            self.phase = Phase::StyledArgument;
        }
        if let Some(close) = self.quote {
            if character == close {
                self.quote = None;
            }
            return;
        }
        let previous = self.text[..self.offset].chars().next_back();
        if let Some(close) = match character {
            '"' => Some('"'),
            '\'' if previous.is_none_or(|before| {
                before.is_whitespace() || matches!(before, '=' | ',' | '|' | '[' | '{' | '(')
            }) =>
            {
                Some('\'')
            }
            '“' => Some('”'),
            '‘' => Some('’'),
            _ => None,
        } {
            self.quote = Some(close);
            self.begin_argument();
            return;
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

#[derive(Default)]
struct SuffixCursor {
    valid: bool,
    whitespace_start: usize,
    token_start: usize,
    token_end: usize,
    negative_number: bool,
    command_name: bool,
    #[cfg(test)]
    scanned_bytes: usize,
}

struct SuffixToken<'a> {
    start: usize,
    token: &'a str,
    negative_number: bool,
    command_name: bool,
}

impl SuffixCursor {
    /// Callers visit suffix offsets in source order. Reuse a probed whitespace
    /// interval and its token when several boundaries share that suffix;
    /// otherwise each scalar is inspected at most once per pass.
    fn probe<'a>(&mut self, text: &'a str, offset: usize) -> SuffixToken<'a> {
        if self.valid && (self.whitespace_start..=self.token_start).contains(&offset) {
            return SuffixToken {
                start: self.token_start,
                token: &text[self.token_start..self.token_end],
                negative_number: self.negative_number,
                command_name: self.command_name,
            };
        }
        let mut cursor = offset;
        while let Some(character) = text[cursor..].chars().next() {
            if !character.is_whitespace() {
                break;
            }
            cursor += character.len_utf8();
        }
        let token_start = cursor;
        while let Some(character) = text[cursor..].chars().next() {
            if character.is_whitespace() || matches!(character, ',' | '|' | '/') {
                break;
            }
            cursor += character.len_utf8();
        }
        let token = &text[token_start..cursor];
        self.valid = true;
        self.whitespace_start = offset;
        self.token_start = token_start;
        self.token_end = cursor;
        self.negative_number = negative_number_parameter(token);
        self.command_name = super::commands::is_command_name(token);
        #[cfg(test)]
        {
            // Include the token classification pass as well as its search.
            self.scanned_bytes += cursor - offset + token.len();
        }
        SuffixToken {
            start: token_start,
            token,
            negative_number: self.negative_number,
            command_name: self.command_name,
        }
    }
}

fn negative_number_parameter(token: &str) -> bool {
    // A signed number remains a parameter even with a unit or range suffix.
    // A native `.Fl` can override this spelling inference through its own
    // exact macro-instance witness, never through this source-neutral rule.
    token.starts_with('-')
        && !token.starts_with("--")
        && token.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
}

/// A candidate name boundary is used only to enter argument state. It does
/// not itself publish a name; owner and styled/native evidence are separate.
fn option_name_end(text: &str, offset: usize) -> Option<usize> {
    let rest = text.get(offset..)?;
    let leading = rest.len() - rest.trim_start().len();
    let rest = &rest[leading..];
    let end = rest
        .find(|character: char| character.is_whitespace() || matches!(character, ',' | '|' | '/'))
        .unwrap_or(rest.len());
    let token = &rest[..end];
    let name = option_prefix(token)?;
    lexical_option_token(name).then_some(offset + leading + name.len())
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

#[cfg(test)]
mod tests {
    use super::{DeclarationState, SuffixCursor};
    use crate::test_content as fixture;
    use mant_ir::Inline;

    #[test]
    fn suffix_work_is_linear_across_style_edges_slashes_and_shared_whitespace() {
        for count in [256, 1024, 4096] {
            let word = "a".repeat(count);
            let fragments = (0..count)
                .map(|_| Inline::Strong {
                    children: vec![fixture::text("a")],
                })
                .collect::<Vec<_>>();
            let state = DeclarationState::new(fixture::content(), word, &fragments);
            assert_eq!(state.prepass_scan_bytes, 0, "style edges: {count}");

            let whitespace = " ".repeat(count);
            let mut spaced = (0..count)
                .map(|_| Inline::Strong {
                    children: vec![fixture::text(" ")],
                })
                .collect::<Vec<_>>();
            spaced.push(fixture::text("first"));
            let state =
                DeclarationState::new(fixture::content(), format!("{whitespace}first"), &spaced);
            assert!(state.prepass_scan_bytes <= 2 * (count + 5));

            let slashes = "-a/".repeat(count);
            let mut state = DeclarationState::literal(&slashes).within_validated_token();
            for character in slashes.chars() {
                state.separator(character, character == '/');
            }
            assert!(state.suffix.scanned_bytes <= 2 * slashes.len());

            let shared = format!(",{}--all", " ".repeat(count));
            let mut cursor = SuffixCursor::default();
            for offset in 1..=count {
                cursor.probe(&shared, offset);
            }
            assert!(cursor.scanned_bytes <= 2 * shared.len());
        }
    }
}
