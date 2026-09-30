// Copyright (c) 2010-2022, 2025, 2026 Ingo Schwarze <schwarze@openbsd.org>
// Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
//
// Permission to use, copy, modify, and distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
// ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
// ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
// OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! Literal translation of the pinned CVS `term.c` line buffer and its
//! `term_fill()` pass simulation.
//!
//! Upstream decides every definition-field row from ONE flat cell buffer at
//! `term_flushln()` time; streaming approximations at word boundaries
//! provably diverge (a `\z` glyph's `TERMP_BACKBEFORE` retreat eats the blank
//! before the NEXT glyph, an empty operand emits a real separator blank,
//! and a rejected pass clears the whole unprinted remainder). This module
//! records those cells in source order and answers the pass questions the
//! renderer asked, with the same record/break/overflow arithmetic.
//!
/// Reference: term.c v1.295 — `bufferc()` 857-870, `term_flushln()` pass
/// loop 123-231 with the nbr==0 break 143-146, blank consumption 205-207,
/// BRIND continuation 229-230, and reset 233-237; `term_fill()` 263-367
/// (`breakline`/`graph` locals, blank record 296-300, break 294, graph
/// overflow early return 350-351, tail acceptance 362-366); `term_field()`
/// 374-444; `encode1()` BACKBEFORE retreat 901-908 (blank: `col--`,
/// otherwise buffer `'\b'`); `term_word()` separator blanks 573-576.
use std::sync::Arc;

use super::tab_stops::TabStops;

const EN: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mandoc) enum FieldCell {
    /// One printable graph with its terminal width.
    Graph { text: char, width: usize },
    /// An ordinary breakable blank (`bufferc(' ')`, term.c:576 and 574-576
    /// for empty operands).
    BreakableBlank,
    /// A non-breaking blank (`ASCII_NBRSP`: `\~`, `\0`, KEPT separators).
    /// `term_fill` counts its width (342-347) and never breaks on it.
    NonBreakingBlank,
    /// A `\p` break marker (`bufferc('\n')`, term.c:657-658). A pass only
    /// arms its LOCAL `breakline` from it (304-306); `term_field` skips it.
    BreakMarker,
    /// A literal tab in the word (`bufferc('\t')` through `encode()`,
    /// term.c:946-958). `term_fill` advances to the next configured tab stop
    /// and counts it as a graph (term.c:337); the `term_flushln()` tail
    /// scan skips it while `TERMP_BRTRSP` moves `vbr` to the next stop
    /// (term.c:179-182). The periodic stops come from
    /// `term_tab_set(p, "T"); term_tab_set(p, ".5i")` (mdoc_term.c:257-259,
    /// man_term.c:160-162): 120 basic units, 24 per character cell.
    Tab,
    /// Source-row reference inserted by `term_tab_ref()` (term.c:873-878).
    /// It changes the local tab origin, not text or graph occupancy.
    TabReference,
    /// `ASCII_NBRZW`: a native buffer cell and graph with zero width.
    ZeroWidthGraph,
    /// A zero-width breakpoint `\:` on the ascii device (`ASCII_BREAK`,
    /// term.c:287-300). Shares the breakable-blank arm with no width of
    /// its own: a pass may break at it, records it as the resume candidate
    /// after a graph, and never prints it (term.c:396-398). Unproduced
    /// while mant is single-device UTF-8 (`\:` buffers `ASCII_NBRZW`
    /// there, chars.c:53); preserved as the -Tascii implementation point.
    #[expect(dead_code)]
    Breakpoint,
    /// The `'\b'` `encode1()` buffers when a BACKBEFORE retreat meets a
    /// non-blank predecessor (term.c:906): fill subtracts the width of the
    /// cell before it (283-286), then the following graph adds its own.
    Backline,
}

/// Writes produced by the text executor before semantic projection.
#[derive(Clone, Debug)]
pub(in crate::mandoc) enum FieldWrite {
    Cell(FieldCell),
    /// A generated Unicode escaped space whose semantic column may already
    /// belong to the detached HEAD. It always executes encode1(U+00A0).
    OwnedBlank {
        projected: bool,
    },
    /// Unknown SPECIAL/invalid NUMBERED buffers `ASCII_NBRZW` directly.
    /// Recovery spelling owns output scalars but is never native width or
    /// an encode1 glyph (term.c:610-638).
    RecoveryGlyph {
        projected_scalars: usize,
    },
    ArmBackafter,
    CancelBackafter,
}

impl FieldWrite {
    pub(in crate::mandoc) fn literal(value: &str) -> Vec<Self> {
        value.chars().map(Self::literal_cell).collect()
    }

    pub(in crate::mandoc) fn append_literal(writes: &mut Vec<Self>, value: &str) {
        writes.extend(value.chars().map(Self::literal_cell));
    }

    fn literal_cell(character: char) -> Self {
        Self::Cell(match character {
            ' ' => FieldCell::BreakableBlank,
            '\n' => FieldCell::BreakMarker,
            '\t' => FieldCell::Tab,
            // The frozen Unicode reader executes escaped spaces through
            // ESCAPE_SPECIAL -> encode1(U+00A0), not bufferc(ASCII_NBRSP).
            // KEEP's automatic separators remain direct buffered cells.
            '\u{8}' => FieldCell::Backline,
            _ => {
                let mut utf8 = [0u8; 4];
                FieldCell::Graph {
                    text: character,
                    width: mant_ir::geometry::text_width(character.encode_utf8(&mut utf8)),
                }
            }
        })
    }
}
/// Post-execution ownership interval of one word's writes: the landing
/// facts the recorder must register its anchor from, never a prediction
/// made before execution (a BACKBEFORE retreat can pop the separator
/// blank or a previous word's trailing blank, term.c:901-908).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct WordWriteReceipt {
    /// Index of the first cell this word's writes produced that survives
    /// execution with projection ownership. A leading `Backline` is a
    /// zero-width pairing cell, so the paired graph owns the content.
    /// Equal to `end_cell` when the writes produced no cell.
    pub(super) first_content_cell: usize,
    /// `cells.len()` after the writes executed.
    pub(super) end_cell: usize,
}

/// One `term_fill()` result: the slice accepted for the current output
/// line (`nbr` bytes, `vbr` visual width).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct FillPass {
    /// term.c `nbr`: buffer index ENDING the accepted slice (exclusive).
    pub(super) end: usize,
    /// term.c `vbr`, rounded to character columns for IR layout.
    pub(super) width: usize,
    /// Preserve the device's basic units until field-fit decisions finish.
    pub(super) units: usize,
}

impl FillPass {
    fn new(accepted_end: usize, accepted_units: usize) -> Self {
        Self {
            end: accepted_end,
            width: columns(accepted_units),
            units: accepted_units,
        }
    }
}

/// Actual first and continuation field bounds in basic units at one flush checkpoint.
/// BRNEVER widens only the scanner bound, not the tab-reference advance.
#[derive(Clone, Copy)]
pub(super) struct FillTargets {
    pub(super) first: usize,
    pub(super) rest: usize,
    pub(super) unbounded: bool,
}

impl FillTargets {
    pub(super) fn actual(self, first: bool) -> usize {
        if first { self.first } else { self.rest }
    }

    pub(super) fn scan(self, first: bool) -> usize {
        if self.unbounded {
            usize::MAX / 2
        } else {
            self.actual(first)
        }
    }
}

/// The complete pass-loop decision. Rejection of the first pass is
/// distinct from an inapplicable field and from acceptance.
#[derive(Clone, Debug)]
pub(super) enum FlushReceipt {
    Accepted {
        passes: Vec<FillPass>,
    },
    Rejected {
        passes: Vec<FillPass>,
        rejected_from: usize,
        definitive: bool,
    },
}

#[derive(Clone, Copy, Debug, Default)]
enum WordScanPolicy {
    #[default]
    Incremental,
    AwaitFlush,
}

/// The unflushed input field of `term.c::term_flushln()`, kept across the
/// words of one native field.
#[derive(Clone, Debug, Default)]
pub(super) struct FieldBuffer {
    cells: Vec<FieldCell>,
    tabs: Arc<TabStops>,
    /// Persistent tcol->taboff; each pass starts with this reference.
    tab_offset: i64,
    /// Offset after the established scan prefix's passes; the initial offset
    /// above remains available for the complete device receipt.
    pass_tab_offset: i64,
    generation: u64,
    /// Scalar ownership attached to each native write. Replaced zero-advance
    /// graphs remain native graph cells but contribute no projected glyph.
    projection_prefix: Vec<usize>,
    pending_projection_graph: Option<usize>,
    /// `p->tcol->col`: the next pass resumes here (`term_field` advances it
    /// by `nbr`; 205-207 then consumes break blanks; 235 resets it).
    resume: usize,
    /// A completed `\z` glyph left `TERMP_BACKBEFORE` armed: the next graph
    /// retreats over the cell before it (term.c:901-908).
    backbefore_armed: bool,
    backafter_armed: bool,
    committed_passes: Vec<FillPass>,
    /// A configuration change invalidated pending width scans. Subsequent
    /// words only append cells until the real flush runs one fresh scan.
    /// Repeated .ta/word pairs must not replay their growing field history.
    word_scan: WordScanPolicy,
    last_break_marker: Option<usize>,
    word_space_ready: bool,
    significant_positions: Vec<usize>,
    blank_positions: Vec<usize>,
    scan: Option<FillScanner>,
    #[cfg(test)]
    scan_work: usize,
    #[cfg(test)]
    projection_work: usize,
    /// Landing of the first cell pushed by the word currently executing
    /// its writes; tracked only while `apply_writes` runs.
    word_first_content: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default)]
struct FillRegisters {
    index: usize,
    nbr: usize,
    vbr: usize,
    vis: usize,
    breakline: bool,
    graph: bool,
    tab_offset: i64,
}

#[derive(Clone, Debug)]
struct FillScanner {
    target: usize,
    registers: FillRegisters,
    before_last: FillRegisters,
    stopped: Option<PassStop>,
}

#[derive(Clone, Copy, Debug)]
enum PassStop {
    Accepted(FillPass),
    Rejected,
}
impl PassStop {
    fn accepted(self) -> Option<FillPass> {
        match self {
            Self::Accepted(pass) => Some(pass),
            Self::Rejected => None,
        }
    }
}
impl From<Option<FillPass>> for PassStop {
    fn from(pass: Option<FillPass>) -> Self {
        pass.map_or(Self::Rejected, Self::Accepted)
    }
}

impl FieldBuffer {
    fn push_cell(&mut self, cell: FieldCell) {
        if self.word_first_content.is_none() {
            self.word_first_content = Some(self.cells.len());
        }
        match &cell {
            FieldCell::BreakableBlank => self.blank_positions.push(self.cells.len()),
            FieldCell::BreakMarker
            | FieldCell::Tab
            | FieldCell::TabReference
            | FieldCell::ZeroWidthGraph
            | FieldCell::Breakpoint => {}
            _ => self.significant_positions.push(self.cells.len()),
        }
        let projection = usize::from(matches!(
            cell,
            FieldCell::Graph { .. }
                | FieldCell::BreakableBlank
                | FieldCell::NonBreakingBlank
                | FieldCell::BreakMarker
                | FieldCell::Tab
        ));
        if self.projection_prefix.is_empty() {
            self.projection_prefix.push(0);
        }
        self.projection_prefix
            .push(self.projection_prefix.last().copied().unwrap_or(0) + projection);
        self.cells.push(cell);
    }

    pub(super) fn position(&self) -> (u64, usize) {
        (self.generation, self.cells.len())
    }

    /// A compact semantic operand has no visible glyph range. It still owns
    /// these native cells, so their graph and pass decisions are unchanged.
    pub(super) fn hide_projection_since(&mut self, (generation, start): (u64, usize)) {
        let start = if generation == self.generation {
            start.min(self.cells.len())
        } else {
            0
        };
        let mut total = self.projection_prefix.get(start).copied().unwrap_or(0);
        for index in start..self.cells.len() {
            #[cfg(test)]
            {
                self.projection_work += 1;
            }
            total += usize::from(matches!(self.cells[index], FieldCell::BreakMarker));
            self.projection_prefix[index + 1] = total;
        }
    }

    pub(super) fn begin_word(&mut self, tight: bool, spacing: bool, kept: bool) -> usize {
        let separator = usize::from(self.word_space_ready && !tight && spacing);
        self.word_space_ready = true;
        if separator > 0 {
            if kept {
                self.push_non_breaking_blank();
            } else {
                self.push_separator_blank();
            }
        }
        separator
    }

    pub(super) fn has_pending_break_markers(&self) -> bool {
        self.last_break_marker
            .is_some_and(|marker| marker >= self.resume)
    }

    /// A committed scan prefix is still part of this field until a real
    /// flush retires its cells. An explicit marker's field policy therefore
    /// survives scanning past it, but never survives field retirement.
    pub(super) fn has_break_markers(&self) -> bool {
        self.last_break_marker.is_some()
    }

    /// The live pass scanner, rather than projected glyph booleans, proves
    /// whether a pending break's current native interval supplied graph.
    pub(super) fn pending_pass_is_graphless(&self) -> bool {
        self.has_pending_break_markers()
            && self.scan.as_ref().is_some_and(|scan| {
                matches!(scan.stopped, Some(PassStop::Rejected))
                    || (scan.registers.nbr == 0 && !scan.registers.graph)
            })
    }

    pub(super) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub(super) fn configure_tabs(&mut self, tabs: &Arc<TabStops>) -> bool {
        if !Arc::ptr_eq(&self.tabs, tabs) {
            self.tabs = tabs.clone();
            // roff_term_pre_ta() changes stops without printing the buffer.
            // Word-time scan prefixes are provisional until term_flushln():
            // their widths must be reconsidered under the new stops. Truly
            // printed fields have already been retired and have no cells.
            self.scan = None;
            self.resume = 0;
            self.committed_passes.clear();
            self.pass_tab_offset = self.tab_offset;
            if !self.cells.is_empty() {
                self.word_scan = WordScanPolicy::AwaitFlush;
            }
            return true;
        }
        false
    }

    pub(super) const fn word_scan_deferred(&self) -> bool {
        matches!(self.word_scan, WordScanPolicy::AwaitFlush)
    }

    pub(super) fn note_tab_reference(&mut self) {
        // A source row only inserts TABREF after native buffer writes.
        if !self.cells.is_empty() {
            self.push_cell(FieldCell::TabReference);
        }
    }

    pub(super) const fn tab_offset(&self) -> i64 {
        self.tab_offset
    }

    pub(super) fn set_tab_offset(&mut self, offset: i64) {
        if self.tab_offset != offset || self.pass_tab_offset != offset {
            self.tab_offset = offset;
            self.pass_tab_offset = offset;
            self.scan = None;
        }
    }

    /// term.c:233-237: the row ends and the buffer restarts empty (used
    /// both by the accepted-exit and by the nbr==0 wipe).
    pub(super) fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.cells.clear();
        self.projection_prefix.clear();
        self.pending_projection_graph = None;
        self.resume = 0;
        self.backbefore_armed = false;
        self.backafter_armed = false;
        self.committed_passes.clear();
        self.word_scan = WordScanPolicy::Incremental;
        self.last_break_marker = None;
        self.word_space_ready = false;
        self.word_first_content = None;
        self.significant_positions.clear();
        self.blank_positions.clear();
        self.scan = None;
        self.pass_tab_offset = self.tab_offset;
    }

    /// `term_flushln()`235-237 consumes the input field without resetting
    /// `term_word()`'s NOSPACE lifecycle. A later `term_newln()` or macro pre
    /// decides that boundary separately; device viscol/minbl live elsewhere.
    pub(super) fn clear_consumed_field(&mut self) {
        let word_space_ready = self.word_space_ready;
        self.clear();
        self.word_space_ready = word_space_ready;
    }

    /// Execute decoded native writes without consulting projected IR.
    pub(super) fn apply_writes(&mut self, writes: &[FieldWrite]) -> WordWriteReceipt {
        self.word_first_content = None;
        for write in writes {
            match write {
                FieldWrite::Cell(FieldCell::Graph { text, width }) => {
                    self.encode_graph(*text, *width, true);
                }
                FieldWrite::OwnedBlank { projected } => self.encode_graph('\u{a0}', 1, *projected),
                FieldWrite::Cell(cell) => {
                    if matches!(cell, FieldCell::BreakMarker) {
                        self.last_break_marker = Some(self.cells.len());
                    }
                    self.push_cell(cell.clone());
                }
                FieldWrite::RecoveryGlyph { projected_scalars } => {
                    self.push_cell(FieldCell::ZeroWidthGraph);
                    *self.projection_prefix.last_mut().expect("recovery cell") += projected_scalars;
                }
                FieldWrite::ArmBackafter => self.backafter_armed = true,
                FieldWrite::CancelBackafter => self.backafter_armed = false,
            }
        }
        let end_cell = self.cells.len();
        let mut first_content_cell = self.word_first_content.take().unwrap_or(end_cell);
        if matches!(
            self.cells.get(first_content_cell),
            Some(FieldCell::Backline)
        ) {
            first_content_cell += 1;
        }
        WordWriteReceipt {
            first_content_cell,
            end_cell,
        }
    }
    fn encode_graph(&mut self, text: char, width: usize, projected: bool) {
        self.push_graph(text, width);
        if !projected {
            let prefix = self.projection_prefix.last_mut().expect("encoded graph");
            *prefix = prefix.saturating_sub(1);
        }
        if self.backafter_armed {
            self.backafter_armed = false;
            self.backbefore_armed = true;
            self.pending_projection_graph = self.cells.len().checked_sub(1);
        }
    }

    /// A drained IR owner is immutable. Backtracking remains native state,
    /// but cannot revoke a glyph already assigned to the previous owner.
    pub(super) fn detach_projection_owner(&mut self) {
        self.pending_projection_graph = None;
    }

    pub(super) fn push_separator_blank(&mut self) {
        self.push_cell(FieldCell::BreakableBlank);
    }

    /// `bufferc('\n')` for `\p` (term.c:657-658).
    #[cfg(test)]
    pub(super) fn push_break_marker(&mut self) {
        self.last_break_marker = Some(self.cells.len());
        self.push_cell(FieldCell::BreakMarker);
    }

    /// A direct buffered fixed-width blank: automatic KEEP separators
    /// (term.c:574-580). Escaped Unicode spaces use `encode1()` instead.
    pub(super) fn push_non_breaking_blank(&mut self) {
        self.push_cell(FieldCell::NonBreakingBlank);
    }

    /// `encode1()` writes a graph (term.c:345-349 tail with 901-908): a
    /// pending `TERMP_BACKBEFORE` retreat first consumes the blank directly
    /// before it (`col--`), or buffers `'\b'` over a non-blank cell.
    pub(super) fn push_graph(&mut self, text: char, width: usize) {
        if self.backbefore_armed {
            self.backbefore_armed = false;
            match self.cells.last() {
                // A literal tab occupies a buffer byte exactly like a
                // blank (term.c:944-964) and the retreat pops it the same
                // way (`buf[col-1] == '\t'`, term.c:901-904). It never
                // registered in `blank_positions`, so leave that ledger
                // untouched. A NonBreakingBlank buffers ASCII_NBRSP, a
                // different byte, and keeps the `'\b'` arm below.
                Some(FieldCell::Tab | FieldCell::BreakableBlank) => {
                    self.invalidate_projection_scan(self.cells.len().saturating_sub(1));
                    if matches!(self.cells.last(), Some(FieldCell::BreakableBlank)) {
                        self.blank_positions.pop();
                    }
                    self.cells.pop();
                    self.projection_prefix.pop();
                    self.pending_projection_graph = None;
                }
                Some(_) => {
                    if let Some(graph) = self.pending_projection_graph.take() {
                        // A glyph can stay pending across invisible controls.
                        // Each replacement advances the pending graph, so
                        // these adjusted suffixes are disjoint over a field.
                        let contribution = self.projection_prefix[graph + 1]
                            .saturating_sub(self.projection_prefix[graph]);
                        for prefix in &mut self.projection_prefix[graph + 1..] {
                            #[cfg(test)]
                            {
                                self.projection_work += 1;
                            }
                            *prefix = prefix.saturating_sub(contribution);
                        }
                    }
                    self.push_cell(FieldCell::Backline);
                }
                _ => {}
            }
        }
        self.push_cell(FieldCell::Graph { text, width });
    }

    /// Arm the retreat for the NEXT glyph after a completed `\z` glyph
    /// (encode1 tail, term.c:924-927: BACKAFTER converts to BACKBEFORE).
    #[cfg(test)]
    pub(super) fn arm_backbefore(&mut self) {
        self.backbefore_armed = true;
    }

    /// `term_flushln()` clears both backtracking flags with the row
    /// (term.c:235-237).
    pub(super) fn clear_backtracking(&mut self) {
        self.backbefore_armed = false;
    }

    /// One `term_fill()` pass from `resume` (term.c:263-367).
    ///
    /// Returns `None` for the nbr=0 rejection (143-146): nothing in this
    /// slice may print and the caller wipes the remainder. Otherwise the
    /// accepted slice is `cells[..end]`; the breaking blank (if
    /// any) sits at `accepted_end` and is consumed separately by
    /// [`Self::consume_break_blanks`] (term.c:205-207).
    /// Continue a pass from its last inspected cell. Appending a word does
    /// not restart `term_fill()` over the cumulative field. The last word is
    /// provisional; only a real stop commits nbr. BACKBEFORE can rewrite the
    /// final blank, so keep the checkpoint immediately before that cell.
    #[cfg(test)]
    pub(super) fn fill_pass(&mut self, vtarget: usize) -> Option<FillPass> {
        self.fill_pass_units(vtarget.saturating_mul(EN))
    }

    pub(super) fn fill_pass_units(&mut self, vtarget: usize) -> Option<FillPass> {
        // term_fill() compares basic units with half an EN of tolerance.
        let vtarget = vtarget.saturating_add(EN / 2);
        if self.scan.as_ref().is_none_or(|scan| scan.target != vtarget) {
            let registers = FillRegisters {
                index: self.resume,
                tab_offset: self.pass_tab_offset,
                ..FillRegisters::default()
            };
            self.scan = Some(FillScanner {
                target: vtarget,
                registers,
                before_last: registers,
                stopped: None,
            });
        }
        let scan = self.scan.as_mut().expect("initialized field scan");
        if let Some(result) = scan.stopped {
            return result.accepted();
        }
        while scan.registers.index < self.cells.len() {
            #[cfg(test)]
            {
                self.scan_work += 1;
            }
            scan.before_last = scan.registers;
            let registers = &mut scan.registers;
            let ic = registers.index;
            match self.cells[ic] {
                FieldCell::Backline => {
                    let width =
                        ic.checked_sub(1)
                            .map_or(0, |previous| match self.cells[previous] {
                                FieldCell::Graph { width, .. } => width.saturating_mul(EN),
                                FieldCell::NonBreakingBlank | FieldCell::BreakableBlank => EN,
                                _ => 0,
                            });
                    registers.vis = registers.vis.saturating_sub(width);
                }
                FieldCell::BreakableBlank | FieldCell::Breakpoint => {
                    let vn = registers.vis
                        + EN * usize::from(matches!(self.cells[ic], FieldCell::BreakableBlank));
                    if registers.breakline || vn > vtarget {
                        let result = finish_pass(*registers, vtarget);
                        scan.stopped = Some(PassStop::from(result));
                        return result;
                    }
                    if registers.graph {
                        registers.nbr = ic;
                        registers.vbr = registers.vis;
                        registers.graph = false;
                    }
                    registers.vis = vn;
                }
                FieldCell::BreakMarker => registers.breakline = true,
                FieldCell::ZeroWidthGraph => registers.graph = true,
                FieldCell::NonBreakingBlank | FieldCell::Graph { .. } => {
                    let width = match self.cells[ic] {
                        FieldCell::Graph { width, .. } => width,
                        _ => 1,
                    };
                    registers.vis += width.saturating_mul(EN);
                    registers.graph = true;
                    if registers.vis > vtarget && registers.nbr > 0 {
                        let result = Some(FillPass::new(registers.nbr, registers.vbr));
                        scan.stopped = Some(PassStop::from(result));
                        return result;
                    }
                }
                FieldCell::Tab => {
                    // term.c:327-338: TABREF and the persistent tcol offset
                    // determine the origin under the current .ta settings.
                    registers.vis = advance_tab(&self.tabs, registers.vis, registers.tab_offset);
                    registers.graph = true;
                    if registers.vis > vtarget && registers.nbr > 0 {
                        let result = Some(FillPass::new(registers.nbr, registers.vbr));
                        scan.stopped = Some(PassStop::from(result));
                        return result;
                    }
                }
                FieldCell::TabReference => {
                    registers.tab_offset = -signed(registers.vis).saturating_add(signed(EN));
                }
            }
            registers.index += 1;
        }
        finish_pass(scan.registers, vtarget)
    }

    fn invalidate_projection_scan(&mut self, position: usize) {
        let Some(scan) = &mut self.scan else {
            return;
        };
        if position + 1 == scan.registers.index {
            scan.registers = scan.before_last;
            scan.stopped = None;
        } else if position < scan.registers.index {
            self.scan = None;
        } else if position == scan.registers.index {
            scan.stopped = None;
        }
    }

    pub(super) fn flush_receipt(&self, targets: FillTargets, brtrsp: bool) -> FlushReceipt {
        let mut scan = self.clone();
        let mut passes = self.committed_passes.clone();
        loop {
            let first = passes.is_empty();
            let Some(pass) = scan.fill_pass_units(targets.scan(first)) else {
                return FlushReceipt::Rejected {
                    passes,
                    rejected_from: scan.resume_offset(),
                    definitive: scan
                        .scan
                        .as_ref()
                        .is_some_and(|scanner| scanner.stopped.is_some()),
                };
            };
            passes.push(pass);
            // term.c:165-168 advances the reference after each printed pass.
            scan.advance_tab_offset(pass.units, targets.actual(first));
            scan.advance_past(pass.end);
            // term_flushln() tests the remaining buffer before consuming
            // ordinary blanks at a genuine continuation boundary.
            if scan.resume_offset() >= scan.cells.len() || scan.only_ignorable_remainder(brtrsp) {
                return FlushReceipt::Accepted { passes };
            }
            scan.consume_break_blanks();
        }
    }

    /// `term_field()`374-444 prints buffered padding only when a real
    /// encoded glyph follows it. Internal NBRZW and recovery spellings do
    /// not advance the device; Unicode whitespace glyphs do.
    pub(super) fn printed_columns(
        &self,
        start: usize,
        end: usize,
        mut tab_offset: i64,
        initial_padding: usize,
    ) -> Option<usize> {
        let mut column = 0usize;
        let mut pending_units = initial_padding;
        let mut device_columns = 0usize;
        let mut printed = None;
        for index in start..end.min(self.cells.len()) {
            match self.cells[index] {
                FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => {
                    column += EN;
                    pending_units += EN;
                }
                FieldCell::Graph { width, .. } => {
                    // term_field() advances deferred blanks before EACH
                    // graph. ascii_advance() rounds that individual advance
                    // with half-EN tolerance, capped at 256 EN; rounding the
                    // complete field instead changes later vfield decisions
                    // after fractional tabs (term_ascii.c:279-299).
                    device_columns += columns(pending_units.min(256 * EN)) + width;
                    pending_units = 0;
                    column += width.saturating_mul(EN);
                    printed = Some(device_columns);
                }
                FieldCell::Backline => {
                    let width = match index.checked_sub(1).and_then(|i| self.cells.get(i)) {
                        Some(FieldCell::Graph { width, .. }) => width.saturating_mul(EN),
                        Some(FieldCell::BreakableBlank | FieldCell::NonBreakingBlank) => EN,
                        _ => 0,
                    };
                    device_columns = (device_columns + columns(pending_units.min(256 * EN)))
                        .saturating_sub(columns(width));
                    pending_units = 0;
                    column = column.saturating_sub(width);
                    printed = Some(device_columns);
                }
                FieldCell::Tab => {
                    // term_field() treats a tab like deferred whitespace:
                    // the advance only flushes when a later graph prints.
                    let next = advance_tab(&self.tabs, column, tab_offset);
                    pending_units += next.saturating_sub(column);
                    column = next;
                }
                FieldCell::TabReference => {
                    tab_offset = -signed(column).saturating_add(signed(EN));
                }
                FieldCell::BreakMarker | FieldCell::ZeroWidthGraph | FieldCell::Breakpoint => {}
            }
        }
        printed
    }

    /// Translate only projection positions, never native execution facts.
    /// Invisible native cells have no corresponding scalar in semantic IR.
    pub(super) fn projection_length(&self, start: usize, end: usize) -> usize {
        let low = start.min(self.cells.len());
        let high = end.min(self.cells.len());
        self.projection_prefix
            .get(high)
            .copied()
            .unwrap_or(0)
            .saturating_sub(self.projection_prefix.get(low).copied().unwrap_or(0))
    }

    /// A pass with a genuine following suffix is established for ordinary
    /// appends. A changed Tab configuration invalidates these pending scans;
    /// a real flush retires them. Neither operation re-feeds printed fields.
    pub(super) fn commit_pass(&mut self, pass: FillPass, tab_target: usize) {
        self.advance_tab_offset(pass.units, tab_target);
        self.committed_passes.push(pass);
        self.scan = None;
        self.advance_past(pass.end);
        self.consume_break_blanks();
    }

    pub(super) fn has_committed_pass(&self) -> bool {
        !self.committed_passes.is_empty()
    }

    pub(super) fn committed_pass_count(&self) -> usize {
        self.committed_passes.len()
    }

    pub(super) fn has_non_ignorable_after(&self, position: usize, brtrsp: bool) -> bool {
        self.significant_positions
            .last()
            .is_some_and(|&index| index >= position)
            || (brtrsp
                && self
                    .blank_positions
                    .last()
                    .is_some_and(|&index| index >= position))
    }

    /// term.c:205-207: input blanks at the resume point are consumed by
    /// the automatic line break itself.
    pub(super) fn consume_break_blanks(&mut self) {
        while self.resume < self.cells.len()
            && matches!(self.cells[self.resume], FieldCell::BreakableBlank)
        {
            self.resume += 1;
        }
    }

    /// The nbr=0 wipe (term.c:233-237 reached through 145-146): the whole
    /// unprinted remainder dies with the row.
    #[cfg(test)]
    pub(super) fn wipe_remainder(&mut self) {
        self.clear();
    }

    /// `term_field()` committed `nbr` cells and advanced `col`
    /// (term.c:443); drop them so later passes index the remainder.
    pub(super) fn advance_past(&mut self, accepted_end: usize) {
        self.resume = self.resume.max(accepted_end.min(self.cells.len()));
        self.scan = None;
    }

    /// term.c:177-198: whether anything but ignorable cells (blanks,
    /// markers; trailing blanks under BRTRSP count) remains unprinted.
    pub(super) fn only_ignorable_remainder(&self, brtrsp: bool) -> bool {
        !self.has_non_ignorable_after(self.resume, brtrsp)
    }

    /// term.c:177-196: the trailing ignorable-cell sweep `term_flushln()`
    /// runs after each printed pass. Starting from the last pass's
    /// `accepted_end` with its `vbr`, blanks add one EN and a tab jumps
    /// to the next configured stop while `TERMP_BRTRSP` is set; markers and
    /// zero-width cells never stop the sweep, everything else does. The
    /// result is the `vbr` the final row decision at term.c:250-253 sees.
    pub(super) fn brtrsp_tail_sweep(&self, from: usize, vbr: usize, brtrsp: bool) -> usize {
        let mut vbr = vbr;
        for cell in self.cells.iter().skip(from.min(self.cells.len())) {
            match cell {
                FieldCell::BreakableBlank => {
                    if brtrsp {
                        vbr += EN;
                    }
                }
                FieldCell::Tab => {
                    if brtrsp {
                        vbr = self.tabs.next_stop(vbr);
                    }
                }
                FieldCell::BreakMarker
                | FieldCell::TabReference
                | FieldCell::ZeroWidthGraph
                | FieldCell::Breakpoint => {}
                FieldCell::Graph { .. } | FieldCell::NonBreakingBlank | FieldCell::Backline => {
                    break;
                }
            }
        }
        vbr
    }

    pub(super) fn cells(&self) -> &[FieldCell] {
        &self.cells
    }

    pub(super) fn resume_offset(&self) -> usize {
        self.resume
    }

    fn advance_tab_offset(&mut self, width: usize, target: usize) {
        self.pass_tab_offset = self
            .pass_tab_offset
            .saturating_add(signed(width.min(target)))
            .saturating_add(signed(EN));
    }
}

fn finish_pass(registers: FillRegisters, target: usize) -> Option<FillPass> {
    let mut end = registers.nbr;
    let mut width = registers.vbr;
    // term_fill() accepts an unbroken final word even beyond the margin.
    if registers.graph && (registers.vis <= target || end == 0) {
        end = registers.index;
        width = registers.vis;
    }
    (end > 0).then_some(FillPass::new(end, width))
}

fn columns(units: usize) -> usize {
    units.saturating_add((EN - 1) / 2) / EN
}

fn signed(units: usize) -> i64 {
    i64::try_from(units).unwrap_or(i64::MAX)
}

fn advance_tab(tabs: &TabStops, vis: usize, offset: i64) -> usize {
    let origin = signed(vis).saturating_add(offset).max(0);
    let next = tabs.next_stop(usize::try_from(origin).unwrap_or(usize::MAX));
    usize::try_from(signed(next).saturating_sub(offset)).unwrap_or(0)
}

#[cfg(test)]
mod term_fill_contract_tests {
    use super::{FieldBuffer, FieldCell};

    /// (a) of the fixed-CVS oracle pair: `\zX\p` + `\p Y` keeps Y. The
    /// retreat eats the blank before Y, so pass two resumes at the second
    /// marker and accepts Y itself (term.c:901-908 with 263-367; pass one
    /// accepts `X` + marker, nbr = the breaking blank's index).
    #[test]
    fn retreat_blank_keeps_the_following_graph() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('X', 1);
        buffer.arm_backbefore();
        buffer.push_break_marker(); // \p of word one
        buffer.push_separator_blank(); // word two's separator
        buffer.push_break_marker(); // \p of word two
        buffer.push_separator_blank(); // " Y" operand blank
        buffer.push_graph('Y', 1); // retreat eats the operand blank
        let first = buffer
            .fill_pass(usize::MAX / 2)
            .expect("pass one accepts X");
        assert_eq!(first.end, 2, "X + marker; blank breaks");
        buffer.advance_past(first.end);
        buffer.consume_break_blanks();
        let second = buffer
            .fill_pass(usize::MAX / 2)
            .expect("pass two accepts Y");
        assert!(second.end >= 4);
        assert_eq!(second.width, 1);
    }

    /// (b) of the pair: the empty operand's real blank survives after the
    /// marker, so the next pass stops graphless at it (nbr stays 0 through
    /// term.c:362-366) and the remainder is wiped (143-146 with 233-237).
    #[test]
    fn blank_after_marker_rejects_and_wipes() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('X', 1);
        buffer.arm_backbefore();
        buffer.push_break_marker(); // word one's \p
        buffer.push_separator_blank(); // word two's separator
        buffer.push_break_marker(); // word two's \p
        buffer.push_separator_blank(); // `.No ""` term_word("") blank
        buffer.push_separator_blank(); // Y's own word separator
        buffer.push_graph('Y', 1); // retreat eats only Y's own blank
        let first = buffer
            .fill_pass(usize::MAX / 2)
            .expect("pass one accepts X");
        buffer.advance_past(first.end);
        buffer.consume_break_blanks();
        assert!(
            buffer.fill_pass(usize::MAX / 2).is_none(),
            "the surviving blank under breakline rejects the pass"
        );
        buffer.wipe_remainder();
        assert!(buffer.is_empty());
    }

    /// An armed marker with no graph before the stop also rejects: `\p`
    /// alone before a separator blank never prints (term.c:294 with
    /// 362-366 leaving nbr at 0).
    #[test]
    fn armed_marker_without_graph_rejects() {
        let mut buffer = FieldBuffer::default();
        buffer.push_break_marker();
        buffer.push_separator_blank();
        buffer.push_graph('X', 1);
        assert!(buffer.fill_pass(usize::MAX / 2).is_none());
    }

    /// term.c:350-351: a graph that overruns the target returns at the
    /// last recorded blank; the blank itself is consumed with the break
    /// (205-207), so the next pass starts at the overflowing word.
    #[test]
    fn vtarget_overrun_breaks_at_the_last_recorded_blank() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('a', 1);
        buffer.push_separator_blank();
        buffer.push_graph('b', 1);
        buffer.push_separator_blank();
        buffer.push_graph('c', 1);
        let pass = buffer.fill_pass(2).expect("breaks inside the slice");
        assert_eq!(pass.end, 1, "accepts `a`; blank consumed");
        buffer.advance_past(pass.end);
        buffer.consume_break_blanks();
        assert_eq!(buffer.resume_offset(), 2, "next pass starts at `b`");
    }

    /// term.c:362-366: a word running to the buffer end is accepted whole,
    /// even past the target (nbr==0 has no recorded candidate to return
    /// to; the 350-351 guard only fires when one exists).
    #[test]
    fn trailing_word_is_accepted_whole() {
        let mut buffer = FieldBuffer::default();
        buffer.push_separator_blank();
        buffer.push_graph('a', 1);
        buffer.push_graph('b', 1);
        let pass = buffer.fill_pass(1).expect("whole word accepted");
        assert_eq!(pass.end, 3);
        assert_eq!(pass.width, 3, "the leading blank counts width");
    }

    /// A non-breaking blank counts width and never breaks
    /// (`ASCII_NBRSP` through the default branch, term.c:342-347).
    #[test]
    fn non_breaking_blank_counts_width_without_breaking() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('a', 1);
        buffer.push_non_breaking_blank();
        buffer.push_graph('b', 1);
        let pass = buffer.fill_pass(1).expect("nbr does not stop the pass");
        assert_eq!(pass.end, 3);
        assert_eq!(pass.width, 3);
    }

    /// term.c:283-286 with 906: a BACKBEFORE retreat over a graph buffers
    /// `'\b'`; fill subtracts the predecessor's width before the new graph
    /// adds its own, so a `\z` overstrike keeps one column.
    #[test]
    fn backline_retreat_subtracts_the_previous_width() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('X', 1);
        buffer.arm_backbefore();
        buffer.push_graph('Y', 1); // buffers '\b' over X
        assert_eq!(
            buffer.cells(),
            &[
                super::FieldCell::Graph {
                    text: 'X',
                    width: 1
                },
                super::FieldCell::Backline,
                super::FieldCell::Graph {
                    text: 'Y',
                    width: 1
                },
            ][..]
        );
        let pass = buffer.fill_pass(usize::MAX / 2).expect("accepted");
        assert_eq!(pass.width, 1, "overstrike keeps one column");
    }
    #[test]
    fn source_cells_and_projection_ownership_are_independent() {
        use super::{FieldCell, FieldWrite};
        // Exact pristine fixture overstrike_projection.roff renders C. The
        // native A/B cells stay graphs (encode1), while their zero-advance
        // projections are replaced. Neither fact can be inferred from IR.
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&[
            FieldWrite::ArmBackafter,
            FieldWrite::Cell(FieldCell::Graph {
                text: 'A',
                width: 1,
            }),
            FieldWrite::ArmBackafter,
            FieldWrite::Cell(FieldCell::Graph {
                text: 'B',
                width: 1,
            }),
            FieldWrite::Cell(FieldCell::Graph {
                text: 'C',
                width: 1,
            }),
        ]);
        assert_eq!(
            buffer
                .cells()
                .iter()
                .filter(|cell| matches!(cell, FieldCell::Graph { .. }))
                .count(),
            3
        );
        assert_eq!(buffer.projection_length(0, buffer.cells().len()), 1);
        assert!(buffer.fill_pass(usize::MAX / 2).is_some());
    }

    #[test]
    fn unicode_spaces_and_zero_width_scalars_use_encode1() {
        // Exact pristine fixed-space-native and zero-unicode-native inputs
        // were run before this assertion. term_word()629 and 817 call
        // encode1() for Unicode glyphs; ASCII_NBRSP/NBRZW controls are
        // separate bufferc() writes even when their visible widths agree.
        for scalar in ['\u{a0}', '\u{200b}'] {
            let mut buffer = FieldBuffer::default();
            buffer.apply_writes(&[super::FieldWrite::ArmBackafter]);
            buffer.apply_writes(&super::FieldWrite::literal(&scalar.to_string()));
            assert!(buffer.backbefore_armed);
            assert!(!buffer.backafter_armed);
            assert!(
                matches!(buffer.cells()[0], super::FieldCell::Graph { text, .. } if text == scalar)
            );
        }
    }

    #[test]
    fn detached_head_blank_executes_without_reassigning_its_projection() {
        // Exact pristine runin-head-pending/plain cases execute one generated
        // Unicode escaped space regardless of the surviving semantic gap.
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&[super::FieldWrite::ArmBackafter]);
        buffer.apply_writes(&super::FieldWrite::literal("X"));
        buffer.detach_projection_owner();
        let before = buffer.cells().len();
        buffer.apply_writes(&[super::FieldWrite::OwnedBlank { projected: false }]);
        assert!(matches!(buffer.cells()[before], super::FieldCell::Backline));
        assert!(matches!(
            buffer.cells()[before + 1],
            super::FieldCell::Graph { text: '\u{a0}', .. }
        ));
        assert_eq!(buffer.projection_length(0, before), 1, "HEAD keeps X");
        assert_eq!(buffer.projection_length(before, buffer.cells().len()), 0);
        assert!(!buffer.backbefore_armed);
    }

    #[test]
    fn growing_unbroken_suffix_is_scanned_incrementally() {
        let mut buffer = FieldBuffer::default();
        for _ in 0..16_384 {
            buffer.push_graph('a', 1);
            let _ = buffer.fill_pass(usize::MAX / 2);
        }
        assert!(buffer.scan_work <= buffer.cells().len() * 2);
    }

    #[test]
    fn many_passes_in_one_operand_do_not_rescan_projection_prefixes() {
        // Exact pristine single_operand_many_passes.roff keeps one row per
        // aa\p bb\p segment. This scale test counts source-cell work rather
        // than wall time, including the native-to-projection prefix index.
        let mut buffer = FieldBuffer::default();
        for _ in 0..8192 {
            buffer.push_graph('a', 1);
            buffer.push_graph('a', 1);
            buffer.push_break_marker();
            buffer.push_separator_blank();
        }
        buffer.push_graph('a', 1);
        let mut count = 0;
        while let Some(pass) = buffer.fill_pass(usize::MAX / 2) {
            let _ = buffer.projection_length(0, pass.end);
            count += 1;
            if !buffer.has_non_ignorable_after(pass.end, false) {
                break;
            }
            buffer.commit_pass(pass, usize::MAX / 2);
        }
        assert_eq!(count, 8193);
        assert!(buffer.scan_work <= buffer.cells().len() * 2);
        assert_eq!(
            buffer.projection_work, 0,
            "prefix lookups never walk earlier cells"
        );
    }

    #[test]
    fn ignored_tail_after_a_pending_pass_uses_cached_native_presence() {
        let mut buffer = FieldBuffer::default();
        buffer.push_graph('X', 1);
        buffer.push_break_marker();
        for _ in 0..16_384 {
            buffer.push_separator_blank();
            buffer.push_break_marker();
            let pass = buffer.fill_pass(usize::MAX / 2).expect("X prefix");
            assert!(!buffer.has_non_ignorable_after(pass.end, false));
        }
        assert!(buffer.scan_work <= buffer.cells().len() * 2);
    }

    #[test]
    fn recovery_cell_preserves_native_flags_and_separate_projection_width() {
        // Exact \z\[unknownname]YZ fixture first verified with pristine
        // CVS: raw output Y\bZ. term.c:620-638 buffers ASCII_NBRZW directly,
        // so it cannot consume BACKAFTER; encode1(Y) does, then Z replaces Y.
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&[
            super::FieldWrite::ArmBackafter,
            super::FieldWrite::RecoveryGlyph {
                projected_scalars: 0,
            },
        ]);
        buffer.apply_writes(&super::FieldWrite::literal("YZ"));
        assert_eq!(
            buffer.cells,
            [
                FieldCell::ZeroWidthGraph,
                FieldCell::Graph {
                    text: 'Y',
                    width: 1
                },
                FieldCell::Backline,
                FieldCell::Graph {
                    text: 'Z',
                    width: 1
                }
            ]
        );
        assert_eq!(buffer.projection_length(0, buffer.cells.len()), 1);

        // Ordinary unknown spelling is retained by semantic recovery only:
        // a zero-width native graph still owns its entire visible spelling.
        let mut buffer = FieldBuffer::default();
        buffer.apply_writes(&[super::FieldWrite::RecoveryGlyph {
            projected_scalars: 14,
        }]);
        let pass = buffer.fill_pass(1).expect("native zero-width graph");
        assert_eq!(pass.width, 0);
        assert_eq!(buffer.projection_length(0, pass.end), 14);
    }
}
