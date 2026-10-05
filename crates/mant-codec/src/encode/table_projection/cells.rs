//! Project bounded table topology and finish each accepted cell boundary.
use super::blocks::plain_blocks;
use super::{ParagraphTail, Projection, Tail, join};
use crate::encode::mapped::MappedText;
use mant_ir::{
    TableCell, TableCellKind, TableRow, TableRowPlan, TableRuleCellKind, bounded_table_rows,
};

pub(super) fn project_rows(
    rows: &[TableRow],
    track: bool,
) -> impl Iterator<Item = Projection> + '_ {
    bounded_table_rows(rows)
        .into_iter()
        .zip(rows)
        .filter_map(|(plan, row)| (!mant_ir::table_row_is_navigation_only(row)).then_some(plan))
        .map(move |row| {
            let mut output = match row {
                TableRowPlan::Empty => Projection::text(MappedText::default(), Tail::EndRow, true),
                TableRowPlan::WholeRule { double } => Projection::text(
                    if double { "===" } else { "---" }.to_owned().into(),
                    Tail::Shared,
                    true,
                ),
                TableRowPlan::LayoutRule { cells } => Projection::text(
                    MappedText::join(
                        cells.iter().map(|cell| {
                            match cell {
                                TableRuleCellKind::Horizontal => "---",
                                TableRuleCellKind::DoubleHorizontal => "===",
                            }
                            .to_owned()
                            .into()
                        }),
                        " | ",
                    ),
                    Tail::Shared,
                    true,
                ),
                TableRowPlan::Dense { slots } => join(
                    slots.into_iter().map(|cell| {
                        cell.map_or_else(
                            || Projection::text(MappedText::default(), Tail::Shared, true),
                            |cell| plain_cell(cell, track),
                        )
                    }),
                    " | ",
                ),
                TableRowPlan::Sparse { cells } => join(
                    cells.into_iter().map(|positioned| {
                        let mut value = plain_cell(positioned.cell, track);
                        value.mapped.insert(
                            0,
                            &format!("column {}: ", positioned.column.saturating_add(1)),
                        );
                        value
                    }),
                    " | ",
                ),
            };
            // A whole data row consumes its pending boundary. A later row
            // begins an independent budget; navigation-only rows were filtered
            // before reaching this physical boundary.
            output.complete_gap();
            output
        })
}

fn plain_cell(cell: &TableCell, track: bool) -> Projection {
    // tbl_term.c::term_tbl/tbl_hrule distinguish rules from suppressed payload.
    let rule = match cell.kind {
        TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => Some("---"),
        TableCellKind::DoubleHorizontalRule | TableCellKind::IsolatedDoubleHorizontalRule => {
            Some("===")
        }
        TableCellKind::Text => None,
    };
    let mut output = rule.map_or_else(
        || plain_blocks(&cell.blocks, "; ", ParagraphTail::OpenCellRow, track),
        |rule| Projection::text(rule.to_owned().into(), Tail::Shared, true),
    );
    // An actual data cell retains its structural row even without glyphs.
    output.structural_row();
    if cell.break_after {
        // Explicit closure consumes this row's existing gap once. Later
        // leading requests use a new budget even if the gap is already full.
        output.complete_gap();
        if !matches!(output.tail, Tail::CompletedRows) {
            output.tail = Tail::EndRow;
        }
    }
    output
}
