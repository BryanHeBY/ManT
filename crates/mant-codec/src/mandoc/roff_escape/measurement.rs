use super::{RoffInlineEvent, decoder::Decoder};

/// Device measurement of an unevaluated width sample. This follows
/// `term.c::term_strlen()` rather than executing `term_word()`: `\\z` skips
/// the next measured glyph, font/motion controls have no width, and an
/// overstrike measures its widest unescaped constituent.
pub(in crate::mandoc) fn width_sample(source: &str) -> usize {
    let mut width = 0_usize;
    let mut skip = false;
    let mut decoder = Decoder::new(source);
    decoder.measurement = true;
    for event in decoder.decode() {
        match event {
            RoffInlineEvent::Text(text) => {
                let tail = if skip && !text.is_empty() {
                    skip = false;
                    text.get(text.chars().next().map_or(0, char::len_utf8)..)
                        .unwrap_or_default()
                } else {
                    &text
                };
                width = width.saturating_add(mant_ir::geometry::text_width(tail));
            }
            RoffInlineEvent::Glyph(text) => {
                if skip {
                    skip = false;
                } else {
                    width = width.saturating_add(mant_ir::geometry::text_width(&text));
                }
            }
            RoffInlineEvent::BreakableHyphen | RoffInlineEvent::NonBreakingWordBlank => {
                if skip {
                    skip = false;
                } else {
                    width = width.saturating_add(1);
                }
            }
            RoffInlineEvent::DeviceName => {
                if skip {
                    skip = false;
                } else {
                    width = width.saturating_add(4);
                }
            }
            RoffInlineEvent::ZeroAdvance => skip = true,
            RoffInlineEvent::Overstrike { source, .. } => {
                let mut decoder = Decoder::new(&source);
                let mut maximum = 0;
                while let Some(character) = decoder.take_character() {
                    if character == '\\' {
                        if let Some(trigger) = decoder.take_character() {
                            decoder.decode_escape(trigger);
                        }
                    } else {
                        maximum =
                            maximum.max(mant_ir::geometry::text_width(&character.to_string()));
                    }
                }
                width = width.saturating_add(maximum);
            }
            RoffInlineEvent::FallbackGlyph(_)
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Breakpoint
            | RoffInlineEvent::Font(_)
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::EmptyDestination
            | RoffInlineEvent::LineBreak
            | RoffInlineEvent::NoSpace
            | RoffInlineEvent::Presentation { .. } => {}
        }
    }
    width
}
