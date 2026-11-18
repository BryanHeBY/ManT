//! Compact native terminal slots shared by word execution and tbl measurement.

use super::field_buffer::{FieldCell, FieldWrite};
use crate::mandoc::roff_escape::{RoffFont, horizontal_line_stroke, overstrike_operands};
use libmandoc_rs::TableColumnMetrics;
use std::{collections::BTreeMap, sync::Arc};

const EN: usize = 24;

/// A populated span in the final native buffer. Repeated device strokes
/// can be scanned in bulk without adding an author scalar to the span.
pub(in crate::mandoc::inline) struct FillSlot {
    pub(in crate::mandoc::inline) cell: FieldCell,
    pub(in crate::mandoc::inline) count: usize,
    pub(in crate::mandoc::inline) owner: Option<usize>,
}

#[derive(Clone)]
enum Pattern {
    Plain(FieldCell),
    Overstrike {
        cells: Arc<[FieldCell]>,
        phase: usize,
    },
}

impl Pattern {
    fn cell(&self, offset: usize) -> &FieldCell {
        match self {
            Self::Plain(cell) => cell,
            Self::Overstrike { cells, phase } => {
                &cells[(phase + offset % cells.len()) % cells.len()]
            }
        }
    }

    fn shifted(&self, offset: usize) -> Self {
        match self {
            Self::Plain(_) => self.clone(),
            Self::Overstrike { cells, phase } => Self::Overstrike {
                cells: Arc::clone(cells),
                phase: (phase + offset % cells.len()) % cells.len(),
            },
        }
    }

    fn append_fill_slots(&self, out: &mut Vec<FillSlot>, count: usize, owner: Option<usize>) {
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
                };
                out.push(tail);
            }
            out.push(FillSlot {
                cell: cell.clone(),
                count,
                owner,
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
}

struct Run {
    end: usize,
    pattern: Pattern,
    owner: Option<usize>,
}

#[derive(Default)]
pub(in crate::mandoc::inline) struct NativeSlots {
    runs: BTreeMap<usize, Run>,
    cursor: usize,
    extent: usize,
    backafter: bool,
    backbefore: bool,
    prior_input: bool,
    prior_edit: Option<PriorSlotEdit>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::mandoc::inline) enum PriorSlotEdit {
    Rewind,
    Unresolved,
}

impl NativeSlots {
    pub(in crate::mandoc::inline) fn after_entry(prior_input: bool) -> Self {
        Self {
            prior_input,
            ..Self::default()
        }
    }

    pub(in crate::mandoc::inline) fn prior_edit(&self) -> Option<PriorSlotEdit> {
        self.prior_edit
    }

    pub(in crate::mandoc::inline) fn arm_backafter(&mut self) {
        self.backafter = true;
    }

    pub(in crate::mandoc::inline) fn backafter(&self) -> bool {
        self.backafter
    }

    /// `term_word` holds both col and lastcol. Spacing advances through old
    /// cells without overwriting them; a later glyph may split any run.
    fn advance(&mut self, count: usize, blank: FieldCell, owner: Option<usize>) {
        let end = self.cursor.saturating_add(count);
        if end > self.extent {
            self.runs.insert(
                self.extent,
                Run {
                    end,
                    pattern: Pattern::Plain(blank),
                    owner,
                },
            );
            self.extent = end;
        }
        self.cursor = end;
    }

    pub(in crate::mandoc::inline) fn motion(&mut self, argument: Option<&str>) {
        if std::mem::take(&mut self.backafter) {
            return;
        }
        let Some(argument) = argument else { return };
        let (absolute, argument) = argument
            .strip_prefix('|')
            .map_or((false, argument), |a| (true, a));
        let Some((length, _)) = TableColumnMetrics::motion_argument(argument) else {
            return;
        };
        if absolute && self.prior_input {
            // term_word() resolves absolute HORIZ against col in the whole
            // field, including writes by earlier operands (term.c:678–714).
            self.prior_edit = Some(PriorSlotEdit::Unresolved);
            return;
        }
        let relative = if absolute {
            i64::from(length)
                .saturating_sub(i64::try_from(self.cursor.saturating_mul(EN)).unwrap_or(i64::MAX))
        } else {
            i64::from(length)
        };
        if relative > 0 {
            let cells = usize::try_from(relative).unwrap_or(usize::MAX).div_ceil(EN);
            let cells = cells.saturating_sub(usize::from(std::mem::take(&mut self.backbefore)));
            self.advance(cells, FieldCell::NonBreakingBlank, None);
        } else if relative < 0 {
            let cells = usize::try_from(relative.unsigned_abs()).unwrap_or(usize::MAX) / EN;
            let retreat = usize::from(std::mem::take(&mut self.backbefore));
            if self.prior_input && cells.saturating_add(retreat) > self.cursor {
                self.prior_edit = Some(PriorSlotEdit::Rewind);
                return;
            }
            // The pre-column remainder changes tcol.offset upstream, not
            // lastcol or the available width after the occupied prefix.
            self.cursor = self.cursor.saturating_sub(cells.saturating_add(retreat));
        }
    }

    pub(in crate::mandoc::inline) fn line(
        &mut self,
        argument: &str,
        cell_width: usize,
        font: RoffFont,
    ) {
        let Some(stroke) = horizontal_line_stroke(argument, cell_width) else {
            return;
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
        let lead = count.min(2);
        for _ in 0..lead {
            self.glyph(cell.clone(), font, None);
        }
        if count > lead {
            let prefix = decoration_prefix(&cell, font);
            if prefix.clone().next().is_none() {
                if cell == FieldCell::BreakableBlank {
                    self.advance(count - lead, cell, None);
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
    }

    pub(in crate::mandoc::inline) fn overstrike(
        &mut self,
        source: &str,
        font: RoffFont,
        owner: Option<usize>,
    ) {
        let mut last_glyph = None;
        // term_word() calls encode1 for each operand and arms a retreat
        // between consecutive writes. It keeps all physical byte slots
        // even though reading text retains one final visible glyph.
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
            let projected =
                owner.is_some() && matches!(cell, FieldCell::Graph { .. } | FieldCell::Hyphen);
            self.glyph(cell, font, None);
            if projected {
                last_glyph = self.cursor.checked_sub(1);
            }
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
        if let Some(run) = last_glyph.and_then(|slot| self.runs.get_mut(&slot)) {
            run.owner = owner;
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
        for prefix in decoration_prefix(&cell, font) {
            self.write_run(prefix, 1, None);
        }
        if matches!(
            cell,
            FieldCell::BreakableBlank | FieldCell::NonBreakingBlank
        ) {
            self.advance(1, cell, owner);
        } else {
            self.write_run(cell, 1, owner);
        }
        self.backbefore = std::mem::take(&mut self.backafter);
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
                buffer.motion(Some("1024n"));
                buffer.motion(Some("-1024n"));
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
