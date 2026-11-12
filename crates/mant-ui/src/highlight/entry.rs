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

#[cfg(test)]
fn accents(text: &str, names: &[Range<usize>]) -> Vec<Accent> {
    structure_accents(text, text, names, &[])
}

fn structure_accents(
    text: &str,
    scan_text: &str,
    names: &[Range<usize>],
    protected: &[Range<usize>],
) -> Vec<Accent> {
    if text.len() > MAX_FORM_BYTES {
        return Vec::new();
    }
    let mut result = Vec::new();
    let mut stack = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut opaque = false;
    let mut name_index = 0;
    let mut protected_index = 0;
    for (scalar, (byte, character)) in text.char_indices().enumerate() {
        if contains_at(protected, &mut protected_index, scalar) {
            continue;
        }
        if contains_at(names, &mut name_index, scalar) {
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
        if character == ':' && url_start(scan_text, byte) {
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
                let adjacent =
                    scan_text[..byte].ends_with('|') || scan_text[byte + 1..].starts_with('|');
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

fn contains_at(ranges: &[Range<usize>], index: &mut usize, scalar: usize) -> bool {
    while ranges.get(*index).is_some_and(|range| range.end <= scalar) {
        *index += 1;
    }
    ranges
        .get(*index)
        .is_some_and(|range| range.contains(&scalar))
}

/// Form bindings use scalar coordinates, while the shared signature scanner
/// uses bytes. Convert once for the complete owner, before source projection.
pub(crate) fn form_accents(
    text: &str,
    names: &[Range<usize>],
    emphasis: &[Range<usize>],
    protected: &[Range<usize>],
) -> Vec<Accent> {
    if text.len() > MAX_FORM_BYTES {
        return Vec::new();
    }
    let boundaries: Vec<_> = text
        .char_indices()
        .map(|(byte, _)| byte)
        .chain(std::iter::once(text.len()))
        .collect();
    let protected_scalars: Vec<_> = protected
        .iter()
        .map(|range| {
            boundaries
                .binary_search(&range.start)
                .expect("protected UTF-8 start")
                ..boundaries
                    .binary_search(&range.end)
                    .expect("protected UTF-8 end")
        })
        .collect();
    let scan_text = crate::highlight::source::syntax_text(text, protected);
    let mut result = structure_accents(text, &scan_text, names, &protected_scalars);
    let names = names
        .iter()
        .filter_map(|name| Some(*boundaries.get(name.start)?..*boundaries.get(name.end)?))
        .collect::<Vec<_>>();
    let mut syntax = crate::highlight::usage::accents(
        &scan_text,
        crate::highlight::usage::Context::Entry,
        emphasis,
        &names,
    );
    let marked = (!text.is_empty()).then_some(0..text.len());
    let prose =
        crate::highlight::usage::prose_accents(text, marked.as_slice(), emphasis, protected);
    crate::highlight::usage::overlay(&mut syntax, prose);
    let mut placeholders = Vec::new();
    let mut name_index = 0;
    for accent in syntax {
        if accent.role == crate::highlight::usage::Role::Placeholder {
            while names
                .get(name_index)
                .is_some_and(|name| name.end <= accent.bytes.start)
            {
                name_index += 1;
            }
            if names
                .get(name_index)
                .is_some_and(|name| name.start < accent.bytes.end)
            {
                continue;
            }
            let start = boundaries
                .binary_search(&accent.bytes.start)
                .expect("token UTF-8 boundary");
            let end = boundaries
                .binary_search(&accent.bytes.end)
                .expect("token UTF-8 boundary");
            placeholders.push(Accent {
                range: start..end,
                role: StyleRole::EntryPlaceholder,
            });
        }
    }
    crate::highlight::ranges::overlay(
        &mut result,
        placeholders,
        |accent| accent.range.clone(),
        |accent, range| Accent {
            range,
            role: accent.role,
        },
    );
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

    pub(crate) fn spans(
        &mut self,
        text: &str,
        base: Style,
        theme: &Theme,
        linked: bool,
    ) -> Vec<Span<'static>> {
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
                .map_or(base, |accent| {
                    // Argument guesses stay below authored link colors, just
                    // as in code/synopsis spans. Existing structural accents
                    // keep their independent delimiter/operator affordance.
                    if linked && accent.role == StyleRole::EntryPlaceholder {
                        base
                    } else {
                        base.patch(theme.style(accent.role))
                    }
                });
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
