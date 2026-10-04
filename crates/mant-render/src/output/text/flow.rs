//! Keep resolved gaps separate from literal line breaks until the entire
//! content flow is assembled. Transparent containers must not create a new
//! gap budget, and literal blank lines must not be mistaken for requests.
use mant_ir::geometry::GapPlan;

use super::layout::LayoutText;

#[derive(Default)]
pub(super) struct Flow {
    parts: Vec<Part>,
}

enum Part {
    Text(LayoutText),
    CompletedText(LayoutText),
    Literal(LayoutText),
    Gap(u16),
    /// Consume completed gap rows at a whole table-data-row boundary without
    /// manufacturing a second empty physical row.
    CompleteGap,
}

impl Flow {
    pub(super) fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub(super) fn has_physical_rows(&self) -> bool {
        self.parts.iter().any(|part| match part {
            Part::Text(_) | Part::CompletedText(_) | Part::Literal(_) => true,
            Part::Gap(rows) => *rows > 0,
            Part::CompleteGap => false,
        })
    }
    pub(super) fn text(value: LayoutText) -> Self {
        let mut result = Self::default();
        result.push_text(value);
        result
    }

    pub(super) fn literal(value: LayoutText) -> Self {
        Self {
            parts: vec![Part::Literal(value)],
        }
    }

    /// A paragraph whose last retained blank row was closed by authored
    /// content. Its delimiter survives EOF and table-cell collection.
    pub(super) fn completed_text(value: LayoutText) -> Self {
        Self {
            parts: vec![Part::CompletedText(value)],
        }
    }

    pub(super) fn push_text(&mut self, value: LayoutText) {
        if !value.is_empty() {
            self.parts.push(Part::Text(value));
        }
    }

    /// Put a run-in marker before the first physical row without flattening
    /// the flow's completed tail or its pending vertical boundaries.
    pub(super) fn prefix_first_row(mut self, removed: &str, prefix: &str) -> Self {
        if let Some(text) = self.parts.iter_mut().find_map(|part| match part {
            Part::Text(text) | Part::CompletedText(text) | Part::Literal(text) => Some(text),
            Part::Gap(_) | Part::CompleteGap => None,
        }) {
            *text = std::mem::take(text).strip_prefix(removed).prefixed(prefix);
        } else {
            self.push_text(prefix.into());
        }
        self
    }

    pub(super) fn gap(&mut self, rows: u16) {
        self.parts.push(Part::Gap(rows));
    }

    pub(super) fn extend(&mut self, other: Self) {
        self.parts.extend(other.parts);
    }

    /// Retain cell gap requests until the actual parent consumes them.
    /// A successor can reuse one provisional literal tail only when no
    /// completed gap intervenes; this never trims author whitespace.
    pub(super) fn append_stack_cell(&mut self, mut cell: Self, break_after: bool) {
        let completed = cell.parts.iter().rev().find_map(|part| match part {
            Part::Text(_) | Part::Literal(_) => Some(false),
            Part::CompletedText(_) | Part::CompleteGap => Some(true),
            Part::Gap(rows) if *rows > 0 => Some(true),
            Part::Gap(_) => None,
        });
        let has_row = cell.parts.iter().any(|part| {
            matches!(
                part,
                Part::Text(_) | Part::CompletedText(_) | Part::Literal(_)
            )
        });
        if has_row {
            let leading_gap = cell
                .parts
                .iter()
                .take_while(|part| matches!(part, Part::Gap(_)))
                .any(|part| matches!(part, Part::Gap(rows) if *rows > 0));
            if !leading_gap {
                self.retire_open_tail();
            }
        }
        for part in &mut cell.parts {
            if let Part::Text(text) = part {
                // A collected cell retains its final open physical row,
                // including one supplied by an ordinary Paragraph.
                *part = Part::Literal(std::mem::take(text));
            }
        }
        self.extend(cell);
        if break_after {
            if completed == Some(true) {
                // An explicit close consumes the existing completed gap
                // boundary once; the following cell starts a fresh budget.
                self.complete_table_gap();
            } else {
                self.close_stack_cell();
            }
        }
    }

    pub(super) fn complete_table_gap(&mut self) {
        if self
            .parts
            .iter()
            .rev()
            .take_while(|part| matches!(part, Part::Gap(_)))
            .any(|part| matches!(part, Part::Gap(rows) if *rows > 0))
        {
            self.parts.push(Part::CompleteGap);
        }
    }

    fn retire_open_tail(&mut self) {
        for part in self.parts.iter_mut().rev() {
            match part {
                Part::Gap(rows) if *rows > 0 => return,
                Part::Text(text) | Part::Literal(text) => {
                    if text.visible.ends_with('\n') {
                        // split(true) removes exactly the provisional row's
                        // delimiter and carries its zero-width decoration.
                        *text = LayoutText::join(text.split(true), "\n");
                    }
                    return;
                }
                Part::CompletedText(_) => return,
                Part::Gap(_) | Part::CompleteGap => {}
            }
        }
    }

    fn close_stack_cell(&mut self) {
        for part in self.parts.iter_mut().rev() {
            match part {
                Part::Gap(rows) if *rows > 0 => return,
                Part::Text(text) | Part::Literal(text) => {
                    *part = Part::CompletedText(std::mem::take(text));
                    return;
                }
                Part::CompletedText(_) | Part::CompleteGap => break,
                Part::Gap(_) => {}
            }
        }
        self.parts.push(Part::CompletedText(LayoutText::default()));
    }

    pub(super) fn finish(self, preceding_content: bool) -> String {
        self.finish_layout(preceding_content).rendered
    }

    pub(super) fn finish_layout(self, preceding_content: bool) -> LayoutText {
        self.finish_rows(preceding_content, true).0
    }

    /// A rendered cell is split into physical rows before the following
    /// content arrives. Completed trailing gap rows need their own final
    /// delimiter; otherwise `split_terminator` would consume one as a mere
    /// close of the preceding printed row.
    pub(super) fn finish_cell(self) -> (LayoutText, bool) {
        self.finish_rows(false, false)
    }

    fn finish_rows(
        self,
        preceding_content: bool,
        complete_literal_tail: bool,
    ) -> (LayoutText, bool) {
        let mut output = LayoutText::default();
        let mut gap = GapPlan::default();
        let mut has_content = preceding_content;
        let mut final_empty_literal_row = false;
        let mut final_completed_row = false;
        for part in self.parts {
            let completed_row = matches!(&part, Part::CompletedText(_));
            let empty_literal_tail = matches!(
                &part,
                Part::Literal(text) if text.is_empty() || text.visible.ends_with('\n')
            );
            match part {
                Part::Gap(rows) => gap.append_resolved(rows),
                Part::CompleteGap => {
                    let rows = gap.rows(0);
                    if rows > 0 {
                        if has_content {
                            output.push_plain("\n");
                        }
                        output.push_plain(&"\n".repeat(usize::from(rows)));
                        has_content = false;
                        final_empty_literal_row = false;
                        final_completed_row = true;
                    }
                    gap = GapPlan::default();
                }
                Part::Text(text) | Part::CompletedText(text) | Part::Literal(text) => {
                    if has_content {
                        output.push_plain("\n");
                    }
                    output.push_plain(&"\n".repeat(usize::from(gap.rows(0))));
                    output.append(&text);
                    has_content = true;
                    final_empty_literal_row = empty_literal_tail;
                    final_completed_row = completed_row;
                    gap = GapPlan::default();
                }
            }
        }
        let rows = gap.rows(0);
        output.push_plain(&"\n".repeat(usize::from(rows)));
        // The final printed row also needs its closing delimiter outside a
        // table cell. Otherwise EOF consumes one completed empty row as the
        // preceding row's terminator (term_vspace's endline, term.c:489).
        // An empty literal tail is a physical row, even without a later
        // printed part to close it. Its delimiter must survive EOF just as
        // term_newln/term_vspace survive leaving NODE_NOFILL (man_term.c).
        // A cell's open final row may receive the next column; collecting
        // that fragment does not complete the surrounding physical row.
        let completed =
            rows > 0 || final_completed_row || (complete_literal_tail && final_empty_literal_row);
        if has_content && completed {
            output.push_plain("\n");
        }
        (output, completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_eof_gap_has_its_own_row_delimiters() {
        // Exact ALPHA/.sp 1/2/3 and rejected-empty-buffer sources ran the
        // pristine oracle first. term_vspace ends an additional physical
        // row; EOF must not consume that row as BEFORE's terminator.
        for rows in [1, 2, 3] {
            let mut flow = Flow::text("BEFORE".into());
            flow.gap(rows);
            let rendered = flow.finish(false);
            assert_eq!(
                rendered,
                format!("BEFORE{}", "\n".repeat(usize::from(rows) + 1))
            );
            assert_eq!(rendered.lines().count(), usize::from(rows) + 1);
        }
        let mut empty = Flow::default();
        empty.gap(2);
        assert_eq!(empty.finish(false), "\n\n");
    }

    #[test]
    fn transparent_children_share_a_budget_but_literal_rows_do_not() {
        let mut parent = Flow::text("BEFORE".into());
        parent.gap(3000);
        let mut child = Flow::default();
        child.gap(3000);
        child.push_text("AFTER\n\nLITERAL".into());
        parent.extend(child);
        assert_eq!(
            parent.finish(false),
            format!("BEFORE{}AFTER\n\nLITERAL", "\n".repeat(4097))
        );
    }

    #[test]
    fn stacked_cell_edges_share_the_active_cursor_until_a_real_row_arrives() {
        for leading in [true, false] {
            let mut parent = Flow::text("BEFORE".into());
            parent.gap(3000);
            let mut cell = Flow::default();
            cell.gap(3000);
            cell.push_text("BODY".into());
            if leading {
                parent.append_stack_cell(cell, false);
            } else {
                let mut first = Flow::text("FIRST".into());
                first.gap(3000);
                parent = first;
                parent.append_stack_cell(cell, false);
            }
            parent.complete_table_gap();
            let output = parent.finish(false);
            assert_eq!(output.matches('\n').count(), 4097);
            assert!(output.ends_with("BODY"));
        }
    }

    #[test]
    fn whole_data_row_consumes_gap_only_receipts_without_an_extra_blank() {
        for break_after in [false, true] {
            let mut parent = Flow::text("BEFORE".into());
            let mut cell = Flow::default();
            cell.gap(3000);
            parent.append_stack_cell(cell, break_after);
            parent.complete_table_gap();
            parent.gap(3000);
            parent.push_text("AFTER".into());
            let output = parent.finish(false);
            assert_eq!(output.matches('\n').count(), 6001);
            assert!(output.ends_with("AFTER"));
        }
        let mut nested = Flow::default();
        nested.gap(1);
        nested.complete_table_gap();
        let mut outer = Flow::default();
        outer.append_stack_cell(nested, true);
        assert_eq!(outer.finish(false), "\n");
    }

    #[test]
    fn ordinary_cell_open_rows_keep_their_receipt_through_eof_and_successors() {
        let mut eof = Flow::default();
        eof.append_stack_cell(Flow::text("A\n".into()), false);
        assert_eq!(eof.finish(false), "A\n\n");
        for (closed, expected) in [(false, "A\nB"), (true, "A\n\nB")] {
            let mut row = Flow::default();
            row.append_stack_cell(Flow::text("A\n".into()), closed);
            row.append_stack_cell(Flow::text("B".into()), false);
            assert_eq!(row.finish(false), expected);
        }
    }
}
