//! Lexical proof for audits of terminal source words.

use super::{PresentationKind, RoffInlineEvent, decode_with_status, horizontal_line_stroke};

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
    leading_graph: LeadingGraph,
}

#[derive(Default, PartialEq, Eq)]
enum LeadingGraph {
    #[default]
    Empty,
    Invisible,
    Proved,
    Unproved,
    Rejected,
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

    /// An advancing authored glyph preceded any unclassified write or break.
    /// A later unknown edit can affect spelling without making the initial
    /// native field glyphless.
    #[must_use]
    pub fn has_leading_graph(&self) -> bool {
        self.leading_graph == LeadingGraph::Proved
    }

    /// An empty initial pass rejected the remainder at a word-end break.
    #[must_use]
    pub fn rejects_leading_pass(&self) -> bool {
        self.leading_graph == LeadingGraph::Rejected
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
            self.mark_unclassified();
        }
        let mut events = decoded.events.into_iter().peekable();
        while let Some(event) = events.next() {
            match event {
                RoffInlineEvent::Text(text) => {
                    if let Some((before, _)) = text.split_once(' ') {
                        self.prove_graph(before);
                        self.resolve_break();
                    }
                    self.append(&text);
                }
                RoffInlineEvent::Glyph(text) => self.append(&text),
                RoffInlineEvent::BreakableHyphen => self.append("-"),
                RoffInlineEvent::NonBreakingWordBlank => self.append("\u{a0}"),
                RoffInlineEvent::DeviceName => self.append("utf8"),
                RoffInlineEvent::ZeroWidthGlyph => {
                    self.set(Buffered);
                    self.invisible_graph();
                }
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
                    if let Some(text) = terminal.as_deref()
                        && matches!(self.advance, Advance::Ordinary)
                    {
                        self.prove_graph(text);
                    }
                    self.mark_unclassified();
                    if buffered {
                        self.set(Buffered);
                    }
                    if let Some(text) = terminal {
                        self.append(&text);
                    } else if buffered {
                        // ESCAPE_OVERSTRIKE still passes blank operands to
                        // encode1(). A following graph can replace the held
                        // blank. An empty operand never consumes BACKAFTER.
                        if let Advance::BufferedAndArmed(start) = std::mem::take(&mut self.advance)
                        {
                            self.text.truncate(start);
                        }
                    }
                }
                RoffInlineEvent::HorizontalMotion { argument, .. } => {
                    self.motion(argument.as_deref());
                }
                RoffInlineEvent::Link(_) | RoffInlineEvent::FallbackGlyph(_) => {
                    self.mark_unclassified();
                }
                RoffInlineEvent::Presentation {
                    kind: PresentationKind::HorizontalLine,
                    argument,
                } => {
                    self.mark_unclassified();
                    // A nonpositive length draws to the field margin when
                    // space remains. Check whether a stroke is possible,
                    // without making the observer assume a field width.
                    if argument
                        .as_deref()
                        .and_then(|argument| horizontal_line_stroke(argument, 1))
                        .is_some()
                    {
                        self.set(Buffered);
                        // ESCAPE_HLINE on UTF-8 calls encode1() (term.c).
                        // The first stroke consumes BACKAFTER. The source
                        // audit omits the stroke to keep the potential
                        // author spelling available for candidate scans.
                        // Placement and device differences still need review.
                        if let Advance::Buffered(start) | Advance::BufferedAndArmed(start) =
                            std::mem::take(&mut self.advance)
                        {
                            self.text.truncate(start);
                        }
                    }
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

    fn mark_unclassified(&mut self) {
        self.set(Unclassified);
        if matches!(
            self.leading_graph,
            LeadingGraph::Empty | LeadingGraph::Invisible
        ) {
            self.leading_graph = LeadingGraph::Unproved;
        }
    }

    fn motion(&mut self, argument: Option<&str>) {
        use libmandoc_rs::TableColumnMetrics;
        // term_word() consumes an armed BACKAFTER before measuring HORIZ.
        match self.advance {
            Advance::Armed => self.advance = Advance::Ordinary,
            Advance::BufferedAndArmed(start) => self.advance = Advance::Buffered(start),
            _ => {
                let motion = argument.and_then(|argument| {
                    let absolute = argument.strip_prefix('|');
                    TableColumnMetrics::motion_argument(absolute.unwrap_or(argument))
                        .map(|(units, _)| (units, absolute.is_some()))
                });
                // Entering a previous slot can invalidate even earlier ink.
                if motion.is_some_and(|(units, absolute)| absolute || units < 0) {
                    self.leading_graph = LeadingGraph::Unproved;
                }
                if motion == Some((0, false)) {
                    // No slots change. Exact ordinal spelling after a
                    // motion remains a separate lexical decision.
                    self.set(Unclassified);
                } else {
                    self.mark_unclassified();
                }
            }
        }
    }

    fn prove_graph(&mut self, text: &str) {
        // A first advancing source graph participates in term_fill() before
        // a word-end break can reject an unprinted field.
        if matches!(
            self.leading_graph,
            LeadingGraph::Empty | LeadingGraph::Invisible
        ) && text
            .chars()
            .any(|ch| (!ch.is_whitespace() && !ch.is_control()) || ch == '\u{a0}')
        {
            self.leading_graph = LeadingGraph::Proved;
        } else if text.contains('\t') {
            self.invisible_graph();
        }
    }

    fn invisible_graph(&mut self) {
        if self.leading_graph == LeadingGraph::Empty {
            self.leading_graph = LeadingGraph::Invisible;
        }
    }

    fn resolve_break(&mut self) {
        if self.has(WordEndBreak) {
            // A terminal \p does not break the line until term_fill()
            // visits a real space or ASCII_BREAK in its buffer.
            if self.leading_graph == LeadingGraph::Empty {
                self.leading_graph = LeadingGraph::Rejected;
            }
            self.set(EmittedBreak);
            self.effects &= !(WordEndBreak as u8);
        }
    }

    fn append(&mut self, text: &str) {
        let Some(first) = text.chars().next() else {
            return;
        };
        match std::mem::take(&mut self.advance) {
            Advance::Ordinary => {
                self.prove_graph(text);
                self.text.push_str(text);
            }
            Advance::Armed => {
                self.prove_graph(text);
                let remainder = &text[first.len_utf8()..];
                if remainder.is_empty() {
                    self.advance = Advance::Buffered(self.text.len());
                    self.text.push(first);
                } else {
                    // term_word()/encode1() can overwrite the held cell.
                    // The surviving spelling is evidence for a candidate,
                    // never confirmation of the resulting field position.
                    self.mark_unclassified();
                    self.text.push_str(remainder);
                }
            }
            Advance::Buffered(start) => {
                self.mark_unclassified();
                self.text.truncate(start);
                self.text.push_str(text);
            }
            Advance::BufferedAndArmed(start) => {
                self.mark_unclassified();
                self.text.truncate(start);
                self.advance = Advance::Armed;
                self.append(text);
            }
        }
    }
}
