//! Repeated native BODY rows compose their origin before actual reader cells.
use mant_ir::{Block, ResolvedContent};
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, style::Modifier};

#[path = "../../mant-engine/tests/roff_lowering/name_body_handoffs/origins/fixture.rs"]
mod fixture;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

use reader_actions::{click, copied_selections, opened_targets, positions, select_span};
use terminal_cells::{assert_row, paint};

fn assert_reader_rows(content: &ResolvedContent, case: &fixture::Case, round: usize) {
    for width in [40, 78] {
        let rendered = DocumentView::new(content).render(width);
        let buffer = paint(&rendered, width);
        let start = rendered.anchor_row("synopsis").unwrap() + 1;
        let expected = if round == 0 {
            case.native_rows
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        } else {
            case.portable_rows()
        };
        assert_eq!(
            rendered.anchor_row("next"),
            Some(start + expected.len() + 1),
            "{}/{width}/read{round}: no extra physical row",
            case.id
        );
        for (offset, row) in expected.iter().enumerate() {
            assert_row(
                &buffer,
                start + offset,
                3,
                row,
                &format!("{}/{width}/read{round}", case.id),
            );
            if round == 0 {
                for (column, font) in case.native_styles[offset].iter().enumerate() {
                    let cell = &buffer[(
                        (column + 3).try_into().unwrap(),
                        (start + offset).try_into().unwrap(),
                    )];
                    for (flag, modifier) in [(1, Modifier::BOLD), (2, Modifier::ITALIC)] {
                        assert_eq!(
                            cell.modifier.contains(modifier),
                            font & flag != 0,
                            "{}/{width}: native font {offset}/{column}",
                            case.id
                        );
                    }
                }
            }
        }
        let matches = rendered.search("leading");
        assert_eq!(matches.len(), 1);
        let found = &matches[0];
        assert_eq!(found.row, start + 1);
        let authored_column = 3 + if round == 0 { 7 } else { 1 };
        assert_eq!(found.start_column, authored_column);
        assert_eq!(found.end_column, authored_column + "leading".len());
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 48)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let locations = positions(terminal.backend().buffer(), "leading");
        assert_eq!(locations.len(), 1);
        let (column, row) = locations[0];
        click(&mut app, column, row);
        assert_eq!(opened_targets(&mut app), Vec::<String>::new());
        select_span(&mut app, column - 1, row, column + 6, None);
        assert_eq!(
            copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("original authored row selection")
                };
                text
            }),
            [" leading"],
            "{}/{width}/read{round}: authored SP copies; layout padding does not",
            case.id
        );
        assert_eq!(rendered.search("probeint"), []);
        if case.id == "two-leading-rows" {
            let second = rendered.search("second");
            assert_eq!(second.len(), 1);
            assert_eq!(second[0].row, start + 2);
            assert_eq!(second[0].start_column, authored_column);
        }
    }
}

#[test]
fn native_name_rows_keep_origins_fonts_search_and_author_blanks_after_two_reads() {
    // The two sources bind pristine ASCII/UTF-8 at 40/78 plus HTML/tree/lint.
    // Nm BODY assigns six columns once (mdoc_term.c::termp_nm_pre); each
    // leading TEXT starts a real row before its source blank is written.
    // Ordinary Markdown omits reading geometry but preserves that blank.
    for case in fixture::cases() {
        let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
        let mut content = json_roundtrip::json_string_roundtrip(&original);
        let section = &content.document.as_ref().unwrap().sections[1];
        let [Block::Paragraph { layout, .. }] = section.blocks.as_slice() else {
            panic!("{}: original complete name owner", case.id)
        };
        assert_eq!(layout.continuation_indent_columns, 6, "{}", case.id);
        for round in 0..=2 {
            assert_reader_rows(&content, &case, round);
            if round < 2 {
                let markdown = mant_codec::encode::render_markdown(&content);
                content = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(&markdown, None).unwrap(),
                );
            }
        }
    }
}
