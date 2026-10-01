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
    Gap(u16),
}

impl Flow {
    pub(super) fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub(super) fn has_physical_rows(&self) -> bool {
        self.parts.iter().any(|part| match part {
            Part::Text(_) => true,
            Part::Gap(rows) => *rows > 0,
        })
    }
    pub(super) fn text(value: LayoutText) -> Self {
        let mut result = Self::default();
        result.push_text(value);
        result
    }

    pub(super) fn literal(value: LayoutText) -> Self {
        Self {
            parts: vec![Part::Text(value)],
        }
    }

    pub(super) fn push_text(&mut self, value: LayoutText) {
        if !value.is_empty() {
            self.parts.push(Part::Text(value));
        }
    }

    pub(super) fn gap(&mut self, rows: u16) {
        self.parts.push(Part::Gap(rows));
    }

    pub(super) fn extend(&mut self, other: Self) {
        self.parts.extend(other.parts);
    }

    pub(super) fn finish(self, preceding_content: bool) -> String {
        self.finish_layout(preceding_content).rendered
    }

    pub(super) fn finish_layout(self, preceding_content: bool) -> LayoutText {
        self.finish_rows(preceding_content).0
    }

    /// A rendered cell is split into physical rows before the following
    /// content arrives. Completed trailing gap rows need their own final
    /// delimiter; otherwise `split_terminator` would consume one as a mere
    /// close of the preceding printed row.
    pub(super) fn finish_cell(self) -> (LayoutText, bool) {
        self.finish_rows(false)
    }

    fn finish_rows(self, preceding_content: bool) -> (LayoutText, bool) {
        let mut output = LayoutText::default();
        let mut gap = GapPlan::default();
        let mut has_content = preceding_content;
        for part in self.parts {
            match part {
                Part::Gap(rows) => gap.append_resolved(rows),
                Part::Text(text) => {
                    if has_content {
                        output.push_plain("\n");
                    }
                    output.push_plain(&"\n".repeat(usize::from(gap.rows(0))));
                    output.append(&text);
                    has_content = true;
                    gap = GapPlan::default();
                }
            }
        }
        let rows = gap.rows(0);
        output.push_plain(&"\n".repeat(usize::from(rows)));
        // The final printed row also needs its closing delimiter outside a
        // table cell. Otherwise EOF consumes one completed empty row as the
        // preceding row's terminator (term_vspace's endline, term.c:489).
        if has_content && rows > 0 {
            output.push_plain("\n");
        }
        (output, rows > 0)
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
}
