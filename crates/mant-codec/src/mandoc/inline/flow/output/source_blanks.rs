//! Map a consumed native source-prefix cell to its formatter word.

use super::super::Inline;
use super::super::field_buffer::LeadingBlankRetreat;
use super::native_passes::native_word_marker;

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
