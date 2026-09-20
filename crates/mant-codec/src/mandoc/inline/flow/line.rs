//! Renderer-neutral execution state corresponding to CVS `termp`/`termp_col`.
//!
//! Buffer positions count execution cells. Visual widths are separate basic
//! units, matching `term.h`; projected IR is never read back into this model.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ExecCell {
    Glyph { width: usize },
    BreakableBlank { width: usize },
    NonBreakingBlank { width: usize },
    NonBreakingZeroWidth,
    Backspace { width: usize },
    BreakPoint,
    WordEndBreak,
    Tab,
    TabReference,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct TermBuffer {
    cells: Vec<ExecCell>,
    /// `tcol->col`: first not-yet-consumed execution cell.
    read: usize,
}

impl TermBuffer {
    fn write(&mut self, cell: ExecCell) {
        self.cells.push(cell);
    }

    fn write_pos(&self) -> usize {
        self.cells.len()
    }

    fn occupied(&self) -> bool {
        self.read < self.cells.len()
    }

    fn discard(&mut self) {
        self.cells.clear();
        self.read = 0;
    }

    /// Direct translation of CVS `term_fill()` for the execution-cell subset
    /// represented by the renderer-neutral IR.
    fn fill(&self, target: usize) -> FilledField {
        let mut accepted_end = self.read;
        let mut accepted_width = 0usize;
        let mut visual = 0usize;
        let mut break_at_word_end = false;
        let mut graph = false;

        for (index, cell) in self.cells.iter().enumerate().skip(self.read) {
            match *cell {
                ExecCell::Backspace { width } => visual = visual.saturating_sub(width),
                ExecCell::BreakableBlank { width } => {
                    let next = visual.saturating_add(width);
                    if break_at_word_end || next > target {
                        break;
                    }
                    if graph {
                        accepted_end = index;
                        accepted_width = visual;
                        graph = false;
                    }
                    visual = next;
                }
                ExecCell::BreakPoint => {
                    if break_at_word_end {
                        break;
                    }
                    if graph {
                        accepted_end = index;
                        accepted_width = visual;
                        graph = false;
                    }
                }
                ExecCell::WordEndBreak => break_at_word_end = true,
                ExecCell::TabReference => {}
                ExecCell::Tab => {
                    visual = next_tab_stop(visual);
                    graph = true;
                    if visual > target && accepted_end > self.read {
                        break;
                    }
                }
                ExecCell::NonBreakingZeroWidth => graph = true,
                ExecCell::NonBreakingBlank { width } | ExecCell::Glyph { width } => {
                    visual = visual.saturating_add(width);
                    graph = true;
                    if visual > target && accepted_end > self.read {
                        break;
                    }
                }
            }
        }

        if graph && (visual <= target || accepted_end == self.read) {
            accepted_end = self.cells.len();
            accepted_width = visual;
        }
        FilledField {
            accepted_end,
            visual_width: accepted_width,
        }
    }

    fn has_graph(&self) -> bool {
        self.cells[self.read..].iter().any(|cell| {
            matches!(
                cell,
                ExecCell::Glyph { .. }
                    | ExecCell::NonBreakingBlank { .. }
                    | ExecCell::NonBreakingZeroWidth
                    | ExecCell::Tab
            )
        })
    }

    fn visible_width(&self) -> usize {
        let mut width = 0usize;
        let mut accepted = 0usize;
        for cell in &self.cells[self.read..] {
            match *cell {
                ExecCell::Glyph { width: cell_width }
                | ExecCell::NonBreakingBlank { width: cell_width } => {
                    width = width.saturating_add(cell_width);
                    accepted = width;
                }
                ExecCell::BreakableBlank { width: cell_width } => {
                    width = width.saturating_add(cell_width);
                }
                ExecCell::Backspace { width: cell_width } => {
                    width = width.saturating_sub(cell_width);
                    accepted = width;
                }
                ExecCell::Tab => {
                    width = next_tab_stop(width);
                    accepted = width;
                }
                ExecCell::NonBreakingZeroWidth
                | ExecCell::BreakPoint
                | ExecCell::WordEndBreak
                | ExecCell::TabReference => {}
            }
        }
        accepted
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct FilledField {
    accepted_end: usize,
    visual_width: usize,
}

const fn next_tab_stop(column: usize) -> usize {
    column.saturating_add(8usize.saturating_sub(column % 8))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct TermColumnState {
    right_margin: usize,
    offset: usize,
    tab_offset: usize,
}

impl Default for TermColumnState {
    fn default() -> Self {
        Self {
            right_margin: usize::MAX / 4,
            offset: 0,
            tab_offset: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(in crate::mandoc) struct TermLineState {
    visual_column: usize,
    trailing_space: usize,
    minimum_blank: usize,
    temporary_indent: Option<isize>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct TermFlags(u32);

impl TermFlags {
    const NO_SPACE: u32 = 1 << 1;
    const NONO_SPACE: u32 = 1 << 2;
    const NON_BREAKING_WORD: u32 = 1 << 3;
    const KEEP: u32 = 1 << 4;
    const PRE_KEEP: u32 = 1 << 5;
    const BACK_AFTER: u32 = 1 << 6;
    const BACK_BEFORE: u32 = 1 << 7;
    const NO_BREAK: u32 = 1 << 8;
    const BREAK_TRAILING_SPACE: u32 = 1 << 9;
    const BREAK_INDENT: u32 = 1 << 10;
    const HANG: u32 = 1 << 11;
    const NO_PAD: u32 = 1 << 12;
    const NO_SPLIT: u32 = 1 << 13;
    const SPLIT: u32 = 1 << 14;
    const NO_NEWLINE: u32 = 1 << 15;
    const BREAK_NEVER: u32 = 1 << 16;

    const fn contains(self, flag: u32) -> bool {
        self.0 & flag != 0
    }

    fn set(&mut self, flag: u32, enabled: bool) {
        if enabled {
            self.0 |= flag;
        } else {
            self.0 &= !flag;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct PendingSeparators {
    field_minimum: usize,
    word_boundary: usize,
}

impl PendingSeparators {
    pub(super) const fn cells(self) -> usize {
        self.field_minimum.saturating_add(self.word_boundary)
    }
    pub(super) const fn has_word_boundary(self) -> bool {
        self.word_boundary > 0
    }
    pub(super) const fn field_minimum(self) -> usize {
        self.field_minimum
    }
    pub(super) const fn word_boundary(self) -> usize {
        self.word_boundary
    }
    pub(super) const fn is_empty(self) -> bool {
        self.field_minimum == 0 && self.word_boundary == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SeparatorKind {
    FieldMinimum,
    WordBoundary,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct FlushOutcome {
    pub(super) field_accepted: bool,
    pub(super) field_width: usize,
    pub(super) leading_cells: usize,
    pub(super) row_ended: bool,
}

/// Private execution machine mirroring the selected CVS `struct termp`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::mandoc) struct FormatterMachine {
    buffer: TermBuffer,
    column: TermColumnState,
    line: TermLineState,
    max_right_margin: usize,
    flags: TermFlags,
    /// Projection bookkeeping only; never consulted for formatter decisions.
    projected_separators: PendingSeparators,
}

impl Default for FormatterMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatterMachine {
    pub(in crate::mandoc) const fn new() -> Self {
        Self {
            buffer: TermBuffer {
                cells: Vec::new(),
                read: 0,
            },
            column: TermColumnState {
                right_margin: usize::MAX / 4,
                offset: 0,
                tab_offset: 0,
            },
            line: TermLineState {
                visual_column: 0,
                trailing_space: 0,
                minimum_blank: 0,
                temporary_indent: None,
            },
            max_right_margin: usize::MAX / 4,
            flags: TermFlags(0),
            projected_separators: PendingSeparators {
                field_minimum: 0,
                word_boundary: 0,
            },
        }
    }

    pub(super) fn buffer_is_occupied(&self) -> bool {
        self.buffer.occupied()
    }
    pub(super) fn buffer_is_invisible(&self) -> bool {
        self.buffer.has_graph() && self.buffer.visible_width() == 0
    }
    pub(super) fn buffer_is_visible(&self) -> bool {
        self.buffer.visible_width() > 0
    }
    pub(super) fn cursor_advanced(&self) -> bool {
        self.buffer.write_pos() > 0
    }
    pub(super) fn row_is_active(&self) -> bool {
        self.buffer_is_occupied() || self.line.visual_column > 0
    }
    pub(super) fn write_zero_width_graph(&mut self) {
        self.buffer.write(ExecCell::NonBreakingZeroWidth);
    }
    pub(super) fn occupy_invisible(&mut self) {
        self.write_zero_width_graph();
    }
    pub(super) fn occupy_visible(&mut self) {
        if !self.buffer.has_graph() {
            self.buffer.write(ExecCell::Glyph { width: 1 });
        }
    }
    pub(super) fn write_visible(&mut self, width: usize) {
        if width > 0 {
            self.buffer.write(ExecCell::Glyph { width });
        }
    }
    pub(super) fn advance_visible(&mut self, width: usize) {
        self.write_visible(width);
    }
    pub(super) fn write_breakable_blank(&mut self, width: usize) {
        if width > 0 {
            self.buffer.write(ExecCell::BreakableBlank { width });
        }
    }
    pub(super) fn write_non_breaking_blank(&mut self, width: usize) {
        if width > 0 {
            self.buffer.write(ExecCell::NonBreakingBlank { width });
        }
    }
    pub(super) fn write_word_end_break(&mut self) {
        self.buffer.write(ExecCell::WordEndBreak);
    }
    pub(super) fn reset_buffer_projection(&mut self, width: usize, graph: bool) {
        self.buffer.discard();
        if graph {
            self.buffer.write(ExecCell::Glyph { width });
        }
    }
    pub(super) fn buffer_width(&self) -> usize {
        self.buffer.visible_width()
    }
    pub(super) fn row_width(&self) -> usize {
        self.line.visual_column.saturating_add(self.buffer_width())
    }
    pub(super) fn set_geometry(&mut self, offset: usize, right_margin: usize) {
        self.column.offset = offset;
        self.column.right_margin = right_margin;
    }
    pub(super) const fn geometry(&self) -> (usize, usize) {
        (self.column.offset, self.column.right_margin)
    }
    pub(super) fn leading_cells_if_flushed(&self) -> usize {
        let margin = if self.flags.contains(TermFlags::NO_PAD)
            || self.column.offset < self.line.visual_column
        {
            0
        } else {
            self.column.offset - self.line.visual_column
        };
        margin.max(self.line.minimum_blank)
    }
    pub(super) fn begin_definition_field(
        &mut self,
        trailing_space: usize,
        body_column: usize,
        wraps: bool,
    ) {
        self.set_geometry(0, body_column);
        self.line.trailing_space = trailing_space;
        self.flags.set(TermFlags::NO_SPACE, true);
        self.flags.set(TermFlags::NO_BREAK, true);
        self.flags.set(TermFlags::BREAK_INDENT, true);
        self.flags.set(TermFlags::BREAK_TRAILING_SPACE, wraps);
        self.flags.set(TermFlags::HANG, !wraps);
    }
    pub(super) fn apply_break_indent(&mut self) {
        if !self.flags.contains(TermFlags::BREAK_INDENT) {
            return;
        }
        self.column.offset = self.column.right_margin;
        self.column.right_margin = self.max_right_margin;
        self.line.trailing_space = 0;
        self.flags.set(TermFlags::NO_BREAK, false);
        self.flags.set(TermFlags::BREAK_INDENT, false);
        self.flags.set(TermFlags::NO_SPACE, true);
    }
    pub(super) fn clear_definition_field(&mut self) {
        self.flags.set(TermFlags::NO_BREAK, false);
        self.flags.set(TermFlags::BREAK_TRAILING_SPACE, false);
        self.flags.set(TermFlags::BREAK_INDENT, false);
        self.flags.set(TermFlags::HANG, false);
        self.line.trailing_space = 0;
    }
    pub(super) fn finish_definition_scope(&mut self) {
        self.clear_definition_field();
    }
    pub(super) fn set_temporary_indent(&mut self, indent: Option<isize>) {
        self.line.temporary_indent = indent;
    }
    pub(super) fn queue_separator(&mut self, cells: usize, kind: SeparatorKind) {
        match kind {
            SeparatorKind::FieldMinimum => {
                self.line.minimum_blank = cells;
                self.projected_separators.field_minimum = cells;
            }
            SeparatorKind::WordBoundary => self.projected_separators.word_boundary = cells,
        }
    }
    pub(super) const fn pending_separators(&self) -> PendingSeparators {
        self.projected_separators
    }
    pub(super) fn take_separators(&mut self) -> PendingSeparators {
        std::mem::take(&mut self.projected_separators)
    }
    pub(super) fn clear_separators(&mut self) {
        self.projected_separators = PendingSeparators::default();
    }
    pub(super) fn restore_separators(&mut self, separators: PendingSeparators) {
        self.projected_separators = separators;
        self.line.minimum_blank = separators.field_minimum;
    }

    /// Execute one CVS `term_flushln()` field in renderer-neutral units.
    pub(super) fn flush_field(&mut self) -> FlushOutcome {
        let leading_cells = self.leading_cells_if_flushed();
        let field_target = self
            .column
            .right_margin
            .saturating_sub(self.line.visual_column.saturating_add(leading_cells));
        let target = if self.flags.contains(TermFlags::BREAK_NEVER) {
            usize::MAX / 4
        } else if self.flags.contains(TermFlags::NO_BREAK) {
            self.max_right_margin
                .saturating_sub(self.line.visual_column.saturating_add(leading_cells))
        } else {
            field_target
        };
        let filled = self.buffer.fill(target);
        let field_accepted = filled.accepted_end > self.buffer.read;
        let actual_leading = if field_accepted { leading_cells } else { 0 };
        if field_accepted {
            self.line.visual_column = self
                .line
                .visual_column
                .saturating_add(actual_leading)
                .saturating_add(filled.visual_width);
            self.buffer.read = filled.accepted_end;
        }
        let field_width = filled.visual_width;
        self.buffer.discard();
        self.line.minimum_blank = self.line.trailing_space;
        self.projected_separators.field_minimum = self.line.minimum_blank;
        self.flags.set(TermFlags::BACK_AFTER, false);
        self.flags.set(TermFlags::BACK_BEFORE, false);
        self.flags.set(TermFlags::NO_PAD, false);

        let overruns = self.flags.contains(TermFlags::BREAK_TRAILING_SPACE)
            && field_width.saturating_add(self.line.trailing_space) > field_target;
        let row_ended = !self.flags.contains(TermFlags::HANG)
            && (!self.flags.contains(TermFlags::NO_BREAK) || overruns);
        if row_ended {
            self.endline();
        }
        FlushOutcome {
            field_accepted,
            field_width,
            leading_cells: actual_leading,
            row_ended,
        }
    }
    pub(super) fn term_newline(&mut self) -> Option<FlushOutcome> {
        let outcome = self.row_is_active().then(|| self.flush_field());
        self.column.tab_offset = 0;
        outcome
    }
    pub(super) fn roff_break(&mut self) -> Option<FlushOutcome> {
        let outcome = self.term_newline();
        self.apply_break_indent();
        outcome
    }
    pub(super) fn margin_flush(&mut self) -> Option<FlushOutcome> {
        if !self.cursor_advanced() {
            return None;
        }
        self.flags.set(TermFlags::NO_BREAK, true);
        let outcome = self.flush_field();
        self.flags.set(TermFlags::NO_BREAK, false);
        self.flags.set(TermFlags::NO_SPACE, false);
        Some(outcome)
    }
    pub(super) fn commit_field(&mut self) -> bool {
        self.flush_field().field_accepted
    }
    pub(super) fn discard_empty_field(&mut self) {
        self.buffer.discard();
    }
    pub(super) fn discard_input(&mut self) {
        self.buffer.discard();
    }
    /// CVS `endline()` ends device output but does not clear `minbl`.
    pub(super) fn endline(&mut self) {
        self.line.visual_column = 0;
        // This is projection bookkeeping for a blank already buffered by a
        // prior word. A device newline consumes it; `minbl` remains live.
        self.projected_separators.word_boundary = 0;
    }
    pub(super) fn end_row(&mut self) {
        self.buffer.discard();
        self.endline();
        self.column.tab_offset = 0;
    }
    pub(super) fn inherit(&mut self, other: Self) {
        *self = other;
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecCell, FormatterMachine, TermFlags};

    #[test]
    fn buffer_indices_are_independent_from_visual_width() {
        let mut formatter = FormatterMachine::new();
        formatter.write_visible(2);
        assert_eq!(formatter.buffer.write_pos(), 1);
        assert_eq!(formatter.buffer_width(), 2);
    }

    #[test]
    fn whitespace_only_fields_are_not_accepted() {
        let mut formatter = FormatterMachine::new();
        formatter
            .buffer
            .write(ExecCell::BreakableBlank { width: 1 });
        let outcome = formatter.flush_field();
        assert!(!outcome.field_accepted);
        assert_eq!(outcome.field_width, 0);
    }

    #[test]
    fn no_pad_ignores_left_margin_but_not_minimum_blank() {
        let mut formatter = FormatterMachine::new();
        formatter.set_geometry(8, 16);
        formatter.line.minimum_blank = 2;
        formatter.flags.set(TermFlags::NO_PAD, true);
        assert_eq!(formatter.leading_cells_if_flushed(), 2);
    }
}
