use super::{Block, Inline, LayoutHint, ListItem, ListKind, TableCell};

pub(super) fn navigation_table(widths: &[u16], origin: i32, cells: Vec<Vec<Block>>) -> Block {
    Block::Table {
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: cells
                .into_iter()
                .map(|blocks| TableCell {
                    break_after: false,
                    blocks,
                    kind: mant_ir::TableCellKind::Text,
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                })
                .collect(),
        }],
        column_preferences: declared_preferences(widths),
        layout: LayoutHint {
            indent_columns: origin,
            ..Default::default()
        },
        source: None,
    }
}

pub(super) fn paragraph(text: &str, indent: i32) -> Block {
    Block::Paragraph {
        children: vec![Inline::Text { value: text.into() }],
        inline_layout: mant_ir::InlineLayout::default(),
        layout: LayoutHint {
            indent_columns: indent,
            ..Default::default()
        },
        source: None,
    }
}

pub(super) fn plain_list(blocks: Vec<Block>, indent: i32) -> Block {
    Block::List {
        kind: ListKind::Plain,
        compact: true,
        items: vec![ListItem {
            layout: mant_ir::ListItemLayout::default(),
            blocks,
            source: None,
            entry: None,
        }],
        layout: LayoutHint {
            indent_columns: indent,
            ..Default::default()
        },
        source: None,
    }
}

pub(super) fn declared_column_table(widths: &[u16], cells: &[&str]) -> Block {
    Block::Table {
        column_preferences: declared_preferences(widths),
        rows: vec![mant_ir::TableRow {
            kind: mant_ir::TableRowKind::Data,
            cells: cells
                .iter()
                .map(|text| TableCell {
                    break_after: false,
                    kind: mant_ir::TableCellKind::Text,
                    blocks: vec![paragraph(text, 0)],
                    column_span: 1,
                    row_span: 1,
                    alignment: None,
                })
                .collect(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

fn declared_preferences(widths: &[u16]) -> mant_ir::ColumnPreferences {
    if widths.is_empty() {
        return mant_ir::ColumnPreferences::default();
    }
    mant_ir::ColumnPreferences {
        widths: widths.to_vec(),
        gap_columns: match widths.len() {
            n if n < 5 => 4,
            5 => 3,
            _ => 1,
        },
        advance_limit_columns: Some(256),
        extra_width_columns: Some(10),
    }
}
