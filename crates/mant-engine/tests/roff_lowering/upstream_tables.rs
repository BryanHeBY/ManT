//! Table regressions assert native ownership and logical layout, not snapshots alone.
use super::*;

fn man(body: &str) -> mant_ir::Document {
    parse_manual_bytes(
        std::path::Path::new("table.1"),
        format!(".TH PROBE 1\n.SH DESCRIPTION\n{body}\n").as_bytes(),
    )
    .unwrap()
}

#[test]
fn decoded_empty_table_cells_do_not_recover_control_spelling_as_content() {
    // Native tbl retains these strings; tbl_term passes them to term_word,
    // where IGNORE/font/motion controls successfully emit no visible glyph.
    for payload in [r"\&", r"\fB", r"\h'0'"] {
        let document = man(&format!(".TS\nl l l.\nLEFT\t{payload}\tRIGHT\n.TE"));
        let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("table: {document:?}")
        };
        assert_eq!(rows[0].cells.len(), 3);
        let [Block::Paragraph { children, .. }] = rows[0].cells[1].blocks.as_slice() else {
            panic!("decoded empty cell")
        };
        assert!(children.is_empty(), "{payload}: {children:?}");
        assert!(!document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.unexpanded-table-cell")
        }));
    }
}

#[test]
fn escaped_literal_backslash_cells_are_not_decoded_a_second_time() {
    for payload in [r"\e&", r"\\&"] {
        let document = man(&format!(".TS\nl l l.\nLEFT\t{payload}\tRIGHT\n.TE"));
        let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("table: {document:?}")
        };
        let [Block::Paragraph { children, .. }] = rows[0].cells[1].blocks.as_slice() else {
            panic!("literal cell")
        };
        assert_eq!(inline_text(children), r"\&", "{payload}");
    }
}

#[test]
fn horizontal_spans_keep_following_cells_in_the_same_logical_column() {
    // Exact pinned -Ttree/-Tascii/-Tutf8 replay: tbl_data.c skips the span
    // layout cell, leaving RIGHT at column 2; tbl_term.c places every row in
    // the same table-wide column frame. The reader's pipe glyphs occupy cells
    // of their own; portable Markdown keeps its compact fence projection.
    let document = man(".TS\nl s l\nl l l.\nTOPSPAN\tRIGHT\nLEFT\tMIDDLE\tEND\n.TE");
    let [
        Block::Table {
            rows, column_count, ..
        },
    ] = document.sections[0].blocks.as_slice()
    else {
        panic!("one table owns both rows");
    };
    assert_eq!(*column_count, Some(3));
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].cells.len(), 2);
    assert_eq!(rows[0].cells[0].column_span, 2);
    assert_eq!(mant_ir::TableGrid::new(rows).rows[0][1].column, 2);
    let query = ResolvedContent {
        label: "probe".into(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let rendered = mant_render::render_query_text(&query);
    let rows = rendered
        .lines()
        .filter(|line| line.starts_with("TOPSPAN") || line.starts_with("LEFT"))
        .collect::<Vec<_>>();
    assert_eq!(rows, ["TOPSPAN |        | RIGHT", "LEFT    | MIDDLE | END"]);
    assert_eq!(rows[0].find("RIGHT"), rows[1].find("END"));

    let markdown = mant_codec::encode::render_markdown(&query);
    let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let [Block::Preformatted { children, .. }] = decoded.document.as_ref().unwrap().sections[0]
        .blocks
        .as_slice()
    else {
        panic!("portable table fence changed: {markdown}");
    };
    assert_eq!(
        inline_text(children),
        "TOPSPAN |  | RIGHT\nLEFT | MIDDLE | END"
    );
}

#[test]
fn adjacent_tables_remain_distinct_but_layout_restarts_do_not_split() {
    for leading_rule in ["", "_\n"] {
        let document = man(&format!(
            ".TS\nl.\nFIRST\n.T&\nr.\nSECOND\n.TE\n.TS\nl.\n{leading_rule}THIRD\nFOURTH\n.TE"
        ));
        let [
            Block::Table { rows: first, .. },
            Block::Table { rows: second, .. },
        ] = document.sections[0].blocks.as_slice()
        else {
            panic!("separate tables: {document:?}")
        };
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2 + usize::from(!leading_rule.is_empty()));
        if !leading_rule.is_empty() {
            assert_eq!(second[0].kind, mant_ir::TableRowKind::HorizontalRule);
        }
        assert_eq!(
            first[1].cells[0].alignment,
            Some(mant_ir::TableAlignment::Right)
        );
    }
}

#[test]
fn table_rule_cells_never_resurrect_suppressed_source_payload() {
    for (layout, payload) in [
        ("_", "HIDDEN"),
        ("=", "HIDDEN"),
        ("l", "_"),
        ("l", "="),
        ("l", r"\_"),
        ("l", r"\="),
    ] {
        let document = man(&format!(".TS\nl {layout} l.\nLEFT\t{payload}\tRIGHT\n.TE"));
        let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
            panic!("table: {document:?}")
        };
        assert_eq!(rows[0].cells.len(), 3);
        assert!(
            rows[0].cells[1].blocks.is_empty(),
            "{layout}: {payload}: {rows:?}"
        );
        for (cell, expected) in [(&rows[0].cells[0], "LEFT"), (&rows[0].cells[2], "RIGHT")] {
            let [Block::Paragraph { children, .. }] = cell.blocks.as_slice() else {
                panic!("paragraph")
            };
            assert_eq!(inline_text(children), expected);
        }
    }
    let document = man(".TS\nl.\n\\&_\n.TE");
    let [Block::Table { rows, .. }] = document.sections[0].blocks.as_slice() else {
        panic!("table")
    };
    let [Block::Paragraph { children, .. }] = rows[0].cells[0].blocks.as_slice() else {
        panic!("paragraph")
    };
    assert_eq!(inline_text(children), "_");
}

#[test]
fn layout_only_rule_rows_retain_each_column_strength() {
    let document = man(
        ".TS\n_\nl.\nSINGLE\n.TE\n.TS\n=\nl.\nDOUBLE\n.TE\n.TS\n_ =\nl l.\nLEFT\tRIGHT\n.TE\n.TS\n_.\nIGNORED\n.TE\n.TS\n=.\nIGNORED\n.TE\n.TS\n_ =.\nLEFT\tRIGHT\n.TE",
    );
    let tables = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Table { rows, .. } => Some(rows),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tables.len(), 6);
    assert_eq!(
        tables[0][0].kind,
        mant_ir::TableRowKind::LayoutRule {
            cells: vec![mant_ir::TableRuleCellKind::Horizontal],
        }
    );
    assert_eq!(
        tables[1][0].kind,
        mant_ir::TableRowKind::LayoutRule {
            cells: vec![mant_ir::TableRuleCellKind::DoubleHorizontal],
        }
    );
    assert_eq!(
        tables[2][0].kind,
        mant_ir::TableRowKind::LayoutRule {
            cells: vec![
                mant_ir::TableRuleCellKind::Horizontal,
                mant_ir::TableRuleCellKind::DoubleHorizontal,
            ],
        }
    );
    assert_eq!(tables[3][0].kind, tables[0][0].kind);
    assert_eq!(tables[4][0].kind, tables[1][0].kind);
    assert_eq!(tables[5][0].kind, tables[2][0].kind);
    assert!(tables[3..].iter().all(|rows| rows[0].cells.is_empty()));
    let query = ResolvedContent {
        label: "probe".into(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let rendered = mant_render::render_query_text(&query);
    assert!(rendered.contains("---"), "{rendered}");
    assert!(rendered.contains("==="), "{rendered}");
    assert!(rendered.contains("--- | ==="), "{rendered}");
    assert!(!rendered.contains("IGNORED"), "{rendered}");
}
