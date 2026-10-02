//! Tokenizes formatter-level roff escapes before semantic AST lowering.
//!
//! libmandoc intentionally retains several GNU roff extensions inside text
//! nodes. This module is the sole boundary allowed to interpret those bytes:
//! consumers receive typed events and can never mistake an escape operand for
//! visible document text.

mod glyphs;
mod grammar;
mod projection;
mod scanner;
use glyphs::{
    dedicated_special_character, documented_groff_composite_character, unicode_special_characters,
};
pub(super) use projection::visible_text;
use scanner::{ArgumentCompletion, QuotedOutcome, scan_argument, scan_quoted_argument};

use crate::text_safety::push_terminal_safe;
use libmandoc_rs::SpecialCharacter;

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
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ZeroAdvanceMachine<T> {
    armed: bool,
    pending: Option<T>,
    recoveries: Vec<T>,
    recoveries_before_pending: usize,
}

impl<T> ZeroAdvanceMachine<T> {
    pub(super) const fn new() -> Self {
        Self {
            armed: false,
            pending: None,
            recoveries: Vec::new(),
            recoveries_before_pending: 0,
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

    pub(super) const fn pending_ref(&self) -> Option<&T> {
        self.pending.as_ref()
    }

    /// Change projection metadata without consuming either backtracking flag.
    pub(super) fn pending_mut(&mut self) -> Option<&mut T> {
        self.pending.as_mut()
    }

    pub(super) fn cancel_armed(&mut self) -> bool {
        std::mem::take(&mut self.armed)
    }

    pub(super) fn project_glyph(&mut self, glyph: T) -> Option<(T, bool)> {
        if self.pending.is_some() {
            self.recoveries_before_pending = self.recoveries.len();
        }
        if self.armed {
            self.armed = false;
            self.pending = Some(glyph);
            return None;
        }
        let replaced_pending = self.pending.take().is_some();
        Some((glyph, replaced_pending))
    }

    /// Unknown SPECIAL/invalid NUMBERED uses `bufferc(ASCII_NBRZW)`, not
    /// `encode1()`: its recovery spelling cannot consume either backtracking
    /// flag (term.c:610-638). Defer only spelling following a pending glyph,
    /// so source order remains stable when that native glyph later survives
    /// or is overstruck. The queue contains this unresolved suffix only.
    pub(super) fn project_fallback(&mut self, glyph: T) -> Option<(T, bool)> {
        if self.armed {
            return None;
        }
        if self.pending.is_some() {
            self.recoveries.push(glyph);
            return None;
        }
        Some((glyph, false))
    }

    pub(super) fn take_recoveries(&mut self) -> Vec<T> {
        self.recoveries_before_pending = 0;
        std::mem::take(&mut self.recoveries)
    }

    pub(super) fn resolve_word_boundary(&mut self) -> Option<Vec<T>> {
        (!self.armed).then(|| self.take_pending()).flatten()
    }

    pub(super) fn take_pending(&mut self) -> Option<Vec<T>> {
        let glyph = self.pending.take()?;
        let position = self.recoveries_before_pending;
        let mut output = self.take_recoveries();
        output.insert(position, glyph);
        Some(output)
    }

    pub(super) fn discard_pending(&mut self) {
        self.pending = None;
        self.recoveries.clear();
        self.recoveries_before_pending = 0;
    }

    pub(super) fn clear(&mut self) {
        self.armed = false;
        self.discard_pending();
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
        RoffInlineEvent::DeviceName | RoffInlineEvent::BreakableHyphen => {
            InlineEventEffect::Visible
        }
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
    Decoder::new(source).decode_with_status()
}

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
            RoffInlineEvent::BreakableHyphen => {
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

/// One quoted escape argument, classified the way fixed CVS `roff_escape()`
/// ends it.
enum DelimitedArgument {
    /// Ended at a real closing delimiter, or at `\N`'s first non-digit.
    Closed(String),
    /// Input ended first; only `\A`, `\o` and `\w` keep their payload.
    Unclosed(Option<String>),
    /// The delimiter itself was rejected (`ESCAPE_DELIM)`: the owning family
    /// renders nothing and the argument ends right after it.
    Rejected,
    /// No delimiter was present at all.
    Missing,
}

struct Decoder {
    measurement: bool,
    budget_exhausted: bool,
    argument_completion: ArgumentCompletion,
    argument_kind: grammar::EscapeKind,
    characters: Vec<char>,
    index: usize,
    events: Vec<RoffInlineEvent>,
    text: String,
}

impl Decoder {
    fn new(source: &str) -> Self {
        Self {
            measurement: false,
            budget_exhausted: false,
            argument_completion: ArgumentCompletion::Complete,
            argument_kind: grammar::EscapeKind::Undefined,
            characters: source.chars().collect(),
            index: 0,
            events: Vec::new(),
            text: String::with_capacity(source.len()),
        }
    }

    fn decode(self) -> Vec<RoffInlineEvent> {
        self.decode_with_status().events
    }

    fn decode_with_status(mut self) -> DecodedText {
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
        DecodedText {
            events: self.events,
            budget_exhausted: self.budget_exhausted,
        }
    }

    fn decode_escape(&mut self, trigger: char) {
        self.argument_completion = ArgumentCompletion::Complete;
        self.argument_kind = grammar::syntax(trigger).kind;
        match trigger {
            'f' => self.decode_font(),
            'm' | 'M' => {
                let argument = self.take_argument(trigger);
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::Color,
                    argument,
                });
            }
            's' => {
                let argument = self.take_argument('s');
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::PointSize,
                    argument,
                });
            }
            'X' => self.decode_postprocessor_escape(),
            '(' | '[' => self.decode_named_character(trigger),
            'C' => self.decode_character_descriptor(),
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
            ' ' | '~' | '0' => self.emit(RoffInlineEvent::Glyph("\u{a0}".to_owned())),
            'p' => self.emit(RoffInlineEvent::LineBreak),
            // Opaque formatter state supported by mandoc_escape(3). These
            // operands must be consumed even though ManT does not render the
            // corresponding device state.
            'F' | 'g' | 'k' | 'n' | 'O' | 'V' | 'Y' => {
                let argument = self.take_argument(trigger);
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::FormatterState,
                    argument,
                });
            }
            '*' => {
                let argument = self.take_argument(trigger);
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
                // An unclosed overstrike keeps its scanned payload (the CVS
                // "Aow" families); rejected delimiters cannot occur for
                // `\o`, which never appears in the rejection sets.
                let source = match self.take_delimited_argument('o') {
                    DelimitedArgument::Closed(source)
                    | DelimitedArgument::Unclosed(Some(source)) => source,
                    DelimitedArgument::Unclosed(None)
                    | DelimitedArgument::Rejected
                    | DelimitedArgument::Missing => String::new(),
                };
                let terminal = overstrike_terminal_glyph(&source);
                self.emit(RoffInlineEvent::Overstrike { source, terminal });
            }
            'A' | 'b' | 'D' | 'R' | 'Z' => {
                let argument = self.take_presentation_argument(trigger);
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::Postprocessor,
                    argument,
                });
            }
            'h' => self.decode_horizontal_motion(),
            'H' | 'L' | 'l' | 'S' | 'v' | 'x' => {
                let argument = self.take_presentation_argument(trigger);
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
            // hints, and `\:` buffers ASCII_NBRZW on this UTF-8 device
            // (chars.c:53 unicode column 0; term.c:631-632): a zero-width
            // graph, never a break point. The ASCII device's ASCII_BREAK
            // byte (and thus FieldCell::Breakpoint) has no producer while
            // mant is single-device UTF-8. None of these triggers is
            // printable document content.
            '%' | '&' | ')' | ',' | '/' | ':' | '^' | 'a' | 'd' | 't' | 'u' | '{' | '|' | '}' => {
                self.emit(RoffInlineEvent::ZeroWidthGlyph);
            }
            'c' => self.emit(RoffInlineEvent::NoSpace),
            // `\!`, `\?`, `\r` are ESCAPE_UNSUPP (roff_escape.c:156-160):
            // term_word()'s default arm continues without any buffer
            // footprint on both devices (term.c:800-801).
            '!' | '?' | 'r' => {}
            // An undefined escape prints its trigger without the backslash in
            // roff. Keeping that behavior preserves intentional literal text
            // while all known control families are handled above.
            other => push_terminal_safe(&mut self.text, other),
        }
        // term.c::term_word() writes ASCII_NBRZW for a successfully parsed
        // IGNORE escape, including opaque controls. It is a real zero-width
        // buffer cell: .br can close its line and retire an armed \z.
        let syntax = grammar::syntax(trigger);
        if self.argument_kind == grammar::EscapeKind::Ignore
            && syntax.argument != grammar::ArgumentShape::None
        {
            self.emit(RoffInlineEvent::ZeroWidthGlyph);
        }
    }

    fn decode_font(&mut self) {
        let operand = self.take_argument('f').unwrap_or_default();
        // Only mandoc_font's complete valid results execute term_fontrepl/
        // last. Missing/incomplete/unknown operands are ERROR, not Roman or
        // Previous; the shared scanner still delivers their exact extent.
        if self.argument_kind == grammar::EscapeKind::Font {
            self.emit(if operand == "P" || operand.is_empty() {
                RoffInlineEvent::PreviousFont
            } else {
                RoffInlineEvent::Font(font(&operand))
            });
        }
    }

    fn decode_named_character(&mut self, trigger: char) {
        let start = self.index;
        let name = self.take_argument(trigger).unwrap_or_default();
        if trigger == '[' && self.argument_completion == ArgumentCompletion::Rejected {
            // Retain the established SPECIAL source fallback, but only for
            // bytes actually consumed by ESC_ARG. Its suffix is ordinary
            // text and is not part of the fallback glyph.
            let spelling = format!("\\[{}", self.range_string(start..self.index));
            self.emit(RoffInlineEvent::FallbackGlyph(spelling));
        } else {
            let syntax = if trigger == '[' {
                NamedCharacterSyntax::Bracketed
            } else {
                NamedCharacterSyntax::TwoCharacter
            };
            self.push_special_character(&name, syntax);
        }
    }

    fn decode_postprocessor_escape(&mut self) {
        let command = self.take_presentation_argument('X');
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

    /// `\C` names one character with its quoted argument.  Only a closed,
    /// non-empty descriptor reaches the catalog: CVS renders unclosed,
    /// empty and rejected `\C` arguments as `ESCAPE_ERROR` with no buffer
    /// footprint.
    fn decode_character_descriptor(&mut self) {
        if let DelimitedArgument::Closed(name) = self.take_delimited_argument('C')
            && !name.is_empty()
        {
            self.push_special_character(&name, NamedCharacterSyntax::CharacterDescriptor);
        }
    }
    fn decode_numbered_glyph(&mut self) {
        let start = self.index;
        let (argument, closed) = if self
            .characters
            .get(self.index)
            .is_some_and(char::is_ascii_digit)
        {
            // The digit form has no delimiter and CVS rejects it outright;
            // ManT instead retains the spelling visibly, like every other
            // unaccepted numbered form below.
            (self.take_argument('N'), false)
        } else {
            match self.take_delimited_argument('N') {
                DelimitedArgument::Closed(argument) => (Some(argument), true),
                // Unclosed and rejected delimiters are ESCAPE_ERROR (or a
                // dropped payload) upstream and render nothing; ManT's
                // established recovery design keeps the consumed spelling
                // visible instead.
                DelimitedArgument::Unclosed(argument) => (argument, false),
                DelimitedArgument::Rejected | DelimitedArgument::Missing => (None, false),
            }
        };
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
        // A positive literal advance only exists for a closed argument with
        // a kept payload: unclosed and rejected motion renders nothing.
        let argument = self.take_presentation_argument('h');
        if !self.measurement && argument.as_deref().is_some_and(is_positive_literal_motion) {
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
        // term_strlen uses the pinned catalog and a single ESCAPE_UNICODE
        // scalar. GNU fallback/composite display spellings are not native
        // declaration glyphs and must not create a device width or consume z.
        if self.measurement && (name.contains('_') || unicode_special_characters(name).is_none()) {
            self.push_native_special_character(name, syntax);
            return;
        }
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
        self.push_native_special_character(name, syntax);
    }

    fn push_native_special_character(&mut self, name: &str, syntax: NamedCharacterSyntax) {
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
            ASCII_TABREF | ASCII_BREAK | ASCII_NBRZW => {}
            ASCII_HYPH => self.emit(RoffInlineEvent::BreakableHyphen),
            ASCII_NBRSP => self.emit(RoffInlineEvent::Glyph("\u{a0}".to_owned())),
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

    fn take_argument(&mut self, trigger: char) -> Option<String> {
        let argument = scan_argument(&self.characters, self.index, trigger);
        self.index = argument.end;
        self.argument_completion = argument.completion;
        self.argument_kind = argument.kind;
        self.budget_exhausted |= argument.completion == ArgumentCompletion::BudgetExceeded;
        argument.payload.map(|range| {
            let mut value = self.range_string(argument.prefix);
            value.push_str(&self.range_string(range));
            value
        })
    }

    /// Take the quoted argument of `outer` with the shared bounded scanner,
    /// applying the fixed CVS delimiter rules: escaped delimiters, literal
    /// closers, `\N`'s digit rule, and rejected delimiters.
    fn take_delimited_argument(&mut self, outer: char) -> DelimitedArgument {
        let (outcome, end, budget_exhausted) =
            scan_quoted_argument(&self.characters, self.index, outer);
        self.index = end;
        self.budget_exhausted |= budget_exhausted;
        self.argument_completion = if budget_exhausted {
            ArgumentCompletion::BudgetExceeded
        } else {
            match &outcome {
                QuotedOutcome::Closed { .. } => ArgumentCompletion::Complete,
                QuotedOutcome::Rejected { .. } => ArgumentCompletion::Rejected,
                QuotedOutcome::Missing => ArgumentCompletion::Missing,
                QuotedOutcome::Unclosed { .. } => ArgumentCompletion::Incomplete,
            }
        };
        self.argument_kind = match &outcome {
            QuotedOutcome::Rejected { kind } if !budget_exhausted => *kind,
            _ => scanner::completion_kind(grammar::syntax(outer).kind, self.argument_completion),
        };
        match outcome {
            QuotedOutcome::Closed { payload } => {
                DelimitedArgument::Closed(self.range_string(payload))
            }
            QuotedOutcome::Unclosed { payload } => {
                DelimitedArgument::Unclosed(payload.map(|range| self.range_string(range)))
            }
            QuotedOutcome::Rejected { .. } => DelimitedArgument::Rejected,
            QuotedOutcome::Missing => DelimitedArgument::Missing,
        }
    }

    /// Payload a presentation-only family still reports: closed arguments
    /// plus the `Aow` unclosed survivors.
    fn take_presentation_argument(&mut self, outer: char) -> Option<String> {
        match self.take_delimited_argument(outer) {
            DelimitedArgument::Closed(argument) | DelimitedArgument::Unclosed(Some(argument)) => {
                Some(argument)
            }
            DelimitedArgument::Unclosed(None)
            | DelimitedArgument::Rejected
            | DelimitedArgument::Missing => None,
        }
    }

    fn range_string(&self, range: std::ops::Range<usize>) -> String {
        self.characters[range].iter().collect()
    }
}

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
mod delimiter_scanning;
#[cfg(test)]
mod rule_matrix;
#[cfg(test)]
mod tests;
