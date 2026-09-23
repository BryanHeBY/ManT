//! Logical native tbl lowering; physical fixed geometry is a separate path.

use libmandoc_rs::structured::{
    NativeBlock, NativeTableAlignment, NativeTableCell, NativeTableCellKind, NativeTableRowKind,
};
use mant_ir::{
    Block, LayoutHint, TableAlignment, TableCell, TableCellKind, TableRow, TableRowKind,
    TableRuleCellKind,
};

use super::{
    NativeProjectionError, NativeProseProjection,
    address::AddressPlan,
    content::{root_inlines, source_for},
    index::NativeLoweringIndex,
    store::NativeContentMap,
};

pub(super) fn lower_table(
    projection: &NativeProseProjection,
    index: &NativeLoweringIndex,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    block: &NativeBlock,
) -> Result<Block, NativeProjectionError> {
    let native = projection.document();
    let table_index = index
        .table_by_block
        .get(block.key().get() as usize - 1)
        .copied()
        .flatten()
        .ok_or(NativeProjectionError::InvalidRelation(
            "table block has no table record",
        ))?;
    let table = &native.tables()[table_index];
    let mut rows = Vec::new();
    rows.try_reserve_exact(index.rows_by_table[table_index].len())
        .map_err(|_| NativeProjectionError::InvalidRelation("table row allocation"))?;
    for &row_index in &index.rows_by_table[table_index] {
        let native_row = &native.table_rows()[row_index];
        let mut cells = Vec::new();
        cells
            .try_reserve_exact(index.cells_by_row[row_index].len())
            .map_err(|_| NativeProjectionError::InvalidRelation("table cell allocation"))?;
        for &cell_index in &index.cells_by_row[row_index] {
            cells.push(lower_cell(
                projection,
                addresses,
                content,
                &native.table_cells()[cell_index],
            )?);
        }
        let kind = match native_row.kind() {
            NativeTableRowKind::Data => TableRowKind::Data,
            NativeTableRowKind::HorizontalRule => TableRowKind::HorizontalRule,
            NativeTableRowKind::DoubleHorizontalRule => TableRowKind::DoubleHorizontalRule,
            NativeTableRowKind::LayoutRule => {
                let strengths = cells
                    .iter()
                    .map(|cell| match cell.kind {
                        TableCellKind::HorizontalRule | TableCellKind::IsolatedHorizontalRule => {
                            Ok(TableRuleCellKind::Horizontal)
                        }
                        TableCellKind::DoubleHorizontalRule
                        | TableCellKind::IsolatedDoubleHorizontalRule => {
                            Ok(TableRuleCellKind::DoubleHorizontal)
                        }
                        TableCellKind::Text => Err(NativeProjectionError::InvalidRelation(
                            "layout rule contains text cell",
                        )),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                cells.clear();
                TableRowKind::LayoutRule { cells: strengths }
            }
        };
        rows.push(TableRow { kind, cells });
    }
    Ok(Block::Table {
        rows,
        fixed_view: table
            .fixed_view()
            .map(|key| content.fixed_view(key))
            .transpose()?,
        layout: LayoutHint::default(),
        source: source_for(projection, table.provenance()),
    })
}

fn lower_cell(
    projection: &NativeProseProjection,
    addresses: &AddressPlan,
    content: &NativeContentMap,
    cell: &NativeTableCell,
) -> Result<TableCell, NativeProjectionError> {
    let point = cell.point().ok_or(NativeProjectionError::InvalidRelation(
        "table cell has no content point",
    ))?;
    let public_point = content.point(point)?;
    let public_root = content
        .store()
        .point(public_point)
        .ok_or(NativeProjectionError::InvalidRelation(
            "table cell point is missing",
        ))?
        .root;
    let children = root_inlines(
        projection,
        addresses,
        content,
        content.native_root(public_root)?,
    )?;
    let kind = match cell.kind() {
        NativeTableCellKind::Text => TableCellKind::Text,
        NativeTableCellKind::HorizontalRule => TableCellKind::HorizontalRule,
        NativeTableCellKind::DoubleHorizontalRule => TableCellKind::DoubleHorizontalRule,
        NativeTableCellKind::IsolatedHorizontalRule => TableCellKind::IsolatedHorizontalRule,
        NativeTableCellKind::IsolatedDoubleHorizontalRule => {
            TableCellKind::IsolatedDoubleHorizontalRule
        }
    };
    if kind != TableCellKind::Text && !children.is_empty() {
        return Err(NativeProjectionError::InvalidRelation(
            "rule cell contains logical text",
        ));
    }
    let source = source_for(projection, cell.provenance());
    let blocks = if children.is_empty() {
        Vec::new()
    } else {
        vec![Block::Paragraph {
            children,
            layout: LayoutHint::default(),
            source,
        }]
    };
    Ok(TableCell {
        kind,
        blocks,
        column_span: u16::try_from(cell.column_span()).map_err(|_| {
            NativeProjectionError::InvalidRelation("table column span exceeds IR range")
        })?,
        row_span: u16::try_from(cell.row_span()).map_err(|_| {
            NativeProjectionError::InvalidRelation("table row span exceeds IR range")
        })?,
        alignment: Some(match cell.alignment() {
            NativeTableAlignment::Left => TableAlignment::Left,
            NativeTableAlignment::Center => TableAlignment::Center,
            NativeTableAlignment::Right => TableAlignment::Right,
        }),
        source,
    })
}
