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
mod cells;
mod flush;
mod scan;
mod writes;

pub(super) use cells::WordWriteReceipt;
pub(in crate::mandoc) use cells::{FieldCell, FieldWrite};

use std::collections::BTreeSet;
use std::sync::Arc;

use super::tab_stops::TabStops;

const EN: usize = 24;

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
    /// Why `term_fill` stopped scanning, independent of the accepted prefix.
    /// A marker can arm breakline after the last accepted graph; the next
    /// blank then stops the pass without that marker being inside `end`.
    pub(super) boundary: FillBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FillBoundary {
    BufferEnd,
    WordEndBreak,
    Width,
    /// A native break candidate after `ASCII_HYPH`. It locates BODY's
    /// final device row without asserting a hard break in source text.
    Hyphen,
}

impl FillPass {
    fn new(accepted_end: usize, accepted_units: usize) -> Self {
        Self {
            end: accepted_end,
            width: columns(accepted_units),
            units: accepted_units,
            boundary: FillBoundary::BufferEnd,
        }
    }

    fn stopped_at(mut self, boundary: FillBoundary) -> Self {
        self.boundary = boundary;
        self
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
        native_cells: Vec<FieldCell>,
    },
    Rejected {
        passes: Vec<FillPass>,
        rejected_from: usize,
        definitive: bool,
        native_cells: Vec<FieldCell>,
    },
}

impl FlushReceipt {
    /// The normalized cells of this same `term_fill()` sweep. Source ownership
    /// remains in `FieldBuffer`, while native skip/tail rules use these bytes.
    pub(super) fn native_cells(&self) -> &[FieldCell] {
        match self {
            Self::Accepted { native_cells, .. } | Self::Rejected { native_cells, .. } => {
                native_cells
            }
        }
    }
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
pub(in crate::mandoc::inline) struct FieldBuffer {
    cells: Vec<FieldCell>,
    tabs: Arc<TabStops>,
    /// Persistent tcol->taboff; each pass starts with this reference.
    tab_offset: i64,
    /// Offset after the established scan prefix's passes; the initial offset
    /// above remains available for the complete device receipt.
    pass_tab_offset: i64,
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
    // term_fill() normalizes scanned ASCII_HYPH cells in-place, even when
    // the pass accepted an earlier prefix. Only real/committed scans own
    // this prefix; uncommitted width predictions leave native input intact.
    normalized_until: usize,
    /// A configuration change invalidated pending width scans. Subsequent
    /// words only append cells until the real flush runs one fresh scan.
    /// Repeated .ta/word pairs must not replay their growing field history.
    word_scan: WordScanPolicy,
    last_break_marker: Option<usize>,
    /// Accepted native row events already projected. A .ta change restarts
    /// width scanning, but cannot execute the same marker pass twice.
    projected_pass_ends: std::collections::BTreeSet<usize>,
    word_space_ready: bool,
    significant_positions: BTreeSet<usize>,
    blank_positions: BTreeSet<usize>,
    scan: Option<FillScanner>,
    #[cfg(test)]
    scan_work: usize,
    #[cfg(test)]
    projection_work: usize,
    /// Landing of the first cell pushed by the word currently executing
    /// its writes; tracked only while `apply_writes` runs.
    word_first_content: Option<usize>,
    /// Projection acknowledgement within this live buffer, not a native
    /// cell/width fact. Repeated views of one flush cannot emit its empty
    /// accepted-pass endline twice; retirement clears the cursor.
    completed_empty_pass_end: Option<usize>,
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
    break_candidate: Option<FillBoundary>,
}

impl FillRegisters {
    fn consume_hyphen(&mut self, target: usize) -> Option<PassStop> {
        self.graph = true;
        self.vis += EN;
        if self.vis > target {
            self.index += 1;
            let result = finish_pass(*self, target)
                .map(|pass| pass.stopped_at(self.break_candidate.unwrap_or(FillBoundary::Hyphen)));
            return Some(PassStop::from(result));
        }
        self.nbr = self.index + 1;
        self.vbr = self.vis;
        self.break_candidate = Some(FillBoundary::Hyphen);
        None
    }
}

#[derive(Clone, Debug)]
struct FillScanner {
    target: usize,
    registers: FillRegisters,
    before_last: FillRegisters,
    stopped: Option<PassStop>,
    scanned_end: usize,
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

fn cell_units(cell: &FieldCell) -> usize {
    match cell {
        FieldCell::Graph { width, .. } => width.saturating_mul(EN),
        FieldCell::Hyphen | FieldCell::BreakableBlank | FieldCell::NonBreakingBlank => EN,
        _ => 0,
    }
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
mod term_fill_contract_tests;
