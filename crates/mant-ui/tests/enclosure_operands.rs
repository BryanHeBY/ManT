//! Native operand evidence reaches actual cells, links and visual copies.
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend, style::Modifier};
#[path = "../../mant-engine/tests/support/case_failures.rs"]
mod case_failures;
#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "../../mant-engine/tests/support/enclosure_operands.rs"]
mod recorded;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

fn assert_cells(content: &ResolvedContent, width: u16, case: &recorded::Case, round: u8) {
    let original = round == 0;
    let rendered = DocumentView::new(content).render(width);
    let buffer = terminal_cells::paint(&rendered, width);
    let rows: Vec<String> = (0..buffer.area.height)
        .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();
    let start = rows
        .iter()
        .position(|row| row.trim_matches(' ') == "ORIGIN")
        .unwrap();
    let end = rows
        .iter()
        .position(|row| row.trim_matches(' ') == "TAIL")
        .unwrap();
    let origin = rows[start].find("ORIGIN").unwrap();
    let mut expected = if original {
        case.rows.clone()
    } else {
        recorded::portable_rows(case)
    };
    if !original && matches!(case.context.as_str(), "tag-head" | "tag-body") {
        // The actual readback retains its list wrapper for both export
        // rounds. A bullet and its two-column continuation origin are
        // portable list structure, not native/authored enclosure cells.
        let last = expected.len() - 2;
        for (row, text) in expected.iter_mut().enumerate().take(last).skip(1) {
            if row == 1 {
                text.insert_str(0, "• ");
            } else if !text.is_empty() {
                text.insert_str(0, "  ");
            }
        }
    }
    assert_eq!(
        end - start + 1,
        expected.len(),
        "{}/{width}/round{round}: exact physical rows: {rows:?}",
        case.id
    );
    for (offset, row) in expected.iter().enumerate() {
        terminal_cells::assert_row(
            &buffer,
            start + offset,
            origin,
            row,
            &format!("{}/{width}/round{round}", case.id),
        );
        if original {
            for (index, bits) in case.styles[offset].iter().enumerate() {
                let cell = &buffer[(
                    (origin + index).try_into().unwrap(),
                    (start + offset).try_into().unwrap(),
                )];
                assert_eq!(
                    cell.modifier.contains(Modifier::BOLD),
                    bits & 1 != 0,
                    "{}: original bold cell {offset}/{index}",
                    case.id
                );
                assert_eq!(
                    cell.modifier.contains(Modifier::ITALIC),
                    bits & 2 != 0,
                    "{}: original italic cell {offset}/{index}",
                    case.id
                );
            }
        }
    }
}
#[test]
fn native_operand_matrix_keeps_exact_cells_after_json_and_real_markdown() {
    // Exact five-profile pristine runs precede these assertions. Eo pre/post
    // writes NBRZW, while Es passes raw normalized operands to term_word;
    // neither an empty word nor NBRZW can be inferred from rendered length.
    case_failures::verify_cases(
        recorded::cases(),
        |case| &case.id,
        |case| {
            let mut content = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
            );
            for round in 0..=2 {
                for width in [40, 78] {
                    assert_cells(&content, width, case, round);
                }
                if round < 2 {
                    content = json_roundtrip::json_string_roundtrip(
                        &mant_loader::load_markdown_text(
                            &mant_codec::encode::render_markdown(&content),
                            None,
                        )
                        .unwrap(),
                    );
                }
            }
        },
    );
}
#[test]
fn invisible_operand_spacing_does_not_expand_link_or_copy_ranges() {
    for case in recorded::cases().into_iter().filter(|case| {
        case.cohort == "cursor-core" && case.context == "plain" && case.source.contains(".Lk")
    }) {
        let mut content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case.source.as_bytes()).unwrap(),
        );
        for round in 0..=2 {
            for width in [40, 78] {
                let mut app = App::new(&content);
                let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let found = positions(terminal.backend().buffer(), "SECOND");
                assert_eq!(found.len(), 1, "{}: accepted whole label", case.id);
                let (column, row) = found[0];
                for offset in 0..6 {
                    click(&mut app, column + offset, row);
                    assert_eq!(
                        opened_targets(&mut app),
                        ["https://example.test"],
                        "{}: click cell",
                        case.id
                    );
                }
                for outside in [column - 1, column + 6] {
                    click(&mut app, outside, row);
                    assert!(
                        opened_targets(&mut app).is_empty(),
                        "{}: generated incoming SP is outside Link",
                        case.id
                    );
                }
                select_span(&mut app, column, row, column + 5, None);
                assert_eq!(
                    copied_selections(&mut app, |request| {
                        let CopyRequest::Selection { text } = request else {
                            panic!("selected cells")
                        };
                        text
                    }),
                    ["SECOND"],
                    "{}: visual copy",
                    case.id
                );
            }
            if round < 2 {
                content = json_roundtrip::json_string_roundtrip(
                    &mant_loader::load_markdown_text(
                        &mant_codec::encode::render_markdown(&content),
                        None,
                    )
                    .unwrap(),
                );
            }
        }
    }
}
