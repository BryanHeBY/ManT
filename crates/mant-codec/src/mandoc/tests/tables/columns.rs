//! columns table-lowering contracts; original sources and gold remain unchanged.
use super::*;

#[test]
fn ragged_matrix_keeps_later_column_rows_through_ir_json() {
    let document = parse_manual_bytes(
        std::path::Path::new("short-matrix.7"),
        b".TH MATRIX 7\n.EQ\nmatrix { lcol { a } rcol { b above c } }\n.EN\n",
    )
    .unwrap();
    let Block::Equation {
        value,
        expression: Some(expression),
        ..
    } = &document.blocks[0]
    else {
        panic!("matrix block");
    };
    // CVS eqn.h stores a matrix as column piles. Pinned eqn_html.c uses the
    // first column's row count and drops `c`; the native tree and UTF-8 oracle
    // retain it, so the shared reading projection uses the longest column.
    assert_eq!(expression.children[0].kind, mant_ir::EquationKind::Matrix);
    assert_eq!(value, "matrix(a, b; , c)");
    // The exact source was checked with fixed CVS -Tutf8/-Thtml/-Ttree.
    // Native argument counters are resolved into Group/Sequence nodes by
    // lowering, and EQN_DEFSIZE's INT_MIN becomes inherited presentation.
    let wire = serde_json::to_value(&document).unwrap();
    assert!(
        wire["blocks"][0]["expression"]
            .get("expectedArgs")
            .is_none()
    );
    assert!(wire["blocks"][0]["expression"].get("actualArgs").is_none());
    assert!(
        wire["blocks"][0]["expression"]["children"][0]
            .get("size")
            .is_none()
    );
    let json = serde_json::to_vec(&document).unwrap();
    let restored: mant_ir::Document = serde_json::from_slice(&json).unwrap();
    let Block::Equation {
        value: restored_value,
        expression: Some(restored_expression),
        ..
    } = &restored.blocks[0]
    else {
        panic!("round-trip matrix");
    };
    assert_eq!(restored_value, &restored_expression.readable_text());
    assert_eq!(mant_ir::validate_document(&restored).len(), 0);
}

#[test]
fn ragged_matrix_keeps_earlier_column_rows_through_ir() {
    // The exact input was checked with the fixed -Ttree/-Thtml/-Tutf8 oracle.
    // CVS eqn_html.c::eqn_box traverses rows in matrix column order and emits
    // an empty trailing cell when the second column is shorter.
    let document = parse_manual_bytes(
        std::path::Path::new("long-first-matrix.7"),
        b".TH MATRIX 7\n.EQ\nmatrix { lcol { a above b } rcol { c } }\n.EN\n",
    )
    .unwrap();
    let Block::Equation {
        value,
        expression: Some(expression),
        ..
    } = &document.blocks[0]
    else {
        panic!("matrix block");
    };
    assert_eq!(value, "matrix(a, c; b, )");
    assert_eq!(value, &expression.readable_text());
    assert_eq!(mant_ir::validate_document(&document).len(), 0);
}

#[test]
fn bl_column_widths_measure_executed_device_glyphs() {
    // mdoc_term.c::termp_it_pre (709-715) weighs each declared `Bl -column`
    // string with term_strlen() after escape execution (term.c:18.2), then
    // folds the basic units through term_hspan(SCALE_BU). Checked against
    // the fixed -Tutf8 reference: `\(lq\(rq` and `a\&b` advance two columns,
    // the two CJK glyphs advance four.
    let document = parse_manual_bytes(
        std::path::Path::new("bl-column-device-widths.7"),
        b".Dd September 8, 2026\n.Dt BL-COLUMN-WIDTH 7\n.Os\n.Sh DESCRIPTION\n.Bl -column \\(lq\\(rq a\\&b \xe4\xb8\xad\xe4\xb8\xad\n.It A Ta B\n.El\n",
    )
    .expect("lower a column list");
    let Block::Table {
        column_preferences, ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("expected one lowered column table");
    };
    assert_eq!(
        column_preferences,
        &mant_ir::ColumnPreferences {
            widths: vec![2, 2, 4],
            gap_columns: 4,
            max_alignment_padding_columns: Some(256),
            extra_width_columns: Some(10),
        }
    );
}

#[test]
fn bl_column_widths_saturate_at_the_ir_bound() {
    // External audit CW01: an oversized declaration must saturate into the
    // IR's u16 columns instead of overflowing. The fixed reference accepts
    // the 70000-character declaration and truncates its single advance.
    let document = parse_manual_bytes(
        std::path::Path::new("bl-column-oversized-width.7"),
        format!(
            ".Dd September 8, 2026\n.Dt BL-COLUMN-WIDE 7\n.Os\n.Sh DESCRIPTION\n.Bl -column {} one\n.It A Ta B\n.El\n",
            "x".repeat(70_000)
        )
        .as_bytes(),
    )
    .expect("lower an oversized column declaration");
    let Block::Table {
        column_preferences, ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("expected one lowered column table");
    };
    assert_eq!(
        column_preferences,
        &mant_ir::ColumnPreferences {
            widths: vec![u16::MAX, 3],
            gap_columns: 4,
            max_alignment_padding_columns: Some(256),
            extra_width_columns: Some(10),
        }
    );
}
