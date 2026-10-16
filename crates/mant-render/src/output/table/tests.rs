use super::*;
use mant_ir::TableCellKind;

#[cfg(test)]
fn table_rows(rows: &[TableRow], mut render_cell: impl FnMut(&TableCell) -> String) -> Vec<String> {
    let prepared = projected_table_rows(
        rows,
        TableProjection {
            gap: 2,
            origin: 0,
            viewport: None,
            bounded_frame: false,
            column_count: None,
        },
        &mant_ir::TableBorders::default(),
        |cell| (render_cell(cell), false, true),
    );
    prepared
        .into_iter()
        .flat_map(|row| match row {
            ProjectedTableRow::FrameWidth(_) => Vec::new(),
            ProjectedTableRow::Content(rows, _, _) => rows,
            ProjectedTableRow::Rule(cells, _) => vec![
                cells
                    .iter()
                    .map(|kind| match kind {
                        TableRuleCellKind::Horizontal => "---",
                        TableRuleCellKind::DoubleHorizontal => "===",
                    })
                    .collect::<Vec<_>>()
                    .join(" | "),
            ],
        })
        .collect()
}

#[cfg(test)]
impl TableText for String {
    fn plain(value: &str) -> Self {
        value.to_owned()
    }
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    fn ends_with_break(&self) -> bool {
        self.ends_with('\n')
    }
    fn width(&self) -> usize {
        mant_ir::geometry::text_width(self)
    }
    fn physical_lines(&self) -> Vec<Self> {
        if self.is_empty() {
            vec![Self::new()]
        } else {
            self.split_terminator('\n').map(str::to_owned).collect()
        }
    }
    fn join(values: &[Self], separator: &str) -> Self {
        values.join(separator)
    }
    fn prefixed(self, prefix: &str) -> Self {
        format!("{prefix}{self}")
    }
    fn append(&mut self, other: &Self) {
        self.push_str(other);
    }
}

fn cell() -> TableCell {
    TableCell {
        preferred_width_columns: None,
        break_after: false,
        kind: TableCellKind::Text,
        blocks: Vec::new(),
        column_span: 1,
        row_span: 1,
        alignment: None,
    }
}

#[test]
fn dense_rows_preserve_cell_line_boundaries() {
    let rows = [TableRow {
        column_borders: Vec::new(),
        kind: mant_ir::TableRowKind::Data,
        cells: vec![cell(), cell()],
    }];
    let mut values = ["A\nB C", "RIGHT"].into_iter();
    assert_eq!(
        table_rows(&rows, |_| values.next().unwrap().to_owned()),
        ["A | RIGHT", "B C"]
    );
}

#[test]
fn dense_rows_preserve_a_leading_empty_cell_line() {
    let rows = [TableRow {
        column_borders: Vec::new(),
        kind: mant_ir::TableRowKind::Data,
        cells: vec![cell()],
    }];
    assert_eq!(table_rows(&rows, |_| "\nBODY".to_owned()), ["", "BODY"]);
    assert_eq!(table_rows(&rows, |_| "BODY\n".to_owned()), ["BODY"]);
    assert_eq!(table_rows(&rows, |_| "\n".to_owned()), [""]);
}

#[test]
fn structural_empty_and_rule_rows_keep_one_physical_line_each() {
    let rows = [
        TableRow {
            column_borders: Vec::new(),
            kind: mant_ir::TableRowKind::Data,
            cells: Vec::new(),
        },
        TableRow {
            column_borders: Vec::new(),
            kind: mant_ir::TableRowKind::HorizontalRule,
            cells: Vec::new(),
        },
        TableRow {
            column_borders: Vec::new(),
            kind: mant_ir::TableRowKind::DoubleHorizontalRule,
            cells: Vec::new(),
        },
    ];
    assert_eq!(table_rows(&rows, |_| unreachable!()), ["", "---", "==="]);
}

#[test]
fn data_rows_render_cell_rule_roles_without_calling_the_text_renderer() {
    let rows = [TableRow {
        column_borders: Vec::new(),
        kind: mant_ir::TableRowKind::Data,
        cells: vec![
            TableCell {
                preferred_width_columns: None,
                kind: TableCellKind::HorizontalRule,
                ..cell()
            },
            TableCell {
                preferred_width_columns: None,
                kind: TableCellKind::DoubleHorizontalRule,
                ..cell()
            },
        ],
    }];
    assert_eq!(table_rows(&rows, |_| unreachable!()), ["--- | ==="]);
}

#[test]
fn sparse_rows_align_multiline_column_labels() {
    let rows = [TableRow {
        column_borders: Vec::new(),
        kind: mant_ir::TableRowKind::Data,
        cells: vec![TableCell {
            preferred_width_columns: None,
            column_span: u32::from(u16::MAX),
            ..cell()
        }],
    }];
    assert_eq!(
        table_rows(&rows, |_| "FIRST\nSECOND".to_owned()),
        ["column 1: FIRST", "  SECOND"]
    );
}

#[test]
fn tall_dense_rows_fall_back_before_slot_amplification() {
    let rows = [TableRow {
        column_borders: Vec::new(),
        kind: mant_ir::TableRowKind::Data,
        cells: vec![
            TableCell {
                preferred_width_columns: None,
                column_span: 255,
                ..cell()
            },
            cell(),
        ],
    }];
    let tall = std::iter::repeat_n("X", 128).collect::<Vec<_>>().join("\n");
    let mut values = [String::new(), tall].into_iter();
    let rendered = table_rows(&rows, |_| values.next().unwrap());
    assert_eq!(rendered.len(), 129);
    assert_eq!(rendered[0], "column 1: ");
    assert_eq!(rendered[1], "column 256: X");
    assert!(rendered[2..].iter().all(|line| line == "  X"));
    assert!(rendered.iter().all(|line| line.len() < 32));
}
