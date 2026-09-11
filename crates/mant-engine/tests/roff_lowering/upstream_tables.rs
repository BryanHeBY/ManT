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
    let document = man(".TS\nl s l\nl l l.\nTOPSPAN\tRIGHT\nLEFT\tMIDDLE\tEND\n.TE");
    let query = ResolvedContent {
        label: "probe".into(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    for rendered in [
        mant_render::render_query_text(&query),
        mant_codec::encode::render_markdown(&query),
    ] {
        assert!(rendered.contains("TOPSPAN |  | RIGHT"), "{rendered}");
        assert!(rendered.contains("LEFT | MIDDLE | END"), "{rendered}");
    }
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
        assert_eq!(second.len(), 2);
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
