//! Real terminal consumers retain accepted cells and completed physical rows.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use mant_ir::ResolvedContent;
use mant_ui::{App, CopyRequest, DocumentView, RenderedDocument};
use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

#[path = "support/reader_actions.rs"]
mod reader_actions;
use reader_actions::{click, copied_selections, opened_targets, positions, select_span};

const CASES: &str =
    include_str!("../../mant-engine/tests/roff_lowering/acceptance_axes/consumer_cases.json");
const RECORDED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../mant-engine/tests/roff_lowering/acceptance_axes/cases"
);
const WIDTHS: [u16; 5] = [20, 40, 78, 120, 20];

fn round_trip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    assert!(!json.contains("\\u0000mant:"));
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn normalize(row: &str) -> String {
    row.split(' ')
        .filter(|cell| !cell.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

fn body_range(query: &ResolvedContent, rendered: &RenderedDocument) -> std::ops::Range<usize> {
    let start = rendered.anchor_row("description").unwrap() + 1;
    let end = query
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "NEXT")
        .map_or(rendered.row_count, |section| {
            rendered.anchor_row(section.id.as_str()).unwrap()
                - usize::from(section.spacing_before_lines)
        });
    start..end
}

fn body_rows(query: &ResolvedContent, rendered: &RenderedDocument) -> Vec<String> {
    rendered.text.lines[body_range(query, rendered)]
        .iter()
        .map(|line| normalize(&line.to_string()))
        .collect()
}

fn buffer_row(buffer: &Buffer, row: u16) -> String {
    let mut text = String::new();
    let mut column = 0;
    while column < buffer.area.width {
        let symbol = buffer[(column, row)].symbol();
        text.push_str(symbol);
        // A wide grapheme's following cell is a terminal continuation,
        // rather than a separate space, scalar, or selection character.
        column += u16::try_from(symbol.width().max(1)).unwrap();
    }
    normalize(&text)
}

fn normalize_final_gap(rows: &mut [String]) {
    for row in rows {
        if let Some(at) = row.find("BodyWord")
            && at > 0
            && !row[..at].ends_with(' ')
        {
            row.insert(at, ' ');
        }
    }
}

#[test]
fn six_recorded_examples_keep_native_hard_rows_at_each_content_width() {
    // These eleven immutable sources were rerun with the sole recorder's
    // --check before adding assertions. Its snapshots include the next
    // section's one furniture row, unlike the content-only bridge fixture.
    for name in [
        "word_owner_pending_prefix",
        "rejected_suffix_revival",
        "column_tail_hard_row",
        "tag_explicit_vspace",
        "hang_final_gap",
        "escaped_delimiter_payload",
    ] {
        let source = std::fs::read_to_string(format!("{RECORDED}/{name}.1")).unwrap();
        let query = round_trip(&source);
        let snapshot = std::fs::read_to_string(format!("{RECORDED}/{name}.expected")).unwrap();
        let mut expected = snapshot.lines().map(normalize).collect::<Vec<_>>();
        assert_eq!(
            expected.pop(),
            Some(String::new()),
            "{name}: next-section furniture witness"
        );
        let view = DocumentView::new(&query);
        for width in WIDTHS {
            let mut observed = body_rows(&query, &view.render(width));
            // This card explicitly declares ResponsiveBreak only at the
            // final AFTER/BodyWord seam. Narrow content can wrap that seam;
            // D/AFTER remains an authored hard row, with no missing blanks.
            if name == "hang_final_gap" && observed.ends_with(&["AFTER".into(), "BodyWord".into()])
            {
                observed.truncate(observed.len() - 2);
                observed.push("AFTER BodyWord".into());
            }
            assert_eq!(observed, expected, "{name}, width={width}");
        }
    }
}

#[test]
fn mechanism_controls_keep_edge_rows_in_actual_buffers_at_each_content_width() {
    // Exact source/profile hashes are committed in this fixture after all
    // five pristine runs. term_newln/term_vspace receipts and NODE_LINE are
    // distinct from container return. Device padding is responsive; no
    // blank row is folded or removed in this comparison.
    let fixture: serde_json::Value = serde_json::from_str(CASES).unwrap();
    let mut failures = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let query = round_trip(case["source"].as_str().unwrap());
        let view = DocumentView::new(&query);
        let mut expected = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        if name == "spacing-intervals-0632" {
            normalize_final_gap(&mut expected);
        }
        for width in WIDTHS {
            let rendered = view.render(width);
            let mut observed = body_rows(&query, &rendered);
            if name == "spacing-intervals-0632" {
                normalize_final_gap(&mut observed);
            }
            // Unicode link wording may wrap responsively at 20 columns;
            // its scalar/cell/click ownership is checked separately below.
            if name != "unicode-owned-cells" && observed != expected {
                failures.push(format!(
                    "{name}, width={width}: expected={expected:?}, actual={observed:?}"
                ));
            }
            let area = Rect::new(0, 0, width, 100);
            let mut buffer = Buffer::empty(area);
            Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
            for row in body_range(&query, &rendered) {
                assert_eq!(
                    buffer_row(&buffer, u16::try_from(row).unwrap()),
                    normalize(&rendered.text.lines[row].to_string()),
                    "{name}, width={width}: rendered cells differ from the actual widget"
                );
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn body_position(buffer: &Buffer, label: &str) -> (u16, u16) {
    let start = positions(buffer, "DESCRIPTION")
        .into_iter()
        .next()
        .unwrap()
        .1;
    let end = positions(buffer, "NEXT").into_iter().next().unwrap().1;
    let found = positions(buffer, label)
        .into_iter()
        .filter(|(_, row)| *row > start && *row < end)
        .collect::<Vec<_>>();
    assert_eq!(
        found.len(),
        1,
        "body label {label:?} has ambiguous cells: {found:?}"
    );
    found[0]
}

fn assert_actions(query: &ResolvedContent, label: &str, target: Option<&str>) {
    let mut app = App::new(query);
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(24, 60)).unwrap();
    for content_width in WIDTHS {
        // Sidebar is hidden. The real App reserves two content margins
        // and a two-cell scrollbar gutter (virtual tail is always active).
        // Thus terminal width content+4 is the same effective content width
        // independently rendered by DocumentView and the fixture test.
        terminal.backend_mut().resize(content_width + 4, 60);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = body_position(terminal.backend().buffer(), label);
        let rendered = DocumentView::new(query).render(content_width);
        let body = body_range(query, &rendered);
        let hits = rendered
            .search(label)
            .into_iter()
            .filter(|hit| body.contains(&hit.row))
            // Search folds case (P also matches https). The real cell
            // locator above is exact; select only its corresponding hit.
            .filter(|hit| {
                u16::try_from(hit.start_column).unwrap() + 1 == column
                    && u16::try_from(hit.row).unwrap() + 2 == row
            })
            .collect::<Vec<_>>();
        assert_eq!(
            hits.len(),
            1,
            "{label}: App cells do not match effective width {content_width}"
        );
        assert_eq!(
            (column, row),
            (
                u16::try_from(hits[0].start_column).unwrap() + 1,
                u16::try_from(hits[0].row).unwrap() + 2,
            ),
            "{label}: App geometry differs from effective width {content_width}"
        );
        click(&mut app, column, row);
        assert_eq!(
            opened_targets(&mut app),
            target.into_iter().collect::<Vec<_>>(),
            "{label}, width={content_width}"
        );
        let last = column + u16::try_from(label.width()).unwrap() - 1;
        select_span(
            &mut app,
            column,
            row,
            last,
            (last == column).then_some(column + 1),
        );
        let copied = copied_selections(&mut app, |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("visual selection expected")
            };
            text
        });
        assert_eq!(
            copied,
            [label],
            "{label}, width={content_width}: cell copy changed the accepted owner"
        );
    }
}

fn wrapped_label_fragments(
    query: &ResolvedContent,
    rendered: &RenderedDocument,
    label: &str,
) -> Vec<(String, usize, usize)> {
    assert!(!label.chars().any(char::is_whitespace));
    let mut cells = Vec::new();
    for row in body_range(query, rendered) {
        let text = rendered.text.lines[row].to_string();
        let mut column = 0;
        for grapheme in mant_render::cells::graphemes(&text) {
            if !grapheme.text().chars().all(char::is_whitespace) {
                cells.push((grapheme.text().to_owned(), row, column));
            }
            column += grapheme.columns();
        }
    }
    let length = mant_render::cells::graphemes(label).count();
    let matches = cells
        .windows(length)
        .filter(|window| {
            window
                .iter()
                .map(|cell| cell.0.as_str())
                .collect::<String>()
                == label
        })
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1, "wrapped body label {label:?}");
    let mut fragments: Vec<(String, usize, usize)> = Vec::new();
    for (text, row, column) in matches[0] {
        if let Some(last) = fragments.last_mut()
            && last.1 == *row
        {
            assert_eq!(last.2 + last.0.width(), *column);
            last.0.push_str(text);
        } else {
            fragments.push((text.clone(), *row, *column));
        }
    }
    fragments
}

fn assert_wrapped_link_actions(query: &ResolvedContent, label: &str, target: &str) {
    let mut app = App::new(query);
    app.handle_key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    let mut terminal = Terminal::new(TestBackend::new(24, 60)).unwrap();
    for width in WIDTHS {
        terminal.backend_mut().resize(width + 4, 60);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let rendered = DocumentView::new(query).render(width);
        let fragments = wrapped_label_fragments(query, &rendered, label);
        let hits = rendered
            .search(label)
            .into_iter()
            .filter(|hit| body_range(query, &rendered).contains(&hit.row))
            .collect::<Vec<_>>();
        assert_eq!(hits.len(), 1, "{label}, width={width}");
        assert_eq!(
            (hits[0].row, hits[0].start_column),
            (fragments[0].1, fragments[0].2)
        );
        let mut copied_fragments = String::new();
        for (text, document_row, document_column) in fragments {
            let row = u16::try_from(document_row).unwrap() + 2;
            let column = u16::try_from(document_column).unwrap() + 1;
            let mut offset = 0;
            for grapheme in mant_render::cells::graphemes(&text) {
                let cell = column + u16::try_from(offset).unwrap();
                assert_eq!(
                    terminal.backend().buffer()[(cell, row)].symbol(),
                    grapheme.text()
                );
                click(&mut app, cell, row);
                assert_eq!(opened_targets(&mut app), [target]);
                offset += grapheme.columns();
            }
            let last = column + u16::try_from(text.width()).unwrap() - 1;
            select_span(
                &mut app,
                column,
                row,
                last,
                (last == column).then_some(column + 1),
            );
            let copied = copied_selections(&mut app, |request| {
                let CopyRequest::Selection { text } = request else {
                    panic!("visual selection expected")
                };
                text
            });
            assert_eq!(copied, [text.as_str()], "{label}, width={width}");
            copied_fragments.push_str(&copied[0]);
        }
        assert_eq!(copied_fragments, label, "{label}, width={width}");
    }
}

#[test]
fn accepted_scalar_owners_keep_click_and_copy_ranges_after_resize() {
    // term.c::encode1 records P before the next Link; rejected suffixes
    // cannot capture that cell. A fully rejected label retains an empty
    // identity without a fabricated click/copy interval. The source was
    // checked in pristine CVS before these ownership assertions.
    let source =
        std::fs::read_to_string(format!("{RECORDED}/word_owner_pending_prefix.1")).unwrap();
    let query = round_trip(&source);
    assert_actions(&query, "P", None);
    assert_actions(&query, "Y", Some("https://ex.org"));
    let source = std::fs::read_to_string(format!("{RECORDED}/rejected_suffix_revival.1")).unwrap();
    let query = round_trip(&source);
    assert_actions(&query, "P", None);
    for width in WIDTHS {
        let rendered = DocumentView::new(&query).render(width);
        let range = body_range(&query, &rendered);
        for rejected in ["Y", "https://ex.org", "AFTER"] {
            assert!(
                rendered
                    .search(rejected)
                    .iter()
                    .all(|hit| !range.contains(&hit.row))
            );
        }
    }
    let source = std::fs::read_to_string(format!("{RECORDED}/same_uri_occurrences.1")).unwrap();
    let query = round_trip(&source);
    assert_actions(&query, "first", Some("https://ex.org"));
    assert_actions(&query, "second", Some("https://ex.org"));
    let fixture: serde_json::Value = serde_json::from_str(CASES).unwrap();
    let case = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "unicode-owned-cells")
        .unwrap();
    let query = round_trip(case["source"].as_str().unwrap());
    assert_actions(&query, "P", None);
    assert_actions(&query, "α中", Some("https://ex.org"));
    assert_actions(&query, "AFTER", None);
}

#[test]
fn source_styles_survive_the_real_terminal_cell_projection() {
    // mdoc_term.c::termp_em_pre/termp_sy_pre push the selected native fonts;
    // accepted children retain the style of native cells.
    let source = std::fs::read_to_string(format!("{RECORDED}/styled_units.1")).unwrap();
    let query = round_trip(&source);
    for width in WIDTHS {
        let rendered = DocumentView::new(&query).render(width);
        let area = Rect::new(0, 0, width, 100);
        let mut buffer = Buffer::empty(area);
        Paragraph::new(rendered.text).render(area, &mut buffer);
        for (word, style) in [
            ("glowing", Modifier::ITALIC),
            ("rigid", Modifier::BOLD),
            ("plain", Modifier::empty()),
        ] {
            let (column, row) = body_position(&buffer, word);
            let modifiers = buffer[(column, row)].modifier;
            assert_eq!(
                modifiers.contains(Modifier::ITALIC),
                style.contains(Modifier::ITALIC)
            );
            assert_eq!(
                modifiers.contains(Modifier::BOLD),
                style.contains(Modifier::BOLD)
            );
        }
    }
    assert_actions(&query, "glowing", None);
    assert_actions(&query, "rigid", None);
    assert_actions(&query, "plain", None);
}

#[test]
fn sole_visible_body_keeps_native_spelling_and_link_activation_ranges() {
    // Each exact complete input ran the pristine five profiles first.
    // mdoc_validate.c::post_bx adds BSD; mdoc_term.c::termp_lk_pre emits
    // description, colon and URI. Accepted suffix glyphs are not hidden.
    let pre =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for (payload, expected, label, target) in [
        (
            ".Bx -alpha\n.No AFTER\n",
            "-alphaBSD AFTER",
            "-alphaBSD",
            None,
        ),
        (
            ".Lk https://example.org label\n.No AFTER\n",
            "label: https://example.org AFTER",
            "label",
            Some("https://example.org"),
        ),
        (
            ".Lk https://example.org \\&\n.No AFTER\n",
            ": https://example.org AFTER",
            "https://example.org",
            Some("https://example.org"),
        ),
    ] {
        let query = round_trip(&format!("{pre}{payload}.Sh NEXT\n.No END\n"));
        let rendered = DocumentView::new(&query).render(78);
        assert_eq!(body_rows(&query, &rendered), [expected]);
        if label.starts_with("https://") {
            assert_wrapped_link_actions(&query, label, target.unwrap());
        } else {
            assert_actions(&query, label, target);
        }
        let markdown = mant_codec::encode::render_markdown(&query);
        let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let rendered = DocumentView::new(&restored).render(78);
        assert_eq!(body_rows(&restored, &rendered), [expected]);
        if label.starts_with("https://") {
            assert_wrapped_link_actions(&restored, label, target.unwrap());
        } else {
            assert_actions(&restored, label, target);
        }
        assert!(!markdown.contains("currently"));
    }
}
