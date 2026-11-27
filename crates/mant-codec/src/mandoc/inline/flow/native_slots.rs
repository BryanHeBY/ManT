//! Compact native terminal slots shared by word execution and tbl measurement.

use super::field_buffer::{FieldCell, FieldWrite};
use crate::mandoc::roff_escape::{RoffFont, horizontal_line_stroke, overstrike_operands};
use libmandoc_rs::TableColumnMetrics;
use std::{collections::BTreeMap, sync::Arc};

const EN: usize = 24;

/// A populated span in the final native buffer. Repeated device strokes
/// can be scanned in bulk without adding an author scalar to the span.
#[derive(Clone)]
pub(in crate::mandoc::inline) struct FillSlot {
    pub(in crate::mandoc::inline) cell: FieldCell,
    pub(in crate::mandoc::inline) count: usize,
    pub(in crate::mandoc::inline) owner: Option<usize>,
    pub(in crate::mandoc::inline) drawing_advance: bool,
    pub(in crate::mandoc::inline) advance_source: Option<usize>,
    pub(in crate::mandoc::inline) underline: Underline,
    pub(in crate::mandoc::inline) owned_deferred_blank: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum Underline {
    None,
    Stroke,
    Overstrike,
}

#[derive(Clone, Debug)]
enum Pattern {
    Plain(FieldCell),
    DeferredAdvance(FieldCell),
    UnderlineDecoration,
    DrawingAdvance,
    ImportedAdvance {
        cell: FieldCell,
        source: usize,
        drawing: bool,
    },
    Overstrike {
        cells: Arc<[FieldCell]>,
        phase: usize,
    },
}

impl Pattern {
    fn cell(&self, offset: usize) -> &FieldCell {
        match self {
            Self::Plain(cell)
            | Self::DeferredAdvance(cell)
            | Self::ImportedAdvance { cell, .. } => cell,
            Self::DrawingAdvance => &FieldCell::NonBreakingBlank,
            Self::UnderlineDecoration => &FieldCell::Graph {
                text: '_',
                width: 1,
            },
            Self::Overstrike { cells, phase } => {
                &cells[(phase + offset % cells.len()) % cells.len()]
            }
        }
    }

    fn shifted(&self, offset: usize) -> Self {
        match self {
            Self::Plain(_)
            | Self::DeferredAdvance(_)
            | Self::UnderlineDecoration
            | Self::DrawingAdvance
            | Self::ImportedAdvance { .. } => self.clone(),
            Self::Overstrike { cells, phase } => Self::Overstrike {
                cells: Arc::clone(cells),
                phase: (phase + offset % cells.len()) % cells.len(),
            },
        }
    }

    fn append_fill_slots(&self, out: &mut Vec<FillSlot>, count: usize, owner: Option<usize>) {
        let advance_source = match self {
            Self::ImportedAdvance { source, .. } => Some(*source),
            _ => None,
        };
        let mut push = |cell: &FieldCell, count| {
            if *cell == FieldCell::Backline
                && let Some(previous) = out.last_mut().filter(|previous| previous.count > 1)
            {
                // A retreat subtracts the preceding *one* glyph. Keep
                // its width separate for callers storing spans in bulk.
                previous.count -= 1;
                let tail = FillSlot {
                    cell: previous.cell.clone(),
                    count: 1,
                    owner: previous.owner,
                    drawing_advance: previous.drawing_advance,
                    advance_source: previous.advance_source,
                    underline: previous.underline,
                    owned_deferred_blank: previous.owned_deferred_blank,
                };
                out.push(tail);
            }
            out.push(FillSlot {
                cell: cell.clone(),
                count,
                owner,
                drawing_advance: self.is_fixed_advance(),
                advance_source,
                underline: if matches!(self, Self::UnderlineDecoration) {
                    Underline::Stroke
                } else {
                    Underline::None
                },
                owned_deferred_blank: matches!(self, Self::DeferredAdvance(_)),
            });
        };
        let Self::Overstrike { cells, phase } = self else {
            if count > 0 {
                push(self.cell(0), count);
            }
            return;
        };
        let period = cells.len();
        let leading = ((period - *phase) % period).min(count);
        let remaining = count - leading;
        let cycles = remaining / period;
        for index in 0..leading {
            push(self.cell(index), 1);
        }
        if cycles != 0 {
            // Complete overstrike patterns all finish at one visual glyph.
            push(&cells[period - 1], cycles);
        }
        for cell in &cells[..remaining % period] {
            push(cell, 1);
        }
    }

    fn is_fixed_advance(&self) -> bool {
        matches!(
            self,
            Self::DrawingAdvance | Self::ImportedAdvance { drawing: true, .. }
        )
    }

    fn into_independent_tail(self) -> Self {
        match self {
            Self::ImportedAdvance { drawing: true, .. } => Self::DrawingAdvance,
            Self::ImportedAdvance { cell, .. } | Self::DeferredAdvance(cell) => Self::Plain(cell),
            pattern => pattern,
        }
    }
}

#[derive(Clone, Debug)]
struct Run {
    end: usize,
    pattern: Pattern,
    owner: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub(in crate::mandoc::inline) struct NativeSlots {
    runs: BTreeMap<usize, Run>,
    cursor: usize,
    extent: usize,
    backafter: bool,
    backbefore: bool,
    backbefore_slot: Option<usize>,
    prior_input: bool,
    prior_edit: Option<PriorSlotEdit>,
    /// Position of this local segment in the full native buffer, if proved.
    origin: Option<usize>,
}

/// The slot suffix of the active field that has no projected author owners.
/// Keep only this editing surface between words, never the earlier text.
#[derive(Clone, Debug)]
pub(in crate::mandoc) struct DeviceTail {
    slots: NativeSlots,
    pub(in crate::mandoc::inline) fill_cells: usize,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum ReplayDomain {
    #[default]
    Scoped,
    Filled,
    DefinitionLine,
    LiteralDefinitionLine,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum EntryCursor {
    #[default]
    Empty,
    AtEnd,
    NeedsReplay,
}

impl EntryCursor {
    pub(in crate::mandoc::inline) const fn has_input(self) -> bool {
        !matches!(self, Self::Empty)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum CursorReposition {
    Absolute,
    RelativeRetreat,
}

#[derive(Default)]
pub(in crate::mandoc) struct SlotEntry {
    pub(in crate::mandoc::inline) domain: ReplayDomain,
    pub(in crate::mandoc::inline) projection_owner: super::field_buffer::FieldProjectionOwner,
    pub(in crate::mandoc::inline) cursor: EntryCursor,
    pub(in crate::mandoc::inline) automatic_blanks: usize,
    pub(in crate::mandoc::inline) kept_blanks: bool,
    pub(in crate::mandoc::inline) trailing_tab_reference: bool,
    pub(in crate::mandoc::inline) definition_row_occupied: bool,
    pub(in crate::mandoc::inline) retreat_blanks: usize,
    pub(in crate::mandoc::inline) device_tail: Option<DeviceTail>,
    pub(in crate::mandoc::inline) prefix: Option<FieldPrefix>,
}

#[derive(Default)]
pub(in crate::mandoc::inline) struct MotionEffect {
    pub(in crate::mandoc::inline) reposition: Option<CursorReposition>,
    pub(in crate::mandoc::inline) before_origin_units: usize,
}

#[derive(Clone, Copy)]
pub(in crate::mandoc::inline) enum PrefixExtension {
    None,
    DirectText,
    Word,
    FilledWord,
    LeadingMarker,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum ReplayPass {
    Incremental,
    Editable,
}

/// A small, uncommitted field with a proved one-to-one slot correspondence.
/// Earlier characters remain owned by the existing projection until replay
/// returns their surviving cell positions.
pub(in crate::mandoc::inline) struct FieldPrefix {
    pub(in crate::mandoc::inline) slot_limit: usize,
    pub(in crate::mandoc::inline) deferred_tail: DeferredTail,
    pub(in crate::mandoc::inline) start: usize,
    pub(in crate::mandoc::inline) cells: Vec<(FieldCell, usize)>,
    /// Existing reading seams whose originating advance normalized to one NBRSP.
    pub(in crate::mandoc::inline::flow) projected_advances: Vec<usize>,
    /// Existing deferred-blank slots that already own a `DeferredBlankRun`.
    pub(in crate::mandoc::inline::flow) deferred_blanks: Vec<usize>,
    pub(in crate::mandoc::inline) automatic: usize,
    pub(in crate::mandoc::inline) cursor_retreat: usize,
    pub(in crate::mandoc::inline) kept: bool,
    pub(in crate::mandoc::inline) tail_fonts: Vec<(std::ops::Range<usize>, RoffFont)>,
    pub(in crate::mandoc::inline) duplicates: Vec<(usize, usize)>,
    /// Automatic underline slots that remain typed even after an edit.
    pub(in crate::mandoc::inline::flow) underline_strokes: Vec<usize>,
    pub(in crate::mandoc::inline) backbefore: bool,
    pub(in crate::mandoc::inline) backafter: bool,
    pub(in crate::mandoc::inline) pending_cell: Option<usize>,
    pub(in crate::mandoc::inline) pass: ReplayPass,
}

impl FieldPrefix {
    pub(in crate::mandoc::inline) fn raw_slot_count(&self) -> usize {
        let base = self.cells.iter().fold(self.automatic, |count, (cell, _)| {
            count.saturating_add(match cell {
                FieldCell::DrawnSpaces(width) | FieldCell::FixedDrawingAdvance(width) => *width,
                _ => 1,
            })
        });
        self.tail_fonts.iter().fold(base, |count, (range, font)| {
            count.saturating_add(range.len().saturating_mul(decoration_slots(*font)))
        })
    }
}

pub(in crate::mandoc::inline) fn decoration_slots(font: RoffFont) -> usize {
    match font {
        RoffFont::Strong | RoffFont::Emphasis | RoffFont::CodeStrong | RoffFont::CodeEmphasis => 2,
        RoffFont::StrongEmphasis => 4,
        RoffFont::Regular | RoffFont::Code => 0,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum DeferredTail {
    #[default]
    Absent,
    Replayable,
    Covered,
}

impl DeviceTail {
    pub(in crate::mandoc::inline) fn into_slots(self) -> NativeSlots {
        self.slots
    }

    pub(in crate::mandoc::inline) fn has_retreated(&self) -> bool {
        self.slots.cursor < self.slots.extent
            || self.slots.backbefore
            || self
                .slots
                .runs
                .iter()
                .rev()
                .map(|(&start, run)| run.pattern.cell(run.end - start - 1))
                .find(|cell| {
                    !matches!(
                        cell,
                        FieldCell::TabReference
                            | FieldCell::ZeroWidthGraph
                            | FieldCell::Breakpoint
                            | FieldCell::BreakMarker
                    )
                })
                == Some(&FieldCell::Backline)
    }

    pub(in crate::mandoc::inline::flow) fn note_tab_reference(&mut self) -> bool {
        // term_tab_ref() only calls bufferc() at lastcol.
        if self.slots.cursor < self.slots.extent {
            return false;
        }
        self.slots.write_run(FieldCell::TabReference, 1, None);
        self.fill_cells += 1;
        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum PriorSlotEdit {
    Rewind,
    Unresolved,
}

impl NativeSlots {
    pub(in crate::mandoc::inline) fn after_entry(prior_input: bool) -> Self {
        Self {
            prior_input,
            origin: (!prior_input).then_some(0),
            ..Self::default()
        }
    }

    pub(in crate::mandoc::inline) fn unowned_tail(&self) -> Option<DeviceTail> {
        // Long lines of device-only strokes stay compressed. Bound the
        // retained run count so fragmented input cannot cause repeated
        // copying of a growing field across term_word() calls.
        const MAX_TAIL_RUNS: usize = 16;
        if self.pending_owner().is_some() {
            return None;
        }
        let mut suffix = self
            .runs
            .iter()
            .rev()
            .take_while(|(_, run)| run.owner.is_none());
        let mut runs = BTreeMap::new();
        let captured = suffix.by_ref().take(MAX_TAIL_RUNS).collect::<Vec<_>>();
        if suffix.next().is_some()
            || captured.is_empty() && self.origin.is_none() && !self.backbefore
        {
            return None;
        }
        let start = captured.last().map_or(self.extent, |(start, _)| **start);
        let cursor = self.cursor.checked_sub(start)?;
        let mut fill = Vec::new();
        for (&at, run) in captured.into_iter().rev() {
            run.pattern.append_fill_slots(&mut fill, run.end - at, None);
            let mut tail = run.clone();
            tail.end -= start;
            tail.pattern = tail.pattern.into_independent_tail();
            runs.insert(at - start, tail);
        }
        Some(DeviceTail {
            fill_cells: fill.len(),
            slots: Self {
                runs,
                cursor,
                extent: self.extent - start,
                origin: self.origin.map(|origin| origin.saturating_add(start)),
                prior_input: self.prior_input || start > 0,
                backbefore: self.backbefore,
                backafter: self.backafter,
                ..Self::default()
            },
        })
    }

    pub(in crate::mandoc::inline) fn enter_with_retreat(&mut self, blanks: usize) {
        self.advance(blanks, FieldCell::BreakableBlank, None);
        self.backbefore = true;
    }

    pub(in crate::mandoc::inline) fn with_field_prefix(prefix: &FieldPrefix) -> Self {
        let mut slots = Self::after_entry(prefix.start != 0);
        let mut aliases = prefix.duplicates.iter().peekable();
        let mut fonts = prefix.tail_fonts.iter().peekable();
        let mut pending_slot = None;
        for (index, (cell, scalars)) in prefix.cells.iter().enumerate() {
            if fonts.peek().is_some_and(|(range, _)| range.end == index) {
                fonts.next();
            }
            if let Some((range, font)) = fonts.peek()
                && range.contains(&index)
            {
                // encode1 writes the decoration before the final glyph.
                // Bold's earlier copy can still be the author's glyph
                // when a subsequent overstrike trims the final copy.
                slots.write_decoration(cell, *font, Some(index));
            }
            if prefix.pending_cell == Some(index) {
                pending_slot = Some(slots.cursor);
            }
            let owner = if aliases.peek().is_some_and(|&&(copy, _)| copy == index) {
                aliases.next().expect("matching alias").1
            } else {
                index
            };
            match cell {
                FieldCell::DrawnSpaces(count) => {
                    slots.advance_pattern(
                        *count,
                        Pattern::ImportedAdvance {
                            cell: FieldCell::BreakableBlank,
                            source: index,
                            drawing: false,
                        },
                        None,
                    );
                }
                FieldCell::FixedDrawingAdvance(count) => {
                    slots.advance_pattern(
                        *count,
                        Pattern::ImportedAdvance {
                            cell: FieldCell::NonBreakingBlank,
                            source: index,
                            drawing: true,
                        },
                        None,
                    );
                }
                FieldCell::NonBreakingBlank
                    if prefix.deferred_blanks.binary_search(&index).is_ok() =>
                {
                    slots.write_pattern(Pattern::DeferredAdvance(cell.clone()), 1, Some(owner));
                }
                FieldCell::NonBreakingBlank
                    if prefix.projected_advances.binary_search(&index).is_ok() =>
                {
                    slots.advance_pattern(
                        1,
                        Pattern::ImportedAdvance {
                            cell: FieldCell::NonBreakingBlank,
                            source: index,
                            drawing: false,
                        },
                        Some(owner),
                    );
                }
                FieldCell::Graph {
                    text: '_',
                    width: 1,
                } if *scalars == 0
                    && owner == index
                    && (prefix.underline_strokes.binary_search(&index).is_ok()
                        || matches!(
                            prefix.cells.get(index + 1),
                            Some((FieldCell::Backline, _))
                        )) =>
                {
                    slots.write_pattern(Pattern::UnderlineDecoration, 1, Some(index));
                }
                _ => slots.write_run(cell.clone(), 1, Some(owner)),
            }
        }
        slots.cursor -= prefix.cursor_retreat;
        if prefix.automatic > 0 {
            let blank = if prefix.kept {
                FieldCell::NonBreakingBlank
            } else {
                FieldCell::BreakableBlank
            };
            for offset in 0..prefix.automatic {
                slots.advance(1, blank.clone(), Some(prefix.cells.len() + offset));
            }
        }
        slots.backbefore = prefix.backbefore;
        slots.backafter = prefix.backafter;
        slots.backbefore_slot = pending_slot;
        slots
    }

    pub(in crate::mandoc::inline) fn prior_edit(&self) -> Option<PriorSlotEdit> {
        self.prior_edit
    }

    pub(in crate::mandoc::inline) fn cursor_before_owner(&self) -> bool {
        self.runs
            .values()
            .rev()
            .find(|run| run.owner.is_some())
            .is_some_and(|run| self.cursor < run.end)
    }

    pub(in crate::mandoc::inline) fn cursor_before_end(&self) -> bool {
        self.cursor < self.extent
    }

    /// Whether `self.cursor` currently points at an existing blank cell in
    /// `tcol->buf` (`term.c:946-955`: `bufferc(' ')` at `col < lastcol` only
    /// preserves a blank when the traversed byte itself is a blank).
    pub(in crate::mandoc::inline) fn cursor_at_blank(&self) -> bool {
        self.cursor < self.extent
            && self
                .runs
                .range(..=self.cursor)
                .next_back()
                .filter(|&(_, run)| self.cursor < run.end)
                .is_some_and(|(&start, run)| {
                    matches!(
                        run.pattern.cell(self.cursor - start),
                        FieldCell::BreakableBlank
                            | FieldCell::NonBreakingBlank
                            | FieldCell::Tab
                            | FieldCell::DrawnSpaces(_)
                            | FieldCell::FixedDrawingAdvance(_)
                    )
                })
    }

    pub(in crate::mandoc::inline) fn has_break_marker(&self) -> bool {
        self.runs
            .values()
            .any(|run| matches!(run.pattern, Pattern::Plain(FieldCell::BreakMarker)))
    }

    /// The raw position of a buffered break immediately after a fixed advance.
    pub(in crate::mandoc::inline) fn trailing_advance_marker(
        &self,
        automatic: usize,
    ) -> Option<usize> {
        let marker = self.extent.checked_sub(automatic + 1)?;
        (self.previous_cell(marker + 1) == Some(&FieldCell::BreakMarker)
            && self
                .runs
                .range(..marker)
                .next_back()
                .is_some_and(|(_, run)| run.end == marker && run.pattern.is_fixed_advance()))
        .then_some(marker)
    }

    pub(in crate::mandoc::inline) fn trailing_open_advance(
        &self,
        automatic: usize,
    ) -> Option<usize> {
        if self.cursor_before_end() || self.backafter() || self.backbefore() {
            return None;
        }
        let boundary = self.extent.checked_sub(automatic)?;
        let (_, run) = self.runs.range(..boundary).next_back()?;
        (run.end == boundary && run.pattern.is_fixed_advance()).then_some(boundary)
    }

    pub(in crate::mandoc::inline) fn has_unpainted_marker_tail(&self) -> bool {
        self.runs
            .range(..self.cursor)
            .rev()
            .map(|(&start, run)| run.pattern.cell(run.end.min(self.cursor) - start - 1))
            .find(|cell| {
                !matches!(
                    cell,
                    FieldCell::BreakableBlank
                        | FieldCell::NonBreakingBlank
                        | FieldCell::Breakpoint
                        | FieldCell::TabReference
                )
            })
            == Some(&FieldCell::BreakMarker)
    }

    pub(in crate::mandoc::inline) fn cursor_in_unread_advance_after_marker(&self) -> bool {
        self.origin == Some(0)
            && self.extent <= super::field_buffer::MAX_EDITABLE_APPEND_CELLS
            && !self.backafter
            && !self.backbefore
            && self
                .runs
                .range(..=self.cursor)
                .next_back()
                .is_some_and(|(&start, run)| {
                    run.end == self.extent
                        && self.cursor < run.end
                        && run.pattern.is_fixed_advance()
                        && self.previous_cell(start) == Some(&FieldCell::BreakMarker)
                })
    }

    pub(in crate::mandoc::inline) fn follows_blank(&self) -> bool {
        matches!(
            self.previous_cell(self.cursor),
            Some(FieldCell::BreakableBlank | FieldCell::NonBreakingBlank | FieldCell::Tab)
        )
    }

    /// A detached cursor can still traverse a tail of controls skipped by
    /// `term_field()`. In particular, `bufferc()` must walk past a buffered
    /// NBRZW instead of appending a separator after it at lastcol.
    pub(in crate::mandoc::inline) fn cursor_before_invisible_tail(&self) -> bool {
        self.cursor_before_end()
            && self
                .runs
                .values()
                .rev()
                .take_while(|run| run.end > self.cursor)
                .all(|run| {
                    matches!(
                        run.pattern,
                        Pattern::Plain(
                            FieldCell::ZeroWidthGraph
                                | FieldCell::BreakMarker
                                | FieldCell::Breakpoint
                                | FieldCell::TabReference
                        )
                    )
                })
    }

    /// An operand wrote ink before leaving the cursor inside a live
    /// prefix. Its next operand can still overwrite the old tail.
    pub(in crate::mandoc::inline) fn contains_ink_from(&self, first_owner: usize) -> bool {
        self.runs.values().any(|run| {
            run.owner.is_some_and(|owner| owner >= first_owner)
                && matches!(
                    run.pattern,
                    Pattern::Plain(FieldCell::Graph { .. } | FieldCell::Hyphen)
                        | Pattern::Overstrike { .. }
                )
        })
    }

    /// Padding written by `tbl_number()` before the actual cell word.
    pub(in crate::mandoc::inline) fn prepend_fixed_blanks(&mut self, count: usize) {
        self.advance(count, FieldCell::NonBreakingBlank, None);
    }

    pub(in crate::mandoc::inline) fn arm_backafter(&mut self) {
        self.backafter = true;
    }

    pub(in crate::mandoc::inline) fn backafter(&self) -> bool {
        self.backafter
    }

    pub(in crate::mandoc::inline) fn backbefore(&self) -> bool {
        self.backbefore
    }

    pub(in crate::mandoc::inline) fn cursor_follows_retained_glyph(&self) -> bool {
        // encode1() backs over the actual col-1 byte. A compact prefix
        // index cannot establish that relationship until expansion.
        self.cursor_before_end()
            && self.backbefore
            && self
                .backbefore_slot
                .is_none_or(|slot| Some(slot) == self.cursor.checked_sub(1))
            && matches!(
                self.previous_cell(self.cursor),
                Some(FieldCell::Graph { width: 1, .. })
            )
    }

    pub(in crate::mandoc::inline) fn cursor_follows_retained_glyph_and_blank(&self) -> bool {
        // term_word() writes the automatic separator before encode1()
        // consumes BACKBEFORE. The retreat must reach that ordinary
        // space, leaving the previously published glyph intact.
        self.cursor_before_end()
            && self.backbefore
            && self.backbefore_slot.is_some_and(|glyph| {
                self.cursor.checked_sub(2) == Some(glyph)
                    && matches!(
                        self.previous_cell(glyph + 1),
                        Some(FieldCell::Graph { width: 1, .. })
                    )
            })
            && self.previous_cell(self.cursor) == Some(&FieldCell::BreakableBlank)
    }

    pub(in crate::mandoc::inline) fn pending_owner(&self) -> Option<usize> {
        let slot = self.backbefore_slot()?;
        self.runs.range(..=slot).next_back()?.1.owner
    }

    pub(in crate::mandoc::inline) fn backbefore_slot(&self) -> Option<usize> {
        self.backbefore_slot
            .filter(|&slot| self.backbefore && slot < self.extent)
    }

    /// `term_word` holds both col and lastcol. Spacing advances through old
    /// cells without overwriting them; a later glyph may split any run.
    fn advance(&mut self, count: usize, blank: FieldCell, owner: Option<usize>) {
        self.advance_pattern(count, Pattern::Plain(blank), owner);
    }

    fn advance_pattern(&mut self, count: usize, pattern: Pattern, owner: Option<usize>) {
        let end = self.cursor.saturating_add(count);
        if end > self.extent {
            self.runs.insert(
                self.extent,
                Run {
                    end,
                    pattern,
                    owner,
                },
            );
            self.extent = end;
        }
        self.cursor = end;
    }

    /// Report the buffered cursor effect independently of the excess
    /// displacement before the complete field's origin.
    pub(in crate::mandoc::inline) fn motion(
        &mut self,
        argument: Option<&str>,
        owner: Option<usize>,
    ) -> MotionEffect {
        if std::mem::take(&mut self.backafter) {
            return MotionEffect::default();
        }
        let Some(argument) = argument else {
            return MotionEffect::default();
        };
        let (absolute, argument) = argument
            .strip_prefix('|')
            .map_or((false, argument), |a| (true, a));
        let Some((length, _)) = TableColumnMetrics::motion_argument(argument) else {
            return MotionEffect::default();
        };
        if absolute && self.prior_input && self.origin.is_none() {
            // term_word() resolves absolute HORIZ against col in the whole
            // field, including writes by earlier operands (term.c:678–714).
            self.prior_edit = Some(PriorSlotEdit::Unresolved);
            return MotionEffect::default();
        }
        let column = self.origin.unwrap_or(0).saturating_add(self.cursor);
        let relative = if absolute {
            i64::from(length)
                .saturating_sub(i64::try_from(column.saturating_mul(EN)).unwrap_or(i64::MAX))
        } else {
            i64::from(length)
        };
        let mut effect = MotionEffect {
            reposition: absolute.then_some(CursorReposition::Absolute),
            ..MotionEffect::default()
        };
        if relative > 0 {
            let cells = usize::try_from(relative).unwrap_or(usize::MAX).div_ceil(EN);
            let cells = cells.saturating_sub(usize::from(std::mem::take(&mut self.backbefore)));
            self.advance_pattern(cells, Pattern::DrawingAdvance, owner);
        } else if relative < 0 {
            let units = usize::try_from(relative.unsigned_abs()).unwrap_or(usize::MAX);
            let backbefore = usize::from(std::mem::take(&mut self.backbefore));
            let retreat = (units / EN).saturating_add(backbefore);
            let within_slots = retreat <= self.cursor;
            if self.prior_input && !within_slots {
                self.prior_edit = Some(PriorSlotEdit::Rewind);
                return MotionEffect::default();
            }
            if !absolute && self.origin == Some(0) {
                // term_word() stores the excess in offset and ti. A
                // fractional displacement can still have a valid raw
                // cursor receipt after whole-cell truncation.
                let available = self.cursor.saturating_sub(backbefore).saturating_mul(EN);
                effect.before_origin_units = units.saturating_sub(available);
            }
            // term.c:701-718: clamping p->col to 0 still retreats p->col below
            // p->tcol->lastcol; record RelativeRetreat whenever cursor moved left.
            let previous_cursor = self.cursor;
            self.cursor = self.cursor.saturating_sub(retreat);
            if !absolute && self.cursor < previous_cursor {
                effect.reposition = Some(CursorReposition::RelativeRetreat);
            }
        }
        effect
    }

    pub(in crate::mandoc::inline) fn line(
        &mut self,
        argument: &str,
        cell_width: usize,
        font: RoffFont,
        owner: Option<usize>,
    ) -> bool {
        let Some(stroke) = horizontal_line_stroke(argument, cell_width) else {
            return false;
        };
        let count = stroke.count;
        let cell = match stroke.glyph {
            ' ' => FieldCell::BreakableBlank,
            text => FieldCell::Graph {
                text,
                width: stroke.width,
            },
        };
        // encode1() applies the active font to printable strokes. The BACK
        // registers can change at most their first two writes.
        let owner = owner.filter(|_| cell == FieldCell::BreakableBlank);
        let lead = count.min(2);
        for _ in 0..lead {
            self.glyph(cell.clone(), font, owner);
        }
        if count > lead {
            let prefix = decoration_prefix(&cell, font);
            if prefix.clone().next().is_none() {
                if cell == FieldCell::BreakableBlank {
                    self.advance(count - lead, cell, owner);
                } else {
                    self.write_run(cell, count - lead, None);
                }
            } else {
                let pattern: Arc<[_]> = prefix.chain([cell]).collect();
                let count = (count - lead).saturating_mul(pattern.len());
                self.write_pattern(
                    Pattern::Overstrike {
                        cells: pattern,
                        phase: 0,
                    },
                    count,
                    None,
                );
            }
        }
        count != 0 && stroke.glyph == ' '
    }

    pub(in crate::mandoc::inline) fn overstrike(
        &mut self,
        source: &str,
        font: RoffFont,
        mut author: impl FnMut(char) -> Option<usize>,
    ) {
        // term_word() calls encode1 for each operand and arms a retreat
        // between consecutive writes. A later edit can retire a covering
        // graph; retain the source of each graph still in the field.
        for (glyph, more_raw) in overstrike_operands(source) {
            let cell = match glyph {
                ' ' => FieldCell::BreakableBlank,
                '\t' => FieldCell::Tab,
                '\u{1c}' => FieldCell::Hyphen,
                '\u{1d}' => FieldCell::Breakpoint,
                '\u{1e}' => FieldCell::ZeroWidthGraph,
                '\u{1f}' => FieldCell::NonBreakingBlank,
                _ => FieldCell::Graph {
                    text: glyph,
                    width: glyph_width(glyph),
                },
            };
            let owner = match cell {
                FieldCell::Graph { text, .. } => author(text),
                FieldCell::Hyphen => author('-'),
                _ => None,
            };
            self.glyph(cell, font, owner);
            if more_raw {
                if self.backbefore {
                    self.backafter = true;
                } else {
                    self.backbefore = true;
                }
            }
        }

        let blank_at_end = matches!(
            self.previous_cell(self.extent),
            Some(FieldCell::BreakableBlank | FieldCell::Tab)
        );
        if self.prior_input && (self.extent == 0 || self.extent == 1 && blank_at_end) {
            // lastcol and the final backspace/blank pair can include bytes
            // preceding this word (term.c:790–796).
            self.prior_edit = Some(PriorSlotEdit::Unresolved);
        } else if blank_at_end && (self.extent > 2 || self.prior_input && self.extent == 2) {
            // The final blank after a retreat removes the generated
            // backspace/blank pair, not the preceding original graph.
            let end = self.extent - 2;
            self.split_at(end);
            drop(self.runs.split_off(&end));
            self.extent = end;
            self.cursor = self.cursor.min(end);
        }
    }

    pub(in crate::mandoc::inline) fn native_write(
        &mut self,
        write: FieldWrite,
        font: RoffFont,
        owner: Option<usize>,
    ) {
        match write {
            FieldWrite::Cell(blank @ (FieldCell::BreakableBlank | FieldCell::NonBreakingBlank)) => {
                self.advance(1, blank, owner);
            }
            FieldWrite::Cell(cell @ (FieldCell::Graph { .. } | FieldCell::Hyphen)) => {
                self.glyph(cell, font, owner);
            }
            FieldWrite::Cell(cell) => self.write_run(cell, 1, owner),
            FieldWrite::RecoveryGlyph { .. } => {
                self.write_run(FieldCell::ZeroWidthGraph, 1, owner);
            }
            FieldWrite::ArmBackafter => self.arm_backafter(),
            FieldWrite::CancelBackafter => self.backafter = false,
            _ => unreachable!("only decoded native event writes reach the slot buffer"),
        }
    }

    /// Raw SP advances without replacing an existing byte. When it crosses
    /// an existing, unprojected blank, that same cell can represent it.
    pub(in crate::mandoc::inline) fn claim_unprojected_blank(
        &mut self,
        owner: usize,
        is_unprojected: impl FnOnce(&FieldCell, usize) -> bool,
        from_current_word: impl Fn(usize) -> bool,
    ) -> bool {
        let Some(slot) = self.runs.get(&self.cursor) else {
            return false;
        };
        if slot.end != self.cursor + 1
            || !slot
                .owner
                .is_some_and(|owner| is_unprojected(slot.pattern.cell(0), owner))
            || !self
                .runs
                .range(..self.cursor)
                .rev()
                .any(|(_, previous)| previous.owner.is_some_and(&from_current_word))
        {
            return false;
        }
        self.runs
            .get_mut(&self.cursor)
            .expect("checked raw blank")
            .owner = Some(owner);
        self.cursor += 1;
        true
    }

    fn glyph(&mut self, cell: FieldCell, font: RoffFont, owner: Option<usize>) {
        if self.backbefore && self.cursor == 0 && self.prior_input {
            self.prior_edit = Some(PriorSlotEdit::Unresolved);
            return;
        }
        if std::mem::take(&mut self.backbefore) && self.cursor > 0 {
            if matches!(
                self.previous_cell(self.cursor),
                Some(FieldCell::BreakableBlank | FieldCell::Tab)
            ) {
                self.cursor -= 1;
            } else {
                self.write_run(FieldCell::Backline, 1, None);
            }
        }
        self.write_decoration(&cell, font, owner);
        if matches!(
            cell,
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
        ) {
            self.advance(1, cell, owner);
        } else {
            self.write_run(cell, 1, owner);
        }
        self.backbefore = std::mem::take(&mut self.backafter);
        self.backbefore_slot = self.backbefore.then(|| self.cursor.saturating_sub(1));
    }

    fn write_decoration(&mut self, cell: &FieldCell, font: RoffFont, owner: Option<usize>) {
        // encode1 writes the same source value twice for bold. Underline
        // and backspace are device controls and never inherit its owner.
        let duplicate = match font {
            RoffFont::Strong | RoffFont::CodeStrong => Some(0),
            RoffFont::StrongEmphasis => Some(2),
            _ => None,
        };
        for (index, value) in decoration_prefix(cell, font).enumerate() {
            if index == 0
                && matches!(
                    font,
                    RoffFont::Emphasis | RoffFont::CodeEmphasis | RoffFont::StrongEmphasis
                )
            {
                self.write_pattern(Pattern::UnderlineDecoration, 1, None);
            } else {
                self.write_run(value, 1, owner.filter(|_| Some(index) == duplicate));
            }
        }
    }

    fn previous_cell(&self, cursor: usize) -> Option<&FieldCell> {
        self.runs
            .range(..cursor)
            .next_back()
            .map(|(&start, run)| run.pattern.cell(cursor - start - 1))
    }

    /// Split only boundary runs. Every removed middle run was previously
    /// inserted by a source event, so overlapping writes are amortized.
    fn write_run(&mut self, cell: FieldCell, count: usize, owner: Option<usize>) {
        self.write_pattern(Pattern::Plain(cell), count, owner);
    }

    fn write_pattern(&mut self, pattern: Pattern, count: usize, owner: Option<usize>) {
        let start = self.cursor;
        let end = start.saturating_add(count);
        if end == start {
            return;
        }
        self.split_at(start);
        self.split_at(end);
        while let Some((&start, _)) = self.runs.range(start..end).next() {
            self.runs.remove(&start);
        }
        self.runs.insert(
            start,
            Run {
                end,
                pattern,
                owner,
            },
        );
        self.cursor = end;
        self.extent = self.extent.max(end);
    }

    fn split_at(&mut self, at: usize) {
        if let Some((&start, run)) = self.runs.range_mut(..at).next_back()
            && at < run.end
        {
            let next = Run {
                end: run.end,
                pattern: run.pattern.shifted(at - start),
                owner: run.owner,
            };
            run.end = at;
            self.runs.insert(at, next);
        }
    }

    pub(in crate::mandoc::inline) fn into_fill_slots(self) -> Vec<FillSlot> {
        let mut cells = Vec::with_capacity(self.runs.len());
        for (start, run) in self.runs {
            run.pattern
                .append_fill_slots(&mut cells, run.end - start, run.owner);
        }
        cells
    }

    pub(in crate::mandoc::inline) fn extent_and_retreat(&self) -> (usize, usize) {
        (self.extent, self.extent - self.cursor)
    }
}

fn decoration_prefix(
    cell: &FieldCell,
    font: RoffFont,
) -> impl Iterator<Item = FieldCell> + Clone + use<> {
    let visible = matches!(cell, FieldCell::Graph { .. } | FieldCell::Hyphen);
    let underline = visible
        && matches!(
            font,
            RoffFont::Emphasis | RoffFont::CodeEmphasis | RoffFont::StrongEmphasis
        );
    let bold = visible
        && matches!(
            font,
            RoffFont::Strong | RoffFont::CodeStrong | RoffFont::StrongEmphasis
        );
    let bold_cell = if *cell == FieldCell::Hyphen {
        FieldCell::Graph {
            text: '-',
            width: 1,
        }
    } else {
        cell.clone()
    };
    [
        underline.then_some(FieldCell::Graph {
            text: '_',
            width: 1,
        }),
        underline.then_some(FieldCell::Backline),
        bold.then_some(bold_cell),
        bold.then_some(FieldCell::Backline),
    ]
    .into_iter()
    .flatten()
}

fn glyph_width(glyph: char) -> usize {
    mant_ir::geometry::text_width(glyph.encode_utf8(&mut [0; 4]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absolute_motion_receipt_requires_the_target_to_execute() {
        // term.c::term_word() consumes BACKAFTER before interpreting
        // HORIZ. The canceled target leaves the relative retreat intact.
        let mut slots = NativeSlots::default();
        for write in FieldWrite::literal("ABC") {
            slots.native_write(write, RoffFont::Regular, None);
        }
        assert_eq!(
            slots.motion(Some("-1n"), None).reposition,
            Some(CursorReposition::RelativeRetreat)
        );
        slots.arm_backafter();
        assert_eq!(slots.motion(Some("|1n"), None).reposition, None);
        assert_eq!(slots.extent_and_retreat(), (3, 1));
        assert_eq!(
            slots.motion(Some("|1n"), None).reposition,
            Some(CursorReposition::Absolute)
        );
        assert_eq!(slots.extent_and_retreat(), (3, 2));
    }

    #[test]
    fn fractional_retreat_preserves_cursor_and_reports_margin_excess() {
        // Pinned `No X Ns No "\h'-1.5n'\p"` writes one native slot and
        // sets ti by the remaining 12 basic units (term.c:706-713).
        let mut slots = NativeSlots::after_entry(false);
        for write in FieldWrite::literal("X") {
            slots.native_write(write, RoffFont::Regular, None);
        }
        let effect = slots.motion(Some("-1.5n"), None);
        assert_eq!(effect.reposition, Some(CursorReposition::RelativeRetreat));
        assert_eq!(effect.before_origin_units, 12);
        assert_eq!(slots.extent_and_retreat(), (1, 1));
    }

    #[test]
    fn repeated_motion_keeps_a_fixed_blank_interval_compact() {
        // The exact 32/256-movement c/r table cells were first checked in
        // pristine CVS. term_word rewinds its write cursor each time
        // while retaining the full previous lastcol for the first pass.
        for movements in [32, 256] {
            let mut buffer = NativeSlots::default();
            for write in FieldWrite::literal("XYZ") {
                buffer.native_write(write, RoffFont::Regular, None);
            }
            for _ in 0..movements {
                buffer.motion(Some("1024n"), None);
                buffer.motion(Some("-1024n"), None);
            }
            assert_eq!(buffer.cursor, 3);
            assert_eq!(buffer.extent, 1027);
            for write in FieldWrite::literal("Q") {
                buffer.native_write(write, RoffFont::Regular, None);
            }
            assert_eq!(buffer.into_fill_slots().len(), 5);
        }
    }
}
