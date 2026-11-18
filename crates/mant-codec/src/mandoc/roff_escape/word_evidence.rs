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
    BufferedAndArmed(usize),
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
        let mut word = Self::default();
        word.extend(source);
        word
    }

    /// Observe the next source word or generated separator in the same
    /// execution scope. Held cells and unproved effects survive node splits.
    pub fn extend(&mut self, source: &str) {
        self.effects &= !(Continues as u8 | EmittedBreak as u8 | ZeroAdvance as u8);
        let decoded = decode_with_status(source);
        if !decoded.proves_native_glyphs() {
            self.set(Unclassified);
        }
        let mut events = decoded.events.into_iter().peekable();
        while let Some(event) = events.next() {
            match event {
                RoffInlineEvent::Text(text) => {
                    if text.contains(' ') {
                        self.resolve_break();
                    }
                    self.append(&text);
                }
                RoffInlineEvent::Glyph(text) => self.append(&text),
                RoffInlineEvent::BreakableHyphen => self.append("-"),
                RoffInlineEvent::NonBreakingWordBlank => self.append("\u{a0}"),
                RoffInlineEvent::DeviceName => self.append("utf8"),
                RoffInlineEvent::ZeroWidthGlyph => self.set(Buffered),
                RoffInlineEvent::Breakpoint => {
                    self.set(Buffered);
                    self.resolve_break();
                }
                RoffInlineEvent::LineBreak => {
                    self.set(Buffered);
                    self.set(WordEndBreak);
                }
                RoffInlineEvent::NoSpace => {
                    // CVS clears an armed BACKAFTER instead of applying
                    // a continuation. A held glyph has BACKBEFORE set.
                    match self.advance {
                        Advance::Armed => self.advance = Advance::Ordinary,
                        Advance::BufferedAndArmed(start) => self.advance = Advance::Buffered(start),
                        _ if events.peek().is_none() => self.set(Continues),
                        _ => {}
                    }
                }
                RoffInlineEvent::ZeroAdvance => self.arm_zero_advance(),
                RoffInlineEvent::Overstrike {
                    terminal, buffered, ..
                } => {
                    self.set(Unclassified);
                    if buffered {
                        self.set(Buffered);
                    }
                    if let Some(text) = terminal {
                        self.append(&text);
                    }
                }
                RoffInlineEvent::HorizontalMotion { .. }
                | RoffInlineEvent::Link(_)
                | RoffInlineEvent::FallbackGlyph(_)
                | RoffInlineEvent::Presentation {
                    kind: PresentationKind::HorizontalLine,
                    ..
                } => {
                    self.set(Unclassified);
                }
                RoffInlineEvent::EmptyDestination => self.append("<>"),
                RoffInlineEvent::Font(_)
                | RoffInlineEvent::PreviousFont
                | RoffInlineEvent::Presentation { .. } => {}
            }
        }
        if matches!(self.advance, Advance::Armed | Advance::BufferedAndArmed(_)) {
            self.set(ZeroAdvance);
        }
    }

    fn arm_zero_advance(&mut self) {
        // ESCAPE_SKIPCHAR sets BACKAFTER without clearing a held BACKBEFORE
        // cell. The next glyph both overwrites and becomes the next held cell.
        self.advance = match self.advance {
            Advance::Buffered(start) | Advance::BufferedAndArmed(start) => {
                Advance::BufferedAndArmed(start)
            }
            _ => Advance::Armed,
        };
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
            Advance::BufferedAndArmed(start) => {
                self.set(Unclassified);
                self.text.truncate(start);
                self.advance = Advance::Armed;
                self.append(text);
            }
        }
    }
}
