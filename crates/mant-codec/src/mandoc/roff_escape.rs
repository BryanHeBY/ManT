//! Tokenizes formatter-level roff escapes before semantic AST lowering.
//!
//! libmandoc intentionally retains several GNU roff extensions inside text
//! nodes. This module is the sole boundary allowed to interpret those bytes:
//! consumers receive typed events and can never mistake an escape operand for
//! visible document text.

mod glyphs;
use glyphs::{
    dedicated_special_character, documented_groff_composite_character, unicode_special_characters,
};

use crate::text_safety::push_terminal_safe;
use libmandoc_rs::SpecialCharacter;

const ASCII_BREAK: char = '\u{1d}';
const ASCII_HYPH: char = '\u{1e}';
const ASCII_NBRSP: char = '\u{1f}';
const MAX_NESTED_ESCAPE_SCAN_DEPTH: usize = 256;

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
    Spacing,
    FormatterState,
    Postprocessor,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum RoffInlineEvent {
    Text(String),
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

/// Formatter-neutral core of CVS `TERMP_BACKAFTER`/`TERMP_BACKBEFORE`.
///
/// Styled terminal output and plain semantic text both use this state so a
/// target/name projection cannot disagree with the visible document about
/// which complete glyph survives `\z`.
pub(super) struct ZeroAdvanceMachine<T> {
    armed: bool,
    pending: Option<T>,
}

impl<T> ZeroAdvanceMachine<T> {
    pub(super) const fn new() -> Self {
        Self {
            armed: false,
            pending: None,
        }
    }

    pub(super) fn arm(&mut self) {
        self.armed = true;
    }

    pub(super) const fn is_armed(&self) -> bool {
        self.armed
    }

    pub(super) const fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub(super) fn cancel_armed(&mut self) -> bool {
        std::mem::take(&mut self.armed)
    }

    pub(super) fn project_glyph(&mut self, glyph: T) -> Option<(T, bool)> {
        if self.armed {
            self.armed = false;
            self.pending = Some(glyph);
            return None;
        }
        let replaced_pending = self.pending.take().is_some();
        Some((glyph, replaced_pending))
    }

    pub(super) fn project_fallback(&mut self, glyph: T) -> Option<(T, bool)> {
        if self.cancel_armed() {
            return None;
        }
        let replaced_pending = self.pending.take().is_some();
        Some((glyph, replaced_pending))
    }

    pub(super) fn resolve_word_boundary(&mut self) -> Option<T> {
        (!self.armed).then(|| self.pending.take()).flatten()
    }

    pub(super) fn take_pending(&mut self) -> Option<T> {
        self.pending.take()
    }

    pub(super) fn discard_pending(&mut self) {
        self.pending = None;
    }

    pub(super) fn clear(&mut self) {
        self.armed = false;
        self.pending = None;
    }
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
        RoffInlineEvent::DeviceName => InlineEventEffect::Visible,
        RoffInlineEvent::Overstrike { terminal, .. } => {
            if terminal.is_some() {
                InlineEventEffect::Visible
            } else {
                InlineEventEffect::StateOnly
            }
        }
        RoffInlineEvent::ZeroWidthGlyph => InlineEventEffect::RowMarker,
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

pub(super) fn source_has_visible_glyph(source: &str) -> bool {
    decode(source)
        .iter()
        .any(|event| inline_event_effect(event) == InlineEventEffect::Visible)
}

/// Decode one libmandoc text node into typed, renderer-independent events.
pub(super) fn decode(source: &str) -> Vec<RoffInlineEvent> {
    Decoder::new(source).decode()
}

/// Project the one terminal cell written by CVS `ESCAPE_OVERSTRIKE`.
///
/// The terminal encoder repeatedly backs up over the same cell.  A graphic
/// glyph replaces the cell, while a trailing blank or tab does not erase a
/// graphic already stored there; CVS subsequently trims its generated
/// backspace/blank pair.  This particular `term.c` loop advances past the
/// reverse-solidus introducer but then treats the remaining escape spelling
/// as cell input (`\fI` therefore leaves `I`).  Preserve that fixed-CVS
/// behavior rather than applying the ordinary roff decoder in this context.
fn overstrike_terminal_glyph(source: &str) -> Option<String> {
    let mut cell = None;
    for character in source.chars() {
        if character == '\\' {
            continue;
        }
        let character = match character {
            ASCII_BREAK => continue,
            ASCII_HYPH => '-',
            ASCII_NBRSP => ' ',
            other => other,
        };
        if matches!(character, ' ' | '\t') {
            continue;
        }
        let mut glyph = String::new();
        push_terminal_safe(&mut glyph, character);
        if !glyph.trim_matches([' ', '\t']).is_empty() {
            cell = Some(glyph);
        }
    }
    cell
}

/// CVS terminal filling recognizes only its ordinary ASCII word blank as a
/// break opportunity. Tabs, NBSP (including Unicode and numbered spellings),
/// and other Unicode whitespace are formatter glyphs, not prose separators.
pub(in crate::mandoc) const fn is_formatter_word_blank(character: char) -> bool {
    character == ' '
}

struct PlainTextProjection {
    output: String,
    zero_advance: ZeroAdvanceMachine<String>,
    pending_word_end_break: bool,
    suppress_break_whitespace: bool,
}

impl PlainTextProjection {
    const fn new() -> Self {
        Self {
            output: String::new(),
            zero_advance: ZeroAdvanceMachine::new(),
            pending_word_end_break: false,
            suppress_break_whitespace: false,
        }
    }

    fn append_text(&mut self, value: &str) {
        for character in value.chars() {
            if self.pending_word_end_break && is_formatter_word_blank(character) {
                if let Some(glyph) = self.zero_advance.resolve_word_boundary() {
                    self.output.push_str(&glyph);
                    self.suppress_break_whitespace = true;
                    continue;
                }
                self.output.push('\n');
                self.pending_word_end_break = false;
                self.suppress_break_whitespace = true;
                continue;
            }
            if self.suppress_break_whitespace && is_formatter_word_blank(character) {
                continue;
            }
            self.suppress_break_whitespace = false;
            if matches!(character, '\n' | '\r') {
                if let Some(glyph) = self.zero_advance.take_pending() {
                    self.output.push_str(&glyph);
                }
                self.output.push(character);
            } else if self.zero_advance.is_armed() {
                let _ = self.zero_advance.project_glyph(character.to_string());
            } else if self.zero_advance.has_pending() {
                if is_formatter_word_blank(character) {
                    if let Some(glyph) = self.zero_advance.take_pending() {
                        self.output.push_str(&glyph);
                    }
                    continue;
                }
                let Some((character, _)) = self.zero_advance.project_glyph(character.to_string())
                else {
                    continue;
                };
                self.output.push_str(&character);
            } else if let Some((character, _)) =
                self.zero_advance.project_glyph(character.to_string())
            {
                self.output.push_str(&character);
            }
        }
    }

    fn append_glyph(&mut self, value: String) {
        self.suppress_break_whitespace = false;
        if let Some((value, _)) = self.zero_advance.project_glyph(value) {
            self.output.push_str(&value);
        }
    }

    fn append_event(&mut self, event: RoffInlineEvent) {
        match event {
            RoffInlineEvent::Text(value) => self.append_text(&value),
            RoffInlineEvent::Glyph(value)
            | RoffInlineEvent::Overstrike {
                terminal: Some(value),
                ..
            } => self.append_glyph(value),
            RoffInlineEvent::FallbackGlyph(value) => {
                if let Some((value, _)) = self.zero_advance.project_fallback(value) {
                    self.suppress_break_whitespace = false;
                    self.output.push_str(&value);
                }
            }
            RoffInlineEvent::DeviceName => self.append_text("utf8"),
            RoffInlineEvent::ZeroAdvance => self.zero_advance.arm(),
            RoffInlineEvent::EmptyDestination => {
                self.suppress_break_whitespace = false;
                self.output.push_str("<>");
            }
            RoffInlineEvent::LineBreak => self.pending_word_end_break = true,
            RoffInlineEvent::NoSpace => {
                self.zero_advance.cancel_armed();
            }
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::ZeroWidthGlyph
            | RoffInlineEvent::PreviousFont
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::Overstrike { terminal: None, .. }
            | RoffInlineEvent::Presentation { .. } => {}
        }
    }

    fn finish(mut self) -> String {
        if let Some(glyph) = self.zero_advance.take_pending() {
            self.output.push_str(&glyph);
        }
        self.output
    }
}

/// Return only the visible characters of a roff-encoded identifier or label.
pub(super) fn visible_text(source: &str) -> String {
    let mut projection = PlainTextProjection::new();
    for event in decode(source) {
        projection.append_event(event);
    }
    projection.finish()
}

struct Decoder {
    characters: Vec<char>,
    index: usize,
    events: Vec<RoffInlineEvent>,
    text: String,
}

impl Decoder {
    fn new(source: &str) -> Self {
        Self {
            characters: source.chars().collect(),
            index: 0,
            events: Vec::new(),
            text: String::with_capacity(source.len()),
        }
    }

    fn decode(mut self) -> Vec<RoffInlineEvent> {
        'input: while self.index < self.characters.len() {
            let character = self.characters[self.index];
            if character != '\\' {
                self.push_source_character(character);
                self.index += 1;
                continue;
            }

            self.index += 1;
            let Some(mut trigger) = self.take_character() else {
                self.text.push('\\');
                break;
            };
            // `\\E` is the copy-mode-safe escape character.  It makes the
            // next trigger behave exactly as if the copy had contained a
            // literal backslash.  Flatten it here instead of recursively
            // decoding nested copies: hostile input can contain an arbitrary
            // number of `\\E` prefixes, while each prefix consumes one byte.
            while trigger == 'E' {
                let Some(next) = self.take_character() else {
                    self.text.push('\\');
                    continue 'input;
                };
                trigger = next;
            }
            self.decode_escape(trigger);
        }
        self.flush_text();
        self.events
    }

    fn decode_escape(&mut self, trigger: char) {
        match trigger {
            'f' => {
                let operand = self.take_opaque_argument().unwrap_or_default();
                self.emit(if operand == "P" || operand.is_empty() {
                    RoffInlineEvent::PreviousFont
                } else {
                    RoffInlineEvent::Font(font(&operand))
                });
            }
            'm' | 'M' => {
                let argument = self.take_opaque_argument();
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::Color,
                    argument,
                });
            }
            's' => {
                let argument = self.take_size_argument();
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::PointSize,
                    argument,
                });
            }
            'X' => self.decode_postprocessor_escape(),
            '(' => {
                let name = self.take_counted(2);
                self.push_special_character(&name, NamedCharacterSyntax::TwoCharacter);
            }
            '[' => {
                let name = self.take_until(']');
                self.push_special_character(&name, NamedCharacterSyntax::Bracketed);
            }
            'C' => {
                let name = self.take_delimited_argument().unwrap_or_default();
                self.push_special_character(&name, NamedCharacterSyntax::CharacterDescriptor);
            }
            // CVS `roff_escape()` classifies these historical one-character
            // forms as named special characters, not undefined literals.
            // Route them through the pinned catalog just like `\(XX`: in
            // particular, `\'` is the catalog acute accent rather than an
            // ASCII apostrophe.  GNU groff likewise has dedicated escape
            // tokens for the left quote, right quote and underscore forms.
            '`' | '\'' | '_' => self
                .push_special_character(&trigger.to_string(), NamedCharacterSyntax::TwoCharacter),
            '-' => self.text.push('-'),
            'e' | '\\' => self.text.push('\\'),
            // These are formatter glyphs, not breakable source whitespace.
            // Keeping them as one glyph lets a pending `\\p` pass across the
            // displayed blank and break only at the next real word boundary.
            ' ' | '~' | '0' => self.emit(RoffInlineEvent::Glyph(" ".to_owned())),
            'p' => self.emit(RoffInlineEvent::LineBreak),
            // Opaque formatter state supported by mandoc_escape(3). These
            // operands must be consumed even though ManT does not render the
            // corresponding device state.
            'F' | 'g' | 'k' | 'n' | 'O' | 'V' | 'Y' => {
                let argument = self.take_opaque_argument();
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::FormatterState,
                    argument,
                });
            }
            '*' => {
                let argument = self.take_opaque_argument();
                if argument.as_deref() == Some(".T") {
                    // CVS roff_escape.c retains the special `\\*[.T]`
                    // device escape in text.  Its UTF-8 terminal renderer
                    // then emits `utf8` (term.c, ESCAPE_DEVICE).  ManT's
                    // renderer is likewise Unicode terminal text, so this is
                    // visible content rather than an opaque string request.
                    self.emit(RoffInlineEvent::DeviceName);
                } else {
                    self.emit(RoffInlineEvent::Presentation {
                        kind: PresentationKind::FormatterState,
                        argument,
                    });
                }
            }
            'o' => {
                let source = self.take_delimited_argument().unwrap_or_default();
                let terminal = overstrike_terminal_glyph(&source);
                self.emit(RoffInlineEvent::Overstrike { source, terminal });
            }
            'A' | 'b' | 'D' | 'R' | 'Z' => {
                let argument = self.take_delimited_argument();
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::Postprocessor,
                    argument,
                });
            }
            'h' => self.decode_horizontal_motion(),
            'H' | 'L' | 'l' | 'S' | 'v' | 'x' => {
                let argument = self.take_delimited_argument();
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::Motion,
                    argument,
                });
            }
            'N' => self.decode_numbered_glyph(),
            // `term_word()` carries TERMP_BACKAFTER/BACKBEFORE across calls:
            // `\\z` is therefore an execution state transition, not a local
            // character-skipping shortcut. Keep the following escapes in the
            // normal decoder so their operands and font effects survive.
            'z' => self.emit(RoffInlineEvent::ZeroAdvance),
            '%' if self.characters.get(self.index..self.index + 2) == Some(&['<', '>']) => {
                self.index += 2;
                self.emit(RoffInlineEvent::EmptyDestination);
            }
            // These requests affect formatter state or introduce zero-width
            // hints. Their trigger byte is never printable document content.
            '%' | '&' | ')' | ',' | '/' | '^' | 'a' | 'd' | 'r' | 't' | 'u' | '{' | '|' | '}' => {
                self.emit(RoffInlineEvent::ZeroWidthGlyph);
            }
            'c' => self.emit(RoffInlineEvent::NoSpace),
            '!' | '?' | ':' => {
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::Spacing,
                    argument: None,
                });
            }
            // An undefined escape prints its trigger without the backslash in
            // roff. Keeping that behavior preserves intentional literal text
            // while all known control families are handled above.
            other => push_terminal_safe(&mut self.text, other),
        }
    }

    fn decode_postprocessor_escape(&mut self) {
        let command = self.take_delimited_argument();
        match command.as_deref() {
            Some("tty: link") => self.emit(RoffInlineEvent::Link(None)),
            Some(command) => {
                if let Some(target) = command.strip_prefix("tty: link ") {
                    self.emit(RoffInlineEvent::Link(Some(target.to_owned())));
                } else {
                    self.emit(RoffInlineEvent::Presentation {
                        kind: PresentationKind::Postprocessor,
                        argument: Some(command.to_owned()),
                    });
                }
            }
            None => self.emit(RoffInlineEvent::Presentation {
                kind: PresentationKind::Postprocessor,
                argument: None,
            }),
        }
    }

    fn decode_numbered_glyph(&mut self) {
        let start = self.index;
        let delimiter = self.characters.get(start).copied();
        let argument = if self
            .characters
            .get(self.index)
            .is_some_and(char::is_ascii_digit)
        {
            Some(self.take_counted(1))
        } else {
            self.take_delimited_argument()
        };
        let closed = delimiter.is_some_and(|delimiter| {
            !delimiter.is_ascii_digit()
                && self.index > start + 1
                && self.characters.get(self.index - 1) == Some(&delimiter)
        });
        // mandoc's mchars_num2char accepts only the 8-bit terminal range.
        // Preserve unsupported spellings as a terminal fallback event so a
        // link-identity projection can still follow CVS HTML and omit them.
        if let Some(number) = argument
            .as_deref()
            .filter(|_| closed)
            .and_then(|value| value.parse::<u8>().ok())
        {
            let character = char::from(number);
            if character == ' ' {
                // Only an actual ASCII formatter blank can realize `\p`.
                self.text.push(character);
            } else {
                // term.c treats every other numbered character as one graph
                // for filling purposes. Its UTF-8 terminal replaces unsafe
                // C0/C1 controls rather than silently turning them into
                // breakable spaces.
                let character = if (character < ' ' && character != '\t')
                    || ('\u{7f}'..='\u{9f}').contains(&character)
                {
                    '\u{fffd}'
                } else {
                    character
                };
                self.emit(RoffInlineEvent::Glyph(character.to_string()));
            }
        } else {
            let mut value = String::from(r"\N");
            for character in &self.characters[start..self.index] {
                push_terminal_safe(&mut value, *character);
            }
            self.emit(RoffInlineEvent::FallbackGlyph(value));
        }
    }

    fn decode_horizontal_motion(&mut self) {
        let argument = self.take_delimited_argument();
        if argument.as_deref().is_some_and(is_positive_literal_motion) {
            // ManT does not reproduce formatter geometry, but an explicit
            // positive advance is still a semantic word boundary. Retaining
            // one space matters when `\c` suppresses the input-line break.
            self.text.push(' ');
        }
        self.emit(RoffInlineEvent::Presentation {
            kind: PresentationKind::HorizontalMotion,
            argument,
        });
    }

    fn push_special_character(&mut self, name: &str, syntax: NamedCharacterSyntax) {
        if let Some(value) = dedicated_special_character(name) {
            self.emit(RoffInlineEvent::Glyph(value.to_owned()));
            return;
        }
        if let Some(value) = documented_groff_composite_character(name) {
            self.emit(RoffInlineEvent::Glyph(value));
            return;
        }
        if let Some(value) = unicode_special_characters(name) {
            let mut glyph = String::new();
            for character in value.chars() {
                push_terminal_safe(&mut glyph, character);
            }
            self.emit(RoffInlineEvent::Glyph(glyph));
            return;
        }
        match libmandoc_rs::special_character(name) {
            Some(SpecialCharacter::Visible(character)) => {
                let mut glyph = String::new();
                push_terminal_safe(&mut glyph, character);
                self.emit(RoffInlineEvent::Glyph(glyph));
            }
            Some(SpecialCharacter::ZeroWidth) => self.emit(RoffInlineEvent::ZeroWidthGlyph),
            None => self.push_unknown_special_character(name, syntax),
        }
    }

    fn push_unknown_special_character(&mut self, name: &str, syntax: NamedCharacterSyntax) {
        let (prefix, suffix) = match syntax {
            NamedCharacterSyntax::TwoCharacter => (r"\(", ""),
            NamedCharacterSyntax::Bracketed => (r"\[", "]"),
            NamedCharacterSyntax::CharacterDescriptor => (r"\C'", "'"),
        };
        let mut value = String::from(prefix);
        for character in name.chars() {
            push_terminal_safe(&mut value, character);
        }
        value.push_str(suffix);
        self.emit(RoffInlineEvent::FallbackGlyph(value));
    }

    fn push_source_character(&mut self, character: char) {
        match character {
            ASCII_BREAK => {}
            ASCII_HYPH => self.text.push('-'),
            ASCII_NBRSP => self.emit(RoffInlineEvent::Glyph(" ".to_owned())),
            other => push_terminal_safe(&mut self.text, other),
        }
    }

    fn emit(&mut self, event: RoffInlineEvent) {
        self.flush_text();
        self.events.push(event);
    }

    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            self.events
                .push(RoffInlineEvent::Text(std::mem::take(&mut self.text)));
        }
    }

    fn take_character(&mut self) -> Option<char> {
        let character = self.characters.get(self.index).copied()?;
        self.index += 1;
        Some(character)
    }

    fn take_opaque_argument(&mut self) -> Option<String> {
        match self.characters.get(self.index).copied()? {
            '[' => {
                self.index += 1;
                Some(self.take_until(']'))
            }
            '(' => {
                self.index += 1;
                Some(self.take_counted(2))
            }
            _ => self.take_character().map(|character| character.to_string()),
        }
    }

    fn take_size_argument(&mut self) -> Option<String> {
        let mut value = String::new();
        let mut has_sign = false;
        if matches!(
            self.characters.get(self.index),
            Some('+' | '-' | &ASCII_HYPH)
        ) {
            has_sign = true;
            value.push(self.take_character()?);
        }

        let first = self.characters.get(self.index).copied()?;
        match first {
            '[' => {
                self.index += 1;
                value.push_str(&self.take_until(']'));
            }
            '(' => {
                self.index += 1;
                value.push_str(&self.take_counted(2));
            }
            '\'' => {
                value.push_str(&self.take_delimited_argument().unwrap_or_default());
            }
            '1' | '2' | '3'
                if !has_sign
                    && self
                        .characters
                        .get(self.index + 1)
                        .is_some_and(char::is_ascii_digit) =>
            {
                value.push_str(&self.take_counted(2));
            }
            _ => value.push(self.take_character()?),
        }
        Some(value)
    }

    fn take_delimited_argument(&mut self) -> Option<String> {
        let delimiter = self.take_character()?;
        Some(self.take_until(delimiter))
    }

    fn take_until(&mut self, delimiter: char) -> String {
        let start = self.index;
        while self.index < self.characters.len() && self.characters[self.index] != delimiter {
            if self.characters[self.index] == '\\' {
                self.index = scan_nested_escape_end(&self.characters, self.index);
            } else {
                self.index += 1;
            }
        }
        let value = self.characters[start..self.index].iter().collect();
        self.index += usize::from(self.index < self.characters.len());
        value
    }

    fn take_counted(&mut self, count: usize) -> String {
        let end = (self.index + count).min(self.characters.len());
        let value = self.characters[self.index..end].iter().collect();
        self.index = end;
        value
    }
}

#[derive(Clone, Copy)]
enum EscapeScanTask {
    Escape,
    Until(char),
    Counted(usize),
    Opaque,
    Size,
    Delimited,
}

/// Return the end of one complete nested escape without recursive descent.
///
/// CVS `roff_escape()` recursively parses nested escapes before testing an
/// outer delimiter. We preserve that grammar but use a bounded heap stack:
/// one valid input text node must never be able to overflow the Rust call
/// stack. If the shared native nesting budget is exceeded, consuming the
/// remainder is conservative because no later delimiter can be proven to
/// belong to the outer request.
fn scan_nested_escape_end(characters: &[char], start: usize) -> usize {
    let mut index = start;
    let mut tasks = vec![EscapeScanTask::Escape];
    while let Some(task) = tasks.pop() {
        if !advance_escape_scan_task(characters, &mut index, task, &mut tasks)
            || tasks.len() > MAX_NESTED_ESCAPE_SCAN_DEPTH
        {
            return characters.len();
        }
    }
    index
}

/// Execute one iterative equivalent of a recursive `roff_escape()` frame.
/// Returning false means the shared nesting budget was exhausted.
fn advance_escape_scan_task(
    characters: &[char],
    index: &mut usize,
    task: EscapeScanTask,
    tasks: &mut Vec<EscapeScanTask>,
) -> bool {
    match task {
        EscapeScanTask::Escape => {
            if characters.get(*index) != Some(&'\\') {
                return true;
            }
            *index += 1;
            while characters.get(*index) == Some(&'E') {
                *index += 1;
            }
            let Some(trigger) = characters.get(*index).copied() else {
                return true;
            };
            *index += 1;
            schedule_escape_argument(characters, index, trigger, tasks);
        }
        EscapeScanTask::Until(delimiter) => loop {
            match characters.get(*index).copied() {
                None => break,
                Some(character) if character == delimiter => {
                    *index += 1;
                    break;
                }
                Some('\\') => {
                    if tasks.len() + 2 > MAX_NESTED_ESCAPE_SCAN_DEPTH {
                        return false;
                    }
                    tasks.push(EscapeScanTask::Until(delimiter));
                    tasks.push(EscapeScanTask::Escape);
                    break;
                }
                Some(_) => *index += 1,
            }
        },
        EscapeScanTask::Counted(mut remaining) => {
            while remaining > 0 {
                match characters.get(*index).copied() {
                    None => break,
                    Some('\\') => {
                        if tasks.len() + 2 > MAX_NESTED_ESCAPE_SCAN_DEPTH {
                            return false;
                        }
                        tasks.push(EscapeScanTask::Counted(remaining));
                        tasks.push(EscapeScanTask::Escape);
                        break;
                    }
                    Some(_) => {
                        *index += 1;
                        remaining -= 1;
                    }
                }
            }
        }
        EscapeScanTask::Opaque => match characters.get(*index).copied() {
            Some('\\') => tasks.push(EscapeScanTask::Escape),
            Some('[') => {
                *index += 1;
                tasks.push(EscapeScanTask::Until(']'));
            }
            Some('(') => {
                *index += 1;
                tasks.push(EscapeScanTask::Counted(2));
            }
            Some(_) => *index += 1,
            None => {}
        },
        EscapeScanTask::Size => {
            let has_sign = matches!(characters.get(*index), Some('+' | '-' | &ASCII_HYPH));
            if has_sign {
                *index += 1;
            }
            match characters.get(*index).copied() {
                Some('\\') => tasks.push(EscapeScanTask::Escape),
                Some('[') => {
                    *index += 1;
                    tasks.push(EscapeScanTask::Until(']'));
                }
                Some('(') => {
                    *index += 1;
                    tasks.push(EscapeScanTask::Counted(2));
                }
                Some('\'') => tasks.push(EscapeScanTask::Delimited),
                Some('1' | '2' | '3')
                    if !has_sign
                        && characters.get(*index + 1).is_some_and(char::is_ascii_digit) =>
                {
                    *index = (*index + 2).min(characters.len());
                }
                Some(_) => *index += 1,
                None => {}
            }
        }
        EscapeScanTask::Delimited => {
            let Some(delimiter) = characters.get(*index).copied() else {
                return true;
            };
            *index += 1;
            tasks.push(EscapeScanTask::Until(delimiter));
        }
    }
    true
}

fn schedule_escape_argument(
    characters: &[char],
    index: &mut usize,
    trigger: char,
    tasks: &mut Vec<EscapeScanTask>,
) {
    match trigger {
        '(' => tasks.push(EscapeScanTask::Counted(2)),
        '[' => tasks.push(EscapeScanTask::Until(']')),
        '$' | '*' | 'F' | 'M' | 'O' | 'V' | 'Y' | 'f' | 'g' | 'k' | 'm' | 'n' => {
            tasks.push(EscapeScanTask::Opaque);
        }
        's' => tasks.push(EscapeScanTask::Size),
        'N' if characters.get(*index).is_some_and(char::is_ascii_digit) => {
            *index = (*index + 1).min(characters.len());
        }
        'A' | 'B' | 'C' | 'D' | 'H' | 'L' | 'N' | 'R' | 'S' | 'X' | 'Z' | 'b' | 'h' | 'l' | 'o'
        | 'v' | 'w' | 'x' => tasks.push(EscapeScanTask::Delimited),
        _ => {}
    }
}

/// Decode groff's bracketed Unicode character names.
///
/// libmandoc's input pre-converter represents raw UTF-8 with the same
/// `uXXXX` names, so this one boundary handles both explicit `\[uXXXX]`
/// escapes and ordinary non-ASCII source text. Composite names use one base
/// scalar followed by underscore-separated combining scalars.
/// Recognize a positive literal relative advance without attempting to
/// evaluate roff expressions or absolute (`|`) positions.
///
/// A single visible space is a safe text-mode approximation for forms such as
/// `+01`, `1n`, and `.5m`.  Negative, zero, register-based, and compound
/// expressions remain presentation-only because guessing their evaluated sign
/// could create text that the formatter never displayed.
fn is_positive_literal_motion(argument: &str) -> bool {
    let argument = argument.trim();
    let argument = argument.strip_prefix('+').unwrap_or(argument);
    if argument.is_empty() || argument.starts_with(['-', '|', '\\']) {
        return false;
    }

    let mut saw_digit = false;
    let mut saw_nonzero = false;
    let mut saw_decimal = false;
    let mut end = 0;
    for (index, character) in argument.char_indices() {
        match character {
            '0'..='9' => {
                saw_digit = true;
                saw_nonzero |= character != '0';
                end = index + character.len_utf8();
            }
            '.' if !saw_decimal => {
                saw_decimal = true;
                end = index + 1;
            }
            _ => break,
        }
    }
    if !saw_digit || !saw_nonzero {
        return false;
    }

    let suffix = &argument[end..];
    suffix.is_empty()
        || (suffix.len() == 1
            && suffix
                .chars()
                .all(|character| character.is_ascii_alphabetic()))
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NamedCharacterSyntax {
    TwoCharacter,
    Bracketed,
    CharacterDescriptor,
}

#[cfg(test)]
mod tests;
