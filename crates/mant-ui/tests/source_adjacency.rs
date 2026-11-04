//! Original Fo siblings survive JSON, final cells and real reader actions.

use mant_ir::ResolvedContent;
use mant_protocol::QueryBundle;
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Modifier};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

#[path = "support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

#[path = "../../mant-engine/tests/support/case_failures.rs"]
mod case_failures;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;
use case_failures::verify_cases;
use json_roundtrip::json_string_roundtrip as roundtrip;
use terminal_cells::{assert_row, paint};

const BODY_ORIGIN: usize = 3;

struct Case {
    id: String,
    source: String,
    rows: Vec<String>,
    native_indents: Vec<usize>,
    font_runs: Vec<FontRun>,
    link: Option<Link>,
}

struct FontRun {
    row: usize,
    start: usize,
    text: String,
    bold: bool,
    italic: bool,
}

struct Link {
    uri: String,
    anchor: String,
}

impl Case {
    fn from_value(value: &Value) -> Self {
        Self {
            id: value["id"].as_str().unwrap().to_owned(),
            source: value["source"].as_str().unwrap().to_owned(),
            rows: value["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| row.as_str().unwrap().to_owned())
                .collect(),
            native_indents: value["native_indents"]
                .as_array()
                .unwrap()
                .iter()
                .map(number)
                .collect(),
            font_runs: value["font_runs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|run| FontRun {
                    row: number(&run["row"]),
                    start: number(&run["start"]),
                    text: run["text"].as_str().unwrap().to_owned(),
                    bold: run["bold"].as_bool().unwrap(),
                    italic: run["italic"].as_bool().unwrap(),
                })
                .collect(),
            link: (!value["link"].is_null()).then(|| Link {
                uri: value["link"]["uri"].as_str().unwrap().to_owned(),
                anchor: value["link"]["anchor"].as_str().unwrap().to_owned(),
            }),
        }
    }
}

fn cases() -> Vec<Case> {
    let fixture: Value = serde_json::from_str(include_str!("source_adjacency/cases.json")).unwrap();
    assert_eq!(
        fixture["oracle_sha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    let cases: Vec<_> = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(Case::from_value)
        .collect();
    assert_eq!(cases.len(), 4);
    cases
}

fn number(value: &Value) -> usize {
    value.as_u64().unwrap().try_into().unwrap()
}

fn row_origins(case: &Case, content: &ResolvedContent, native_geometry: bool) -> Vec<usize> {
    let section = &content.document.as_ref().unwrap().sections[1];
    let [mant_ir::Block::Paragraph { inline_layout, .. }] = section.blocks.as_slice() else {
        panic!(
            "{}: one complete function owner, native geometry {native_geometry}: {:?}",
            case.id, section.blocks
        )
    };
    case.native_indents
        .iter()
        .enumerate()
        .map(|(row, indent)| {
            let indent = if native_geometry { *indent } else { 0 };
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
    case: &Case,
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
        let rows = &case.rows;
        assert_eq!(rendered.anchor_row("next"), Some(start + rows.len() + 1));
        for (offset, row) in rows.iter().enumerate() {
            assert_row(
                &buffer,
                start + offset,
                origins[offset],
                row,
                &format!("{}/{width}", case.id),
            );
            for column in 0..usize::from(width) {
                let cell = &buffer[(
                    column.try_into().unwrap(),
                    (start + offset).try_into().unwrap(),
                )];
                for (key, modifier) in [("bold", Modifier::BOLD), ("italic", Modifier::ITALIC)] {
                    let expected_style = case.font_runs.iter().any(|run| {
                        run.row == offset
                            && (if key == "bold" { run.bold } else { run.italic })
                            && (origins[offset] + run.start
                                ..origins[offset] + run.start + run.text.width())
                                .contains(&column)
                    });
                    assert_eq!(
                        cell.modifier.contains(modifier),
                        expected_style,
                        "{}/{width}: {key} at {offset}/{column}",
                        case.id
                    );
                }
            }
        }
        for run in &case.font_runs {
            let word = run.text.as_str();
            let matches: Vec<_> = rendered
                .search(word)
                .into_iter()
                .filter(|hit| (start..start + rows.len()).contains(&hit.row))
                .collect();
            assert_eq!(
                matches.len(),
                1,
                "{}: one exact occurrence of {word}",
                case.id
            );
            assert_eq!(
                (
                    matches[0].row,
                    matches[0].start_column,
                    matches[0].end_column
                ),
                (
                    start + run.row,
                    origins[run.row] + run.start,
                    origins[run.row] + run.start + word.width()
                )
            );
        }
        if preserve_anchor && let Some(link) = &case.link {
            assert_eq!(rendered.anchor_row(&link.anchor), Some(start));
        }
    }
    assert_eq!(view.render(78).text, initial);
}

fn only_position(buffer: &Buffer, word: &str) -> (u16, u16) {
    let positions = positions(buffer, word);
    assert_eq!(positions.len(), 1, "one fully visible {word}");
    positions[0]
}

fn assert_reader_actions(case: &Case, content: &ResolvedContent) {
    for width in [40, 78] {
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 48)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let word = if case.link.is_some() { "LINK" } else { "arg" };
        let (column, row) = only_position(terminal.backend().buffer(), word);
        if let Some(link) = &case.link {
            // Check each accepted label cell plus both adjacent boundaries.
            // The generated colon/space/URI are outside the Lk label owner.
            for offset in 0..word.width() {
                click(&mut app, column + u16::try_from(offset).unwrap(), row);
                assert_eq!(opened_targets(&mut app), [link.uri.as_str()]);
            }
            for boundary in [column - 1, column + u16::try_from(word.width()).unwrap()] {
                click(&mut app, boundary, row);
                assert_eq!(opened_targets(&mut app), Vec::<String>::new());
            }
        }
        select_span(
            &mut app,
            column,
            row,
            column + u16::try_from(word.width()).unwrap() - 1,
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
    verify_cases(
        cases(),
        |case| &case.id,
        |case| {
            let source = case.source.as_str();
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
        },
    );
}
