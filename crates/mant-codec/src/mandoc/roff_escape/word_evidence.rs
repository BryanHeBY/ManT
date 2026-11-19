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
    plain_slots: PlainSlots,
}

/// A bounded shape proof for a default-font, unbroken ASCII prefix. Any
/// other writer leaves the actual overwritten cells for separate review.
enum PlainSlots {
    Prefix(usize),
    Covering(usize),
    Covered,
    Unknown,
}

impl Default for PlainSlots {
    fn default() -> Self {
        Self::Prefix(0)
    }
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

    /// Placement remains unknown despite reading the source operands.
    #[must_use]
    pub fn has_unproved_graphs(&self) -> bool {
        self.leading_graph == LeadingGraph::Unproved
    }

    /// An empty initial pass rejected the remainder at a word-end break.
    #[must_use]
    pub fn rejects_leading_pass(&self) -> bool {
        self.leading_graph == LeadingGraph::Rejected
            || matches!(self.plain_slots, PlainSlots::Covered)
    }

    /// Font- or macro-generated device writes prevent the simple ASCII
    /// slot proof without invalidating an independently proved glyph.
    pub fn leave_plain_slots(&mut self) {
        self.plain_slots = PlainSlots::Unknown;
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
            self.leave_plain_slots();
        }
        let mut events = decoded.events.into_iter().peekable();
        while let Some(event) = events.next() {
            match event {
                RoffInlineEvent::Text(text) => {
                    if let Some((before, _)) = text.split_once(' ') {
                        self.prove_graph(before);
                        self.resolve_break();
                    }
                    self.plain_text(&text);
                    self.append(&text);
                }
                RoffInlineEvent::Glyph(text) => self.other_text(&text),
                RoffInlineEvent::BreakableHyphen => self.other_text("-"),
                RoffInlineEvent::NonBreakingWordBlank => self.other_text("\u{a0}"),
                RoffInlineEvent::DeviceName => self.other_text("utf8"),
                RoffInlineEvent::ZeroWidthGlyph => {
                    self.set(Buffered);
                    self.invisible_graph();
                    self.leave_plain_slots();
                }
                RoffInlineEvent::Breakpoint => {
                    self.set(Buffered);
                    self.resolve_break();
                    self.leave_plain_slots();
                }
                RoffInlineEvent::LineBreak => {
                    self.set(Buffered);
                    self.set(WordEndBreak);
                    self.write_break();
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
                RoffInlineEvent::ZeroAdvance => {
                    self.arm_zero_advance();
                    self.leave_plain_slots();
                }
                RoffInlineEvent::Overstrike {
                    source,
                    terminal,
                    buffered,
                } => {
                    self.overstrike(&source, terminal.as_deref(), buffered);
                }
                RoffInlineEvent::HorizontalMotion { argument, .. } => {
                    self.motion(argument.as_deref());
                }
                RoffInlineEvent::Link(_) => {
                    // The decoder supplies its separate zero-width cell.
                    self.set(Unclassified);
                }
                RoffInlineEvent::FallbackGlyph(_) => {
                    self.mark_unclassified();
                    self.leave_plain_slots();
                }
                RoffInlineEvent::Presentation {
                    kind: PresentationKind::HorizontalLine,
                    argument,
                } => {
                    self.mark_unclassified();
                    self.leave_plain_slots();
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
                RoffInlineEvent::EmptyDestination => self.other_text("<>"),
                RoffInlineEvent::Font(_) | RoffInlineEvent::PreviousFont => {
                    self.leave_plain_slots();
                }
                RoffInlineEvent::Presentation { .. } => {}
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

    fn plain_text(&mut self, text: &str) {
        if let PlainSlots::Prefix(length) = &mut self.plain_slots
            && text.bytes().all(|byte| byte.is_ascii_graphic())
        {
            *length = length.saturating_add(text.len());
        } else if !text.is_empty() {
            self.leave_plain_slots();
        }
    }

    fn other_text(&mut self, text: &str) {
        self.leave_plain_slots();
        self.append(text);
    }

    fn write_break(&mut self) {
        self.plain_slots = match self.plain_slots {
            PlainSlots::Covering(1) => {
                self.leading_graph = LeadingGraph::Empty;
                PlainSlots::Covered
            }
            PlainSlots::Covering(left) => PlainSlots::Covering(left - 1),
            PlainSlots::Covered => PlainSlots::Covered,
            _ => PlainSlots::Unknown,
        };
    }

    fn overstrike(&mut self, source: &str, terminal: Option<&str>, buffered: bool) {
        self.leave_plain_slots();
        if let Some(text) = terminal
            && matches!(self.advance, Advance::Ordinary)
        {
            self.prove_graph(text);
        }
        if source == " " && !self.has(WordEndBreak) && matches!(self.advance, Advance::Ordinary) {
            // encode1() writes this blank, and trim can only recede into
            // another blank. It does not displace a later ordinary glyph.
            self.set(Unclassified);
        } else {
            self.mark_unclassified();
        }
        if buffered {
            self.set(Buffered);
        }
        if let Some(text) = terminal {
            self.append(text);
        } else if buffered
            && let Advance::BufferedAndArmed(start) = std::mem::take(&mut self.advance)
        {
            self.text.truncate(start);
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
                self.record_motion(motion);
            }
        }
    }

    fn record_motion(&mut self, motion: Option<(i32, bool)>) {
        self.set(Unclassified);
        let Some((units, absolute)) = motion else {
            // ESCAPE_HORIZ ignores an invalid operand.
            return;
        };
        if !absolute && units >= 0 {
            if units > 0 {
                if matches!(self.advance, Advance::Ordinary) {
                    self.invisible_graph();
                }
                self.leave_plain_slots();
            }
            return;
        }
        if let PlainSlots::Prefix(length @ 1..) = self.plain_slots
            && (absolute && units == 0 || !absolute && units.unsigned_abs() as usize / 24 >= length)
        {
            self.plain_slots = PlainSlots::Covering(length);
        } else {
            self.leave_plain_slots();
        }
        // An empty field can change its origin but contains no earlier
        // glyph to displace. Other rewinds need a checked overwrite.
        if self.leading_graph != LeadingGraph::Empty || self.has(Buffered) {
            self.leading_graph = LeadingGraph::Unproved;
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
