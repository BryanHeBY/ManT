//! Tokenizes formatter-level roff escapes before semantic AST lowering.
//!
//! libmandoc intentionally retains several GNU roff extensions inside text
//! nodes. This module is the sole boundary allowed to interpret those bytes:
//! consumers receive typed events and can never mistake an escape operand for
//! visible document text.

mod decoder;
mod glyphs;
mod grammar;
mod measurement;
mod projection;
mod scanner;
mod zero_advance;

use decoder::Decoder;
pub(super) use measurement::width_sample;
pub(super) use projection::visible_text;
pub(super) use zero_advance::ZeroAdvanceMachine;

const ASCII_TABREF: char = '\u{1a}';
const ASCII_BREAK: char = '\u{1d}';
const ASCII_HYPH: char = '\u{1c}';
const ASCII_NBRZW: char = '\u{1e}';
const ASCII_NBRSP: char = '\u{1f}';

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RoffFont {
    Regular,
    Strong,
    Emphasis,
    StrongEmphasis,
    Code,
    CodeStrong,
    CodeEmphasis,
}

impl From<libmandoc_rs::NormalizedFont> for RoffFont {
    fn from(font: libmandoc_rs::NormalizedFont) -> Self {
        match font {
            libmandoc_rs::NormalizedFont::Emphasis => Self::Emphasis,
            libmandoc_rs::NormalizedFont::Symbolic => Self::Strong,
            libmandoc_rs::NormalizedFont::Literal => Self::Code,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PresentationKind {
    Color,
    PointSize,
    HorizontalMotion,
    Motion,
    FormatterState,
    Postprocessor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RoffInlineEvent {
    Text(String),
    /// NBRWORD converts a literal source ASCII blank to `ASCII_NBRSP`.
    /// Its first native scan is graph, but its printed scalar is still SP.
    NonBreakingWordBlank,
    /// The parser's `ASCII_HYPH` cell. It displays '-' but remains a native
    /// `term_fill` break candidate, unlike an authored nonbreaking \- escape.
    BreakableHyphen,
    /// One source-level glyph whose printable fallback spans several
    /// characters, for example an unknown `\\[name]` escape.
    Glyph(String),
    /// An unrecognized source glyph. It remains visible normally, but cannot
    /// be made into a terminal `\\z` glyph because no output glyph exists to
    /// overstrike.
    FallbackGlyph(String),
    /// The formatter's current output-device name. Terminal and HTML
    /// projections intentionally spell this differently (`utf8` vs `html`).
    DeviceName,
    /// A roff overstrike request. Terminal geometry is outside the IR, but
    /// HTML link identities retain its final source glyph.
    Overstrike {
        source: String,
        terminal: Option<String>,
    },
    /// `\\z` makes the next complete glyph zero-advance. The decoder keeps
    /// decoding that glyph and every intervening formatter control; the
    /// cross-node projection decides whether a later glyph overstrikes it.
    ZeroAdvance,
    /// An invisible glyph buffered by the terminal formatter. Unlike a font
    /// selection, it occupies a literal output row without adding text.
    ZeroWidthGlyph,
    /// The named `\\:` control writes `ASCII_BREAK` on both terminal devices.
    /// It offers a zero-width break candidate without arming `term_fill`'s
    /// graph flag. The catalog's U+200B belongs to HTML, not terminal prose.
    Breakpoint,
    Font(RoffFont),
    PreviousFont,
    Link(Option<String>),
    /// Exact legacy Sphinx `\%<>` output. It is invisible only when the
    /// preceding visible text proves that it belongs to a manual reference.
    EmptyDestination,
    LineBreak,
    /// `\\c` conditionally suppresses the next input-line boundary.  CVS
    /// `term_word()` gives it a special interaction with `\\z`: when a
    /// zero-advance glyph is waiting to be overstruck, `\\c` cancels that
    /// backtracking state instead of enabling a source-line join.
    NoSpace,
    Presentation {
        kind: PresentationKind,
        argument: Option<String>,
    },
}

/// The observable role of one decoded inline event.
///
/// Consumers need this shared classification when deciding whether a
/// formatter request can be crossed while looking for visible body content.
/// It intentionally distinguishes an invisible row marker (`\&`) from a
/// pure state transition (such as a font selection) and from a real line
/// boundary. Adding a new decoder event must therefore make its effect
/// explicit instead of silently flowing through a catch-all branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InlineEventEffect {
    Visible,
    RowMarker,
    StateOnly,
    LineBoundary,
}

pub(super) fn inline_event_effect(event: &RoffInlineEvent) -> InlineEventEffect {
    match event {
        RoffInlineEvent::Text(value)
        | RoffInlineEvent::Glyph(value)
        | RoffInlineEvent::FallbackGlyph(value) => {
            if value.is_empty() {
                InlineEventEffect::StateOnly
            } else {
                InlineEventEffect::Visible
            }
        }
        RoffInlineEvent::DeviceName
        | RoffInlineEvent::BreakableHyphen
        | RoffInlineEvent::NonBreakingWordBlank => InlineEventEffect::Visible,
        RoffInlineEvent::Overstrike { terminal, .. } => {
            if terminal.is_some() {
                InlineEventEffect::Visible
            } else {
                InlineEventEffect::StateOnly
            }
        }
        RoffInlineEvent::ZeroWidthGlyph | RoffInlineEvent::Breakpoint => {
            InlineEventEffect::RowMarker
        }
        RoffInlineEvent::LineBreak | RoffInlineEvent::EmptyDestination => {
            InlineEventEffect::LineBoundary
        }
        RoffInlineEvent::ZeroAdvance
        | RoffInlineEvent::NoSpace
        | RoffInlineEvent::Font(_)
        | RoffInlineEvent::PreviousFont
        | RoffInlineEvent::Link(_)
        | RoffInlineEvent::Presentation { .. } => InlineEventEffect::StateOnly,
    }
}

/// Decode one libmandoc text node into typed, renderer-independent events.
pub(super) fn decode(source: &str) -> Vec<RoffInlineEvent> {
    decode_with_status(source).events
}

pub(super) struct DecodedText {
    pub(super) events: Vec<RoffInlineEvent>,
    pub(super) budget_exhausted: bool,
}

/// Parsing evidence accompanies events; safety loss is not a displayed glyph.
pub(super) fn decode_with_status(source: &str) -> DecodedText {
    decode_word_with_status(source, WordSpace::Breakable)
}

#[derive(Clone, Copy, Default, Eq, PartialEq)]
pub(in crate::mandoc) enum WordSpace {
    #[default]
    Breakable,
    NonBreaking,
}

pub(in crate::mandoc) fn decode_word_with_status(source: &str, spaces: WordSpace) -> DecodedText {
    decode_with_options(source, true, spaces)
}

/// Decode an identity operand without synthesizing reading-layout spaces.
/// CVS `html.c::print_encode()` ignores horizontal motion in attributes; the
/// terminal reading approximation belongs only to the visible text stream.
pub(in crate::mandoc) fn decode_link_operand(source: &str) -> Vec<RoffInlineEvent> {
    decode_with_options(source, false, WordSpace::Breakable).events
}

fn decode_with_options(
    source: &str,
    reading_motion_spacing: bool,
    spaces: WordSpace,
) -> DecodedText {
    // term.c::term_word() copies ordinary runs before interpreting escapes.
    // Sentinel and unsafe control bytes still need the full classifier; the
    // fast path must not bypass their masking or native-cell evidence.
    if !(source.contains('\\') || spaces == WordSpace::NonBreaking && source.contains(' '))
        && source
            .chars()
            .all(|character| !character.is_control() || matches!(character, '\t' | '\n' | '\r'))
    {
        return DecodedText {
            events: if source.is_empty() {
                Vec::new()
            } else {
                vec![RoffInlineEvent::Text(source.to_owned())]
            },
            budget_exhausted: false,
        };
    }
    let mut decoder = Decoder::new(source);
    decoder.reading_motion_spacing = reading_motion_spacing;
    decoder.word_space = spaces;
    decoder.decode_with_status()
}

/// CVS terminal filling recognizes only its ordinary ASCII word blank as a
/// break opportunity. Tabs, NBSP (including Unicode and numbered spellings),
/// and other Unicode whitespace are formatter glyphs, not prose separators.
pub(in crate::mandoc) const fn is_formatter_word_blank(character: char) -> bool {
    character == ' '
}

pub(super) fn font(name: &str) -> RoffFont {
    match name {
        "B" | "3" => RoffFont::Strong,
        "I" | "2" => RoffFont::Emphasis,
        "BI" | "4" => RoffFont::StrongEmphasis,
        "C" | "CR" | "CW" | "V" => RoffFont::Code,
        "CB" | "VB" => RoffFont::CodeStrong,
        "CI" | "VI" => RoffFont::CodeEmphasis,
        _ => RoffFont::Regular,
    }
}

#[cfg(test)]
mod delimiter_scanning;
#[cfg(test)]
mod rule_matrix;
#[cfg(test)]
mod tests;
