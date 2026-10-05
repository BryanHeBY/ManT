//! Accepted portable table rows preserve physical contribution and ownership.
mod blocks;
mod cells;
use super::mapped::MappedText;
use cells::project_rows;
use mant_ir::{EntryOwner, TableRow, geometry::GapPlan};

#[derive(Clone, Copy, Default)]
enum Tail {
    #[default]
    Shared,
    Open,
    EndRow,
    CompletedRows,
}

#[derive(Clone, Copy)]
enum ParagraphTail {
    OpenCellRow,
    BlockBoundary,
}

/// Accepted IR row facts, not formatter state. Pending boundaries remain
/// separate through transparent containers until the next physical contribution.
/// Generated separators never acquire an owner.
#[derive(Default)]
struct Projection {
    mapped: MappedText,
    tail: Tail,
    physical: bool,
    /// An empty open row has an origin, but no padding until content arrives.
    open_row_indent: i32,
    leading_gap: GapPlan,
    pending_gap: GapPlan,
}

impl Projection {
    fn text(mapped: MappedText, tail: Tail, physical: bool) -> Self {
        Self {
            mapped,
            tail,
            physical,
            ..Self::default()
        }
    }

    fn gap(&mut self, rows: u16) {
        if self.physical {
            self.pending_gap.append_resolved(rows);
        } else {
            self.leading_gap.append_resolved(rows);
        }
    }

    fn append(&mut self, mut value: Self, separator: &str) {
        self.gap(value.leading_gap.rows(0));
        if !value.physical {
            self.gap(value.pending_gap.rows(0));
            return;
        }
        // A literal empty BODY can occupy the existing shared open row without
        // adding a glyph or closing it. Keep that row's deferred HEAD origin
        // until a later piece actually receives it, including another cell.
        let retains_open_row = self.physical
            && self.pending_gap.rows(0) == 0
            && matches!(self.tail, Tail::Open)
            && matches!(value.tail, Tail::Shared)
            && separator.is_empty()
            && value.mapped.text.is_empty();
        if self.physical {
            if self.pending_gap.rows(0) > 0 {
                self.complete_gap();
            } else {
                match self.tail {
                    Tail::EndRow => self.mapped.text.push('\n'),
                    Tail::CompletedRows => {}
                    Tail::Open => {
                        if !separator.contains('\n')
                            && (!separator.is_empty()
                                || value
                                    .mapped
                                    .text
                                    .split('\n')
                                    .next()
                                    .is_some_and(|row| !row.is_empty()))
                        {
                            self.mapped.text.push_str(
                                &" ".repeat(mant_ir::geometry::padding(self.open_row_indent)),
                            );
                        }
                        self.mapped.text.push_str(separator);
                    }
                    Tail::Shared => self.mapped.text.push_str(separator),
                }
            }
        }
        self.mapped.append(std::mem::take(&mut value.mapped));
        self.physical = true;
        if !retains_open_row {
            self.tail = value.tail;
            self.open_row_indent = value.open_row_indent;
        }
        self.pending_gap = value.pending_gap;
    }

    fn complete_gap(&mut self) {
        let rows = self.pending_gap.rows(0);
        if rows > 0 {
            // A literal open tail is an existing empty row. Close it before
            // appending independent resolved empty rows (term.c::term_vspace).
            if self.physical && !matches!(self.tail, Tail::CompletedRows) {
                self.mapped.text.push('\n');
            }
            self.mapped.text.push_str(&"\n".repeat(usize::from(rows)));
            self.tail = Tail::CompletedRows;
            self.pending_gap = GapPlan::default();
        }
    }

    fn structural_row(&mut self) {
        if !self.physical {
            self.pending_gap = std::mem::take(&mut self.leading_gap);
            if self.pending_gap.rows(0) > 0 {
                // These requests already own the empty data rows. Keep their
                // budget pending until another cell or this whole row ends;
                // structural occupancy adds no second empty-row delimiter.
                self.tail = Tail::CompletedRows;
            }
            self.physical = true;
        }
    }

    fn with_owner(mut self, owner: EntryOwner<'_>, track: bool) -> Self {
        self.mapped = self.mapped.with_owner(owner, track);
        self
    }

    fn finish(mut self) -> MappedText {
        let leading = self.leading_gap.rows(0);
        if leading > 0 {
            self.mapped.insert(0, &"\n".repeat(usize::from(leading)));
        }
        self.complete_gap();
        self.mapped
    }
}

fn join(values: impl IntoIterator<Item = Projection>, separator: &str) -> Projection {
    let mut output = Projection::default();
    for value in values {
        output.append(value, separator);
    }
    output
}

pub(super) fn rows(rows: &[TableRow], track: bool) -> Vec<MappedText> {
    project_rows(rows, track).map(Projection::finish).collect()
}

#[cfg(test)]
mod tests;
