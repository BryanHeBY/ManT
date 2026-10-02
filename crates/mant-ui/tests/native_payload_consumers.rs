//! Real tbl/eqn payloads retain native cells and explicit reader actions.
use mant_ir::ResolvedContent;
use mant_protocol::{DocumentOpenTarget, QueryBundle};
use mant_ui::{App, CopyRequest, DocumentView, ReaderServices, RenderedDocument};
use ratatui::{
    Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier, widgets::Widget,
};
use serde_json::Value;

#[path = "support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

fn fixtures() -> Value {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mant-engine/tests/native_payload_consumers/cases.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(fixture["count"], 9);
    assert_eq!(fixture["expectations_from_product"], false);
    fixture
}

fn text<'a>(case: &'a Value, key: &str) -> &'a str {
    case[key].as_str().unwrap()
}

fn round_trip(case: &Value) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(text(case, "source").as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    let decoded: QueryBundle = serde_json::from_str(&json).unwrap();
    decoded.into()
}

fn body_hit(rendered: &RenderedDocument, word: &str) -> mant_ui::RenderedSearchMatch {
    let start = rendered.anchor_row("description").unwrap();
    let end = rendered.anchor_row("next").unwrap();
    let hits: Vec<_> = rendered
        .search(word)
        .into_iter()
        .filter(|hit| hit.row > start && hit.row < end)
        .collect();
    let [hit] = hits.as_slice() else {
        panic!("one body hit {word}: {:?}", rendered.text)
    };
    hit.clone()
}

fn cells(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
    let mut cells = Buffer::empty(area);
    rendered.text.clone().render(area, &mut cells);
    cells
}

fn assert_word_cells(rendered: &RenderedDocument, buffer: &Buffer, word: &str, width: u16) {
    let hit = body_hit(rendered, word);
    let row = u16::try_from(hit.row).unwrap();
    let first = u16::try_from(hit.start_column).unwrap();
    assert_eq!(
        buffer[(first, row)].symbol(),
        word.chars().next().unwrap().to_string()
    );
    // At 20 cells the mathematical and table payloads can wrap a token;
    // the public match still owns its first actual cell. All tokens fit on
    // their selected row at the other widths. Count scalars/cells, not bytes.
    if width >= 40 {
        assert_eq!(
            hit.end_column - hit.start_column,
            mant_ir::geometry::text_width(word)
        );
        for (index, character) in word.chars().enumerate() {
            assert_eq!(
                buffer[(first + u16::try_from(index).unwrap(), row)].symbol(),
                character.to_string()
            );
        }
    }
}

fn assert_font_cells(rendered: &RenderedDocument, buffer: &Buffer) {
    let hit = body_hit(rendered, "ABC");
    let row = u16::try_from(hit.row).unwrap();
    let column = u16::try_from(hit.start_column).unwrap();
    for (offset, bold) in [(0, true), (1, false), (2, true)] {
        assert_eq!(
            buffer[(column + offset, row)]
                .modifier
                .contains(Modifier::BOLD),
            bold
        );
    }
    let italic = body_hit(rendered, "STYLEITALIC");
    assert!(
        buffer[(
            u16::try_from(italic.start_column).unwrap(),
            u16::try_from(italic.row).unwrap()
        )]
            .modifier
            .contains(Modifier::ITALIC)
    );
}

fn assert_equation_rows(case: &Value, rendered: &RenderedDocument, width: u16) {
    if text(case, "kind") != "eqn" || width < 40 {
        return;
    }
    let start = rendered.anchor_row("description").unwrap();
    let tail = body_hit(rendered, "TailWord");
    let rows: Vec<_> = rendered.text.lines[start + 1..tail.row]
        .iter()
        .map(ToString::to_string)
        .filter(|row| {
            let row = row.trim();
            !row.is_empty() && row != text(case, "owner_name")
        })
        .collect();
    // Only auto margins/soft wrapping vary; the entire accepted mathematical
    // payload must still appear, including each authored fence side.
    let visible = rows.iter().map(|row| row.trim()).collect::<String>();
    assert_eq!(visible, text(case, "equation"), "{} at {width}", case["id"]);
}

fn assert_view(case: &Value, query: &ResolvedContent) {
    let view = DocumentView::new(query);
    let initial = view.render(20).text;
    for width in [20, 40, 78, 120, 20] {
        let rendered = view.render(width);
        let buffer = cells(&rendered, width);
        let mut previous = None;
        for word in case["words"].as_array().unwrap() {
            let word = word.as_str().unwrap();
            assert_word_cells(&rendered, &buffer, word, width);
            let hit = body_hit(&rendered, word);
            let position = (hit.row, hit.start_column);
            if let Some(previous) = previous {
                assert!(
                    position > previous,
                    "{}: source payload order at {width}",
                    case["id"]
                );
            }
            previous = Some(position);
        }
        if text(case, "id") == "table_fonts" {
            assert_font_cells(&rendered, &buffer);
        }
        assert_equation_rows(case, &rendered, width);
        assert!(!rendered.text.to_string().contains('\u{fffd}'));
    }
    assert_eq!(view.render(20).text, initial, "resize cache");
}

fn body_position(buffer: &Buffer, word: &str) -> (u16, u16) {
    let start = positions(buffer, "DESCRIPTION")
        .into_iter()
        .max_by_key(|(column, _)| *column)
        .unwrap()
        .1;
    let end = positions(buffer, "NEXT")
        .into_iter()
        .max_by_key(|(column, _)| *column)
        .unwrap()
        .1;
    let matches: Vec<_> = positions(buffer, word)
        .into_iter()
        .filter(|(_, row)| *row > start && *row < end)
        .collect();
    let [position] = matches.as_slice() else {
        panic!("one actual body position for {word}")
    };
    *position
}

fn assert_copy(query: &ResolvedContent, word: &str) {
    for width in [80, 120] {
        let mut app = App::new(query);
        let mut terminal = Terminal::new(TestBackend::new(width, 64)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = body_position(terminal.backend().buffer(), word);
        let cells = u16::try_from(mant_ir::geometry::text_width(word)).unwrap();
        // A one-cell Unicode operand needs an intermediate drag to enter
        // selection mode; the final glyph range remains exactly one cell.
        select_span(
            &mut app,
            column,
            row,
            column + cells - 1,
            Some(column + cells),
        );
        let copied = copied_selections(&mut app, |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("actual cell selection")
            };
            text
        });
        assert_eq!(copied, [word], "copy at {width}");
    }
}

fn assert_link_actions(query: &ResolvedContent) {
    for width in [80, 120] {
        let mut app = App::new(query);
        let mut terminal = Terminal::new(TestBackend::new(width, 64)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        for label in ["CELLONE", "CELLTWO"] {
            let (column, row) = body_position(terminal.backend().buffer(), label);
            assert!(
                terminal.backend().buffer()[(column, row)]
                    .modifier
                    .contains(Modifier::UNDERLINED)
            );
            click(&mut app, column, row);
            assert_eq!(opened_targets(&mut app), ["https://example.org"]);
        }
        let (column, row) = body_position(terminal.backend().buffer(), "printf(3)");
        click(&mut app, column, row);
        let mut opened = Vec::new();
        let mut open = |target: &DocumentOpenTarget| {
            opened.push(target.clone());
            Err("test host deliberately does not load another manual".to_owned())
        };
        app.service_pending(&mut ReaderServices {
            open_document: Some(&mut open),
            ..Default::default()
        });
        assert_eq!(
            opened,
            [DocumentOpenTarget::Address {
                // A retained section qualifies the manual identity before
                // the host request; section-less names use Manual instead.
                address: mant_protocol::DocumentAddress::Manual {
                    name: "printf".into(),
                    manual_section: "3".into(),
                },
            }]
        );
        assert_eq!(opened_targets(&mut app).len(), 0);
    }
}

#[test]
fn table_source_cells_keep_fonts_spans_copy_and_typed_activation() {
    // The exact committed sources ran five pristine profiles first.
    // tbl_term.c::tbl_word selects the cell font before its in-word escapes;
    // T&/span ownership comes from tbl_data.c. Lk/Xr have recorded UNSUPP
    // lint=4 and use the manual's admitted source-cell recovery contract;
    // each original occurrence remains separately clickable after JSON.
    let fixture = fixtures();
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| text(case, "kind") == "tbl")
    {
        let query = round_trip(case);
        assert_view(case, &query);
        assert_copy(&query, text(case, "lookup"));
        if text(case, "id") == "table_recovered_links" {
            assert_link_actions(&query);
        }
    }
}

#[test]
fn equation_source_values_keep_unicode_fences_cells_and_copy_after_resize() {
    // eqn.c retains the authored sides separately; eqn_term.c::eqn_box
    // establishes group/operator scope. The selected readable projection
    // differs in operator spacing and matrix layout, but no operand/fence
    // may disappear in JSON, responsive Buffer output or an explicit copy.
    let fixture = fixtures();
    for case in fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| text(case, "kind") != "tbl")
    {
        let query = round_trip(case);
        assert_view(case, &query);
        assert_copy(&query, text(case, "lookup"));
    }
}
