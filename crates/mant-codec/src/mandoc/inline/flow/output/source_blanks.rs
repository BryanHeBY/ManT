//! Map a consumed native source-prefix cell to its formatter word.

use super::super::Inline;
use super::super::field_buffer::LeadingBlankRetreat;
use super::native_passes::native_word_marker;

/// A checked negative HORIZ can replace a previous word's live BACKBEFORE
/// glyph. Its native owner remains the same whether that glyph was settled
/// before the word or released into the new word's borrowed projection.
pub(super) fn remove_rewound_glyph(
    nodes: &mut [Inline],
    incoming: &mut [Inline],
    owner: &str,
    receipt: &super::super::field_buffer::RewoundGlyph,
) {
    edit_rewound_glyph(nodes, incoming, owner, receipt, false);
}

struct ReverseOwnerCursor<'a, 'b> {
    owner: &'a str,
    remaining: usize,
    group: usize,
    candidate: Option<(&'b mut String, usize)>,
    retain_style: bool,
    trailing_only: bool,
    crossed_ink: bool,
    projection: Option<Inline>,
    generation: usize,
}

impl<'b> ReverseOwnerCursor<'_, 'b> {
    fn reset_group(&mut self) {
        self.group = 0;
        self.candidate = None;
        self.projection = None;
    }

    fn wrapped(&mut self, children: &'b mut [Inline], wrap: impl FnOnce(Inline) -> Inline) -> bool {
        let generation = self.generation;
        let matched = self.visit(children);
        if generation != self.generation {
            self.projection = self.projection.take().map(wrap);
        }
        matched
    }

    fn visit(&mut self, nodes: &'b mut [Inline]) -> bool {
        for node in nodes.iter_mut().rev() {
            if let Some(marker) = native_word_marker(node) {
                if marker == self.owner {
                    if self.remaining < self.group {
                        return true;
                    }
                    self.remaining -= self.group;
                }
                self.reset_group();
                continue;
            }
            let is_code = matches!(node, Inline::Code { .. });
            match node {
                Inline::Strong { children } => {
                    if self.wrapped(children, |glyph| Inline::Strong {
                        children: vec![glyph],
                    }) {
                        return true;
                    }
                }
                Inline::Emphasis { children } => {
                    if self.wrapped(children, |glyph| Inline::Emphasis {
                        children: vec![glyph],
                    }) {
                        return true;
                    }
                }
                Inline::Link {
                    target,
                    title,
                    children,
                } => {
                    if self.wrapped(children, |glyph| Inline::Link {
                        target: target.clone(),
                        title: title.clone(),
                        children: vec![glyph],
                    }) {
                        return true;
                    }
                }
                Inline::Text { value } | Inline::Code { value } => {
                    let scalars = value.chars().count();
                    let has_ink = self.trailing_only && value.chars().any(|ch| !ch.is_whitespace());
                    if let Some(relative) = self.remaining.checked_sub(self.group)
                        && let Some((byte, scalar)) = value.char_indices().rev().nth(relative)
                        && (!self.trailing_only
                            || !self.crossed_ink
                                && value[byte + scalar.len_utf8()..]
                                    .chars()
                                    .all(char::is_whitespace))
                    {
                        self.generation += 1;
                        self.projection = self.retain_style.then(|| {
                            let value = scalar.to_string();
                            if is_code {
                                Inline::Code { value }
                            } else {
                                Inline::Text { value }
                            }
                        });
                        self.candidate = Some((value, byte));
                    }
                    self.crossed_ink |= has_ink;
                    self.group += scalars;
                }
                Inline::Equation { value, .. } => {
                    // Never change the cache of an already published equation.
                    self.group += value.chars().count();
                    self.crossed_ink |= !value.is_empty();
                }
                Inline::LineBreak { .. } => self.crossed_ink = true,
                Inline::Anchor { id, .. }
                    if id.as_str().starts_with(super::INTERNAL_FIELD_PREFIX) =>
                {
                    self.reset_group();
                }
                Inline::Anchor { .. } => {}
            }
            if self.trailing_only && self.crossed_ink && self.candidate.is_none() {
                return false;
            }
        }
        false
    }
}

pub(in crate::mandoc::inline::flow) fn take_rewound_glyph(
    nodes: &mut [Inline],
    incoming: &mut [Inline],
    owner: &str,
    receipt: &super::super::field_buffer::RewoundGlyph,
) -> Option<Inline> {
    edit_rewound_glyph(nodes, incoming, owner, receipt, true)?.projection
}

pub(super) struct RewoundGlyphEdit {
    pub(super) projection: Option<Inline>,
}

pub(super) fn edit_rewound_glyph(
    nodes: &mut [Inline],
    incoming: &mut [Inline],
    owner: &str,
    receipt: &super::super::field_buffer::RewoundGlyph,
    retain_style: bool,
) -> Option<RewoundGlyphEdit> {
    rewound_glyph(
        nodes,
        incoming,
        owner,
        receipt,
        RewindProjection::Remove { retain_style },
    )
}

pub(super) fn has_trailing_rewound_blank(
    nodes: &mut [Inline],
    owner: &str,
    receipt: &super::super::field_buffer::RewoundGlyph,
) -> bool {
    rewound_glyph(
        nodes,
        &mut [],
        owner,
        receipt,
        RewindProjection::CheckTrailingBlank,
    )
    .is_some()
}

#[derive(Clone, Copy)]
enum RewindProjection {
    Remove { retain_style: bool },
    CheckTrailingBlank,
}

fn rewound_glyph(
    nodes: &mut [Inline],
    incoming: &mut [Inline],
    owner: &str,
    receipt: &super::super::field_buffer::RewoundGlyph,
    mode: RewindProjection,
) -> Option<RewoundGlyphEdit> {
    // A delayed glyph may re-enter the prior word after its earlier output
    // marker. Count the owner's scalar suffix backwards through all wrappers.
    let mut cursor = ReverseOwnerCursor {
        owner,
        remaining: receipt.following_scalars,
        group: 0,
        candidate: None,
        retain_style: match mode {
            RewindProjection::Remove { retain_style } => retain_style,
            RewindProjection::CheckTrailingBlank => true,
        },
        trailing_only: matches!(mode, RewindProjection::CheckTrailingBlank),
        crossed_ink: false,
        projection: None,
        generation: 0,
    };
    let found = cursor.visit(incoming) || {
        // The incoming leading unqualified payload belongs to the new word.
        cursor.reset_group();
        cursor.visit(nodes)
    };
    if found
        && let Some((value, byte)) = cursor.candidate
        && value[byte..].starts_with(receipt.scalar)
    {
        if matches!(mode, RewindProjection::Remove { .. }) {
            value.remove(byte);
        }
        return Some(RewoundGlyphEdit {
            projection: cursor.projection,
        });
    }
    None
}

/// A raw SP can cross an old cell without writing (term.c:948–952).
/// Remove the exact source scalar only when it has no projection cell;
/// embedded surviving padding is represented by its source position.
pub(super) fn remove_rewound_source_blanks(nodes: &mut [Inline], owner: &str, offsets: &[usize]) {
    struct Cursor<'a> {
        owner: &'a str,
        active: bool,
        scalar: usize,
        offsets: std::iter::Peekable<std::slice::Iter<'a, usize>>,
    }
    impl Cursor<'_> {
        fn remove(&mut self, nodes: &mut [Inline]) {
            for node in nodes {
                if self.offsets.peek().is_none() {
                    break;
                }
                if let Some(marker) = native_word_marker(node) {
                    self.active = marker == self.owner;
                    continue;
                }
                match node {
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => self.remove(children),
                    Inline::Text { value } | Inline::Code { value } if self.active => {
                        value.retain(|character| {
                            let removed = self.offsets.next_if(|&&offset| offset == self.scalar);
                            self.scalar += 1;
                            removed.is_none() || character != ' '
                        });
                    }
                    Inline::Equation { value, .. } if self.active => {
                        self.scalar += value.chars().count();
                        while self
                            .offsets
                            .next_if(|&&offset| offset < self.scalar)
                            .is_some()
                        {}
                    }
                    _ => {}
                }
            }
        }
    }
    Cursor {
        owner,
        active: true,
        scalar: 0,
        offsets: offsets.iter().peekable(),
    }
    .remove(nodes);
}

/// Remove the exact raw blank if it is still projected. When an ordinary
/// SP can represent the same surviving automatic cell, return its original
/// styling and ownership for the separator emitter to use exactly once.
pub(super) fn take_retreated_source_blank(
    nodes: &mut [Inline],
    owner: &str,
    receipt: Option<LeadingBlankRetreat>,
) -> Option<Inline> {
    let receipt = receipt?;
    let offset = receipt.offset?;
    let mut cursor = PrefixCursor {
        owner,
        active: true,
        remaining: Some(offset),
        character: receipt.character,
    };
    let authored = cursor.take(nodes);
    // A tab has different width. A rearm changes the next graph's column.
    // For a longer prefix, retain the still-authored runs in source order.
    if receipt.character == ' ' && !receipt.rearmed && offset == 0 {
        authored
    } else {
        None
    }
}

struct PrefixCursor<'a> {
    owner: &'a str,
    active: bool,
    remaining: Option<usize>,
    character: char,
}

impl PrefixCursor<'_> {
    fn take(&mut self, nodes: &mut [Inline]) -> Option<Inline> {
        for node in nodes {
            self.remaining?;
            if let Some(marker) = native_word_marker(node) {
                self.active = marker == self.owner;
                continue;
            }
            let taken = match node {
                Inline::Strong { children } => self.take(children).map(|blank| Inline::Strong {
                    children: vec![blank],
                }),
                Inline::Emphasis { children } => {
                    self.take(children).map(|blank| Inline::Emphasis {
                        children: vec![blank],
                    })
                }
                Inline::Link {
                    target,
                    title,
                    children,
                } => self.take(children).map(|blank| Inline::Link {
                    target: target.clone(),
                    title: title.clone(),
                    children: vec![blank],
                }),
                Inline::Text { value } if self.active => {
                    self.take_value(value).map(|value| Inline::Text { value })
                }
                Inline::Code { value } if self.active => {
                    self.take_value(value).map(|value| Inline::Code { value })
                }
                Inline::Equation { .. } | Inline::LineBreak { .. } if self.active => {
                    self.remaining = None;
                    return None;
                }
                _ => None,
            };
            if taken.is_some() {
                return taken;
            }
        }
        None
    }
    fn take_value(&mut self, value: &mut String) -> Option<String> {
        let remaining = self.remaining?;
        // The receipt gives the offset within the word's authored prefix.
        // Check the bytes to detect a blank the projector already consumed.
        let Some((byte, character)) = value.char_indices().nth(remaining) else {
            self.remaining = value
                .bytes()
                .all(is_raw_blank)
                .then(|| remaining - value.len());
            return None;
        };
        self.remaining = None;
        if character != self.character || !value[..byte].bytes().all(is_raw_blank) {
            return None;
        }
        value.remove(byte);
        Some(character.to_string())
    }
}

fn is_raw_blank(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mandoc::inline::flow::field_buffer::{FieldBuffer, FieldWrite};

    #[test]
    fn retreated_source_blank_maps_to_one_accepted_automatic_cell() {
        // Fixed CVS executes ALPHA\zX, an empty word, and " BETA" as
        // ALPHAX + two spaces + BETA. term.c:901-908 pops only the raw
        // current-word blank, leaving each word's automatic cell.
        let mut buffer = FieldBuffer::default();
        let mut writes = FieldWrite::literal("ALPHA");
        writes.push(FieldWrite::ArmBackafter);
        FieldWrite::append_literal(&mut writes, "X");
        buffer.apply_writes(&writes);
        let empty_start = buffer.cells().len();
        buffer.begin_word(false, false);
        buffer.apply_writes(&[]);
        let current_start = buffer.cells().len();
        let separator = buffer.begin_word(false, false);
        let receipt = buffer.apply_writes(&FieldWrite::literal(" BETA"));
        assert_eq!(buffer.surviving_word_separator(current_start, separator), 1);
        assert_eq!(buffer.projection_length(empty_start, current_start), 1);
        assert_eq!(buffer.projection_length(current_start, receipt.end_cell), 5);

        let current = format!("{}8", super::super::INTERNAL_FIELD_WORD);
        let previous = format!("{}7", super::super::INTERNAL_FIELD_WORD);
        let mut nodes = [
            Inline::anchor(previous),
            Inline::Text { value: " ".into() },
            Inline::anchor(current.clone()),
            Inline::Emphasis {
                children: vec![Inline::Text {
                    value: " BETA".into(),
                }],
            },
        ];
        let separator = take_retreated_source_blank(&mut nodes, &current, receipt.leading_retreat);
        assert_eq!(mant_ir::inline_plain_text(&nodes[..2]), " ");
        assert_eq!(mant_ir::inline_plain_text(&nodes[2..]), "BETA");
        assert_eq!(
            separator,
            Some(Inline::Emphasis {
                children: vec![Inline::Text { value: " ".into() }],
            })
        );
    }
}
