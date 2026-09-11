//! Codec-internal lowering contracts; no query or rendering dependencies.
use super::*;

#[test]
fn bounds_distinct_tbl_equation_normalization_work() {
    let mut source =
        String::from(".TH TABLE-EQN-BUDGET 3\n.SH DESCRIPTION\n.EQ\ndelim %%\n.EN\n.TS\nl.\n");
    for index in 0..=MAX_INLINE_EQUATION_NORMALIZATIONS {
        writeln!(source, "%x{index}%").expect("write fixture row");
    }
    source.push_str(".TE\n");

    let document = parse_manual_bytes(
        std::path::Path::new("table-inline-equation-budget.3"),
        source.as_bytes(),
    )
    .expect("lower a bounded number of table equations");

    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("manual.inline-equation-budget")
        })
    );
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected equation table");
    };
    assert_eq!(rows.len(), MAX_INLINE_EQUATION_NORMALIZATIONS + 1);
}

#[test]
fn tbl_text_block_prefers_native_parse_time_string_expansion() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-expanded-string.1"),
        b".TH TBL-EXPANDED-STRING 1\n.ds Aq \\(aq\n.SH DESCRIPTION\n.TS\nl.\nT{\nThere\\*(Aqs\nT}\n.TE\n",
    )
    .expect("lower tbl source with a parse-time string expansion");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    assert_eq!(
        inline_text(match &rows[0].cells[0].blocks[0] {
            Block::Paragraph { children, .. } => children,
            other => panic!("expected table paragraph, got {other:?}"),
        }),
        "There's"
    );
}

#[test]
fn tbl_text_blocks_follow_native_request_dispatch_without_replaying_high_level_macros() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-request-dispatch.1"),
        b".Dd September 12, 2026\n.Dt TBLPROBE 1\n.Os\n.Sh DESCRIPTION\n.TS\nl.\nT{\n.BR A / B .\nT}\nT{\n.Sm off\n.Em WORD\nT}\nT{\n.Fl Fl help\nT}\nT{\n.sp 1\nSPACED\nT}\n.TE\n",
    )
    .expect("lower CVS mandoc tbl dispatch witness");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let cells = rows
        .iter()
        .map(|row| match row.cells[0].blocks.as_slice() {
            [Block::Paragraph { children, .. }] => inline_text(children),
            blocks => panic!("expected one table cell paragraph: {blocks:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(cells, ["A / B .", "off WORD", "Fl help", "SPACED"]);
}

#[test]
fn tbl_source_recovery_never_promotes_roff_comments_to_cells_or_text_blocks() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-inline-comment.3"),
        b".TH TBL-INLINE-COMMENT 3\n.SH DESCRIPTION\n.TS\nl l.\nleft\tright\\\" ignored ordinary-cell payload\nT{ \\\" real text-block marker with comment\n.BR linked (3) \\\" ignored text-block payload\nT}\tplain\n.TE\n",
    )
    .expect("lower table comments through the native roff lexical boundary");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let table_text = rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.blocks)
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        table_text.contains("left right linked (3) plain"),
        "{table_text}"
    );
    assert!(!table_text.contains("ignored"), "{table_text}");
    assert!(!table_text.contains("payload"), "{table_text}");
}

#[test]
fn tbl_source_recovery_uses_a_document_level_ec_escape_change() {
    let document = parse_manual_bytes(
        std::path::Path::new("tbl-alternate-escape-comment.3"),
        b".TH TBL-ALTERNATE-ESCAPE-COMMENT 3\n.ec @\n.SH DESCRIPTION\n.TS\nl l.\nleft\tright\t@\" ignored third source cell\n.TE\n.ec\n",
    )
    .expect("lower a table after a native .ec escape change");
    let Block::Table { rows, .. } = &document.sections[0].blocks[0] else {
        panic!("expected one lowered table");
    };
    let table_text = rows
        .iter()
        .flat_map(|row| &row.cells)
        .flat_map(|cell| &cell.blocks)
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(inline_text(children)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(table_text.contains("left right"), "{table_text}");
    assert!(!table_text.contains("ignored"), "{table_text}");
}
