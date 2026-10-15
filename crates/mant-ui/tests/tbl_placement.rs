//! Native table metadata reaches both viewport readers after actual JSON.
use mant_ir::{Block, ResolvedContent, TableAlignment};
use mant_ui::DocumentView;

#[test]
fn captions_cells_and_whole_table_frames_keep_independent_alignment() {
    // Every exact source below ran through pinned pristine ASCII, UTF-8,
    // HTML and lint before these assertions were written. tbl_term.c computes
    // one table-wide offset; roff_term.c::pre_ce aligns the separate caption.
    // Reader gaps/borders are deliberately different from native geometry.
    for options in ["center;\n", "centre;\n", ""] {
        let source = format!(
            ".TH PROBE 1 \"October 15, 2026\"\n.SH DESCRIPTION\n.ce 1\nCAPTION\n.TS\n{options}c r l.\nAAA\tBBB\tCCC\nLONGLEFT\tLONGMIDDLE\tLONGRIGHT\n_\n.TE\n"
        );
        let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
        let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let content: ResolvedContent = restored.into();
        assert_eq!(content, original);
        let Block::Table {
            alignment,
            column_count,
            rows,
            ..
        } = &content.document.as_ref().unwrap().sections[0].blocks[1]
        else {
            panic!("table after caption")
        };
        assert_eq!(*column_count, Some(3));
        assert_eq!(
            *alignment,
            (!options.is_empty()).then_some(TableAlignment::Center)
        );
        assert_eq!(
            rows[0]
                .cells
                .iter()
                .map(|cell| cell.alignment)
                .collect::<Vec<_>>(),
            [
                Some(TableAlignment::Center),
                Some(TableAlignment::Right),
                Some(TableAlignment::Left)
            ]
        );
        let view = DocumentView::new(&content);
        for width in [40_u16, 80, 120] {
            let rendered = view.render(width);
            let origin = if options.is_empty() {
                3
            } else {
                3 + (usize::from(width) - 3 - 31) / 2
            };
            for (word, offset) in [
                ("AAA", 2),
                ("BBB", 17),
                ("CCC", 22),
                ("LONGLEFT", 0),
                ("LONGMIDDLE", 10),
                ("LONGRIGHT", 22),
            ] {
                assert_eq!(
                    rendered.search(word)[0].start_column,
                    origin + offset,
                    "{options:?}, {width}, {word}: {:?}",
                    rendered.text
                );
            }
            assert_eq!(
                rendered.search("CAPTION")[0].start_column,
                3 + (usize::from(width) - 3 - 7) / 2
            );
            assert_eq!(
                rendered.text.lines[rendered.search("LONGLEFT")[0].row + 1].to_string(),
                format!("{}{}", " ".repeat(origin), "─".repeat(31))
            );
        }
        let narrow = view.render(20);
        for (first, second) in [("AAA", "BBB"), ("BBB", "CCC")] {
            assert_eq!(
                narrow.search(second)[0].row,
                narrow.search(first)[0].row + 1
            );
        }
        let cli = mant_render::render_query_text_at_width(&content, 80, |_, text| text.into());
        let origin = if options.is_empty() { 0 } else { (80 - 33) / 2 };
        for (word, offset) in [
            ("AAA", 2),
            ("BBB", 18),
            ("CCC", 24),
            ("LONGLEFT", 0),
            ("LONGMIDDLE", 11),
            ("LONGRIGHT", 24),
        ] {
            assert_eq!(
                cli.lines()
                    .find(|row| row.contains(word))
                    .unwrap()
                    .find(word),
                Some(origin + offset),
                "{options:?}, {word}: {cli}"
            );
        }
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let readback = mant_render::render_query_text(&readback);
        assert!(readback.contains("AAA | BBB | CCC"));
        assert!(readback.contains("LONGLEFT | LONGMIDDLE | LONGRIGHT"));
    }
}
