use super::style::{flush_segment, styled_link, styled_segment};
use super::{Font, Inline, is_formatter_word_blank};
use crate::mandoc::roff_escape::ZeroAdvanceMachine;

/// Bounded semantic projection of CVS mandoc's `TERMP_BACKAFTER`/
/// `TERMP_BACKBEFORE` state for `\\z`.
///
/// The terminal formatter carries that state across `term_word()` calls.  A
/// pending glyph therefore belongs to the surrounding inline stream rather
/// than to the one text node that happened to contain the escape.
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub(in crate::mandoc) struct ZeroAdvanceState {
    machine: ZeroAdvanceMachine<Inline>,
    fragment_started_pending: bool,
    resolved_preexisting: bool,
    output_owners: Vec<PendingOutputOwner>,
    /// The word began with a `\p` marker whose blank the pending glyph's
    /// retreat consumes: the cross-word separator survives in the native
    /// buffer (term.c:573-576 with 901-908) and must print after the glyph.
    marker_blank_separator: bool,
    /// Non-graph bytes (' ' and '\t') that passed the pending glyph and
    /// stay buffered upstream until the next graph's retreat decides, by
    /// the last-byte rule (term.c:901-908), which single one dies. Order
    /// matters: `\zA <TAB>X` prints `A X` while `\zA<TAB> X` keeps the
    /// tab geometry and drops the blank.
    held: Vec<char>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PendingOutputOwner {
    pending_at_entry: bool,
    emitted_before_owner: bool,
}

impl ZeroAdvanceState {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            marker_blank_separator: false,
            held: Vec::new(),
            machine: ZeroAdvanceMachine::new(),
            fragment_started_pending: false,
            resolved_preexisting: false,
            output_owners: Vec::new(),
        }
    }

    /// A semantic wrapper can begin while BACKBEFORE still owns a glyph from
    /// preceding source. Track that glyph through the shared formatter until
    /// it is emitted or overstruck, including across nested wrappers.
    pub(in crate::mandoc) fn begin_output_owner(&mut self) {
        self.output_owners.push(PendingOutputOwner {
            pending_at_entry: self.machine.has_pending(),
            emitted_before_owner: false,
        });
    }

    pub(in crate::mandoc) fn end_output_owner(&mut self) -> bool {
        self.output_owners
            .pop()
            .is_some_and(|owner| owner.emitted_before_owner)
    }

    /// A paragraph drain or physical row closes zero-advance projection but
    /// does not end a semantic owner spanning that output boundary.  Keep
    /// the ownership ledger until the enclosing macro reaches its post.
    pub(in crate::mandoc) fn reset_projection(&mut self, bare_armed: bool) {
        self.note_pending_replaced();
        self.machine.clear();
        self.held.clear();
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
        self.inherit_armed(bare_armed);
    }

    fn note_pending_emitted(&mut self) {
        for owner in &mut self.output_owners {
            if owner.pending_at_entry {
                owner.emitted_before_owner = true;
                owner.pending_at_entry = false;
            }
        }
    }

    fn note_pending_replaced(&mut self) {
        for owner in &mut self.output_owners {
            owner.pending_at_entry = false;
        }
    }

    fn project_glyph(&mut self, glyph: Inline) -> Option<(Inline, bool)> {
        if self.machine.has_pending() {
            self.note_pending_replaced();
        }
        self.machine.project_glyph(glyph)
    }

    fn project_fallback(&mut self, glyph: Inline) -> Option<(Inline, bool)> {
        self.machine.project_fallback(glyph)
    }

    fn take_pending(&mut self) -> Option<Vec<Inline>> {
        let glyph = self.machine.take_pending();
        if glyph.is_some() {
            self.note_pending_emitted();
        }
        glyph
    }

    pub(super) fn begin_fragment(&mut self) {
        self.fragment_started_pending = self.machine.has_pending();
        self.resolved_preexisting = false;
    }

    pub(super) fn arm(&mut self) {
        // CVS permits TERMP_BACKAFTER and TERMP_BACKBEFORE at the same time.
        // A second `\\z` arms the next glyph without prematurely discarding
        // the completed zero-advance glyph at the current output position.
        self.machine.arm();
    }

    pub(in crate::mandoc) fn inherit_armed(&mut self, armed: bool) {
        if armed {
            self.arm();
        }
    }

    /// Record that this word's entry boundary carried a separator the
    /// marker-blank retreat keeps alive (`ph` shape, term.c:901-908:
    /// the retreat eats the marker's own blank, not the word separator).
    pub(in crate::mandoc) fn note_marker_blank_separator(&mut self) {
        self.marker_blank_separator = true;
    }

    pub(super) fn take_marker_blank_separator(&mut self) -> bool {
        std::mem::take(&mut self.marker_blank_separator)
    }

    pub(in crate::mandoc) fn clear_marker_blank_separator(&mut self) {
        self.marker_blank_separator = false;
    }

    /// Blanks buffered behind the pending glyph when a `\p` marker
    /// arrives: the next graph's retreat lands on the marker's own blank,
    /// so the held bytes survive and print after the settled glyph.
    pub(super) fn take_held(&mut self) -> String {
        self.held.drain(..).collect()
    }

    pub(in crate::mandoc) fn take_armed(&mut self) -> bool {
        self.machine.cancel_armed()
    }

    /// Resolve a pending zero-advance glyph at a formatter-inserted word
    /// boundary. CVS `term_word()` writes that virtual blank before the next
    /// glyph; the blank consumes the backtracking position, so the glyph
    /// survives and the next word joins it without a visible space.
    pub(in crate::mandoc) fn resolve_at_word_boundary(&mut self) -> Option<Vec<Inline>> {
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
        if self.machine.is_armed() {
            return None;
        }
        let mut glyph = self.take_pending()?;
        if !self.held.is_empty() {
            // The held bytes behind the glyph survive the boundary: the
            // next word's automatic separator is the cell its retreat
            // consumes (term.c:901-908), so the word joins directly
            // after the survivors instead of after a fresh blank.
            let survivors = self.held.drain(..).collect::<String>();
            glyph.push(Inline::Text { value: survivors });
        }
        Some(glyph)
    }

    /// A pending zero-advance glyph is visible formatter state even before a
    /// previous ordinary word has committed an IR node.  In particular,
    /// `\\zX` at the beginning of a source line must survive the implicit
    /// boundary before the next word rather than be mistaken for an empty
    /// stream.
    pub(in crate::mandoc) const fn has_pending_glyph(&self) -> bool {
        self.machine.has_pending() && !self.machine.is_armed()
    }

    /// A completed BACKBEFORE glyph occupies the native formatter cell even
    /// when another `\z` has already armed BACKAFTER for the next glyph.
    pub(in crate::mandoc) const fn has_buffered_glyph(&self) -> bool {
        self.machine.has_pending()
    }

    pub(in crate::mandoc) fn has_printable_pending_glyph(&self) -> bool {
        self.machine.pending_ref().is_some_and(|glyph| {
            mant_ir::inline_plain_text(std::slice::from_ref(glyph))
                .chars()
                .any(|character| !character.is_whitespace())
        })
    }

    /// `term_flushln()` clears both backtracking flags when the native tag row
    /// ends. A bare `\z` or buffered blank cannot act on the next BODY row.
    pub(in crate::mandoc) fn discard_at_row_end(&mut self) {
        self.note_pending_replaced();
        self.machine.clear();
        self.held.clear();
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
    }

    /// Execute fixed-width formatter cells generated by a run-in list body.
    ///
    /// CVS emits `\\ ` rather than an ordinary word separator for mdoc
    /// inset/diagnostic gaps.  Such a cell is a graph (NBRSP): it runs
    /// `encode1()`, so its retreat eats one buffered blank cell and the
    /// completed zero-advance glyph settles before the survivors
    /// (term.c:901-908); with no buffered blank it overstrikes the pending
    /// glyph instead.  Treating it as generic whitespace would settle the
    /// wrong glyph at the IR split.
    pub(in crate::mandoc) fn append_generated_cells(
        &mut self,
        count: usize,
        output: &mut Vec<Inline>,
        font: Font,
    ) {
        for _ in 0..count {
            if self.machine.has_pending() && !self.held.is_empty() {
                self.settle_pending_before_graph(output);
                if self.machine.is_armed() {
                    let _ = self
                        .machine
                        .project_glyph(styled_segment(" ".to_owned(), font));
                    continue;
                }
                output.push(styled_segment(" ".to_owned(), font));
                continue;
            }
            if let Some((cell, _)) = self.project_glyph(styled_segment(" ".to_owned(), font)) {
                output.extend(self.machine.take_recoveries());
                output.push(cell);
            }
        }
    }

    /// Shared settlement for any graph event arriving while a completed
    /// zero-advance glyph has non-graph bytes buffered behind it: the
    /// graph's own `encode1()` retreat eats exactly the last buffered
    /// blank (term.c:901-908), the glyph settles before the surviving
    /// blanks, and the graph itself follows as ordinary output.
    fn settle_pending_before_graph(&mut self, output: &mut Vec<Inline>) {
        self.held.pop();
        let survivors = self.held.drain(..).collect::<String>();
        if let Some(glyph) = self.take_pending() {
            output.extend(glyph);
        }
        if !survivors.is_empty() {
            output.push(Inline::Text { value: survivors });
        }
    }

    /// Discard a completed glyph emitted by an operand whose compact output
    /// is suppressed. A bare `\\z` remains armed: CVS carries that request
    /// into the next formatter word, whereas `\\zX` has already produced the
    /// hidden glyph `X` and must not lend it to a later visible operand.
    pub(in crate::mandoc) fn discard_hidden_pending_glyph(&mut self) {
        self.note_pending_replaced();
        self.machine.discard_pending();
        self.held.clear();
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
    }

    /// Execute CVS `ESCAPE_NOSPACE` against the pending `\\z` state.
    ///
    /// `term_word()` clears only `TERMP_BACKAFTER` before it considers a
    /// trailing `\\c` a request to join the following input line. A completed
    /// zero-advance glyph is `TERMP_BACKBEFORE` state and remains pending.
    pub(super) fn cancel_armed_for_no_space(&mut self) -> bool {
        self.machine.cancel_armed()
    }

    /// The output-free recovery path can only carry a bare armed `\\z`.
    /// Keep its state transition encapsulated instead of letting consumers
    /// treat the representation of pending glyphs as public behavior.
    /// Feed formatter-generated text through the same projection as authored
    /// glyphs. Brackets from `.OP`, generated declaration punctuation, and
    /// implicit wrapper text can overwrite a pending `\z` glyph just like a
    /// source glyph in CVS `term_word()`.
    pub(in crate::mandoc) fn append_generated_text(
        &mut self,
        value: &str,
        output: &mut Vec<Inline>,
        font: Font,
    ) {
        let mut buffer = String::new();
        for character in value.chars() {
            if matches!(character, '\n' | '\r') {
                flush_segment(output, &mut buffer, font, None);
                if let Some(glyph) = self.take_pending() {
                    output.extend(glyph);
                }
                buffer.push(character);
                continue;
            }
            if self.machine.is_armed() && (is_formatter_word_blank(character) || character == '\t')
            {
                // encode()'s non-graph branch buffers the tab directly
                // (term.c:944-964); the arm survives it.
                self.flush_recoveries(output, &mut buffer, font, None);
                buffer.push(character);
                continue;
            }
            if self.machine.is_armed() {
                let _ = self.project_glyph(styled_segment(character.to_string(), font));
                continue;
            }
            if self.machine.has_pending() {
                if is_formatter_word_blank(character) || character == '\t' {
                    // The same encode() non-graph buffering as authored
                    // text (term.c:944-964): the byte stays behind the
                    // glyph until the next graph's retreat decides, by
                    // the last-byte rule (term.c:901-908), which single
                    // held byte dies.
                    self.held.push(character);
                    continue;
                }
                if !self.held.is_empty() {
                    self.settle_pending_before_graph(output);
                    if self.machine.is_armed() {
                        let _ = self.project_glyph(styled_segment(character.to_string(), font));
                        continue;
                    }
                    self.flush_recoveries(output, &mut buffer, font, None);
                    buffer.push(character);
                    continue;
                }
                let Some((_, replaced)) =
                    self.project_glyph(styled_segment(character.to_string(), font))
                else {
                    continue;
                };
                if replaced && self.fragment_started_pending {
                    self.resolved_preexisting = true;
                }
            }
            self.flush_recoveries(output, &mut buffer, font, None);
            buffer.push(character);
        }
        flush_segment(output, &mut buffer, font, None);
    }

    pub(super) fn append_text(
        &mut self,
        value: &str,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        for character in value.chars() {
            if matches!(character, '\n' | '\r') {
                self.flush(output, buffer, font, link);
                buffer.push(character);
                continue;
            }
            if self.machine.has_pending() {
                if is_formatter_word_blank(character) || character == '\t' {
                    // Hold the non-graph byte instead of consuming it:
                    // upstream's encode() writes blanks and tabs straight
                    // into the buffer (term.c:944-964) without touching
                    // the backtracking flags, so only the next graph's
                    // BACKBEFORE retreat decides, by the last-byte rule
                    // (term.c:901-908), which single held byte dies.  A
                    // `\p` marker between the two redirects the retreat
                    // onto the marker's own blank, and these survive.
                    self.held.push(character);
                    continue;
                }
                if !self.held.is_empty() {
                    // This graph's retreat ate the last held byte (col--):
                    // the completed zero-advance glyph prints before the
                    // survivors, glued.  Any later `\z` still converts at
                    // this graph's encode1() tail (term.c:924-926), so the
                    // graph itself may become the next deferred target.
                    self.held.pop();
                    let survivors = self.held.drain(..).collect::<String>();
                    self.flush(output, buffer, font, link);
                    if !survivors.is_empty() {
                        self.flush_recoveries(output, buffer, font, link);
                        buffer.push_str(&survivors);
                    }
                    if self.machine.is_armed() {
                        let _ = self.project_glyph(styled_link(character.to_string(), font, link));
                        continue;
                    }
                    self.flush_recoveries(output, buffer, font, link);
                    buffer.push(character);
                    continue;
                }
                let Some((_, replaced)) =
                    self.project_glyph(styled_link(character.to_string(), font, link))
                else {
                    continue;
                };
                if replaced && self.fragment_started_pending {
                    self.resolved_preexisting = true;
                }
                self.flush_recoveries(output, buffer, font, link);
                buffer.push(character);
                continue;
            }
            if self.machine.is_armed() {
                if is_formatter_word_blank(character) || character == '\t' {
                    // A word blank — and likewise a literal tab — is
                    // buffered through encode()'s non-graph branch
                    // (term.c:944-964); it never runs encode1(), so
                    // TERMP_BACKAFTER survives it and the arm only
                    // converts at the next graph (924-926).
                    self.flush_recoveries(output, buffer, font, link);
                    buffer.push(character);
                    continue;
                }
                let _ = self.project_glyph(styled_link(character.to_string(), font, link));
                continue;
            }
            self.flush_recoveries(output, buffer, font, link);
            buffer.push(character);
        }
    }

    fn flush_recoveries(
        &mut self,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        let recoveries = self.machine.take_recoveries();
        if !recoveries.is_empty() {
            flush_segment(output, buffer, font, link);
            output.extend(recoveries);
        }
    }

    pub(super) fn append_glyph(
        &mut self,
        value: &str,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        if !self.held.is_empty() {
            // A graph event (an escaped Unicode space is `encode1()`
            // output, term.c:886-927) retreats by the last-byte rule just
            // like an ordinary graph: the pending glyph settles before the
            // surviving held bytes instead of being replaced.
            self.held.pop();
            let survivors = self.held.drain(..).collect::<String>();
            self.flush(output, buffer, font, link);
            if !survivors.is_empty() {
                self.flush_recoveries(output, buffer, font, link);
                buffer.push_str(&survivors);
            }
            if self.machine.is_armed() {
                let _ = self.project_glyph(styled_link(value.to_owned(), font, link));
                return;
            }
            self.flush_recoveries(output, buffer, font, link);
            buffer.push_str(value);
            return;
        }
        let Some((_, replaced)) = self.project_glyph(styled_link(value.to_owned(), font, link))
        else {
            return;
        };
        if replaced && self.fragment_started_pending {
            self.resolved_preexisting = true;
        }
        self.flush_recoveries(output, buffer, font, link);
        buffer.push_str(value);
    }

    pub(super) fn append_fallback_glyph(
        &mut self,
        value: &str,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        if let Some((_, _)) = self.project_fallback(styled_link(value.to_owned(), font, link)) {
            buffer.push_str(value);
        }
    }

    pub(super) const fn fallback_is_projected(&self) -> bool {
        !self.machine.is_armed()
    }

    pub(super) fn flush(
        &mut self,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        flush_segment(output, buffer, font, link);
        if let Some(glyph) = self.take_pending() {
            if self.fragment_started_pending {
                self.resolved_preexisting = true;
            }
            output.extend(glyph);
        }
    }

    pub(in crate::mandoc) fn finish_into(&mut self, output: &mut Vec<Inline>) {
        if let Some(glyph) = self.take_pending() {
            output.extend(glyph);
        }
        self.machine.clear();
        // A blank still held at the drain is trailing whitespace;
        // term_field() never prints it (term.c:389-398).
        self.held.clear();
    }

    pub(super) fn take_preceding_join(&mut self) -> bool {
        let joined = self.fragment_started_pending && self.resolved_preexisting;
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
        joined
    }
}

impl Default for ZeroAdvanceState {
    fn default() -> Self {
        Self::new()
    }
}
