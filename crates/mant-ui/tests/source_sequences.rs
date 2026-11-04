//! Recovered cells retain their independently executed source rows in real cells.

use mant_ir::ResolvedContent;
use mant_ui::DocumentView;
use ratatui::style::Modifier;
use serde::Deserialize;

#[path = "../../mant-engine/tests/support/case_failures.rs"]
mod case_failures;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

use case_failures::verify_cases;
use json_roundtrip::json_string_roundtrip;
use terminal_cells::{assert_row, paint};

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
    source_entries: Vec<SourceEntry>,
}

#[derive(Deserialize)]
struct SourceEntry {
    id: String,
    section: String,
    source: String,
    native: Expected,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    section: String,
    source: String,
    contract: String,
    code_text: Option<String>,
    whole: Expected,
    independent: Expected,
}

#[derive(Clone, Deserialize)]
struct Expected {
    rows: Vec<String>,
    styles: Vec<Vec<u8>>,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../mant-engine/tests/roff_lowering/source_sequences/cases.json"
    ))
    .unwrap();
    let mut selected: Vec<_> = fixture
        .cases
        .into_iter()
        .filter(|case| match case.contract.as_str() {
            "reference-head" => matches!(
                case.id.as_str(),
                "rs-synopsis-bare-zero" | "rs-on-authors-1" | "rs-off-authors-1"
            ),
            "reference-root" => case.id.ends_with("authors-1"),
            _ => true,
        })
        .collect();
    selected.extend(fixture.source_entries.into_iter().map(|entry| Case {
        id: entry.id,
        section: entry.section,
        source: entry.source,
        contract: "source-root".into(),
        code_text: None,
        independent: entry.native.clone(),
        whole: entry.native,
    }));
    selected
}

fn assert_cells(content: &ResolvedContent, width: u16, case: &Case, round: u8) {
    let native_styles = round == 0;
    let rendered = DocumentView::new(content).render(width);
    let buffer = paint(&rendered, width);
    let start = rendered.anchor_row(&case.section.to_lowercase()).unwrap() + 1;
    let oracle = if case.contract == "recovered" {
        &case.independent
    } else {
        &case.whole
    };
    let portable_rows;
    let expected = if case.contract == "reference-head" && !native_styles {
        // Portable Markdown retains content/hard rows, not native tag offsets.
        portable_rows = oracle
            .rows
            .iter()
            .map(|row| row.trim_start_matches(' ').to_owned())
            .collect::<Vec<_>>();
        &portable_rows
    } else {
        &oracle.rows
    };
    assert_eq!(
        rendered.anchor_row("next"),
        Some(start + expected.len() + 1),
        "{}/{width}/round{round}: exact native hard rows",
        case.id
    );
    for (offset, row) in expected.iter().enumerate() {
        assert_row(
            &buffer,
            start + offset,
            3,
            row,
            &format!("{}/{width}/round{round}", case.id),
        );
        if native_styles {
            for (column, font) in oracle.styles[offset].iter().enumerate() {
                let cell = &buffer[(
                    (3 + column).try_into().unwrap(),
                    (start + offset).try_into().unwrap(),
                )];
                for (flag, modifier) in [(1, Modifier::BOLD), (2, Modifier::ITALIC)] {
                    // In intentionally compacts its executed spelling into
                    // renderer-neutral Code (inline::append_include). Check
                    // that established semantic policy separately; all other
                    // native Bold/Underline cells retain the oracle evidence.
                    let code_column = case.code_text.as_ref().is_some_and(|code| {
                        row.find(code)
                            .is_some_and(|start| (start..start + code.len()).contains(&column))
                    });
                    assert_eq!(
                        cell.modifier.contains(modifier),
                        !code_column && font & flag != 0,
                        "{}/{width}: native/Code font at {offset}/{column}",
                        case.id
                    );
                }
            }
        }
    }
}

#[test]
fn complete_recovered_sequences_reach_json_and_actual_buffers_40_and_78() {
    // Every whole table and independent fragment ran all five pristine
    // profiles first. Whole-table UNSUPP is retained in the shared fixture;
    // the accepted enhancement follows the independently executed fragment.
    verify_cases(
        cases(),
        |case| &case.id,
        |case| {
            let source = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
            let mut content = json_string_roundtrip(&source);
            for round in 0..=2 {
                for width in [40, 78] {
                    assert_cells(&content, width, case, round);
                }
                if round < 2 {
                    let markdown = mant_codec::encode::render_markdown(&content);
                    content = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    if case.contract == "reference-head" {
                        // Portable Markdown maps a native definition owner
                        // to one bullet item. Peel only that documented outer
                        // wrapper before comparing the item's physical rows.
                        let section = content
                            .document
                            .as_mut()
                            .unwrap()
                            .sections
                            .iter_mut()
                            .find(|section| section.heading.plain_text() == case.section)
                            .unwrap();
                        if let [mant_ir::Block::List { items, .. }] = section.blocks.as_mut_slice()
                        {
                            assert_eq!(items.len(), 1);
                            section.blocks = std::mem::take(&mut items[0].blocks);
                        }
                    }
                }
            }
        },
    );
}

#[path = "support/reader_actions.rs"]
mod reader_actions;

#[test]
fn recovered_external_labels_keep_actual_click_search_and_copy_ranges() {
    use mant_ui::{App, CopyRequest};
    use ratatui::{Terminal, backend::TestBackend};
    use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

    let case = cases()
        .into_iter()
        .find(|case| case.id == "cell-external-identity")
        .unwrap();
    let original = mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&original);
    for width in [40, 78] {
        let rendered = DocumentView::new(&content).render(width);
        let hits = rendered.search("LINK");
        assert_eq!(hits.len(), 1);
        let mut app = App::new(&content);
        let mut terminal = Terminal::new(TestBackend::new(width, 48)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let found = positions(terminal.backend().buffer(), "LINK");
        assert_eq!(found.len(), 1);
        let (column, row) = found[0];
        for offset in 0..4 {
            click(&mut app, column + offset, row);
            assert_eq!(opened_targets(&mut app), ["https://example.test"]);
        }
        for boundary in [column - 1, column + 4] {
            click(&mut app, boundary, row);
            assert_eq!(opened_targets(&mut app), Vec::<String>::new());
        }
        select_span(&mut app, column, row, column + 3, None);
        assert_eq!(
            copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("pointer selection remains a body selection")
                };
                text
            }),
            ["LINK"]
        );
    }
}
