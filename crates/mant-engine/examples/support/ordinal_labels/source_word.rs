//! Bounded terminal word evidence shared by the ordinal census and BODY audit.

use libmandoc_rs::{SpecialCharacter, special_character};

#[repr(u8)]
#[derive(Clone, Copy)]
pub(super) enum WordEffect {
    Buffered = 1,
    Unclassified = 2,
    Continues = 4,
    EmittedBreak = 8,
    WordEndBreak = 16,
    ZeroAdvance = 32,
}

use WordEffect::{Buffered, Continues, EmittedBreak, Unclassified, WordEndBreak, ZeroAdvance};

#[derive(Default)]
pub(super) struct NativeWord {
    pub(super) text: String,
    effects: u8,
}

impl NativeWord {
    pub(super) fn has(&self, effect: WordEffect) -> bool {
        self.effects & effect as u8 != 0
    }

    fn set(&mut self, effect: WordEffect) {
        self.effects |= effect as u8;
    }

    fn clear(&mut self, effect: WordEffect) {
        self.effects &= !(effect as u8);
    }

    fn push(&mut self, glyph: char) {
        self.clear(ZeroAdvance);
        self.text.push(glyph);
    }

    pub(super) fn read(source: &str) -> Self {
        let mut word = Self::default();
        let mut rest = source;
        while let Some(offset) = rest.find('\\') {
            word.literal(&rest[..offset]);
            let escape = rest[offset + 1..].trim_start_matches('E');
            let Some(kind) = escape.chars().next() else {
                word.set(Unclassified);
                return word;
            };
            let tail = &escape[kind.len_utf8()..];
            rest = word.escape(kind, tail);
        }
        word.literal(rest);
        word
    }

    fn literal(&mut self, text: &str) {
        // term_fill() acts on an armed \p only at an actual word boundary.
        // A terminal \p has no following SP or ASCII_BREAK to trigger it.
        if self.has(WordEndBreak) && text.contains(' ') {
            self.set(EmittedBreak);
            self.clear(WordEndBreak);
        }
        if !text.is_empty() {
            self.clear(ZeroAdvance);
            self.text.push_str(text);
        }
    }

    fn escape<'a>(&mut self, kind: char, rest: &'a str) -> &'a str {
        match kind {
            '&' | '%' | ')' | ',' | '/' | '^' | 'a' | 'd' | 't' | 'u' | '{' | '|' | '}' => {
                self.set(Buffered);
            }
            ':' => {
                self.set(Buffered);
                if self.has(WordEndBreak) {
                    self.set(EmittedBreak);
                }
                self.clear(WordEndBreak);
            }
            'c' if rest.is_empty() => self.set(Continues),
            'c' => {}
            'p' => {
                self.set(Buffered);
                self.set(WordEndBreak);
            }
            'z' => {
                // term_word() arms backspacing. A following literal scalar
                // still emits a glyph; bare \z is only a register change.
                self.set(ZeroAdvance);
            }
            'f' => return self.carried(font_suffix(rest)),
            ' ' | '0' | '~' => self.push('\u{a0}'),
            '[' => return self.special(rest.split_once(']')),
            '(' => {
                return self.special(rest.get(..2).zip(rest.get(2..)));
            }
            'C' => return self.special(quoted(rest)),
            'N' => return self.numbered(quoted(rest)),
            'D' | 'H' | 'L' | 'R' | 'S' | 'X' | 'Z' | 'b' | 'v' | 'x' => {
                self.set(Buffered);
                return self.carried(quoted(rest).map(|(_, remainder)| remainder));
            }
            'h' | 'l' | 'o' => {
                // The native handler may move the field, draw, or
                // overstrike. This bounded audit does not infer its result.
                self.set(Unclassified);
                return self.carried(quoted(rest).map(|(_, remainder)| remainder));
            }
            '\'' | '-' | '_' | '`' | 'e' => {
                let mut utf8 = [0; 4];
                self.record_special(kind.encode_utf8(&mut utf8));
            }
            '!' | '?' | 'r' => self.set(Unclassified),
            _ => {
                // roff_escape.c :: ESCAPE_UNDEF -> term_word()'s literal.
                self.push(kind);
            }
        }
        rest
    }

    fn carried<'a>(&mut self, remainder: Option<&'a str>) -> &'a str {
        if remainder.is_none() {
            self.set(Unclassified);
        }
        remainder.unwrap_or_default()
    }

    fn special<'a>(&mut self, value: Option<(&str, &'a str)>) -> &'a str {
        let Some((name, remainder)) = value else {
            self.set(Unclassified);
            return "";
        };
        self.record_special(name);
        remainder
    }

    fn record_special(&mut self, name: &str) {
        if let Some(glyph) = unicode_character(name) {
            self.push(glyph);
            return;
        }
        match special_character(name) {
            Some(SpecialCharacter::Visible(glyph)) => self.push(glyph),
            Some(SpecialCharacter::ZeroWidth) => self.set(Buffered),
            None => self.set(Unclassified),
        }
    }

    fn numbered<'a>(&mut self, value: Option<(&str, &'a str)>) -> &'a str {
        let Some((number, remaining)) = value else {
            self.set(Unclassified);
            return "";
        };
        match number.parse::<u8>().ok().map(char::from) {
            Some(glyph) if !glyph.is_control() => self.push(glyph),
            _ => self.set(Unclassified),
        }
        remaining
    }
}

fn unicode_character(name: &str) -> Option<char> {
    let digits = name.strip_prefix('u')?;
    if !(4..=6).contains(&digits.len()) {
        return None;
    }
    let glyph = char::from_u32(u32::from_str_radix(digits, 16).ok()?)?;
    (!glyph.is_control()).then_some(glyph)
}

fn quoted(argument: &str) -> Option<(&str, &str)> {
    let delimiter = argument.chars().next()?;
    if !delimiter.is_ascii() || " %&()*+-./0123456789:<=>\\".contains(delimiter) {
        return None;
    }
    argument[1..].split_once(delimiter)
}

fn font_suffix(font: &str) -> Option<&str> {
    if let Some(named) = font.strip_prefix('[') {
        return named.split_once(']').map(|(_, suffix)| suffix);
    }
    if let Some(named) = font.strip_prefix('(') {
        return named.get(2..);
    }
    font.get(font.chars().next()?.len_utf8()..)
}
