//! Source scanning cannot replace finalized tbl payload or steal a sibling.
use super::{Block, Inline, Parser, inline_text, lower_mandoc_document, parse_manual_bytes};

#[test]
fn uncertain_text_block_sentinels_retain_native_words_and_safe_neighbors() {
    // Every exact fixture ran all five pristine profiles first. In CVS
    // tbl_data.c::tbl_cdata a close requires EOL or the executed tab byte.
    // T}word consumes the prefix but appends word; leading space keeps T}.
    // Our bounded optional scanner is not a table-option interpreter. Its
    // incomplete candidate must decline against the complete native stream.
    for (source, first) in [
        (
            include_str!("table_source_payloads/fixtures/word.1"),
            "FIRST word MIDDLE",
        ),
        (
            include_str!("table_source_payloads/fixtures/space.1"),
            "FIRST  T} MIDDLE",
        ),
        (
            include_str!("table_source_payloads/fixtures/custom.1"),
            "FIRST",
        ),
        (
            include_str!("table_source_payloads/fixtures/nospace.1"),
            "FIRST",
        ),
    ] {
        let path = std::path::Path::new("sentinels.1");
        let native = Parser::default()
            .parse_bytes(path, source.as_bytes())
            .unwrap();
        let owned = lower_mandoc_document(path, &native);
        let lowered = parse_manual_bytes(path, source.as_bytes()).unwrap();
        let json = serde_json::to_string(&lowered).unwrap();
        let decoded: mant_ir::Document = serde_json::from_str(&json).unwrap();
        for document in [&owned, &lowered, &decoded] {
            let [Block::Table { rows, .. }, Block::Paragraph { children, .. }] =
                document.sections[0].blocks.as_slice()
            else {
                panic!("one table and independent following text");
            };
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].cells.len(), 2);
            let cells: Vec<_> = rows[0]
                .cells
                .iter()
                .map(|cell| {
                    let [Block::Paragraph { children, .. }] = cell.blocks.as_slice() else {
                        panic!("complete native cell");
                    };
                    inline_text(children)
                })
                .collect();
            assert_eq!(cells, [first, "LAST"]);
            assert_eq!(inline_text(children), "AFTER");
            assert!(mant_ir::content_complete(&document.diagnostics));
        }
        let Block::Table { rows, .. } = &lowered.sections[0].blocks[0] else {
            unreachable!()
        };
        let Block::Paragraph { children, .. } = &rows[0].cells[1].blocks[0] else {
            unreachable!()
        };
        assert!(matches!(children.as_slice(), [Inline::Strong { .. }]));
    }
}
