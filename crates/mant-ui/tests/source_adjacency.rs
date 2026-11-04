//! Original Fo siblings survive JSON, final cells and real reader actions.

use mant_ir::ResolvedContent;
use mant_protocol::QueryBundle;
use mant_ui::{App, CopyRequest, DocumentView, RenderedDocument};
use ratatui::{
    Terminal, backend::TestBackend, buffer::Buffer, layout::Rect, style::Modifier, widgets::Widget,
};
use serde_json::Value;

#[path = "support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const BODY_ORIGIN: usize = 3;

fn fixture() -> Value {
    let fixture: Value = serde_json::from_str(include_str!("source_adjacency/cases.json")).unwrap();
    assert_eq!(
        fixture["oracle_sha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 4);
    fixture
}

fn number(value: &Value) -> usize {
    value.as_u64().unwrap().try_into().unwrap()
}

fn roundtrip(content: &ResolvedContent) -> ResolvedContent {
    let original = QueryBundle::from(content);
    let encoded = serde_json::to_string(&original).unwrap();
    let decoded: QueryBundle = serde_json::from_str(&encoded).unwrap();
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        serde_json::to_value(&decoded).unwrap()
    );
    decoded.into()
}

fn paint(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    rendered.text.clone().render(area, &mut buffer);
    buffer
}

fn row_origins(case: &Value, content: &ResolvedContent, native_geometry: bool) -> Vec<usize> {
    let section = &content.document.as_ref().unwrap().sections[1];
    let [mant_ir::Block::Paragraph { inline_layout, .. }] = section.blocks.as_slice() else {
        panic!(
            "{}: one complete function owner, native geometry {native_geometry}: {:?}",
            case["id"], section.blocks
        )
    };
    case["native_indents"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(row, indent)| {
            let indent = if native_geometry { number(indent) } else { 0 };
            // Native Fo's hanging field gives the second row +4 columns.
            // Markdown exports content/hard rows and omits this row geometry.
            assert_eq!(
                inline_layout.row_layout(row).indent_columns,
                i32::try_from(indent).unwrap()
            );
            BODY_ORIGIN + indent
        })
        .collect()
}

fn assert_cells(
    case: &Value,
    content: &ResolvedContent,
    preserve_anchor: bool,
    native_geometry: bool,
) {
    let origins = row_origins(case, content, native_geometry);
    let view = DocumentView::new(content);
    let initial = view.render(78).text;
    for width in [40, 78, 40, 78] {
        let rendered = view.render(width);
        let buffer = paint(&rendered, width);
        let start = rendered.anchor_row("synopsis").unwrap() + 1;
        let rows = case["rows"].as_array().unwrap();
        assert_eq!(rendered.anchor_row("next"), Some(start + rows.len() + 1));
        for (offset, row) in rows.iter().enumerate() {
            let row = row.as_str().unwrap();
            for column in 0..usize::from(width) {
                let expected = column
                    .checked_sub(origins[offset])
                    .and_then(|index| row.get(index..index + 1))
                    .unwrap_or(" ");
                let cell = &buffer[(
                    column.try_into().unwrap(),
                    (start + offset).try_into().unwrap(),
                )];
                assert_eq!(
                    cell.symbol(),
                    expected,
                    "{}/{width}: row {offset}, column {column}",
                    case["id"]
                );
                for (key, modifier) in [("bold", Modifier::BOLD), ("italic", Modifier::ITALIC)] {
                    let expected_style = case["font_runs"].as_array().unwrap().iter().any(|run| {
                        number(&run["row"]) == offset
                            && run[key] == true
                            && (origins[offset] + number(&run["start"])
                                ..origins[offset]
                                    + number(&run["start"])
                                    + run["text"].as_str().unwrap().len())
                                .contains(&column)
                    });
                    assert_eq!(
                        cell.modifier.contains(modifier),
                        expected_style,
                        "{}/{width}: {key} at {offset}/{column}",
                        case["id"]
                    );
                }
            }
        }
        for run in case["font_runs"].as_array().unwrap() {
            let word = run["text"].as_str().unwrap();
            let matches: Vec<_> = rendered
                .search(word)
                .into_iter()
                .filter(|hit| (start..start + rows.len()).contains(&hit.row))
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "{}: one exact occurrence of {word}",
                case["id"]
            );
            assert_eq!(
                (
                    matches[0].row,
                    matches[0].start_column,
                    matches[0].end_column
                ),
                (
                    start + number(&run["row"]),
                    origins[number(&run["row"])] + number(&run["start"]),
                    origins[number(&run["row"])] + number(&run["start"]) + word.len()
                )
            );
        }
        if preserve_anchor && !case["link"].is_null() {
            assert_eq!(
                rendered.anchor_row(case["link"]["anchor"].as_str().unwrap()),
                Some(start)
            );
        }
    }
    assert_eq!(view.render(78).text, initial);
}

fn only_position(buffer: &Buffer, word: &str) -> (u16, u16) {
    let positions = positions(buffer, word);
    assert_eq!(positions.len(), 1, "one fully visible {word}");
    positions[0]
}

fn assert_reader_actions(case: &Value, content: &ResolvedContent) {
    for width in [40, 78] {
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 48)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let word = if case["link"].is_null() {
            "arg"
        } else {
            "LINK"
        };
        let (column, row) = only_position(terminal.backend().buffer(), word);
        if !case["link"].is_null() {
            // Check each accepted label cell plus both adjacent boundaries.
            // The generated colon/space/URI are outside the Lk label owner.
            for offset in 0..word.len() {
                click(&mut app, column + u16::try_from(offset).unwrap(), row);
                assert_eq!(
                    opened_targets(&mut app),
                    [case["link"]["uri"].as_str().unwrap()]
                );
            }
            for boundary in [column - 1, column + u16::try_from(word.len()).unwrap()] {
                click(&mut app, boundary, row);
                assert_eq!(opened_targets(&mut app), Vec::<String>::new());
            }
        }
        select_span(
            &mut app,
            column,
            row,
            column + u16::try_from(word.len()).unwrap() - 1,
            None,
        );
        assert_eq!(
            copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("actual pointer selection")
                };
                text
            }),
            [word]
        );
    }
}

#[test]
fn complete_source_adjacency_reaches_buffer_cells_and_two_markdown_reads() {
    // Exact sources first ran pristine ASCII/UTF-8(78)/HTML/tree/lint. CVS
    // print_mdoc_nodelist preserves n->prev/next through Fa's special handler;
    // synopsis_pre(Ft) flushes before int, Pf raw-next and Fl logical-next
    // suppress only the valid same-line boundary. Fonts follow the native
    // bold Fo/Fl and underlined Fa/Ft/Lk spans (UI maps underline to italic).
    for case in fixture()["cases"].as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let original = roundtrip(&mant_loader::load_roff_bytes(source.as_bytes()).unwrap());
        let json = serde_json::to_value(QueryBundle::from(&original)).unwrap();
        assert_cells(case, &original, true, true);
        assert_reader_actions(case, &original);
        // Addressable export inserts raw HTML destinations; the native reader
        // intentionally retains unsupported HTML as literal text. Its distinct
        // CommonMark/reader contract is checked by the codec finalization tests.
        // These two supported body projections keep the same native payload.
        for preserve_semantics in [false, true] {
            let mut current = original.clone();
            for _ in 0..2 {
                let markdown = mant_codec::encode::render_markdown_with_options(
                    &current,
                    mant_codec::encode::MarkdownOptions {
                        preserve_semantics,
                        ..Default::default()
                    },
                );
                current = roundtrip(&mant_loader::load_markdown_text(&markdown, None).unwrap());
                assert_cells(case, &current, false, false);
                assert_reader_actions(case, &current);
            }
        }
        assert_eq!(
            serde_json::to_value(QueryBundle::from(&original)).unwrap(),
            json
        );
    }
}
