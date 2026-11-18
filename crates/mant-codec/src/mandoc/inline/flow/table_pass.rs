//! Private tbl first-pass measurements over compact native writes.

use super::field_buffer::{FieldCell, FieldWrite, compact_first_width};
use super::{FontState, TabStops};
use crate::mandoc::inline::text_execution::record_native_event;
use crate::mandoc::roff_escape::{PresentationKind, RoffFont, RoffInlineEvent, decode};
use libmandoc_rs::TableColumnMetrics;
use std::{collections::BTreeMap, sync::Arc};

const EN: usize = 24;

pub(super) fn first_pass_width(
    text: &str,
    target: usize,
    cell_width: usize,
    tabs: &Arc<TabStops>,
    font: &mut FontState,
) -> usize {
    let events = decode(text);
    let mut scratch = CellBuffer::default();
    let mut writes = Vec::new();
    for event in &events {
        match event {
            RoffInlineEvent::HorizontalMotion { argument, .. } => {
                scratch.motion(argument.as_deref());
            }
            RoffInlineEvent::Presentation {
                kind: PresentationKind::HorizontalLine,
                argument: Some(argument),
            } => scratch.line(argument, cell_width, font.current),
            RoffInlineEvent::Font(selection) => font.select(*selection),
            RoffInlineEvent::PreviousFont => font.restore(),
            _ => {
                record_native_event(event, &mut writes, false);
                for write in writes.drain(..) {
                    scratch.native_write(write, font.current);
                }
            }
        }
    }
    compact_first_width(scratch.into_fill_cells(), target, tabs)
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
            Self::Overstrike { cells, phase } => &cells[(phase + offset) % cells.len()],
        }
    }

    fn shifted(&self, offset: usize) -> Self {
        match self {
            Self::Plain(_) => self.clone(),
            Self::Overstrike { cells, phase } => Self::Overstrike {
                cells: Arc::clone(cells),
                phase: (phase + offset) % cells.len(),
            },
        }
    }

    fn append_fill_cells(&self, out: &mut Vec<(FieldCell, usize)>, count: usize) {
        let Self::Overstrike { cells, phase } = self else {
            if count > 0 {
                out.push((self.cell(0).clone(), count));
            }
            return;
        };
        let period = cells.len();
        let leading = ((period - *phase) % period).min(count);
        let remaining = count - leading;
        let cycles = remaining / period;
        out.extend((0..leading).map(|slot| (self.cell(slot).clone(), 1)));
        if cycles != 0 {
            // Complete overstrike patterns all finish at one visual glyph.
            out.push((cells[period - 1].clone(), cycles));
        }
        out.extend((0..remaining % period).map(|slot| (cells[slot].clone(), 1)));
    }
}

struct Run {
    end: usize,
    pattern: Pattern,
}

#[derive(Default)]
struct CellBuffer {
    runs: BTreeMap<usize, Run>,
    cursor: usize,
    extent: usize,
    backafter: bool,
    backbefore: bool,
}

impl CellBuffer {
    /// `term_word` holds both col and lastcol. Spacing advances through old
    /// cells without overwriting them; a later glyph may split any run.
    fn advance(&mut self, count: usize, blank: FieldCell) {
        let end = self.cursor.saturating_add(count);
        if end > self.extent {
            self.runs.insert(
                self.extent,
                Run {
                    end,
                    pattern: Pattern::Plain(blank),
                },
            );
            self.extent = end;
        }
        self.cursor = end;
    }

    fn motion(&mut self, argument: Option<&str>) {
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
        let relative = if absolute {
            i64::from(length)
                .saturating_sub(i64::try_from(self.cursor.saturating_mul(EN)).unwrap_or(i64::MAX))
        } else {
            i64::from(length)
        };
        if relative > 0 {
            let cells = usize::try_from(relative).unwrap_or(usize::MAX).div_ceil(EN);
            let cells = cells.saturating_sub(usize::from(std::mem::take(&mut self.backbefore)));
            self.advance(cells, FieldCell::NonBreakingBlank);
        } else if relative < 0 {
            let cells = usize::try_from(relative.unsigned_abs()).unwrap_or(usize::MAX) / EN;
            let retreat = usize::from(std::mem::take(&mut self.backbefore));
            // The pre-column remainder changes tcol.offset upstream, not
            // lastcol or the available width after the occupied prefix.
            self.cursor = self.cursor.saturating_sub(cells.saturating_add(retreat));
        }
    }

    fn line(&mut self, argument: &str, cell_width: usize, font: RoffFont) {
        let Some((units, suffix)) = TableColumnMetrics::motion_argument(argument) else {
            return;
        };
        let glyph = line_glyph(suffix);
        let width = glyph_width(glyph);
        if width == 0 {
            return;
        }
        let length = usize::try_from(units)
            .ok()
            .filter(|units| *units > 0)
            .unwrap_or(cell_width);
        let count = length.div_ceil(width * EN);
        let cell = match glyph {
            ' ' => FieldCell::BreakableBlank,
            _ => FieldCell::Graph { text: glyph, width },
        };
        // encode1() applies the active font to printable strokes. The BACK
        // registers can change at most their first two writes.
        let lead = count.min(2);
        for _ in 0..lead {
            self.glyph(cell.clone(), font);
        }
        if count > lead {
            let prefix = decoration_prefix(&cell, font);
            if prefix.clone().next().is_none() {
                if cell == FieldCell::BreakableBlank {
                    self.advance(count - lead, cell);
                } else {
                    self.write_run(cell, count - lead);
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
                );
            }
        }
    }

    fn native_write(&mut self, write: FieldWrite, font: RoffFont) {
        match write {
            FieldWrite::Cell(blank @ (FieldCell::BreakableBlank | FieldCell::NonBreakingBlank)) => {
                self.advance(1, blank);
            }
            FieldWrite::Cell(cell @ (FieldCell::Graph { .. } | FieldCell::Hyphen)) => {
                self.glyph(cell, font);
            }
            FieldWrite::Cell(cell) => self.write_run(cell, 1),
            FieldWrite::RecoveryGlyph { .. } => self.write_run(FieldCell::ZeroWidthGraph, 1),
            FieldWrite::ArmBackafter => self.backafter = true,
            FieldWrite::CancelBackafter => self.backafter = false,
            _ => unreachable!("only native event writes reach tbl measurement"),
        }
    }

    fn glyph(&mut self, cell: FieldCell, font: RoffFont) {
        if std::mem::take(&mut self.backbefore) && self.cursor > 0 {
            let previous = self
                .runs
                .range(..self.cursor)
                .next_back()
                .map(|(&start, run)| run.pattern.cell(self.cursor - start - 1));
            if matches!(previous, Some(FieldCell::BreakableBlank | FieldCell::Tab)) {
                self.cursor -= 1;
            } else {
                self.write_run(FieldCell::Backline, 1);
            }
        }
        for prefix in decoration_prefix(&cell, font) {
            self.write_run(prefix, 1);
        }
        if cell == FieldCell::BreakableBlank {
            self.advance(1, cell);
        } else {
            self.write_run(cell, 1);
        }
        self.backbefore = std::mem::take(&mut self.backafter);
    }

    /// Split only boundary runs. Every removed middle run was previously
    /// inserted by a source event, so overlapping writes are amortized.
    fn write_run(&mut self, cell: FieldCell, count: usize) {
        self.write_pattern(Pattern::Plain(cell), count);
    }

    fn write_pattern(&mut self, pattern: Pattern, count: usize) {
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
        self.runs.insert(start, Run { end, pattern });
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
            };
            run.end = at;
            self.runs.insert(at, next);
        }
    }

    fn into_fill_cells(self) -> Vec<(FieldCell, usize)> {
        let mut cells = Vec::with_capacity(self.runs.len());
        for (start, run) in self.runs {
            run.pattern.append_fill_cells(&mut cells, run.end - start);
        }
        cells
    }
}

fn decoration_prefix(
    cell: &FieldCell,
    font: RoffFont,
) -> impl Iterator<Item = FieldCell> + Clone + use<> {
    let visible = *cell != FieldCell::BreakableBlank;
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

fn line_glyph(suffix: &str) -> char {
    let glyph = if suffix.starts_with('\\') {
        decode(suffix).first().and_then(|event| match event {
            RoffInlineEvent::Text(value) | RoffInlineEvent::Glyph(value) => value.chars().next(),
            _ => None,
        })
    } else {
        suffix.chars().next().filter(char::is_ascii)
    };
    glyph.filter(|glyph| !glyph.is_control()).unwrap_or('_')
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
            let mut buffer = CellBuffer::default();
            for write in FieldWrite::literal("XYZ") {
                buffer.native_write(write, RoffFont::Regular);
            }
            for _ in 0..movements {
                buffer.motion(Some("1024n"));
                buffer.motion(Some("-1024n"));
            }
            assert_eq!(buffer.cursor, 3);
            assert_eq!(buffer.extent, 1027);
            for write in FieldWrite::literal("Q") {
                buffer.native_write(write, RoffFont::Regular);
            }
            assert_eq!(buffer.into_fill_cells().len(), 5);
        }
    }
}
