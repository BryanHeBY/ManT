//! Ownership transfer of finalized native tbl cells and layout-rule rows.

use super::super::raw::{
    self, CDocument, CTableCell, CTableCellView, CTableRuleCell, CTableRuleCellView,
};
use super::{
    budget::TransferBudget,
    strings::{checked_string, split_visible_text},
};
use crate::{TableAlignment, TableCell, TableCellKind, TableFont, TableRowKind, TableRuleCellKind};

pub(super) fn table_row_kind(
    value: i32,
    layout_rules: Vec<TableRuleCellKind>,
) -> Result<Option<TableRowKind>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(TableRowKind::Data)),
        2 => Ok(Some(TableRowKind::HorizontalRule)),
        3 => Ok(Some(TableRowKind::DoubleHorizontalRule)),
        4 if !layout_rules.is_empty() => Ok(Some(TableRowKind::LayoutRule {
            cells: layout_rules,
        })),
        4 => Err("libmandoc returned an empty layout-only rule row".to_owned()),
        _ => Err("libmandoc returned an unknown table row kind".to_owned()),
    }
}

pub(super) unsafe fn copy_table_cells(
    document: *const CDocument,
    mut pointer: *const CTableCell,
    transfer_budget: &mut TransferBudget,
) -> Result<Vec<TableCell>, String> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        let mut view = std::mem::MaybeUninit::<CTableCellView>::uninit();
        if unsafe { raw::mant_mandoc_table_cell_snapshot(document, pointer, view.as_mut_ptr()) }
            == 0
        {
            return Err("libmandoc returned an invalid borrowed table cell".to_owned());
        }
        let view = unsafe { view.assume_init() };
        let (text, native_text) = split_visible_text(unsafe { checked_string(view.text) }?);
        let cell = TableCell {
            kind: match view.kind {
                1 => TableCellKind::Empty,
                2 => TableCellKind::HorizontalRule,
                3 => TableCellKind::DoubleHorizontalRule,
                4 => TableCellKind::IsolatedHorizontalRule,
                5 => TableCellKind::IsolatedDoubleHorizontalRule,
                _ => TableCellKind::Text,
            },
            font: match view.font {
                0 => None,
                1 => Some(TableFont::Roman),
                2 => Some(TableFont::Bold),
                3 => Some(TableFont::Italic),
                4 => Some(TableFont::BoldItalic),
                5 => Some(TableFont::Code),
                6 => Some(TableFont::CodeBold),
                7 => Some(TableFont::CodeItalic),
                _ => return Err("libmandoc returned an unknown table layout font".to_owned()),
            },
            text,
            native_text,
            text_block: view.text_block != 0,
            source_recovery_safe: view.source_recovery_safe != 0,
            vertical_continuation: view.vertical_continuation != 0,
            column_span: view.column_span.try_into().unwrap_or(u16::MAX),
            row_span: view.row_span.try_into().unwrap_or(u16::MAX),
            alignment: match view.alignment {
                1 => TableAlignment::Center,
                2 => TableAlignment::Right,
                _ => TableAlignment::Left,
            },
        };
        transfer_budget.charge(
            std::mem::size_of::<TableCell>()
                .saturating_add(cell.text.as_ref().map_or(0, String::len))
                .saturating_add(cell.native_text.as_ref().map_or(0, String::len)),
        )?;
        cells.push(cell);
        pointer = view.next;
    }
    Ok(cells)
}

pub(super) unsafe fn copy_table_rule_cells(
    document: *const CDocument,
    mut pointer: *const CTableRuleCell,
    transfer_budget: &mut TransferBudget,
) -> Result<Vec<TableRuleCellKind>, String> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        let mut view = std::mem::MaybeUninit::<CTableRuleCellView>::uninit();
        if unsafe {
            raw::mant_mandoc_table_rule_cell_snapshot(document, pointer, view.as_mut_ptr())
        } == 0
        {
            return Err("libmandoc returned an invalid borrowed table rule cell".to_owned());
        }
        let view = unsafe { view.assume_init() };
        transfer_budget.charge(std::mem::size_of::<TableRuleCellKind>())?;
        cells.push(match view.kind {
            1 => TableRuleCellKind::Horizontal,
            2 => TableRuleCellKind::DoubleHorizontal,
            _ => return Err("libmandoc returned an unknown table rule cell kind".to_owned()),
        });
        pointer = view.next;
    }
    Ok(cells)
}
