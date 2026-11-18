//! Lexical proof for audits of terminal source words.

use super::{PresentationKind, RoffInlineEvent, decode_with_status};

/// Effects of a source word that affect what an independent observer can assert.
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum RoffWordEffect {
    /// A nonprinting cell was buffered (for example a size escape).
    Buffered = 1,
    /// The escape's output or positioning still needs source review.
    Unclassified = 2,
    /// A final continuation escape can suppress the next source boundary.
    Continues = 4,
    /// A word-end break and its following separator were both present.
    EmittedBreak = 8,
    /// A word-end break is still waiting for a separator.
    WordEndBreak = 16,
    /// Bare zero-advance is still armed at the end of the source word.
    ZeroAdvance = 32,
}

use RoffWordEffect::{Buffered, Continues, EmittedBreak, Unclassified, WordEndBreak, ZeroAdvance};

/// Source word spelling and effects, prior to native field or list-layout decisions.
#[derive(Default)]
pub struct RoffWordEvidence {
    /// Potential textual spelling. With an unclassified effect this may be
    /// enough to flag a candidate for review, without proving its output.
    pub text: String,
    effects: u8,
    advance: Advance,
}

#[derive(Default)]
enum Advance {
    #[default]
    Ordinary,
    Armed,
    Buffered(usize),
}

impl RoffWordEvidence {
    /// Report whether this word carries a particular formatter effect.
    #[must_use]
    pub fn has(&self, effect: RoffWordEffect) -> bool {
        self.effects & effect as u8 != 0
    }

    /// Decode one owned terminal source word using the shared roff grammar.
    #[must_use]
    pub fn read(source: &str) -> Self {
        let decoded = decode_with_status(source);
        let mut word = Self::default();
        if !decoded.proves_native_glyphs() {
            word.set(Unclassified);
        }
        let mut events = decoded.events.into_iter().peekable();
        while let Some(event) = events.next() {
            match event {
                RoffInlineEvent::Text(text) => {
                    if text.contains(' ') {
                        word.resolve_break();
                    }
                    word.append(&text);
                }
                RoffInlineEvent::Glyph(text) => word.append(&text),
                RoffInlineEvent::BreakableHyphen => word.append("-"),
                RoffInlineEvent::NonBreakingWordBlank => word.append("\u{a0}"),
                RoffInlineEvent::DeviceName => word.append("utf8"),
                RoffInlineEvent::ZeroWidthGlyph => word.set(Buffered),
                RoffInlineEvent::Breakpoint => {
                    word.set(Buffered);
                    word.resolve_break();
                }
                RoffInlineEvent::LineBreak => {
                    word.set(Buffered);
                    word.set(WordEndBreak);
                }
                RoffInlineEvent::NoSpace => {
                    // CVS clears an armed BACKAFTER instead of applying
                    // a continuation. A held glyph has BACKBEFORE set.
                    if matches!(word.advance, Advance::Armed) {
                        word.advance = Advance::Ordinary;
                    } else if events.peek().is_none() {
                        word.set(Continues);
                    }
                }
                RoffInlineEvent::ZeroAdvance => word.advance = Advance::Armed,
                RoffInlineEvent::Overstrike {
                    terminal, buffered, ..
                } => {
                    word.set(Unclassified);
                    if buffered {
                        word.set(Buffered);
                    }
                    if let Some(text) = terminal {
                        word.append(&text);
                    }
                }
                RoffInlineEvent::HorizontalMotion { .. }
                | RoffInlineEvent::Link(_)
                | RoffInlineEvent::FallbackGlyph(_)
                | RoffInlineEvent::Presentation {
                    kind: PresentationKind::HorizontalLine,
                    ..
                } => {
                    word.set(Unclassified);
                }
                RoffInlineEvent::EmptyDestination => word.append("<>"),
                RoffInlineEvent::Font(_)
                | RoffInlineEvent::PreviousFont
                | RoffInlineEvent::Presentation { .. } => {}
            }
        }
        if matches!(word.advance, Advance::Armed) {
            word.set(ZeroAdvance);
        }
        word
    }

    fn set(&mut self, effect: RoffWordEffect) {
        self.effects |= effect as u8;
    }

    fn resolve_break(&mut self) {
        if self.has(WordEndBreak) {
            // A terminal \p does not break the line until term_fill()
            // visits a real space or ASCII_BREAK in its buffer.
            self.set(EmittedBreak);
            self.effects &= !(WordEndBreak as u8);
        }
    }

    fn append(&mut self, text: &str) {
        let Some(first) = text.chars().next() else {
            return;
        };
        match std::mem::take(&mut self.advance) {
            Advance::Ordinary => self.text.push_str(text),
            Advance::Armed => {
                let remainder = &text[first.len_utf8()..];
                if remainder.is_empty() {
                    self.advance = Advance::Buffered(self.text.len());
                    self.text.push(first);
                } else {
                    // term_word()/encode1() can overwrite the held cell.
                    // The surviving spelling is evidence for a candidate,
                    // never confirmation of the resulting field position.
                    self.set(Unclassified);
                    self.text.push_str(remainder);
                }
            }
            Advance::Buffered(start) => {
                self.set(Unclassified);
                self.text.truncate(start);
                self.text.push_str(text);
            }
        }
    }
}
