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

//! Native cell writes and post-write ownership facts.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::mandoc) enum FieldCell {
    /// One printable graph with its terminal width.
    Graph { text: char, width: usize },
    /// Native `ASCII_HYPH`: its first real scan uses it as a width-break
    /// candidate and immediately converts it to '-' (term.c:307-324),
    /// including lookahead not accepted by that pass.
    Hyphen,
    /// An ordinary breakable blank (`bufferc(' ')`, term.c:576 and 574-576
    /// for empty operands).
    BreakableBlank,
    /// Consecutive generated ASCII spaces with no projected scalars.
    DrawnSpaces(usize),
    /// A compact run of the `ASCII_NBRSP` writes from motion over a drawing.
    FixedDrawingAdvance(usize),
    /// A non-breaking blank (`ASCII_NBRSP`: `\~`, `\0`, KEPT separators).
    /// Its first scan counts its width and normalizes `ASCII_NBRSP` to an
    /// ordinary blank (term.c:340-347); a later pass can break on that byte.
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
    /// A zero-width breakpoint `\:` on both devices (`ASCII_BREAK`,
    /// term.c:287-300). Shares the breakable-blank arm with no width of
    /// its own: a pass may break at it, records it as the resume candidate
    /// after a graph, and never prints it (term.c:395-397). `term_word()`
    /// writes it before the ASCII/UTF8 split (term.c:619-621), independently
    /// of chars.c's U+200B used by HTML.
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
    /// The native font wrote an underline/bold overstrike prefix before
    /// this graph. A rewind counts those bytes, whereas fill consumes the
    /// one resulting display cell (term.c:894–926).
    DecoratedGraph {
        cell: FieldCell,
        prefix: u8,
    },
    /// A generated Unicode escaped space whose semantic column may already
    /// belong to the detached HEAD. It always executes encode1(U+00A0).
    OwnedBlank {
        projected: bool,
    },
    /// A generated nonbreaking blank traversed an existing raw cell.
    AdvanceOverBufferedBlanks(usize),
    /// A drawn stroke occupies terminal cells without adding author text.
    /// Adjacent identical strokes with no intervening break are one run.
    UnprojectedGraph {
        text: char,
        width: usize,
    },
    DrawSpaces(usize),
    MoveInDrawing {
        absolute: bool,
        units: i32,
    },
    /// The motion depends on an earlier native prefix that cannot be replayed.
    UnprovedMotion,
    /// A resolved slot range. Device writing and any backing up already
    /// happened, and only its surviving source projection counts.
    Placed {
        cell: FieldCell,
        scalars: usize,
    },
    /// The already-written cell whose BACKBEFORE remains live at word exit.
    KeepPlacedRetreat {
        from_end: usize,
    },
    PlacedZeroWidthRetreat {
        target_from_end: usize,
        mark_from_end: usize,
    },
    ReplacePlacedTail {
        cells: usize,
        entry_blanks: Option<usize>,
        first_incoming: Option<usize>,
    },
    RetainDeviceTail(Box<crate::mandoc::inline::flow::native_slots::DeviceTail>),
    /// One field cell per raw slot, with the independent col/lastcol
    /// distance retained for the next `term_word()`.
    ProveRawSlots {
        count: usize,
        cursor_retreat: usize,
        backbefore: bool,
        duplicates: Vec<(usize, usize)>,
        editable_marker: bool,
    },
    /// Earlier cell to new cell indices. Empty entries were overwritten.
    ReplaceLivePrefix {
        start: usize,
        positions: Vec<Option<usize>>,
        /// Earlier source slot and surviving physical slot for a settled tail.
        trailing_blank_slots: Vec<(usize, usize)>,
        /// Old advance slot to first surviving slot and columns before new ink.
        retained_advances: std::collections::BTreeMap<usize, (usize, usize)>,
        /// Earlier native advance slots that now have their own output receipt.
        adopted_advances: Vec<usize>,
        first_incoming: usize,
        first_incoming_ink: Option<usize>,
        entry_blanks: usize,
        entry_gap: Option<usize>,
        unpublished: Option<usize>,
    },
    /// These compact writes do not prove individual native byte slots.
    OpaqueRawSlots {
        styled_suffix: Option<(usize, crate::mandoc::roff_escape::RoffFont)>,
        keeps_prior_slots: bool,
    },
    ProveAppendedSlots(AppendedSlotProof),
    /// A typed automatic underscore whose overwritten glyph can leave
    /// paint at the end of an accepted physical row.
    PlacedUnderlineStroke,
    /// Unknown SPECIAL/invalid NUMBERED buffers `ASCII_NBRZW` directly.
    /// Recovery spelling owns output scalars but is never native width or
    /// an encode1 glyph (term.c:610-638).
    RecoveryGlyph {
        projected_scalars: usize,
    },
    /// A breakable source blank consumed by a marker's pass boundary. Its
    /// cell participates in acceptance, but has no authored output scalar.
    UnprojectedBlank,
    /// A projected relative HORIZ advance. The native BACK register can
    /// consume it before it writes any blank to the input buffer.
    MotionPadding {
        consumed_by_backbefore: bool,
    },
    /// A delayed owner was provisionally projected at a break opportunity. Its
    /// original cell stays editable until a pass proves a cover.
    StageBacklineProjection,
    /// An authored SP suppressed by the early word projection. The real
    /// pass can still print it, retaining its original owner and style.
    DeferredBlanks {
        source: DeferredSourceBlank,
        count: usize,
    },
    /// A replayed native advance that owns no provisional text scalar.
    PlacedBlank {
        source: DeferredSourceBlank,
        cell: FieldCell,
    },
    ArmBackafter,
    CancelBackafter,
    /// A positive HORIZ first clears BACKBEFORE without buffering a cell.
    CancelBackbefore,
    /// A negative relative HORIZ. The first write can use the bounded
    /// entry plan; a later write cannot prove the word's cursor interval.
    RewindWordEntry(usize),
}

#[derive(Clone, Debug)]
pub(in crate::mandoc) struct AppendedSlotProof {
    pub(in crate::mandoc::inline) cells: usize,
    /// An exact styled suffix can narrow which slots a later word may edit.
    /// `None` carries forward the existing edit boundary.
    pub(in crate::mandoc::inline) replay_tail: Option<usize>,
    /// The final glyph awaits `BACKBEFORE` in the following word.
    pub(in crate::mandoc::inline) pending: bool,
    pub(in crate::mandoc::inline) fonts:
        Vec<(std::ops::Range<usize>, crate::mandoc::roff_escape::RoffFont)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::mandoc) struct DeferredSourceBlank {
    pub(in crate::mandoc::inline) owner: u64,
    pub(in crate::mandoc::inline) write: usize,
    pub(in crate::mandoc::inline) font: super::super::super::Font,
    /// The whole native word was retained to allow a later edit of this
    /// generated trailing advance before its deferred output.
    pub(in crate::mandoc::inline) replayable_tail: bool,
    /// Lexical separator written by the generated inset/diagonal run-in word.
    pub(in crate::mandoc::inline) generated_run_in: bool,
}

impl DeferredSourceBlank {
    pub(in crate::mandoc::inline) fn marker(self) -> String {
        format!(
            "{}{}/{}",
            super::super::output::DEFERRED_SOURCE_BLANK,
            self.owner,
            self.write
        )
    }
}

#[derive(Clone, Debug)]
pub(super) struct DeferredBlankRun {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) source: DeferredSourceBlank,
    pub(super) run_in_overlap: bool,
    /// The owned native slot follows surviving ink after an overprint;
    /// its reading marker still denotes the original run-in seam.
    pub(super) settled_trailing_run_in: bool,
}

impl FieldWrite {
    pub(in crate::mandoc::inline) fn is_break_marker(&self) -> bool {
        matches!(
            self,
            Self::Cell(FieldCell::BreakMarker)
                | Self::Placed {
                    cell: FieldCell::BreakMarker,
                    ..
                }
        )
    }

    pub(in crate::mandoc) fn literal(value: &str) -> Vec<Self> {
        value.chars().map(Self::literal_cell).collect()
    }

    pub(in crate::mandoc) fn append_literal(writes: &mut Vec<Self>, value: &str) {
        writes.extend(value.chars().map(Self::literal_cell));
    }

    pub(in crate::mandoc) fn append_character(writes: &mut Vec<Self>, character: char) {
        writes.push(Self::literal_cell(character));
    }

    pub(in crate::mandoc::inline) fn literal_cell(character: char) -> Self {
        Self::Cell(match character {
            ' ' => FieldCell::BreakableBlank,
            '\n' => FieldCell::BreakMarker,
            '\t' => FieldCell::Tab,
            // The frozen Unicode reader executes escaped spaces through
            // ESCAPE_SPECIAL -> encode1(U+00A0), not bufferc(ASCII_NBRSP).
            // KEEP's automatic separators remain direct buffered cells.
            '\u{8}' => FieldCell::Backline,
            // Printable ASCII occupies exactly one cell on the frozen
            // Unicode reader. Control sentinels and Unicode retain their
            // separate paths; no text-width scan is needed for this scalar.
            '!'..='~' => FieldCell::Graph {
                text: character,
                width: 1,
            },
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(in crate::mandoc::inline::flow) struct WordWriteReceipt {
    pub(in crate::mandoc::inline::flow) consumed_motion_padding: Vec<usize>,
    /// Index of the first cell this word's writes produced that survives
    /// execution with projection ownership. A leading `Backline` is a
    /// zero-width pairing cell, so the paired graph owns the content.
    /// Equal to `end_cell` when the writes produced no cell.
    pub(in crate::mandoc::inline::flow) first_content_cell: usize,
    /// `cells.len()` after the writes executed.
    pub(in crate::mandoc::inline::flow) end_cell: usize,
    /// `term_field()` writes deferred vbl only at a Graph or backspace.
    /// NBRZW contributes graph acceptance, but never prints this padding.
    pub(in crate::mandoc::inline::flow) prints_padding: bool,
    pub(in crate::mandoc::inline::flow) leading_retreat: Option<LeadingBlankRetreat>,
}

/// The exact current-word source blank removed before its first graph.
/// Automatic separators and held cells of an earlier word are outside this
/// prefix. A rearm can change which later graph occupies the column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::mandoc::inline::flow) struct LeadingBlankRetreat {
    /// Only a blank the text projector retained needs a scalar edit.
    pub(in crate::mandoc::inline::flow) offset: Option<usize>,
    pub(in crate::mandoc::inline::flow) character: char,
    pub(in crate::mandoc::inline::flow) rearmed: bool,
}
