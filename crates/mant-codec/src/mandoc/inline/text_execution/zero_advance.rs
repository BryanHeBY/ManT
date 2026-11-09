use super::{Font, Inline, is_formatter_word_blank};
use crate::mandoc::inline::font::style::{flush_segment, styled_link, styled_segment};
use crate::mandoc::inline::ownership::{Decoration, delayed_origin};
mod membership;
use crate::mandoc::roff_escape::ZeroAdvanceMachine;
use std::sync::Arc;

/// Bounded semantic projection of CVS mandoc's `TERMP_BACKAFTER`/
/// `TERMP_BACKBEFORE` state for `\\z`.
///
/// The terminal formatter carries that state across `term_word()` calls.  A
/// pending glyph therefore belongs to the surrounding inline stream rather
/// than to the one text node that happened to contain the escape.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct ZeroAdvanceState {
    pub(in crate::mandoc) annotations:
        Option<crate::mandoc::inline::ownership::annotations::AnnotationPool>,
    machine: ZeroAdvanceMachine<OwnedGlyph>,
    native_word_owner: Option<u64>,
    fragment_started_pending: bool,
    resolved_preexisting: bool,
    membership: membership::MembershipState,
    /// The word began with a `\p` marker whose blank the pending glyph's
    /// retreat consumes: the cross-word separator survives in the native
    /// buffer (term.c:573-576 with 901-908) and must print after the glyph.
    marker_blank_separator: usize,
    /// Non-graph bytes (' ' and '\t') that passed the pending glyph and
    /// stay buffered upstream until the next graph's retreat decides, by
    /// the last-byte rule (term.c:901-908), which single one dies. Order
    /// matters: `\zA <TAB>X` prints `A X` while `\zA<TAB> X` keeps the
    /// tab geometry and drops the blank.
    held: Vec<HeldCell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct HeldCell {
    character: char,
    font: Font,
    link: Option<Arc<str>>,
    owner: Option<u64>,
    recovery_offset: usize,
    epoch: u64,
    root: u64,
    decoration: Option<Arc<Decoration>>,
    link_continuation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OwnedGlyph {
    value: Inline,
    decoration: Option<Arc<Decoration>>,
    owner: Option<u64>,
    /// This cached tail and the preceding annotated output are one authored
    /// link. The seam joins only those pieces after native acceptance.
    link_continuation: bool,
    epoch: u64,
    root: u64,
}

type ProjectionRun = (Option<u64>, u64, bool, Option<Arc<Decoration>>);

impl ZeroAdvanceState {
    pub(in crate::mandoc) fn is_armed(&self) -> bool {
        self.machine.is_armed()
    }
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            annotations: None,
            marker_blank_separator: 0,
            held: Vec::new(),
            machine: ZeroAdvanceMachine::new(),
            native_word_owner: None,
            fragment_started_pending: false,
            resolved_preexisting: false,
            membership: membership::MembershipState::new(),
        }
    }

    /// A paragraph drain or physical row closes zero-advance projection but
    /// does not end a semantic owner spanning that output boundary.  Keep
    /// the ownership ledger until the enclosing macro reaches its post.
    pub(in crate::mandoc) fn reset_projection(&mut self, bare_armed: bool) {
        self.machine.clear();
        self.held.clear();
        self.fragment_started_pending = false;
        self.resolved_preexisting = false;
        self.inherit_armed(bare_armed);
    }

    pub(in crate::mandoc) fn set_native_word_owner(&mut self, owner: u64) {
        self.native_word_owner = Some(owner);
    }

    fn project_glyph(&mut self, glyph: Inline) -> Option<(Inline, bool)> {
        self.machine
            .project_glyph(OwnedGlyph {
                value: glyph,
                decoration: None,
                owner: self.native_word_owner,
                link_continuation: false,
                epoch: self.membership.output_epoch,
                root: self.membership.semantic_root,
            })
            .map(|(glyph, replaced)| (glyph.value, replaced))
    }

    fn project_fallback(&mut self, glyph: Inline) -> Option<(Inline, bool)> {
        self.machine
            .project_fallback(OwnedGlyph {
                value: glyph,
                decoration: None,
                owner: self.native_word_owner,
                link_continuation: false,
                epoch: self.membership.output_epoch,
                root: self.membership.semantic_root,
            })
            .map(|(glyph, replaced)| (glyph.value, replaced))
    }

    fn project_owned(&self, glyphs: Vec<OwnedGlyph>) -> Vec<Inline> {
        #[cfg(test)]
        crate::mandoc::inline::ownership::metrics::note(|counts| counts.released += glyphs.len());
        let mut output = Vec::new();
        let mut run = Vec::new();
        let mut key: Option<ProjectionRun> = None;
        let flush =
            |key: &mut Option<ProjectionRun>, run: &mut Vec<Inline>, output: &mut Vec<Inline>| {
                let Some((owner, epoch, continuation, decoration)) = key.take() else {
                    return;
                };
                if let Some(owner) = owner {
                    output.push(Inline::anchor(format!(
                        "{}{owner}",
                        crate::mandoc::inline::flow::INTERNAL_FIELD_WORD
                    )));
                }
                #[cfg(test)]
                crate::mandoc::inline::ownership::metrics::note(|counts| counts.runs += 1);
                let mut nodes = std::mem::take(run);
                if let Some(decoration) = decoration {
                    nodes = decoration.apply_nodes(nodes);
                }
                if continuation {
                    nodes.insert(
                        0,
                        Inline::anchor(crate::mandoc::inline::flow::INTERNAL_LINK_SPLIT),
                    );
                }
                if self
                    .membership
                    .output_owners
                    .last()
                    .is_some_and(|scope| epoch < scope.0)
                {
                    output.push(delayed_origin(epoch, nodes));
                } else {
                    output.extend(nodes);
                }
            };
        for glyph in glyphs {
            let next = (
                glyph.owner,
                glyph.epoch,
                glyph.link_continuation,
                glyph.decoration,
            );
            if key.as_ref().is_some_and(|key| key != &next) {
                flush(&mut key, &mut run, &mut output);
            }
            key = Some(next);
            match (run.last_mut(), glyph.value) {
                (Some(Inline::Text { value: before }), Inline::Text { value })
                | (Some(Inline::Code { value: before }), Inline::Code { value }) => {
                    before.push_str(&value);
                }
                (_, node) => run.push(node),
            }
        }
        flush(&mut key, &mut run, &mut output);
        if !output.is_empty()
            && let Some(owner) = self.native_word_owner
        {
            output.push(Inline::anchor(format!(
                "{}{owner}",
                crate::mandoc::inline::flow::INTERNAL_FIELD_WORD
            )));
        }
        output
    }

    fn take_pending(&mut self) -> Option<Vec<Inline>> {
        let glyph = self.machine.take_pending();
        glyph.map(|glyphs| self.project_owned(glyphs))
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
    pub(in crate::mandoc) fn note_marker_blank_separator(&mut self, cells: usize) {
        self.marker_blank_separator = cells;
    }

    pub(super) fn take_marker_blank_separator(&mut self) -> usize {
        std::mem::take(&mut self.marker_blank_separator)
    }

    pub(in crate::mandoc) fn clear_marker_blank_separator(&mut self) {
        self.marker_blank_separator = 0;
    }

    /// Blanks buffered behind the pending glyph when a `\p` marker
    /// arrives: the next graph's retreat lands on the marker's own blank,
    /// so the held bytes survive and print after the settled glyph.
    fn take_deferred_with_held(&mut self) -> Vec<Inline> {
        let (pending, recoveries) = self.machine.take_projection_parts();
        let mut pending = pending;
        let mut held = std::mem::take(&mut self.held).into_iter().peekable();
        let mut output =
            Vec::with_capacity(recoveries.len() + held.len() + usize::from(pending.is_some()));
        for (index, recovery) in recoveries
            .into_iter()
            .map(Some)
            .chain(std::iter::once(None))
            .enumerate()
        {
            if pending.as_ref().is_some_and(|(before, _)| *before == index) {
                output.push(pending.take().expect("pending position").1);
            }
            while held
                .peek()
                .is_some_and(|cell| cell.recovery_offset <= index)
            {
                let cell = held.next().expect("held cell");
                let mut value = cell.character.to_string();
                while held.peek().is_some_and(|next| {
                    next.recovery_offset == cell.recovery_offset
                        && next.font == cell.font
                        && next.link == cell.link
                        && next.owner == cell.owner
                        && next.epoch == cell.epoch
                        && next.root == cell.root
                        && next.decoration == cell.decoration
                        && next.link_continuation == cell.link_continuation
                }) {
                    value.push(held.next().expect("held run").character);
                }
                output.push(OwnedGlyph {
                    value: styled_link(value, cell.font, cell.link.as_deref()),
                    decoration: cell.decoration,
                    owner: cell.owner,
                    epoch: cell.epoch,
                    root: cell.root,
                    link_continuation: cell.link_continuation,
                });
            }
            if let Some(recovery) = recovery {
                output.push(recovery);
            }
        }
        self.project_owned(output)
    }

    pub(super) fn flush_with_held(
        &mut self,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        flush_segment(output, buffer, font, link);
        output.extend(self.take_deferred_with_held());
    }

    fn hold(&mut self, character: char, font: Font, link: Option<&str>) {
        #[cfg(test)]
        crate::mandoc::inline::ownership::metrics::note(|counts| counts.held += 1);
        let link = link.map(|link| {
            self.held
                .last()
                .and_then(|cell| cell.link.as_ref())
                .filter(|previous| previous.as_ref() == link)
                .map_or_else(|| Arc::from(link), Arc::clone)
        });
        self.held.push(HeldCell {
            character,
            font,
            link,
            owner: self.native_word_owner,
            recovery_offset: self.machine.deferred_projection().len(),
            epoch: self.membership.output_epoch,
            root: self.membership.semantic_root,
            decoration: None,
            link_continuation: false,
        });
    }

    fn projected_prefix(&self) -> Vec<OwnedGlyph> {
        let recoveries = self.machine.deferred_projection();
        let mut output = Vec::new();
        let mut recovery = 0;
        let mut held = self.held.iter().peekable();
        while let Some(cell) = held.next() {
            let end = cell.recovery_offset.min(recoveries.len());
            output.extend_from_slice(&recoveries[recovery..end]);
            recovery = end;
            let mut value = cell.character.to_string();
            while held.peek().is_some_and(|next| {
                next.recovery_offset == cell.recovery_offset
                    && next.font == cell.font
                    && next.link == cell.link
                    && next.owner == cell.owner
                    && next.epoch == cell.epoch
                    && next.root == cell.root
                    && next.decoration == cell.decoration
                    && next.link_continuation == cell.link_continuation
            }) {
                value.push(held.next().expect("held run").character);
            }
            output.push(OwnedGlyph {
                value: styled_link(value, cell.font, cell.link.as_deref()),
                decoration: cell.decoration.clone(),
                owner: cell.owner,
                epoch: cell.epoch,
                root: cell.root,
                link_continuation: cell.link_continuation,
            });
        }
        output.extend_from_slice(&recoveries[recovery..]);
        output
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
        if self.machine.has_projected_pending() {
            // These cells follow the already-projected glyph's breakpoint.
            // Returning them as an earlier-row suffix would let the next
            // accepted pass trim them before emitting its newline. Defer
            // their real old-word ownership into the next word's stream,
            // where the shared native pass splitter places the boundary.
            let prefix = self.projected_prefix();
            self.machine.take_pending();
            self.held.clear();
            for glyph in prefix {
                self.machine.defer_projection(glyph);
            }
            // BACKBEFORE consumed the actual incoming word separator even
            // though this branch emits no immediate glyph. Some(empty)
            // conveys that consumed boundary, without an empty IR node.
            return Some(Vec::new());
        }
        // term_word() buffers its separator independently of BACKAFTER.
        // A later encode1() retreats over that blank, so the earlier
        // BACKBEFORE glyph survives even when another \z armed the next
        // graph (term.c:573-589,901-927). Keep the arm for that graph.
        // A pass may already own the glyph while BACKBEFORE still owns
        // these held bytes. The next automatic separator is retreated, not
        // the older authored blanks, even when no glyph remains to drain.
        // A projected glyph can have retired BACKBEFORE while its original
        // recovery/blank projection still awaits the next source word. That
        // receipt has ownership, but no retreat to consume the word separator.
        if !self.machine.has_pending() && self.held.is_empty() {
            return None;
        }
        let glyph = self.take_deferred_with_held();
        (!glyph.is_empty()).then_some(glyph)
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

    /// A glyph buffered by this word can still become the first member of
    /// its annotation when a later word releases it.
    pub(in crate::mandoc) fn buffered_glyph_belongs_to_current_word(&self) -> bool {
        self.native_word_owner.is_some()
            && self
                .machine
                .pending_ref()
                .is_some_and(|glyph| glyph.owner == self.native_word_owner)
    }

    pub(in crate::mandoc) fn has_printable_pending_glyph(&self) -> bool {
        self.pending_visible_characters() > 0
    }

    /// Size of the still-buffered visible owner, before another word causes
    /// it to print. Fallback spelling can contain more than one scalar.
    pub(in crate::mandoc) fn pending_visible_characters(&self) -> usize {
        self.machine.pending_ref().map_or(0, |glyph| {
            mant_ir::inline_plain_text(std::slice::from_ref(&glyph.value))
                .chars()
                .filter(|character| !character.is_whitespace())
                .count()
        })
    }

    /// `term_flushln()` clears both backtracking flags when the native tag row
    /// ends. A bare `\z` or buffered blank cannot act on the next BODY row.
    pub(in crate::mandoc) fn discard_at_row_end(&mut self) {
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
                    let _ = self.project_glyph(styled_segment(" ".to_owned(), font));
                    continue;
                }
                output.push(styled_segment(" ".to_owned(), font));
                continue;
            }
            if let Some((cell, _)) = self.project_glyph(styled_segment(" ".to_owned(), font)) {
                let recoveries = self.machine.take_recoveries();
                output.extend(self.project_owned(recoveries));
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
        output.extend(self.take_deferred_with_held());
    }

    /// Execute CVS `ESCAPE_NOSPACE` against the pending `\\z` state.
    ///
    /// `term_word()` clears only `TERMP_BACKAFTER` before it considers a
    /// trailing `\\c` a request to join the following input line. A completed
    /// zero-advance glyph is `TERMP_BACKBEFORE` state and remains pending.
    pub(super) fn cancel_armed_for_no_space(&mut self) -> bool {
        self.machine.cancel_armed()
    }

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
            if self.machine.has_pending() {
                if is_formatter_word_blank(character) || character == '\t' {
                    // The same encode() non-graph buffering as authored
                    // text (term.c:944-964): the byte stays behind the
                    // glyph until the next graph's retreat decides, by
                    // the last-byte rule (term.c:901-908), which single
                    // held byte dies.
                    self.hold(character, font, None);
                    continue;
                }
                if !self.held.is_empty() {
                    // encode1() settles the completed glyph and its held
                    // blanks before the incoming graph — even when a
                    // second `\z` has armed the graph itself: the retreat
                    // eats one buffered blank, the glyph settles, and the
                    // armed graph then becomes the new zero-width pending
                    // glyph (term.c:901-908 write order, not an enum).
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
                    // The armed case: the graph becomes the pending
                    // zero-width glyph and prints nothing yet.
                    continue;
                };
                if replaced && self.fragment_started_pending {
                    self.resolved_preexisting = true;
                }
                self.flush_recoveries(output, &mut buffer, font, None);
                buffer.push(character);
                continue;
            }
            if self.machine.is_armed() {
                let _ = self.project_glyph(styled_segment(character.to_string(), font));
                continue;
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
        if !value.is_empty()
            && !self.machine.has_pending()
            && !self.machine.is_armed()
            && !value.contains(['\n', '\r'])
        {
            // term.c::encode()/encode1(): absent BACKAFTER/BACKBEFORE and
            // physical line controls, no scalar can alter pending ownership.
            // Recoveries precede the first scalar just as on the slow path.
            self.flush_recoveries(output, buffer, font, link);
            buffer.push_str(value);
            return;
        }
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
                    self.hold(character, font, link);
                    continue;
                }
                if !self.held.is_empty() {
                    // This graph's retreat ate the last held byte (col--):
                    // the completed zero-advance glyph prints before the
                    // survivors, glued.  Any later `\z` still converts at
                    // this graph's encode1() tail (term.c:924-926), so the
                    // graph itself may become the next deferred target.
                    self.held.pop();
                    self.flush_with_held(output, buffer, font, link);
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
        let recoveries = self.project_owned(recoveries);
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
            self.flush_with_held(output, buffer, font, link);
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

    /// HORIZ consumes BACKBEFORE without encode1's retreat. Preserve the
    /// glyph and every following author blank, in recovery/owner order.
    pub(super) fn settle_horizontal_motion(
        &mut self,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        self.flush_with_held(output, buffer, font, link);
        if self.fragment_started_pending {
            self.resolved_preexisting = true;
        }
    }

    /// A pending marker followed by `ASCII_BREAK` assigns the old glyph to
    /// its earlier pass, while `encode1()` still owes the next BACKBEFORE
    /// retreat. Do not use an authored empty glyph to represent that state.
    pub(super) fn commit_pending_projection(
        &mut self,
        output: &mut Vec<Inline>,
        buffer: &mut String,
        font: Font,
        link: Option<&str>,
    ) {
        flush_segment(output, buffer, font, link);
        if let Some(glyph) = self.machine.commit_pending_projection() {
            // Recoveries before this breakpoint have just been projected;
            // older held cells precede every recovery in the new suffix.
            for cell in &mut self.held {
                cell.recovery_offset = 0;
            }
            if self.fragment_started_pending {
                self.resolved_preexisting = true;
            }
            output.extend(self.project_owned(glyph));
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
