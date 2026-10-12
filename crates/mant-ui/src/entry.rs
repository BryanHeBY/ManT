//! Bounded visual structure within validated entry forms, not a shell grammar.

use std::ops::Range;

use ratatui::{style::Style, text::Span};

use crate::theme::{StyleRole, Theme};

pub(crate) const MAX_FORM_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 64;

/// Scalar coordinates in original inline roots; labels convert to byte ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Accent {
    pub(crate) range: Range<usize>,
    pub(crate) role: StyleRole,
}

pub(crate) fn accents(text: &str, names: &[Range<usize>]) -> Vec<Accent> {
    if text.len() > MAX_FORM_BYTES {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut stack = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut opaque = false;
    let mut name_index = 0;
    let mut chars = text.char_indices().enumerate().peekable();
    while let Some((scalar, (byte, character))) = chars.next() {
        while names.get(name_index).is_some_and(|name| name.end <= scalar) {
            name_index += 1;
        }
        if names
            .get(name_index)
            .is_some_and(|name| name.contains(&scalar))
        {
            continue;
        }
        if character.is_whitespace() {
            opaque = false;
        }
        if opaque {
            continue;
        }
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"' | '`') {
            quote = Some(character);
            continue;
        }
        if character == ':' && url_start(text, byte) {
            opaque = true;
            continue;
        }
        let role = match character {
            '[' | '<' | '(' | '{' => {
                if stack.len() == MAX_DEPTH {
                    return Vec::new();
                }
                stack.push((character, scalar));
                None
            }
            ']' | '>' | ')' | '}' => {
                let opening = match character {
                    ']' => '[',
                    '>' => '<',
                    ')' => '(',
                    _ => '{',
                };
                if let Some(&(delimiter, start)) = stack.last()
                    && delimiter == opening
                {
                    stack.pop();
                    result.push(Accent {
                        range: start..start + 1,
                        role: StyleRole::EntryDelimiter,
                    });
                    Some(StyleRole::EntryDelimiter)
                } else {
                    None
                }
            }
            '=' => Some(StyleRole::EntryOperator),
            '|' if stack
                .iter()
                .any(|(delimiter, _)| matches!(delimiter, '[' | '{')) =>
            {
                // A synopsis choice, not a shell pipeline or logical OR.
                let adjacent = text[..byte].ends_with('|')
                    || chars.peek().is_some_and(|(_, (_, next))| *next == '|');
                (!adjacent).then_some(StyleRole::EntrySeparator)
            }
            _ => None,
        };
        if let Some(role) = role {
            result.push(Accent {
                range: scalar..scalar + 1,
                role,
            });
        }
    }
    result.sort_by_key(|accent| accent.range.start);
    result
}

fn url_start(text: &str, byte: usize) -> bool {
    text[byte..].starts_with("://")
        && text[..byte]
            .chars()
            .next_back()
            .is_some_and(|character| character.is_ascii_alphanumeric())
}

/// Consume unchanged original text across arbitrary source/link wrappers.
pub(crate) struct AccentCursor<'a> {
    accents: &'a [Accent],
    scalar: usize,
    index: usize,
}

impl<'a> AccentCursor<'a> {
    pub(crate) const fn new(accents: &'a [Accent]) -> Self {
        Self {
            accents,
            scalar: 0,
            index: 0,
        }
    }

    pub(crate) fn spans(&mut self, text: &str, base: Style, theme: &Theme) -> Vec<Span<'static>> {
        if self.accents.is_empty() {
            return vec![Span::styled(text.to_owned(), base)];
        }
        let mut spans = Vec::new();
        let mut start = 0;
        let mut style = base;
        for (byte, _) in text.char_indices() {
            while self
                .accents
                .get(self.index)
                .is_some_and(|accent| accent.range.end <= self.scalar)
            {
                self.index += 1;
            }
            let next = self
                .accents
                .get(self.index)
                .filter(|accent| accent.range.contains(&self.scalar))
                .map_or(base, |accent| base.patch(theme.style(accent.role)));
            if next != style {
                if byte > start {
                    spans.push(Span::styled(text[start..byte].to_owned(), style));
                }
                start = byte;
                style = next;
            }
            self.scalar += 1;
        }
        if start < text.len() {
            spans.push(Span::styled(text[start..].to_owned(), style));
        }
        spans
    }
}

#[derive(Debug, Clone)]
pub(crate) struct LabelStyles {
    pub(crate) text: String,
    accents: Vec<Accent>,
}

impl LabelStyles {
    pub(crate) fn new(text: &str, mut accents: Vec<Accent>) -> Self {
        let text = crate::text::sanitize_terminal_text(text).into_owned();
        let boundaries = text
            .char_indices()
            .map(|(byte, _)| byte)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        for accent in &mut accents {
            accent.range = boundaries[accent.range.start]..boundaries[accent.range.end];
        }
        Self { text, accents }
    }

    pub(crate) fn role_at(&self, byte: usize) -> Option<StyleRole> {
        let index = self
            .accents
            .partition_point(|accent| accent.range.end <= byte);
        self.accents
            .get(index)
            .filter(|accent| accent.range.contains(&byte))
            .map(|accent| accent.role)
    }
}

pub(crate) type EntryLabels = std::collections::HashMap<String, LabelStyles>;

#[cfg(test)]
mod tests;
