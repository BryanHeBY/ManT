//! Tokenizes formatter-level roff escapes before semantic AST lowering.
//!
//! libmandoc intentionally retains several GNU roff extensions inside text
//! nodes. This module is the sole boundary allowed to interpret those bytes:
//! consumers receive typed events and can never mistake an escape operand for
//! visible document text.

use crate::text_safety::push_terminal_safe;
use libmandoc_rs::SpecialCharacter;

const ASCII_BREAK: char = '\u{1d}';
const ASCII_HYPH: char = '\u{1e}';
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
    Font(RoffFont),
    Link(Option<String>),
    /// Exact legacy Sphinx `\%<>` output. It is invisible only when the
    /// preceding visible text proves that it belongs to a manual reference.
    EmptyDestination,
    LineBreak,
    Presentation {
        kind: PresentationKind,
        argument: Option<String>,
    },
}

/// Decode one libmandoc text node into typed, renderer-independent events.
pub(super) fn decode(source: &str) -> Vec<RoffInlineEvent> {
    Decoder::new(source).decode()
}

/// Return only the visible characters of a roff-encoded identifier or label.
pub(super) fn visible_text(source: &str) -> String {
    let mut output = String::new();
    for event in decode(source) {
        match event {
            RoffInlineEvent::Text(value) => output.push_str(&value),
            RoffInlineEvent::EmptyDestination => output.push_str("<>"),
            RoffInlineEvent::LineBreak => output.push('\n'),
            RoffInlineEvent::Font(_)
            | RoffInlineEvent::Link(_)
            | RoffInlineEvent::Presentation { .. } => {}
        }
    }
    output
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
        while self.index < self.characters.len() {
            let character = self.characters[self.index];
            if character != '\\' {
                self.push_source_character(character);
                self.index += 1;
                continue;
            }

            self.index += 1;
            let Some(trigger) = self.take_character() else {
                self.text.push('\\');
                break;
            };
            self.decode_escape(trigger);
        }
        self.flush_text();
        self.events
    }

    fn decode_escape(&mut self, trigger: char) {
        match trigger {
            'f' => {
                let operand = self.take_opaque_argument().unwrap_or_default();
                self.emit(RoffInlineEvent::Font(font(&operand)));
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
            '-' => self.text.push('-'),
            'e' | '\\' => self.text.push('\\'),
            // `\E` is a copy-mode-safe escape character. Once it reaches a
            // parsed text node, interpret the following trigger exactly as a
            // normal backslash would.
            'E' => {
                if let Some(nested_trigger) = self.take_character() {
                    self.decode_escape(nested_trigger);
                } else {
                    self.text.push('\\');
                }
            }
            ' ' | '~' | '0' => self.text.push(' '),
            'p' => self.emit(RoffInlineEvent::LineBreak),
            // Opaque formatter state supported by mandoc_escape(3). These
            // operands must be consumed even though ManT does not render the
            // corresponding device state.
            'F' | 'g' | 'k' | 'n' | 'O' | 'V' | 'Y' | '*' => {
                let argument = self.take_opaque_argument();
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::FormatterState,
                    argument,
                });
            }
            'A' | 'b' | 'D' | 'R' | 'Z' | 'o' => {
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
            'N' => {
                let argument = if self
                    .characters
                    .get(self.index)
                    .is_some_and(char::is_ascii_digit)
                {
                    Some(self.take_counted(1))
                } else {
                    self.take_delimited_argument()
                };
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::FormatterState,
                    argument,
                });
            }
            'z' => {
                let argument = self.take_character().map(|character| character.to_string());
                self.emit(RoffInlineEvent::Presentation {
                    kind: PresentationKind::Spacing,
                    argument,
                });
            }
            '%' if self.characters.get(self.index..self.index + 2) == Some(&['<', '>']) => {
                self.index += 2;
                self.emit(RoffInlineEvent::EmptyDestination);
            }
            // These requests affect formatter state or introduce zero-width
            // hints. Their trigger byte is never printable document content.
            '!' | '?' | '%' | '&' | ')' | ',' | '/' | '^' | ':' | 'a' | 'c' | 'd' | 'r' | 't'
            | 'u' | '{' | '|' | '}' => {
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
            self.text.push_str(value);
            return;
        }
        match libmandoc_rs::special_character(name) {
            Some(SpecialCharacter::Visible(character)) => {
                push_terminal_safe(&mut self.text, character);
            }
            Some(SpecialCharacter::ZeroWidth) => {}
            None => self.push_unknown_special_character(name, syntax),
        }
    }

    fn push_unknown_special_character(&mut self, name: &str, syntax: NamedCharacterSyntax) {
        let (prefix, suffix) = match syntax {
            NamedCharacterSyntax::TwoCharacter => (r"\(", ""),
            NamedCharacterSyntax::Bracketed => (r"\[", "]"),
            NamedCharacterSyntax::CharacterDescriptor => (r"\C'", "'"),
        };
        self.text.push_str(prefix);
        for character in name.chars() {
            push_terminal_safe(&mut self.text, character);
        }
        self.text.push_str(suffix);
    }

    fn push_source_character(&mut self, character: char) {
        match character {
            ASCII_BREAK => {}
            ASCII_HYPH => self.text.push('-'),
            ASCII_NBRSP => self.text.push(' '),
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
        if matches!(
            self.characters.get(self.index),
            Some('+' | '-' | &ASCII_HYPH)
        ) {
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
                if self
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
            if self.characters[self.index] == '\\' && self.index + 1 < self.characters.len() {
                self.index += 2;
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

fn font(name: &str) -> RoffFont {
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

/// Compatibility folds intentionally chosen by `ManT`. Every other known name
/// comes from the complete catalog pinned by `libmandoc-rs`.
fn dedicated_special_character(name: &str) -> Option<&'static str> {
    match name {
        "en" => Some("–"),
        "em" => Some("—"),
        "aq" | "cq" | "oq" => Some("'"),
        "dq" | "lq" | "rq" => Some("\""),
        "co" => Some("©"),
        "rg" => Some("®"),
        "tm" => Some("™"),
        "bu" => Some("•"),
        "ha" => Some("^"),
        "ti" => Some("~"),
        "rs" => Some("\\"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ASCII_BREAK, ASCII_HYPH, ASCII_NBRSP, PresentationKind, RoffFont, RoffInlineEvent, decode,
        visible_text,
    };

    #[test]
    fn emits_text_font_and_renderer_link_events() {
        assert_eq!(
            decode(r"\X'tty: link https://example.test'\fB\-h\fR\X'tty: link' FILE"),
            vec![
                RoffInlineEvent::Link(Some("https://example.test".to_owned())),
                RoffInlineEvent::Font(RoffFont::Strong),
                RoffInlineEvent::Text("-h".to_owned()),
                RoffInlineEvent::Font(RoffFont::Regular),
                RoffInlineEvent::Link(None),
                RoffInlineEvent::Text(" FILE".to_owned()),
            ]
        );
    }

    #[test]
    fn recognizes_constant_width_and_pandoc_verbatim_font_families() {
        assert_eq!(
            decode(r"\f[C]code\f[V]verbatim\f[VB]bold\f[VI]italic\f[R]"),
            vec![
                RoffInlineEvent::Font(RoffFont::Code),
                RoffInlineEvent::Text("code".to_owned()),
                RoffInlineEvent::Font(RoffFont::Code),
                RoffInlineEvent::Text("verbatim".to_owned()),
                RoffInlineEvent::Font(RoffFont::CodeStrong),
                RoffInlineEvent::Text("bold".to_owned()),
                RoffInlineEvent::Font(RoffFont::CodeEmphasis),
                RoffInlineEvent::Text("italic".to_owned()),
                RoffInlineEvent::Font(RoffFont::Regular),
            ]
        );
    }

    #[test]
    fn consumes_every_supported_argument_shape_as_typed_presentation_state() {
        let events = decode(r"\mX\m(bl\m[blue]\s2\s-2\s(12\s[+12]\s'+3'");
        let presentations = events
            .into_iter()
            .filter_map(|event| match event {
                RoffInlineEvent::Presentation { kind, argument } => Some((kind, argument)),
                _ => None,
            })
            .collect::<Vec<_>>();

        assert_eq!(presentations.len(), 8);
        assert_eq!(presentations[0].1.as_deref(), Some("X"));
        assert_eq!(presentations[1].1.as_deref(), Some("bl"));
        assert_eq!(presentations[2].1.as_deref(), Some("blue"));
        assert!(
            presentations[..3]
                .iter()
                .all(|(kind, _)| *kind == PresentationKind::Color)
        );
        assert_eq!(
            presentations[3..]
                .iter()
                .map(|(_, argument)| argument.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("2"), Some("-2"), Some("12"), Some("+12"), Some("+3")]
        );
    }

    #[test]
    fn normalizes_internal_markers_and_known_zero_width_controls() {
        let source = format!("git{ASCII_HYPH}config{ASCII_NBRSP}(1){ASCII_BREAK}next\\&.\\|.\\|.");

        assert_eq!(visible_text(&source), "git-config (1)next...");
    }

    #[test]
    fn preserves_literal_positive_horizontal_motion_as_a_word_boundary() {
        assert_eq!(visible_text(r"1.\h'+01'\c"), "1. ");
        assert_eq!(visible_text(r"a\h'1n'b"), "a b");
        assert_eq!(visible_text(r"a\h'.5m'b"), "a b");
        assert_eq!(visible_text(r"a\h'-04'b"), "ab");
        assert_eq!(visible_text(r"a\h'+0'b"), "ab");
        assert_eq!(visible_text(r"a\h'|1i'b"), "ab");
        assert_eq!(visible_text(r"a\h'\n[x]'b"), "ab");
    }

    #[test]
    fn retains_legacy_sphinx_empty_destinations_as_typed_evidence() {
        let source = r"btrfs-subvolume(8) \%<>";

        assert_eq!(
            decode(source),
            vec![
                RoffInlineEvent::Text("btrfs-subvolume(8) ".to_owned()),
                RoffInlineEvent::EmptyDestination,
            ]
        );
        assert_eq!(visible_text(source), "btrfs-subvolume(8) <>");
        assert_eq!(visible_text(r"literal \%value"), "literal value");
    }

    #[test]
    fn preserves_roff_reverse_solidus_characters_in_windows_paths() {
        assert_eq!(
            visible_text(r"C:\[rs]path\[rs]file \[rs]\[rs]server\[rs]share"),
            r"C:\path\file \\server\share",
        );
    }

    #[test]
    fn resolves_named_characters_from_the_pinned_mandoc_catalog() {
        assert_eq!(
            visible_text(r"at=\(at ga=\(ga oq=\(oq arrow=\(-> larrow=\(<- mu=\(mu lB=\(lB rB=\(rB"),
            "at=@ ga=` oq=' arrow=→ larrow=← mu=× lB=[ rB=]"
        );
        assert_eq!(visible_text(r"zero=\[:]width"), "zero=width");
    }

    #[test]
    fn retains_unknown_named_characters_in_a_visible_source_form() {
        assert_eq!(
            visible_text(r"a=\(zz b=\[future-glyph] c=\C'other'"),
            r"a=\(zz b=\[future-glyph] c=\C'other'"
        );
    }

    #[test]
    fn malformed_and_undefined_escapes_are_bounded_and_predictable() {
        assert_eq!(visible_text("alpha\\m[unterminated"), "alpha");
        assert_eq!(visible_text("alpha\\"), "alpha\\");
        assert_eq!(visible_text(r"alpha\qbeta"), "alphaqbeta");
        assert_eq!(visible_text(r"\EfBbold\EfR"), "bold");
        assert_eq!(visible_text(r"before\N1after"), "beforeafter");
        assert_eq!(visible_text(r"before\zXafter"), "beforeafter");
    }

    #[test]
    fn masks_terminal_controls_in_source_and_undefined_escapes() {
        assert_eq!(visible_text("before\u{1b}[2Jafter"), "before [2Jafter");
        assert_eq!(visible_text("before\\\u{7}after"), "before after");
    }
}
