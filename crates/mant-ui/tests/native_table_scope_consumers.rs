//! Actual reader cells consume table source scopes and accepted owner mappings.
use mant_ui::{App, CopyRequest, DocumentView};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::Value;
use unicode_width::UnicodeWidthStr;

#[path = "../../mant-engine/tests/support/json_roundtrip.rs"]
mod json_roundtrip;
#[path = "support/reader_actions.rs"]
mod reader_actions;
#[path = "../../mant-engine/tests/support/table_scope_witness.rs"]
mod table_scope_witness;
#[path = "support/terminal_cells.rs"]
mod terminal_cells;

fn word_positions(buffer: &ratatui::buffer::Buffer, label: &str) -> Vec<(u16, u16)> {
    let mut result = Vec::new();
    for row in 0..buffer.area.height {
        let text = (0..buffer.area.width)
            .map(|column| buffer[(column, row)].symbol())
            .collect::<String>();
        for (start, _) in text.match_indices(label) {
            let end = start + label.len();
            let part = |character: char| character.is_alphanumeric() || character == '_';
            if !text[..start].chars().next_back().is_some_and(part)
                && !text[end..].chars().next().is_some_and(part)
            {
                result.push((u16::try_from(text[..start].width()).unwrap(), row));
            }
        }
    }
    result
}

#[test]
fn root_tag_column_and_literal_tables_keep_exact_data_and_rule_rows_in_real_cells() {
    // Reuse exact sources whose five pristine profiles preceded the sealed
    // cell witnesses. Both source dispatchers execute the table-entry newline
    // once, before tbl_term.c handles the complete span sequence.
    let fixture: Value = serde_json::from_str(include_str!(
        "../../mant-engine/tests/roff_lowering/execution_scopes/cases.json"
    ))
    .unwrap();
    let mut checked = 0;
    for case in fixture["cases"].as_array().unwrap() {
        if case["family"] != "M2"
            || !["root", "tag", "column", "literal"]
                .iter()
                .any(|context| case["axes"]["context"] == *context)
        {
            continue;
        }
        checked += 1;
        let content = json_roundtrip::json_string_roundtrip(
            &mant_loader::load_roff_bytes(case["source"].as_str().unwrap().as_bytes()).unwrap(),
        );
        for width in [40, 78] {
            let rendered = DocumentView::new(&content).render(width);
            let buffer = terminal_cells::paint(&rendered, width);
            for row in table_scope_witness::data_rows(case) {
                let positions = row
                    .labels
                    .iter()
                    .map(|label| {
                        let (line, column) =
                            table_scope_witness::cell(case, row.physical_row, label, |word| {
                                word_positions(&buffer, word)
                                    .into_iter()
                                    .map(|(column, row)| (usize::from(row), usize::from(column)))
                                    .collect()
                            });
                        assert_eq!(
                            rendered
                                .search(label)
                                .iter()
                                .filter(|hit| hit.row == line
                                    && hit.start_column == column
                                    && hit.end_column == column + label.width())
                                .count(),
                            1,
                            "original cell has one exact search range, irrespective of headings",
                        );
                        (line, column)
                    })
                    .collect::<Vec<_>>();
                assert!(
                    positions
                        .windows(2)
                        .all(|pair| pair[0].0 == pair[1].0 && pair[0].1 < pair[1].1)
                );
            }
        }
    }
    assert_eq!(checked, 48);
}

fn assert_activation_and_copy(
    content: &mant_ir::ResolvedContent,
    label: &str,
    target: Option<&str>,
) {
    for width in [40, 78] {
        let mut app = App::new(content);
        let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let positions = reader_actions::positions(terminal.backend().buffer(), label);
        assert_eq!(positions.len(), 1, "{width}: {label}");
        let (column, row) = positions[0];
        let columns = u16::try_from(label.width()).unwrap();
        for offset in 0..columns {
            reader_actions::click(&mut app, column + offset, row);
            assert_eq!(
                reader_actions::opened_targets(&mut app),
                target.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                "{width}: {label} link cell"
            );
        }
        for outside in [column.saturating_sub(1), column + columns] {
            reader_actions::click(&mut app, outside, row);
            assert!(
                reader_actions::opened_targets(&mut app).is_empty(),
                "{width}: {label} outside edge"
            );
        }
        reader_actions::select_span(
            &mut app,
            column,
            row,
            column + columns - 1,
            Some(column + columns),
        );
        assert_eq!(
            reader_actions::copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("visual selection")
                };
                text
            }),
            [label],
            "{width}: exact source copy"
        );
    }
}

#[test]
fn reference_collision_refusal_keeps_only_original_accepted_click_cells_after_two_reads() {
    let source = "# Probe\n\n## OPTIONS\n\n- `--safe`: [DECLINED][r\x1bf] [ACCEPTED][r f]\n\n[r f]: https://same.test \"same\"\n[r\x1bf]: https://same.test \"same\"\n";
    let mut current = json_roundtrip::json_string_roundtrip(
        &mant_loader::load_markdown_text(source, None).unwrap(),
    );
    for round in 0..=2 {
        assert_activation_and_copy(&current, "DECLINED", None);
        assert_activation_and_copy(&current, "ACCEPTED", Some("https://same.test"));
        if round < 2 {
            let markdown = mant_codec::encode::render_markdown(&current);
            current = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
        }
    }
}

#[test]
fn code_row_projection_keeps_original_link_anchor_copy_and_second_owner_accessible() {
    let mut content = mant_loader::load_markdown_text("# Owners\n", None).unwrap();
    content.document.as_mut().unwrap().blocks = vec![serde_json::from_value(serde_json::json!({
        "type":"table", "rows":[{"cells":[{"blocks":[
            {"type":"list","kind":{"kind":"plain"},"compact":true,"items":[{"entry":{"id":"first","kind":{"kind":"term"},"case":"sensitive","names":[],"forms":[]},"blocks":[{"type":"preformatted","children":[{"type":"anchor","id":"row-owner"},{"type":"link","target":{"kind":"external","uri":"https://row.test"},"children":[{"type":"text","value":"FIRST"}]},{"type":"text","value":"\r字\nZ"}],"inlineLayout":{"rowHints":[{"row":1,"indentColumns":3}]}}]}]},
            {"type":"list","kind":{"kind":"plain"},"compact":true,"items":[{"entry":{"id":"second","kind":{"kind":"term"},"case":"sensitive","names":[],"forms":[]},"blocks":[{"type":"paragraph","children":[{"type":"text","value":"SECOND"}]}]}]}
        ]}]}]
    })).unwrap()];
    let mut current = json_roundtrip::json_string_roundtrip(&content);
    assert_eq!(
        mant_ir::validate_document(current.document.as_ref().unwrap()),
        []
    );
    assert_activation_and_copy(&current, "FIRST", Some("https://row.test"));
    assert_activation_and_copy(&current, "SECOND", None);
    for round in 0..=2 {
        if round > 0 {
            // Portable table fences retain accepted code payload, without active
            // link syntax. Do not invent clickable padding or retain stale spans.
            assert_activation_and_copy(&current, "FIRST", None);
            assert_activation_and_copy(&current, "SECOND", None);
        }
        for width in [40, 78] {
            let rendered = DocumentView::new(&current).render(width);
            let buffer = terminal_cells::paint(&rendered, width);
            let first = word_positions(&buffer, "FIRST");
            let middle = word_positions(&buffer, "字");
            let last = word_positions(&buffer, "Z");
            let second = word_positions(&buffer, "SECOND");
            assert_eq!(
                (first.len(), middle.len(), last.len(), second.len()),
                (1, 1, 1, 1)
            );
            // Original IR keeps CR as one safe replacement cell in the
            // reader. Markdown Code export intentionally canonicalizes it
            // into a new accepted row; original row1 still belongs to Z.
            assert_eq!(
                middle[0].1,
                first[0].1 + u16::from(round > 0),
                "round{round}/{width}"
            );
            assert_eq!(last[0].1, middle[0].1 + 1, "round{round}/{width}");
            assert_eq!(second[0].1, last[0].1 + 1, "round{round}/{width}");
            assert_eq!(last[0].0, first[0].0 + 3, "row1 hint belongs to Z");
        }
        if round < 2 {
            let markdown = mant_codec::encode::render_markdown(&current);
            current = json_roundtrip::json_string_roundtrip(
                &mant_loader::load_markdown_text(&markdown, None).unwrap(),
            );
        }
    }
}

#[test]
fn original_scmdctl_motor_table_has_one_continuous_frame_and_six_source_rules() {
    use mant_ir::{Block, ResolvedContent, TableRowKind, visit::Visit};
    #[derive(Default)]
    struct Motor<'a>(Vec<&'a Block>);
    impl<'a> Visit<'a> for Motor<'a> {
        fn visit_block(&mut self, block: &'a Block) {
            if matches!(block, Block::Table { source: Some(source), .. } if source.line > 163 && source.line < 191)
            {
                self.0.push(block);
            }
            mant_ir::visit::walk_block(self, block);
        }
    }
    // Complete original NetBSD source ran five pristine profiles first. Its
    // TS163/TE191 contains ten data spans and six explicit rule spans; no
    // independent paragraph/newline belongs between those native spans.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/roff/real/field-boundaries/netbsd-scmdctl.1");
    let content = json_roundtrip::json_string_roundtrip(&ResolvedContent {
        label: "scmdctl".into(),
        address: None,
        tldr: None,
        document: Some(mant_loader::parse_manual_source(&path).unwrap()),
    });
    let mut motor = Motor::default();
    motor.visit_document(content.document.as_ref().unwrap());
    assert_eq!(motor.0.len(), 1);
    let Block::Table { rows, .. } = motor.0[0] else {
        unreachable!()
    };
    assert_eq!(rows.len(), 16);
    assert_eq!(
        rows.iter()
            .filter(|row| row.kind == TableRowKind::Data)
            .count(),
        10
    );
    // Parse and validate the whole page before selecting its original Table
    // for an unambiguous consumer viewport, as the existing real-table suite
    // does. Do not reparse an excerpt or reconstruct any source cell content.
    let mut selected = content.clone();
    let document = selected.document.as_mut().unwrap();
    document.blocks = vec![motor.0[0].clone()];
    document.sections.clear();
    document.heading = None;
    for width in [40, 78] {
        let rendered = DocumentView::new(&selected).render(width);
        let buffer = terminal_cells::paint(&rendered, width);
        assert_motor_frame(&buffer, &rendered, width);
    }
}

fn assert_motor_frame(
    buffer: &ratatui::buffer::Buffer,
    rendered: &mant_ui::RenderedDocument,
    width: u16,
) {
    let (left, right, top, bottom) = motor_frame_bounds(buffer, width);
    let mut rules = Vec::new();
    for row in top + 1..bottom {
        let edge = buffer[(left, row)].symbol();
        if ["├", "╞", "╟"].contains(&edge) {
            assert!(["┤", "╡", "╢"].contains(&buffer[(right, row)].symbol()));
            assert!(
                (left + 1..right)
                    .all(|column| ["─", "═", "┼", "╪", "╬"]
                        .contains(&buffer[(column, row)].symbol())),
                "{width}: complete source rule across all columns"
            );
            rules.push(row);
        } else {
            assert_eq!(edge, "│", "{width}: continuous left frame");
            assert_eq!(
                buffer[(right, row)].symbol(),
                "│",
                "{width}: continuous right frame"
            );
            assert!(
                (left + 1..right).any(|column| buffer[(column, row)]
                    .symbol()
                    .chars()
                    .any(char::is_alphanumeric)),
                "{width}: no invented blank data row"
            );
        }
    }
    assert_eq!(rules.len(), 6, "{width}: exact six source rules");
    let labels = [
        "Subcommand",
        "get",
        "set",
        "invert",
        "bridge",
        "enable",
        "disable",
    ];
    let positions = labels.map(|label| {
        let found = word_positions(buffer, label);
        assert_eq!(found.len(), 1, "{width}: original motor {label}");
        found[0]
    });
    for pair in positions.windows(2) {
        assert!(pair[0].1 < pair[1].1, "{width}: original motor group order");
        assert_eq!(
            rules
                .iter()
                .filter(|row| pair[0].1 < **row && **row < pair[1].1)
                .count(),
            1,
            "{width}: one native rule between original data groups"
        );
    }
    for glyph in ["┌", "┐", "└", "┘", "│", "─", "═"] {
        assert!(
            rendered.search(glyph).is_empty(),
            "frame decoration is not source search text"
        );
    }
}

fn motor_frame_bounds(buffer: &ratatui::buffer::Buffer, width: u16) -> (u16, u16, u16, u16) {
    let corners = |glyph: &str| {
        (0..buffer.area.height)
            .flat_map(|row| {
                let buffer = &buffer;
                (0..buffer.area.width).filter_map(move |column| {
                    (buffer[(column, row)].symbol() == glyph).then_some((column, row))
                })
            })
            .collect::<Vec<_>>()
    };
    let top_left = corners("┌");
    let top_right = corners("┐");
    let bottom_left = corners("└");
    let bottom_right = corners("┘");
    assert_eq!(
        (
            top_left.len(),
            top_right.len(),
            bottom_left.len(),
            bottom_right.len()
        ),
        (1, 1, 1, 1),
        "{width}: one original frame"
    );
    let (left, top) = top_left[0];
    let (right, _) = top_right[0];
    let (_, bottom) = bottom_left[0];
    assert_eq!(top_right[0].1, top);
    assert_eq!(bottom_left[0].0, left);
    assert_eq!(bottom_right[0].0, right);
    assert_eq!(bottom_right[0].1, bottom);
    for column in left + 1..right {
        assert!(
            ["─", "┬", "╤"].contains(&buffer[(column, top)].symbol()),
            "{width}: complete top edge"
        );
        assert!(
            ["─", "┴", "╧"].contains(&buffer[(column, bottom)].symbol()),
            "{width}: complete bottom edge"
        );
    }
    (left, right, top, bottom)
}
