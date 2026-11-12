//! Best-effort code accents, independent of authored styles and layout.
//!
//! Scan one complete block, not individual inline spans or wrapped rows.
//! Manual displays mix source code, commands, configuration and placeholders;
//! no language selection, syntax dependency or filesystem lookup is needed.

use std::ops::Range;

use ratatui::{style::Style, text::Span};

use crate::theme::{CodeStyles, StyleRole};

mod command;
mod composition;
mod keywords;
mod scan;
use super::source;
mod synopsis;

pub(crate) const MAX_BLOCK_BYTES: usize = 64 * 1024;

/// Literal scanning, synopsis evidence and marked prose evidence are
/// independent of display geometry. A synopsis can retain a normal surface.
#[derive(Clone, Copy, Default)]
pub(crate) struct HighlightContext {
    pub(crate) literal: bool,
    pub(crate) synopsis: bool,
    pub(crate) prose: bool,
}

impl From<bool> for HighlightContext {
    fn from(literal: bool) -> Self {
        Self {
            literal,
            synopsis: false,
            prose: literal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Accent {
    bytes: Range<usize>,
    role: StyleRole,
}

/// Cached byte ranges consumed in original inline visitation order. The theme
/// composes source fonts, lexical colors and link/name affordances separately.
#[derive(Default)]
pub(crate) struct CodeHighlights {
    accents: Vec<Accent>,
    cursor: usize,
    offset: usize,
}

impl CodeHighlights {
    #[cfg(test)]
    pub(crate) fn new(value: &str) -> Self {
        Self::with_source(
            &source::Source {
                text: value.into(),
                ..Default::default()
            },
            true.into(),
        )
    }

    pub(crate) fn for_inlines(
        nodes: &[mant_ir::Inline],
        context: HighlightContext,
    ) -> Option<Self> {
        if !context.literal && !context.synopsis && !context.prose {
            return None;
        }
        let Some(source) = source::Source::collect(nodes) else {
            return context.literal.then(Self::default);
        };
        let highlights = Self::with_source(&source, context);
        (context.literal || !highlights.accents.is_empty()).then_some(highlights)
    }

    fn with_source(source: &source::Source, context: HighlightContext) -> Self {
        let value = source.text.as_str();
        let syntax_text = source.syntax_text();
        // Reject the whole block before scanning: a partial scan would lose
        // quote/comment state and miscolor subsequent inline pieces.
        if value.len() > MAX_BLOCK_BYTES {
            return Self::default();
        }
        let mut accents = if context.literal {
            let Some(accents) = scan::accents(&syntax_text) else {
                return Self::default();
            };
            accents
        } else {
            Vec::new()
        };
        let opaque = if context.literal || !context.prose {
            Vec::new()
        } else {
            let Some(opaque) = inline_commands(source, &mut accents) else {
                return Self::default();
            };
            opaque
        };
        let lexical_protected = if context.literal {
            accents
                .iter()
                .filter(|accent| protected_role(accent.role))
                .map(|accent| accent.bytes.clone())
                .collect()
        } else {
            opaque.clone()
        };
        let protected_ranges =
            crate::highlight::ranges::union(&source.protected, &lexical_protected);
        let Some(syntax) = composition::usage_accents(source, context, &protected_ranges) else {
            return Self::default();
        };
        composition::merge(&mut accents, syntax, &opaque);
        exclude(&mut accents, &source.protected);
        Self {
            accents,
            ..Self::default()
        }
    }

    /// `text` must be the next unchanged piece of the original block.
    pub(crate) fn spans(
        &mut self,
        text: &str,
        base: Style,
        styles: CodeStyles<'_>,
    ) -> Vec<Span<'static>> {
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
                        base.patch(styles.style(accent.role)),
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

const fn protected_role(role: StyleRole) -> bool {
    matches!(
        role,
        StyleRole::CodeString | StyleRole::CodeComment | StyleRole::CodeVariable
    )
}

/// Explicit Code leaves are independent fragments, not a mode applied to
/// surrounding prose. Disjoint merged ranges bound their total scan length.
fn inline_commands(
    source: &source::Source,
    accents: &mut Vec<Accent>,
) -> Option<Vec<Range<usize>>> {
    let mut opaque = Vec::new();
    for range in &source.code {
        let fragment = &source.text[range.clone()];
        let first = fragment.trim_start();
        let start = fragment.len() - first.len();
        let command = first
            .find(char::is_whitespace)
            .is_some_and(|end| command::head(fragment, start, start + end));
        let fragment_accents = scan::accents(fragment)?;
        for accent in fragment_accents {
            let bytes = range.start + accent.bytes.start..range.start + accent.bytes.end;
            if protected_role(accent.role) {
                opaque.push(bytes.clone());
            }
            if command {
                accents.push(Accent {
                    bytes,
                    role: accent.role,
                });
            }
        }
    }
    Some(opaque)
}

/// Each protected owner interval is consumed monotonically, independently of
/// inline splitting. In particular, mathematical atoms are never CLI options.
fn exclude(accents: &mut Vec<Accent>, protected: &[Range<usize>]) {
    if protected.is_empty() {
        return;
    }
    let mut index = 0;
    let mut result = Vec::new();
    for accent in accents.drain(..) {
        while protected
            .get(index)
            .is_some_and(|range| range.end <= accent.bytes.start)
        {
            index += 1;
        }
        let mut start = accent.bytes.start;
        for range in protected[index..]
            .iter()
            .take_while(|range| range.start < accent.bytes.end)
        {
            if start < range.start {
                result.push(Accent {
                    bytes: start..range.start,
                    role: accent.role,
                });
            }
            start = start.max(range.end);
        }
        if start < accent.bytes.end {
            result.push(Accent {
                bytes: start..accent.bytes.end,
                role: accent.role,
            });
        }
    }
    *accents = result;
}

const fn signature_role(role: crate::highlight::usage::Role) -> StyleRole {
    use crate::highlight::usage::Role;
    match role {
        Role::Command => StyleRole::CodeCommand,
        Role::Option => StyleRole::CodeOption,
        Role::Placeholder => StyleRole::CodePlaceholder,
        Role::Delimiter => StyleRole::CodeDelimiter,
        Role::Operator => StyleRole::CodeOperator,
    }
}

/// Overlay complete signature tokens without leaving overlapping byte ranges.
/// Both streams are source-ordered; each base accent is split only at actual
/// token boundaries, never at viewport or inline-wrapper boundaries.
fn overlay(base: &mut Vec<Accent>, signature: Vec<Accent>) {
    crate::highlight::ranges::overlay(
        base,
        signature,
        |accent| accent.bytes.clone(),
        |accent, bytes| Accent {
            bytes,
            role: accent.role,
        },
    );
}

#[cfg(test)]
mod tests;
