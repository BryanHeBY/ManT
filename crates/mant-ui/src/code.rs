//! Best-effort code accents, independent of authored styles and layout.
//!
//! Scan one complete block, not individual inline spans or wrapped rows.
//! Manual displays mix source code, commands, configuration and placeholders;
//! no language selection, syntax dependency or filesystem lookup is needed.

use std::ops::Range;

use ratatui::{style::Style, text::Span};

use crate::theme::{self, StyleRole};

mod command;
mod keywords;
mod scan;

pub(crate) const MAX_BLOCK_BYTES: usize = 64 * 1024;

struct Accent {
    bytes: Range<usize>,
    role: StyleRole,
}

/// Cached byte ranges consumed in original inline visitation order. Colors
/// remain weaker than authored markup, links and validated name bindings.
#[derive(Default)]
pub(crate) struct CodeHighlights {
    accents: Vec<Accent>,
    cursor: usize,
    offset: usize,
}

impl CodeHighlights {
    pub(crate) fn new(value: &str) -> Self {
        // Reject the whole block before scanning: a partial scan would lose
        // quote/comment state and miscolor subsequent inline pieces.
        if value.len() > MAX_BLOCK_BYTES {
            return Self::default();
        }
        Self {
            accents: scan::accents(value),
            ..Self::default()
        }
    }

    /// `text` must be the next unchanged piece of the original block.
    pub(crate) fn spans(&mut self, text: &str, base: Style) -> Vec<Span<'static>> {
        let end = self.offset + text.len();
        let mut spans = Vec::new();
        while self.offset < end {
            while self
                .accents
                .get(self.cursor)
                .is_some_and(|accent| accent.bytes.end <= self.offset)
            {
                self.cursor += 1;
            }
            let (stop, style) = self.accents.get(self.cursor).map_or((end, base), |accent| {
                if accent.bytes.start > self.offset {
                    (end.min(accent.bytes.start), base)
                } else {
                    (
                        end.min(accent.bytes.end),
                        base.patch(theme::style(accent.role)),
                    )
                }
            });
            let start = text.len() - (end - self.offset);
            spans.push(Span::styled(
                text[start..start + stop - self.offset].to_owned(),
                style,
            ));
            self.offset = stop;
        }
        spans
    }
}

#[cfg(test)]
mod tests;
