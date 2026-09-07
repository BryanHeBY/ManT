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
