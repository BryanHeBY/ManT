//! Best-effort code accents, independent of authored styles and layout.
//!
//! Scan one complete block, not individual inline spans or wrapped rows.
//! Manual displays mix source code, commands, configuration and placeholders;
//! no language selection, syntax dependency or filesystem lookup is needed.

use std::ops::Range;

use ratatui::{style::Style, text::Span};

use crate::theme::{CodeStyles, StyleRole};

mod command;
mod keywords;
mod scan;
mod synopsis;

pub(crate) const MAX_BLOCK_BYTES: usize = 64 * 1024;

/// Lexical evidence and display geometry are independent. A literal synopsis
/// uses both flags; an ordinary synopsis paragraph uses only `synopsis`.
#[derive(Clone, Copy, Default)]
pub(crate) struct HighlightContext {
    pub(crate) literal: bool,
    pub(crate) synopsis: bool,
}

impl From<bool> for HighlightContext {
    fn from(literal: bool) -> Self {
        Self {
            literal,
            synopsis: false,
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
        Self::with_context(value, true.into(), &[])
    }

    pub(crate) fn for_inlines(
        nodes: &[mant_ir::Inline],
        context: HighlightContext,
    ) -> Option<Self> {
        if !context.literal && !context.synopsis {
            return None;
        }
        let mut value = String::new();
        let mut emphasis = Vec::new();
        let mut bounded = true;
        mant_render::visit_inline_text(nodes, &[], |source, _, text| {
            if bounded && text.len() <= MAX_BLOCK_BYTES - value.len() {
                let start = value.len();
                value.push_str(text);
                if source.emphasis {
                    emphasis.push(start..value.len());
                }
            } else {
                bounded = false;
            }
        });
        let highlights = if bounded {
            Self::with_context(&value, context, &emphasis)
        } else {
            Self::default()
        };
        (context.literal || !highlights.accents.is_empty()).then_some(highlights)
    }

    fn with_context(value: &str, context: HighlightContext, emphasis: &[Range<usize>]) -> Self {
        // Reject the whole block before scanning: a partial scan would lose
        // quote/comment state and miscolor subsequent inline pieces.
        if value.len() > MAX_BLOCK_BYTES {
            return Self::default();
        }
        let mut accents = if context.literal {
            let Some(accents) = scan::accents(value) else {
                return Self::default();
            };
            accents
        } else {
            Vec::new()
        };
        let signature_context = if context.synopsis {
            crate::signature::Context::Synopsis
        } else {
            crate::signature::Context::Inferred
        };
        let mut protected = 0;
        let signature = crate::signature::accents(value, signature_context, emphasis, &[])
            .into_iter()
            .filter_map(|accent| {
                while accents
                    .get(protected)
                    .is_some_and(|base| base.bytes.end <= accent.bytes.start)
                {
                    protected += 1;
                }
                let masked = accents[protected..]
                    .iter()
                    .take_while(|base| base.bytes.start < accent.bytes.end)
                    .any(|base| {
                        matches!(
                            base.role,
                            StyleRole::CodeString
                                | StyleRole::CodeComment
                                | StyleRole::CodeVariable
                        )
                    });
                (!masked).then_some(Accent {
                    bytes: accent.bytes,
                    role: signature_role(accent.role),
                })
            })
            .collect();
        overlay(&mut accents, signature);
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

const fn signature_role(role: crate::signature::Role) -> StyleRole {
    use crate::signature::Role;
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
    if signature.is_empty() {
        return;
    }
    let mut result = Vec::new();
    let mut index = 0;
    for accent in base.drain(..) {
        while signature
            .get(index)
            .is_some_and(|patch| patch.bytes.end <= accent.bytes.start)
        {
            index += 1;
        }
        let mut start = accent.bytes.start;
        for patch in signature[index..]
            .iter()
            .take_while(|patch| patch.bytes.start < accent.bytes.end)
        {
            if start < patch.bytes.start {
                result.push(Accent {
                    bytes: start..patch.bytes.start,
                    role: accent.role,
                });
            }
            start = start.max(patch.bytes.end);
        }
        if start < accent.bytes.end {
            result.push(Accent {
                bytes: start..accent.bytes.end,
                role: accent.role,
            });
        }
    }
    result.extend(signature);
    result.sort_by_key(|accent| accent.bytes.start);
    *base = result;
}

#[cfg(test)]
mod tests;
