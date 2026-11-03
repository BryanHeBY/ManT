//! Pure link-operand identities, independent of terminal word execution.
//!
//! Xr/MR and UR/MT/Lk/Mt/%U share CVS `html.c::print_encode(norecurse=1)`.
//! The caller executes visible source operands once in its existing formatter.
//! Section headings and Sx instead share `authored_section_phrase()` and the
//! `roff.c::deroff()` namespace; this decoder must not replace that contract.
//!
//! Each operand starts with fresh skip state. In particular, Manual name and
//! section identities never carry a trailing bare `\z` into each other or
//! into an HTML template. CVS `print_href()` shares `HTML_SKIPCHAR` while
//! expanding templates, so `printf\z` and `3` with `%N.%S` produce the href
//! bytes `printf3`; the typed identity remains name `printf`, section `3`.
//! These per-operand glyph rules do not emulate arbitrary template bytes.

use crate::mandoc::roff_escape::{RoffInlineEvent, decode_link_operand};

/// Extract a link destination from the source spelling without borrowing its
/// rendered glyph stream.  A `\z` glyph participates in terminal overstrike
/// layout but is not part of mdoc's destination: CVS `mdoc_html.c` builds the
/// `Lk`/`Mt` href from the logical operand while terminal rendering consumes
/// the zero-advance glyph in the surrounding output flow.  Keeping this
/// separate from [`visible_text`](crate::mandoc::roff_escape::visible_text)
/// prevents a later sibling from changing a typed destination and lets the
/// label retain its own formatter state.
pub(in crate::mandoc::inline) fn link_identity_text(source: &str) -> String {
    let mut identity = String::new();
    let mut skip_next_glyph = false;
    for event in decode_link_operand(source) {
        match event {
            RoffInlineEvent::Text(value) => {
                for character in value.chars() {
                    if skip_next_glyph {
                        skip_next_glyph = false;
                    } else {
                        identity.push(character);
                    }
                }
            }
            RoffInlineEvent::Glyph(value) => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else {
                    identity.push_str(&value);
                }
            }
            RoffInlineEvent::BreakableHyphen => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else {
                    identity.push('-');
                }
            }
            RoffInlineEvent::FallbackGlyph(_) => {
                // Unknown and out-of-range glyphs have no HTML codepoint.
                // They remain readable in terminal prose but are omitted by
                // CVS `print_encode(..., norecurse=1)` when building hrefs.
                skip_next_glyph = false;
            }
            RoffInlineEvent::DeviceName => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else {
                    identity.push_str("html");
                }
            }
            RoffInlineEvent::Overstrike { source, .. } => {
                if skip_next_glyph {
                    skip_next_glyph = false;
                } else if let Some(character) = source.chars().next_back() {
                    identity.push(character);
                }
            }
            RoffInlineEvent::ZeroAdvance => skip_next_glyph = true,
            RoffInlineEvent::Breakpoint => {
                // html.c::print_encode() uses chars.c's positive U+200B for
                // direct \:, while term_word() buffers ASCII_BREAK. Keep
                // the HTML destination independent of visible terminal text.
                if !skip_next_glyph {
                    identity.push('\u{200b}');
                }
                skip_next_glyph = false;
            }
            // CVS HTML_SKIPCHAR survives font changes, but every other
            // formatter escape consumes the pending skip before a later
            // address glyph. In particular, `\\z\\c` must not delete the
            // first character following `\\c` from a typed target.
            RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => {}
            RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::EmptyDestination
            | RoffInlineEvent::LineBreak
            | RoffInlineEvent::NoSpace
            | RoffInlineEvent::Presentation { .. } => skip_next_glyph = false,
        }
    }
    identity
}

#[cfg(test)]
mod tests {
    use super::link_identity_text;
    use crate::mandoc::roff_escape::visible_text;

    #[test]
    fn overstrike_terminal_projection_and_html_identity_remain_independent() {
        // CVS term.c trims the trailing backspace/blank pair and leaves C in
        // its one-cell projection. html.c deliberately uses the final source
        // byte for the href instead; neither representation can substitute
        // for the other.
        assert_eq!(visible_text(r"\o'BC '"), "C");
        assert_eq!(link_identity_text(r"\o'BC '"), " ");

        // The fixed-CVS overstrike loop exposes the spelling after a nested
        // reverse-solidus while HTML chooses the same final source byte.
        assert_eq!(visible_text(r"\o'BC\fI'"), "I");
        assert_eq!(link_identity_text(r"\o'BC\fI'"), "I");

        // The delimiter belonging to a nested escape is part of the outer
        // source argument.  Only the final quote closes `\o`, so both the
        // terminal and identity projections see the complete operand.
        for source in [r"\o'BC\N'8''", r"\o'BC\h'1n''", r"\o'BC\C'x''"] {
            assert_eq!(visible_text(source), "'", "{source}");
            assert_eq!(link_identity_text(source), "'", "{source}");
        }
    }

    #[test]
    fn numbered_link_identity_uses_the_same_terminal_codepoint_contract() {
        assert_eq!(link_identity_text(r"prefix\N'0'suffix"), "prefix�suffix");
        assert_eq!(
            link_identity_text(r"prefix\N'160'suffix"),
            "prefix\u{a0}suffix"
        );
    }
}
