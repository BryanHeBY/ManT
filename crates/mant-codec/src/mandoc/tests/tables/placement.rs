//! Whole-table placement and declared topology survive native lowering.
use super::*;

#[test]
fn native_table_placement_keeps_final_columns_and_table_boundaries() {
    // This exact input passed the fixed CVS -Ttree/-Tutf8/-Tlint oracle.
    // tbl_term.c positions the complete table from shared final opts;
    // tbl_layout.c::cell_alloc includes later T& declarations in opts.cols.
    let source = b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.TS\ncentre;\nl.\n_\nAAA\n.T&\nl l l.\nBBB\tCCC\tDDD\n.TE\n.TS\nl.\nLAST\n.TE\n";
    let document = parse_manual_bytes(std::path::Path::new("table-options.1"), source).unwrap();
    let [
        Block::Table {
            alignment,
            column_count,
            rows,
            ..
        },
        Block::Table {
            alignment: next_alignment,
            column_count: next_count,
            rows: next_rows,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("two independently owned tables");
    };
    assert_eq!(*alignment, Some(mant_ir::TableAlignment::Center));
    assert_eq!(*column_count, Some(3));
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].kind, mant_ir::TableRowKind::HorizontalRule);
    assert_eq!(rows[0].cells.len(), 0);
    assert_eq!(rows[1].cells.len(), 1);
    assert_eq!(rows[2].cells.len(), 3);
    assert_eq!(*next_alignment, None);
    assert_eq!(*next_count, Some(1));
    assert_eq!(next_rows.len(), 1);
}

#[test]
fn declared_empty_columns_survive_without_synthetic_cell_owners() {
    // Fixed CVS -Ttree/-Tutf8/-Tlint: out.c::tblcalc retains all three
    // declared columns although tbl_data.c creates only the actual cell.
    let source =
        b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.TS\ncenter;\nl l l.\nAAA\n.TE\n";
    let document =
        parse_manual_bytes(std::path::Path::new("table-empty-columns.1"), source).unwrap();
    let [
        Block::Table {
            alignment,
            column_count,
            rows,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("one native table");
    };
    assert_eq!(*alignment, Some(mant_ir::TableAlignment::Center));
    assert_eq!(*column_count, Some(3));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].cells.len(), 1);
}

#[test]
fn layout_rule_strengths_survive_later_column_declarations() {
    // Exact fixed CVS -Ttree/-Tascii/-Tutf8/-Tlint: tbl_term.c keeps the
    // _/= layout strengths even when the later T& grows shared opts.cols.
    let source = b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.TS\ncenter;\n_ =\nl l.\nA\tB\n.T&\nl l l.\nC\tD\tE\n.TE\n";
    let document = parse_manual_bytes(std::path::Path::new("table-mixed-rules.1"), source).unwrap();
    let [
        Block::Table {
            alignment,
            column_count,
            rows,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("one native table");
    };
    assert_eq!(*alignment, Some(mant_ir::TableAlignment::Center));
    assert_eq!(*column_count, Some(3));
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[0].kind,
        mant_ir::TableRowKind::LayoutRule {
            cells: vec![
                mant_ir::TableRuleCellKind::Horizontal,
                mant_ir::TableRuleCellKind::DoubleHorizontal,
            ],
        }
    );
    assert_eq!(rows[0].cells.len(), 0);
    assert_eq!(rows[1].cells.len(), 2);
    assert_eq!(rows[2].cells.len(), 3);
}

#[test]
fn caption_cell_and_whole_table_alignment_are_independent() {
    // Exact fixed CVS -Ttree/-Tutf8/-Tlint distinguishes roff_term.c pre_ce
    // from tbl_term.c table positioning and tbl_cell c/r/l layout.
    let source = b".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.ce 1\nCAPTION\n.TS\ncenter;\nc r l.\nAAA\tBBB\tCCC\nLONGLEFT\tLONGMIDDLE\tLONGRIGHT\n_\n.TE\n";
    let document = parse_manual_bytes(std::path::Path::new("table-alignments.1"), source).unwrap();
    let [
        Block::Paragraph { inline_layout, .. },
        Block::Table {
            alignment,
            column_count,
            rows,
            ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("caption and table");
    };
    assert_eq!(
        inline_layout.row_hints[0].alignment,
        mant_ir::TextAlignment::Center
    );
    assert_eq!(*alignment, Some(mant_ir::TableAlignment::Center));
    assert_eq!(*column_count, Some(3));
    assert_eq!(
        rows[0]
            .cells
            .iter()
            .map(|cell| cell.alignment)
            .collect::<Vec<_>>(),
        [
            Some(mant_ir::TableAlignment::Center),
            Some(mant_ir::TableAlignment::Right),
            Some(mant_ir::TableAlignment::Left)
        ]
    );
    assert_eq!(rows[2].kind, mant_ir::TableRowKind::HorizontalRule);
}
