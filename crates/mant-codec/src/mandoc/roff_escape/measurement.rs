use super::{RoffInlineEvent, decoder::Decoder, overstrike_operands};

/// Device measurement of an unevaluated width sample. This follows
/// `term.c::term_strlen()` rather than executing `term_word()`: `\\z` skips
/// the next measured glyph, font/motion controls have no width, and an
/// overstrike measures its widest raw operand without consuming the skip.
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
            RoffInlineEvent::Glyph(text, _) => {
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
                // term_strlen() enters mandoc_escape() at the slash,
                // just like term_word() here. It leaves the remaining
                // escape spelling as raw operands.
                let maximum = overstrike_operands(&source)
                    .map(|(glyph, _)| mant_ir::geometry::text_width(glyph.encode_utf8(&mut [0; 4])))
                    .max()
                    .unwrap_or(0);
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
            | RoffInlineEvent::HorizontalMotion { .. }
            | RoffInlineEvent::Presentation { .. } => {}
        }
    }
    width
}

#[cfg(test)]
mod tests {
    use super::width_sample;

    #[test]
    fn source_breakpoint_retires_the_skipped_width_in_a_tbl_sample() {
        // Fixed ASCII/UTF8 term_strlen widths, measured independently
        // as the native box's top rule minus its two border blanks.
        for (word, columns) in [
            ("LEFTYTAIL", 9),
            ("LEFTbYTAIL", 10),
            (r"LEFT\zb\:YTAIL", 9),
            (r"LEFT\z\o'ab'\:YTAIL", 10),
            (r"LEFT\z\o'ab'\&YTAIL", 9),
        ] {
            assert_eq!(width_sample(word), columns, "{word}");
        }
    }

    #[test]
    fn tbl_measurement_reads_the_same_raw_overstrike_operands_as_execution() {
        // In both pinned devices, the l. box width minus its two blank
        // cells witnesses the exact term_strlen() result. Slots after an
        // internal slash still measure, and a skipped glyph outside the
        // overstrike does not change that maximum.
        for (word, columns) in [
            (r"LEFT\o'\&'\:T", 6),
            (r"LEFT\z\o'\&'\:T", 6),
            (r"LEFT\o'\:'\:T", 6),
            (r"LEFT\o'\fR'\:T", 6),
            (r"LEFT\o'\-'\:T", 6),
            (r"LEFT\o'AB'\:T", 6),
            (r"LEFT\o''\:T", 5),
            (r"LEFT\z\o''\:T", 5),
        ] {
            assert_eq!(width_sample(word), columns, "{word}");
        }
    }
}
