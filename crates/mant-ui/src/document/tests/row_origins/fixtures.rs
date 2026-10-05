//! Shared request-relative hard-row origins.

pub(super) fn layout(rows: &[(u32, i32)]) -> mant_ir::InlineLayout {
    mant_ir::InlineLayout {
        row_hints: rows
            .iter()
            .map(|&(row, indent_columns)| mant_ir::RowLayoutHint {
                row,
                indent_columns,
            })
            .collect(),
    }
}
