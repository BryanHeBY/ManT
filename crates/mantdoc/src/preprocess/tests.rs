use crate::{
    DiagnosticCode, NodeKind, Parser, ParserConfig, Severity, Source, SourceName, TableAlignment,
};

#[cfg(feature = "render")]
use super::TableTerminalBorder;
use super::{
    CellFormat, parse_layout_line, table_cell_text, table_field_text, trailing_empty_request,
};

fn parse(bytes: &[u8]) -> crate::ParseReport {
    let name = SourceName::new("preprocess.1").unwrap();
    Parser::default().parse(Source::new(&name, bytes)).unwrap()
}

#[test]
fn table_cells_project_unicode_controls_as_question_marks() {
    assert_eq!(
        table_cell_text("before\0\u{001f}\u{0080}after", false, false).as_ref(),
        "before???after"
    );
    assert_eq!(
        table_cell_text("normal text", false, false).as_ref(),
        "normal text"
    );
    assert_eq!(
        table_cell_text("raw \u{00bf}", true, false).as_ref(),
        "raw ?"
    );
    assert_eq!(
        table_cell_text("UTF-8 \u{00bf}", false, true).as_ref(),
        "UTF-8 \\[u00BF]"
    );
    assert_eq!(
        table_cell_text("UTF-8 \u{0080}", false, true).as_ref(),
        "UTF-8 \\[u0080]"
    );
}

#[test]
fn retains_one_masked_space_after_an_ascii_control_unicode_escape() {
    assert_eq!(table_field_text(r"\[u0000] "), r"\[u0000] ");
    assert_eq!(table_field_text(r"\[u00A0] "), r"\[u00A0]");
    assert_eq!(table_field_text("ordinary "), "ordinary");
}

#[test]
fn converts_basic_tbl_rows_and_display_eqn_before_man_lowering() {
    let report = parse(
            b".TH PREPROCESS 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl r.\nleft:right\n.TE\n.EQ\ndelim $$\nx sup 2\n.EN\n",
        );
    let nodes = report.document.preorder().collect::<Vec<_>>();
    let table = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(table.table_cells().len(), 2);
    assert_eq!(table.table_cells()[0].text.as_deref(), Some("left"));
    assert_eq!(table.table_cells()[1].text.as_deref(), Some("right"));
    assert_eq!(table.table_cells()[1].alignment, TableAlignment::Right);
    let equation = nodes
        .iter()
        .find(|node| node.kind() == NodeKind::Equation)
        .unwrap();
    assert_eq!(equation.equation(), Some("x ^ 2"));
    assert!(
        nodes
            .iter()
            .all(|node| { !matches!(node.macro_name(), Some("TS" | "TE" | "EQ" | "EN")) })
    );
}

#[test]
fn table_font_modifiers_do_not_form_extra_layout_columns() {
    assert_eq!(
        super::parse_format_row("cb|cfCW|ci"),
        vec![
            super::CellFormat::Center,
            super::CellFormat::Center,
            super::CellFormat::Center,
        ]
    );
    assert_eq!(
        super::parse_format_row("cFI | cf(foobar) | cFB"),
        vec![
            super::CellFormat::Center,
            super::CellFormat::Center,
            super::CellFormat::Center,
        ]
    );
}

#[test]
fn tbl_width_modifiers_keep_their_terminal_cell_widths() {
    let parsed = parse_layout_line("lw2 lw(2n) lw(0.16i).");
    assert_eq!(parsed.rows.len(), 1);
    assert_eq!(
        parsed.rows[0]
            .terminal_cells
            .iter()
            .map(|cell| cell.minimum_width)
            .collect::<Vec<_>>(),
        vec![Some(2), Some(2), Some(2)]
    );
}

#[test]
fn comma_separated_tbl_layout_rows_keep_independent_span_state() {
    let parsed = parse_layout_line("l|p-1l bsil|||l,l|l\t ilb^|i||l.");
    assert!(parsed.complete);
    assert_eq!(parsed.rows.len(), 2);
    assert_eq!(
        parsed.rows[0].cells,
        vec![
            CellFormat::Left,
            CellFormat::Left,
            CellFormat::Span,
            CellFormat::Left,
            CellFormat::Left,
        ]
    );
    assert_eq!(
        parsed.rows[1].cells,
        vec![
            CellFormat::Left,
            CellFormat::Left,
            CellFormat::Left,
            CellFormat::Down,
            CellFormat::Left,
        ]
    );
    assert_eq!(parsed.rows[0].vertical_bar_offsets, vec![13]);
    assert_eq!(parsed.rows[1].vertical_bar_offsets, vec![28]);
    assert_eq!(parsed.rows[1].leading_down_offsets, vec![24]);
}

#[test]
fn invalid_tbl_fonts_do_not_terminate_the_layout() {
    let report = parse(
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nlfI lfX\nlfB lfIB\nlfI lf.\nlfB lfI.\nitalic\tone char\nbold\ttwo chars\nitalic\tdot\nbold\titalic\n.TE\n",
        );
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code.as_str(), diagnostic.message.as_ref()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::TBL_UNKNOWN_FONT,
                "unknown font, skipping request: TS fX",
            ),
            (
                DiagnosticCode::TBL_UNKNOWN_FONT,
                "unknown font, skipping request: TS fIB",
            ),
            (
                DiagnosticCode::TBL_UNKNOWN_FONT,
                "unknown font, skipping request: TS f.",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(4, 7), (5, 7), (6, 7)]);
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows.iter()
            .map(|row| {
                row.table_cells()
                    .iter()
                    .map(|cell| cell.text.as_deref().unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [
            vec!["italic", "one char"],
            vec!["bold", "two chars"],
            vec!["italic", "dot"],
            vec!["bold", "italic"],
        ]
    );
}

#[test]
fn tbl_excessive_spacing_reports_the_first_spacing_digit() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nR 10 L.\nleft\tright\n.TE\n");
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::TBL_EXCESSIVE_SPACING
    );
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.message.as_ref(),
        "ignoring excessive spacing in tbl layout: 10"
    );
    let position = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (4, 3));
}

#[test]
fn tbl_layout_reports_a_leading_span_without_rewriting_cells() {
    let report = parse(
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nS L S S\nL L L L L L.\nspan\tend\n1\t2\t3\t4\t5\t6\n.TE\n",
        );
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_LEADING_SPAN);
    assert_eq!(diagnostic.message.as_ref(), "tbl line starts with span");
    let position = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (4, 1));
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows.iter()
            .map(|row| {
                row.table_cells()
                    .iter()
                    .map(|cell| cell.text.as_deref())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>(),
        [
            vec![Some("span"), Some("end")],
            vec![
                Some("1"),
                Some("2"),
                Some("3"),
                Some("4"),
                Some("5"),
                Some("6")
            ],
        ]
    );
    assert_eq!(rows[0].table_cells()[0].text.as_deref(), Some("span"));
    assert_eq!(rows[0].table_cells()[0].column_span, 3);
}

#[test]
fn tbl_text_block_macro_is_retained_as_text_and_reported() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nl.\nT{\n.SM abcd\nT}\n.TE\n");
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_MACRO);
    assert_eq!(diagnostic.severity, Severity::Unsupported);
    assert_eq!(
        diagnostic.message.as_ref(),
        "ignoring macro in table: SM abcd"
    );
    let position = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (6, 2));
    let table = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(table.table_cells()[0].text.as_deref(), Some("abcd"));
}

#[test]
fn nested_tbl_opener_is_reported_without_a_synthetic_empty_row() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl | l .\na:b\n_\nc:d\n.TS\ne:f\n.TE\n");
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_MACRO);
    assert_eq!(diagnostic.severity, Severity::Unsupported);
    assert_eq!(diagnostic.message.as_ref(), "ignoring macro in table: TS");
    let position = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (9, 4));
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .count(),
        4
    );
}

#[test]
fn tbl_option_recovery_preserves_following_layout_and_reports_each_fault() {
    let report = parse(
            b".TH TBL-OPTIONS 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab decimalpoint (,x) %foo box;\nn n .\n10.0\t0.01\n.TE\n.TS\n , box,tab(:)\tdelim($$); l l .\na:b\n.TE\n",
        );
    let actual = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (
                diagnostic.code.as_str(),
                diagnostic.severity,
                diagnostic.message.as_ref(),
                (position.line, position.column),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        [
            (
                DiagnosticCode::TBL_OPTION_ARGUMENT,
                Severity::Error,
                "missing tbl option argument: tab",
                (4, 5),
            ),
            (
                DiagnosticCode::TBL_OPTION_ARGUMENT_SIZE,
                Severity::Error,
                "wrong tbl option argument size: decimalpoint want 1 have 2",
                (4, 19),
            ),
            (
                DiagnosticCode::TBL_OPTION_CHARACTER,
                Severity::Error,
                "non-alphabetic character in tbl options: %",
                (4, 23),
            ),
            (
                DiagnosticCode::TBL_UNKNOWN_OPTION,
                Severity::Error,
                "skipping unknown tbl option: foo",
                (4, 24),
            ),
            (
                DiagnosticCode::TBL_EQN_DELIMITER_OPTION,
                Severity::Unsupported,
                "eqn delim option in tbl: $$",
                (9, 21),
            ),
        ]
    );
    assert_eq!(
        report
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .count(),
        2
    );
}

#[test]
fn tbl_single_layout_discards_and_reports_extra_data_cells() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\na:b:stray\n.TE\n");
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(
        diagnostic.code.as_str(),
        DiagnosticCode::TBL_EXTRA_DATA_CELLS
    );
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(
        diagnostic.message.as_ref(),
        "ignoring extra tbl data cells: stray"
    );
    let position = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (6, 5));
    let table = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(
        table
            .table_cells()
            .iter()
            .map(|cell| cell.text.as_deref())
            .collect::<Vec<_>>(),
        [Some("a"), Some("b")]
    );
}

#[test]
fn empty_tbl_reports_at_the_opener_and_retains_closing_spacing() {
    let report = parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nl.\n.TE\n");
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code.as_str(), DiagnosticCode::TBL_NO_DATA);
    assert_eq!(diagnostic.severity, Severity::Error);
    assert_eq!(diagnostic.message.as_ref(), "tbl without any data cells");
    let position = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (3, 2));
    let spacing = report
        .document
        .preorder()
        .find(|node| node.macro_name() == Some("sp"))
        .unwrap();
    assert_eq!(spacing.kind(), NodeKind::Element);
    let position = report
        .document
        .source_position(spacing.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (5, 2));
}

#[test]
fn empty_tbl_layouts_report_their_terminator() {
    let report = parse(
        b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\n.\nfirst text\n.TE\n.TS\n|.\nsecond text\n.TE\n",
    );
    assert_eq!(report.diagnostics.len(), 2);
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.as_str(),
                    diagnostic.severity,
                    diagnostic.message.as_ref(),
                )
            })
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::TBL_EMPTY_LAYOUT,
                Severity::Error,
                "empty tbl layout",
            ),
            (
                DiagnosticCode::TBL_EMPTY_LAYOUT,
                Severity::Error,
                "empty tbl layout",
            ),
        ]
    );
    let positions = report
        .diagnostics
        .iter()
        .map(|diagnostic| {
            let position = report
                .document
                .source_position(diagnostic.primary.as_ref().unwrap())
                .unwrap();
            (position.line, position.column)
        })
        .collect::<Vec<_>>();
    assert_eq!(positions, [(4, 2), (8, 3)]);
}

#[test]
fn blank_tbl_data_line_is_an_empty_non_line_start_row() {
    let report = parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nl.\nfirst\n\nlast\n.TE\n");
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].table_cells()[0].text.as_deref(), Some("first"));
    assert!(rows[1].table_cells().is_empty());
    assert_eq!(rows[2].table_cells()[0].text.as_deref(), Some("last"));
    assert!(rows.iter().all(|row| !row.flags().line_start));
}

#[test]
fn table_data_continuations_join_before_escape_recovery() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nl.\nfirst \\\nsecond\n.TE\n");
    assert!(report.diagnostics.is_empty());
    let row = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(row.table_cells()[0].text.as_deref(), Some("first second"));
    let position = report
        .document
        .source_position(row.location().unwrap())
        .unwrap();
    assert_eq!((position.line, position.column), (6, 1));
}

#[test]
fn table_rules_remain_empty_rows_without_consuming_layout() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nl\nr.\n_\nleft\nright\n.TE\n");
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].table_cells().is_empty());
    assert_eq!(rows[1].table_cells()[0].text.as_deref(), Some("left"));
    assert_eq!(rows[1].table_cells()[0].alignment, TableAlignment::Left);
    assert_eq!(rows[2].table_cells()[0].text.as_deref(), Some("right"));
    assert_eq!(rows[2].table_cells()[0].alignment, TableAlignment::Right);
}

#[cfg(feature = "render")]
#[test]
fn standalone_tbl_vertical_layout_line_carries_into_the_next_data_layout() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nl\n|\nr.\nfirst\n_\nsecond\n.TE\n");
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 3);
    assert_eq!(
        rows[2]
            .table_terminal()
            .expect("generated tbl row has device metadata")
            .cells[0]
            .before_vertical_rules,
        1
    );
}

#[cfg(feature = "render")]
#[test]
fn tbl_z_modifier_retains_private_width_ignored_state() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nn, nz.\n12.34\n1000.0\n.TE\n");
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert!(
        rows[1]
            .table_terminal()
            .expect("generated tbl row has device metadata")
            .cells[0]
            .width_ignored
    );
}

#[cfg(feature = "render")]
#[test]
fn tbl_x_modifier_retains_private_width_expanding_state() {
    let report = parse(
        b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nlx lx l.\nx:x:value\n.TE\n",
    );
    let row = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .expect("generated tbl row");
    let terminal = row
        .table_terminal()
        .expect("generated tbl row has device metadata");
    assert!(terminal.cells[0].width_expanding);
    assert!(terminal.cells[1].width_expanding);
    assert!(!terminal.cells[2].width_expanding);
}

#[cfg(feature = "render")]
#[test]
fn tbl_horizontal_layout_cells_keep_their_consumed_data_columns() {
    let report = parse(
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\n_ _\nl l\n- -\nl r\n_ ^\nr.\ncolum one:column two\nleft:right\nnot:printed\nright:left\n.TE\n",
        );
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 6);
    let terminal = rows[4]
        .table_terminal()
        .expect("generated tbl row has device metadata");
    assert_eq!(terminal.data_columns, [0, 1]);
    assert_eq!(
        terminal.cells[0].horizontal_rule,
        TableTerminalBorder::Single
    );
}

#[test]
fn numeric_table_columns_project_as_right_aligned() {
    let report = parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nn.\n42\n.TE\n");
    let table = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(table.table_cells()[0].alignment, TableAlignment::Right);
}

#[test]
fn horizontal_only_layout_rows_materialize_before_real_data() {
    let report = parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\n_\nl.\nvalue\n.TE\n");
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert!(rows[0].table_cells().is_empty());
    assert_eq!(rows[1].table_cells()[0].text.as_deref(), Some("value"));
}

#[test]
fn short_layout_omits_empty_trailing_raw_cells() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\nleft:right:\n.TE\n");
    let table = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(table.table_cells().len(), 2);
    assert_eq!(table.table_cells()[0].text.as_deref(), Some("left"));
    assert_eq!(table.table_cells()[1].text.as_deref(), Some("right"));
}

#[test]
fn table_layout_retains_horizontal_and_vertical_spans() {
    let report = parse(
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl s r\n^ s r.\nfirst:right\nignored:continued\n.TE\n",
        );
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].table_cells().len(), 2);
    assert_eq!(rows[0].table_cells()[0].column_span, 2);
    assert_eq!(rows[0].table_cells()[0].row_span, 2);
    assert_eq!(rows[0].table_cells()[1].alignment, TableAlignment::Right);
    assert!(rows[1].table_cells()[0].vertical_continuation);
    assert_eq!(rows[1].table_cells()[0].text.as_deref(), Some("ignored"));
}

#[test]
fn table_text_blocks_stay_one_logical_cell() {
    let report = parse(
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\nT{\nfirst line\nsecond line\nT}:short\n.TE\n",
        );
    let table = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(table.table_cells().len(), 2);
    assert!(table.table_cells()[0].text_block);
    assert_eq!(
        table.table_cells()[0].text.as_deref(),
        Some("first line second line")
    );
    assert_eq!(table.table_cells()[1].text.as_deref(), Some("short"));
}

#[test]
fn adjacent_table_text_blocks_on_one_row_remain_pending() {
    let report = parse(
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl l l.\nT{\nfirst\nT}:middle:T{\nlast\nT}\n.TE\n",
        );
    let tables = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 1);
    assert_eq!(tables[0].table_cells().len(), 3);
    assert!(tables[0].table_cells()[0].text_block);
    assert_eq!(tables[0].table_cells()[0].text.as_deref(), Some("first"));
    assert_eq!(tables[0].table_cells()[1].text.as_deref(), Some("middle"));
    assert!(tables[0].table_cells()[2].text_block);
    assert_eq!(tables[0].table_cells()[2].text.as_deref(), Some("last"));
}

#[test]
fn empty_table_text_blocks_use_a_null_cell_payload() {
    let report =
        parse(b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\nl l.\ntable\tT{\nT}\n.TE\n");
    let table = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(table.table_cells()[0].text.as_deref(), Some("table"));
    assert!(!table.table_cells()[1].text_block);
    assert_eq!(table.table_cells()[1].text, None);
}

#[test]
fn table_layout_resets_keep_the_original_delimiter() {
    let report = parse(
            b".TH TABLE 1 28-Aug-2026\n.SH DESCRIPTION\n.TS\ntab(:);\nl l.\nleft:right\n.T&\nr r.\nsecond:third\n.TE\n",
        );
    let rows = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Table)
        .collect::<Vec<_>>();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].table_cells()[1].alignment, TableAlignment::Left);
    assert_eq!(rows[1].table_cells()[0].alignment, TableAlignment::Right);
    assert_eq!(rows[1].table_cells()[1].text.as_deref(), Some("third"));
}

#[test]
fn equation_normalization_retains_infix_and_visible_forms() {
    let report = parse(
            b".TH EQUATION 1 28-Aug-2026\n.SH DESCRIPTION\n.EQ\ndefine dots 'ldots'\nx hat sub 1 sup 2 + sqrt { a over 2 } + dots\n.EN\n",
        );
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .and_then(crate::NodeRef::equation)
        .unwrap();
    // Accent boxes influence rendered layout but the legacy owned-AST
    // equation projection intentionally omits them.
    assert_eq!(equation, "x _ 1 ^ 2 + sqrt(a / 2) + ...");
}

#[test]
fn paired_position_operators_consume_missing_keyword_operands() {
    let report =
        parse(b".TH EQUATION 1 28-Aug-2026\n.EQ\nx from a to to\n.EN\n.EQ\nx sub 1 sup sup\n.EN\n");
    let equations = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Equation)
        .filter_map(crate::NodeRef::equation)
        .collect::<Vec<_>>();
    assert_eq!(equations, ["x _ a ^ ", "x _ 1 ^ "]);
}

#[test]
fn malformed_fraction_uses_empty_boxes_and_reports_the_display_opener() {
    let report = parse(b".TH EQUATION 1 28-Aug-2026\n.EQ\nover over\n.EN\n");
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .and_then(crate::NodeRef::equation)
        .unwrap();
    assert_eq!(equation, " /  / ");
    assert_eq!(report.diagnostics.len(), 1);
    let diagnostic = &report.diagnostics[0];
    assert_eq!(diagnostic.code.as_str(), DiagnosticCode::EQN_MISSING_BOX);
    assert_eq!(
        diagnostic.message.as_ref(),
        "missing eqn box, using \"\": over"
    );
    let location = report
        .document
        .source_position(diagnostic.primary.as_ref().unwrap())
        .unwrap();
    assert_eq!((location.line, location.column), (2, 2));
}

#[test]
fn font_prefixes_are_transparent_inside_equation_operands() {
    let report =
        parse(b".TH EQUATION 1 28-Aug-2026\n.EQ\nbold a over bold c ; roman I sub bold I sup italic I\n.EN\n");
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .and_then(crate::NodeRef::equation)
        .unwrap();
    assert_eq!(equation, "a / c ; I _ I ^ I");
}

#[test]
fn equation_quoted_atoms_and_compound_text_boxes_keep_distinct_identity() {
    let tokens =
        super::equation_tokens("epsilon \"epsilon\" prime \"prime\" epsilon-prime sqrt a+b");
    assert!(tokens[1].quoted);
    assert!(tokens[3].quoted);
    assert_eq!(
        super::normalize_equation_tokens(&tokens),
        "\\[*e] epsilon \\[fm] prime epsilon - prime sqrt(a + b)"
    );
}

#[test]
fn inline_delimiters_split_prose_and_macro_arguments_into_equation_nodes() {
    let report = parse(
            b".TH EQN 1 28-Aug-2026\n.SH DESCRIPTION\n.EQ\ndelim $$\n.EN\nfor $i sub 1$ values\n.BR Dp \"$x sup 2$\"\n",
        );
    let equations = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Equation)
        .filter_map(crate::NodeRef::equation)
        .collect::<Vec<_>>();
    assert_eq!(equations, ["i _ 1", "x ^ 2"]);
    let visible_text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(visible_text.contains(&"for \\&"));
    assert!(visible_text.contains(&" values"));
}

#[test]
fn inline_delimiter_changes_are_ordered_across_display_equations() {
    let report = parse(
            b".TH EQN 1 28-Aug-2026\n.SH DESCRIPTION\n.EQ\ndelim []alpha\n.EN\ninline [beta]\n.EQ\ndelim offgamma\n.EN\ninline [delta]\n.EQ\ndelim onepsilon\n.EN\ninline [zeta]\n.EQ\ndelim $$\ndelim off\n.EN\ninline $eta$\n.EQ\ndelim on\n.EN\ninline $theta$\n",
        );
    let equations = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Equation)
        .collect::<Vec<_>>();
    assert_eq!(equations.len(), 8);
    assert_eq!(
        equations
            .iter()
            .filter_map(|node| node.equation())
            .collect::<Vec<_>>(),
        ["\\[*a]", "\\[*b]", "\\[*g]", "\\[*e]", "\\[*z]", "\\[*h]"]
    );
    let visible_text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(visible_text.contains(&"inline [delta]"));
    assert!(visible_text.contains(&"inline $eta$"));
    assert_eq!(
        visible_text.iter().filter(|text| **text == "\\&").count(),
        3
    );
}

#[test]
fn inline_equation_delimiters_remain_opaque_inside_tbl_cells() {
    let report = parse(
            b".TH TABLE-EQN 1 28-Aug-2026\n.EQ\ndelim %%\n.EN\n.TS\nl l.\n%0%\tfor values in %[ 0 , ~pi over 2 ]%\n.TE\n",
        );
    let table = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Table)
        .unwrap();
    assert_eq!(table.table_cells()[0].text.as_deref(), Some("%0%"));
    assert_eq!(
        table.table_cells()[1].text.as_deref(),
        Some("for values in %[ 0 , ~pi over 2 ]%")
    );
}

#[test]
fn equation_normalization_matches_legacy_layout_only_and_symbol_boxes() {
    let report = parse(
            b".TH EQN-COMPAT 1 28-Aug-2026\n.EQ\nalpha hat sub 1 sup 2\nleft ceiling sqrt { a over 2 } right ceiling\nroman x size 12 y gsize 9 z\npile { one above two }\n.EN\n",
        );
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .and_then(crate::NodeRef::equation)
        .unwrap();
    assert_eq!(
        equation,
        "\\[*a] _ 1 ^ 2 \\[lc]sqrt(a / 2)\\[rc] x y z one two"
    );
}

#[test]
fn equation_definitions_support_delimiters_and_undef_without_visible_leaks() {
    let report = parse(
        b".TH EQN-DEFINE 1 28-Aug-2026\n.EQ\ndefine item /alpha/\nitem\nundef item\nitem\n.EN\n",
    );
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .and_then(crate::NodeRef::equation)
        .unwrap();
    assert_eq!(equation, "\\[*a] item");
}

#[test]
fn detects_trailing_empty_eqn_definition_requests() {
    assert_eq!(
        trailing_empty_request("define bruch 'over' 1 define"),
        "define"
    );
    assert_eq!(trailing_empty_request("define bruch"), "define bruch");
    assert_eq!(trailing_empty_request("alpha undef"), "undef");
    assert_eq!(trailing_empty_request("tdefine bruch"), "tdefine");
    assert!(trailing_empty_request("define item /alpha/").is_empty());
}

#[test]
fn recursive_equation_definition_keeps_an_empty_equation_and_recovery() {
    let report =
        parse(b".TH EQN-RECURSION 1 28-Aug-2026\n.EQ\ndefine key 'prefix key suffix' key\n.EN\n");
    assert_eq!(
        report
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.as_ref())
            .collect::<Vec<_>>(),
        ["input stack limit exceeded, infinite loop?"]
    );
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .expect("recursive display retains a finite equation node");
    assert_eq!(equation.equation(), None);
}

#[test]
fn equation_definition_budget_returns_the_expanded_prefix() {
    let mut config = ParserConfig::default();
    config.limits.max_equation_tokens = 2;
    let report = Parser::new(config)
        .parse(Source::new(
            &SourceName::new("equation-definition-limit.1").unwrap(),
            b".TH EQN-LIMIT 1 28-Aug-2026\n.EQ\ndefine item /a b c/\nitem\n.EN\n",
        ))
        .unwrap();
    assert!(report.statistics.truncated);
    assert_eq!(
        report.diagnostics[0].code.as_str(),
        DiagnosticCode::LIMIT_EQUATION_TOKENS
    );
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .and_then(crate::NodeRef::equation);
    assert_eq!(equation, Some("a b"));
}

#[test]
fn default_equation_limit_returns_a_finite_legacy_compatible_prefix() {
    let mut source = String::from(".TH DEEP-EQN 1 28-Aug-2026\n.EQ\n");
    for _ in 0..5_000 {
        source.push_str("sqrt { ");
    }
    source.push('x');
    for _ in 0..5_000 {
        source.push_str(" }");
    }
    source.push_str("\n.EN\n");

    let report = parse(source.as_bytes());
    assert!(report.statistics.truncated);
    assert!(report.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_str() == DiagnosticCode::LEGACY_EQUATION_TREE_DEPTH_LIMIT
    }));
    let equation = report
        .document
        .preorder()
        .find(|node| node.kind() == NodeKind::Equation)
        .and_then(crate::NodeRef::equation)
        .unwrap();
    assert!(equation.len() < 4_000, "equation prefix was not bounded");
}

#[test]
fn equation_delim_on_reuses_only_a_delimiter_configured_in_that_display() {
    let report = parse(
            b".TH EQN-DELIM 1 28-Aug-2026\n.EQ\ndelim %%\ndelim off\ndelim on\ntdefine ignored 'not-visible'\n.EN\nvalue %x sup 2% and $raw$\n",
        );
    let equations = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Equation)
        .filter_map(crate::NodeRef::equation)
        .collect::<Vec<_>>();
    assert_eq!(equations, ["x ^ 2"]);
    let text = report
        .document
        .preorder()
        .filter(|node| node.kind() == NodeKind::Text)
        .filter_map(crate::NodeRef::text)
        .collect::<Vec<_>>();
    assert!(text.contains(&"value \\&"));
    assert!(text.contains(&" and $raw$"));
}

#[test]
fn table_and_equation_budgets_keep_finite_prefixes_with_typed_codes() {
    let mut table_config = ParserConfig::default();
    table_config.limits.max_table_rows = 1;
    let table = Parser::new(table_config)
        .parse(Source::new(
            &SourceName::new("table-limits.1").unwrap(),
            b".TH TABLE 1 28-Aug-2026\n.TS\nl.\none\ntwo\n.TE\n",
        ))
        .unwrap();
    assert!(table.statistics.truncated);
    assert_eq!(
        table.diagnostics[0].code.as_str(),
        DiagnosticCode::LIMIT_TABLE_ROWS
    );
    assert_eq!(
        table
            .document
            .preorder()
            .filter(|node| node.kind() == NodeKind::Table)
            .count(),
        1
    );

    let mut equation_config = ParserConfig::default();
    equation_config.limits.max_equation_tokens = 3;
    let equation = Parser::new(equation_config)
        .parse(Source::new(
            &SourceName::new("equation-limits.1").unwrap(),
            b".TH EQN 1 28-Aug-2026\n.EQ\na + b + c\n.EN\n",
        ))
        .unwrap();
    assert!(equation.statistics.truncated);
    assert_eq!(
        equation.diagnostics[0].code.as_str(),
        DiagnosticCode::LIMIT_EQUATION_TOKENS
    );
    assert_eq!(
        equation
            .document
            .preorder()
            .find(|node| node.kind() == NodeKind::Equation)
            .and_then(crate::NodeRef::equation),
        Some("a + b")
    );
}

#[test]
fn unclosed_tbl_and_eqn_ranges_report_recoverable_typed_findings() {
    let report =
        parse(b".TH RECOVERY 1 28-Aug-2026\n.TS\nl.\nT{\nunterminated cell\n.TE\n.EQ\nx sup 2\n");
    let codes = report
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        codes,
        [
            DiagnosticCode::EQN_UNCLOSED_DISPLAY,
            DiagnosticCode::TBL_UNCLOSED_TEXT_BLOCK,
        ]
    );
    assert!(!report.statistics.truncated);
}
