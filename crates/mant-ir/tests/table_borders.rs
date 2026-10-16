//! Border preferences never replace body owners or fabricate data rows.
use mant_ir::{
    Block, Document, DocumentMeta, DocumentSource, Inline, LayoutHint, SourceFormat,
    TableBorderStyle, TableBorders, TableCell, TableCellKind, TableColumnBorder, TableRow,
    TableRowKind, table_row_is_navigation_only, validate_document,
};

fn navigation_row() -> TableRow {
    TableRow {
        kind: TableRowKind::Data,
        column_borders: vec![],
        cells: vec![TableCell {
            blocks: vec![Block::Paragraph {
                children: vec![Inline::anchor("destination")],
                inline_layout: mant_ir::InlineLayout::default(),
                layout: LayoutHint::default(),
                source: None,
            }],
            kind: TableCellKind::Text,
            break_after: false,
            column_span: 1,
            row_span: 1,
            alignment: None,
            preferred_width_columns: None,
        }],
    }
}

fn document(row: TableRow) -> Document {
    Document {
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Markdown,
            path: None,
        },
        meta: DocumentMeta::default(),
        heading: None,
        fragment_aliases: vec![],
        diagnostics: vec![],
        sections: vec![],
        blocks: vec![Block::Table {
            rows: vec![row],
            borders: TableBorders {
                outer: Some(TableBorderStyle::Single),
                ..TableBorders::default()
            },
            alignment: None,
            column_count: None,
            column_preferences: mant_ir::ColumnPreferences::default(),
            layout: LayoutHint::default(),
            source: None,
        }],
    }
}

#[test]
fn global_frames_do_not_materialize_navigation_but_authored_row_edges_are_physical() {
    let mut row = navigation_row();
    assert!(table_row_is_navigation_only(&row));
    let framed = document(row.clone());
    let Block::Table { rows, borders, .. } = &framed.blocks[0] else {
        panic!("table")
    };
    assert!(!borders.is_empty());
    assert!(table_row_is_navigation_only(&rows[0]));
    row.column_borders.push(TableColumnBorder {
        column: 0,
        style: TableBorderStyle::Single,
    });
    assert!(!table_row_is_navigation_only(&row));
    let restored: TableRow = serde_json::from_str(&serde_json::to_string(&row).unwrap()).unwrap();
    assert!(!table_row_is_navigation_only(&restored));
    for kind in [
        TableRowKind::Data,
        TableRowKind::HorizontalRule,
        TableRowKind::DoubleHorizontalRule,
    ] {
        assert!(!table_row_is_navigation_only(&TableRow {
            kind,
            cells: vec![],
            column_borders: vec![]
        }));
    }
}

#[test]
fn row_edges_and_frame_defaults_combine_without_order_dependent_stroke_changes() {
    let mut row = navigation_row();
    row.column_borders = vec![
        TableColumnBorder {
            column: 0,
            style: TableBorderStyle::Single,
        },
        TableColumnBorder {
            column: 1,
            style: TableBorderStyle::Double,
        },
    ];
    let borders = TableBorders {
        outer: Some(TableBorderStyle::Double),
        ..TableBorders::default()
    };
    assert_eq!(
        row.column_border_style(&borders, 0, 1),
        Some(TableBorderStyle::Double)
    );
    assert_eq!(
        row.column_border_style(&borders, 1, 1),
        Some(TableBorderStyle::Double)
    );
    assert_eq!(row.column_border_style(&borders, 2, 1), None);
    for left in [TableBorderStyle::Single, TableBorderStyle::Double] {
        for right in [TableBorderStyle::Single, TableBorderStyle::Double] {
            assert_eq!(left.stronger(right), right.stronger(left));
        }
    }
}

#[test]
fn trusted_rust_values_report_the_same_range_and_soft_width_errors_as_wire_decoding() {
    for (edges, width, code) in [
        (vec![0, 2], None, "ir.invalid-table-column-borders"),
        (vec![1, 0], None, "ir.invalid-table-column-borders"),
        (vec![0, 0], None, "ir.invalid-table-column-borders"),
        (vec![], Some(0), "ir.invalid-table-cell-width"),
    ] {
        let mut row = navigation_row();
        row.column_borders = edges
            .into_iter()
            .map(|column| TableColumnBorder {
                column,
                style: TableBorderStyle::Single,
            })
            .collect();
        row.cells[0].preferred_width_columns = width;
        let document = document(row);
        assert!(
            validate_document(&document)
                .iter()
                .any(|finding| finding.code.as_deref() == Some(code)),
            "{document:?}"
        );
        assert!(
            serde_json::from_str::<Document>(&serde_json::to_string(&document).unwrap()).is_err()
        );
    }
}
