//! Accepted native cells keep text, row and link ownership through consumers.

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use mant_ir::{ReferenceScope, ResolvedContent};
use mant_protocol::{
    ReferenceCount, ReferenceProjection, ReferenceProjectionMode, ReferenceTargetType,
};
use mant_ui::{App, CopyRequest, DocumentView, ReaderServices, RenderedDocument};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Modifier};

const HEADER: &str =
    ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
const FOOTER: &str = ".Sh NEXT\n.No END\n";

fn round_trip(body: &str) -> ResolvedContent {
    let source = format!("{HEADER}{body}{FOOTER}");
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    assert!(
        !json.contains("\\u0000mant:"),
        "private execution owner leaked"
    );
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    restored.into()
}

fn body_hit(rendered: &RenderedDocument, word: &str) -> mant_ui::RenderedSearchMatch {
    let start = rendered.anchor_row("description").unwrap();
    let end = rendered.anchor_row("next").unwrap();
    let hits = rendered
        .search(word)
        .into_iter()
        .filter(|hit| hit.row > start && hit.row < end)
        .collect::<Vec<_>>();
    assert_eq!(hits.len(), 1, "{word}: {:?}", rendered.text);
    hits.into_iter().next().unwrap()
}

fn assert_missing(rendered: &RenderedDocument, word: &str) {
    let start = rendered.anchor_row("description").unwrap();
    let end = rendered.anchor_row("next").unwrap();
    assert!(
        rendered
            .search(word)
            .into_iter()
            .all(|hit| hit.row <= start || hit.row >= end),
        "{word}: {:?}",
        rendered.text
    );
}

fn inventory(query: &ResolvedContent) -> mant_protocol::ReferenceInventory {
    mant_query::project_references(
        query.document.as_ref().unwrap(),
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            target_types: vec![ReferenceTargetType::External],
            ..Default::default()
        },
    )
}

fn pointer(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle_mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    });
}

fn positions(buffer: &Buffer, label: &str) -> Vec<(u16, u16)> {
    let width = u16::try_from(label.len()).unwrap();
    let mut found = Vec::new();
    for row in 1..buffer.area.height.saturating_sub(1) {
        for column in 0..buffer.area.width.saturating_sub(width) {
            let text: String = (column..column + width)
                .map(|x| buffer[(x, row)].symbol())
                .collect();
            if text == label {
                found.push((column, row));
            }
        }
    }
    found
}

fn body_position(buffer: &Buffer, word: &str) -> (u16, u16) {
    let heading_row = positions(buffer, "DESCRIPTION")
        .into_iter()
        .max_by_key(|(column, _)| *column)
        .unwrap()
        .1;
    let end_row = positions(buffer, "NEXT")
        .into_iter()
        .max_by_key(|(column, _)| *column)
        .unwrap()
        .1;
    positions(buffer, word)
        .into_iter()
        .filter(|(_, row)| *row > heading_row && *row < end_row)
        .max_by_key(|(column, _)| *column)
        .expect("visible body word")
}

fn assert_pointer_and_copy(query: &ResolvedContent, word: &str, target: Option<&str>) {
    let mut app = App::new(query);
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    for width in [80, 128, 240, 80] {
        terminal.backend_mut().resize(width, 40);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let (column, row) = body_position(terminal.backend().buffer(), word);
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        );
        pointer(&mut app, MouseEventKind::Up(MouseButton::Left), column, row);
        let mut activated = Vec::new();
        let mut open = |uri: &mant_ui::ExternalUri| {
            activated.push(uri.as_str().to_owned());
            Ok(())
        };
        app.service_pending(&mut ReaderServices {
            open_external: Some(&mut open),
            ..Default::default()
        });
        assert_eq!(
            activated,
            target.into_iter().collect::<Vec<_>>(),
            "{word}, width={width}"
        );
        let last = column + u16::try_from(word.len()).unwrap() - 1;
        pointer(
            &mut app,
            MouseEventKind::Down(MouseButton::Left),
            column,
            row,
        );
        if last == column {
            // A one-cell selection still needs pointer motion to enter drag
            // mode; returning to the starting cell selects exactly that cell.
            pointer(
                &mut app,
                MouseEventKind::Drag(MouseButton::Left),
                column + 1,
                row,
            );
        }
        pointer(&mut app, MouseEventKind::Drag(MouseButton::Left), last, row);
        pointer(&mut app, MouseEventKind::Up(MouseButton::Left), last, row);
        let mut copied = Vec::new();
        let mut copy = |request| {
            let CopyRequest::Selection { text } = request else {
                panic!("visual selection expected")
            };
            copied.push(text);
            Ok(())
        };
        app.service_pending(&mut ReaderServices {
            copy_to_clipboard: Some(&mut copy),
            ..Default::default()
        });
        assert_eq!(
            copied,
            [word],
            "{word}, width={width}: visual copy changed ownership"
        );
    }
}

fn portable(query: &ResolvedContent) -> RenderedDocument {
    let markdown = mant_codec::encode::render_markdown(query);
    let reparsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
    DocumentView::new(&reparsed).render(240)
}

#[test]
fn continued_native_buffers_keep_accepted_words_in_real_rows() {
    // Exact sources ran pristine first: NODE_LINE is suppressed by \c;
    // term.c::term_fill scans BEFORE and the following \p in one buffer.
    for (operand, first, row_delta) in [(r"\p D", "BEFORE", 1), (r"\pD", "BEFORED", 1)] {
        let query = round_trip(&format!(
            ".nf\n.No BEFORE\\c\n.No \"{operand}\"\n.No AFTER\n.fi\n"
        ));
        let view = DocumentView::new(&query);
        for width in [20, 40, 80, 128, 240] {
            let rendered = view.render(width);
            let first = body_hit(&rendered, first);
            let after = body_hit(&rendered, "AFTER");
            if operand == r"\p D" {
                let d = body_hit(&rendered, "D");
                assert_eq!(d.row, first.row + 1);
                assert_eq!(after.row, d.row + row_delta);
            } else {
                assert_eq!(after.row, first.row + row_delta);
            }
        }
        assert_pointer_and_copy(&query, "AFTER", None);
        assert_eq!(portable(&query).search("AFTER").len(), 1);
    }
}

#[test]
fn rejected_link_words_keep_occurrences_without_click_or_copy_ranges() {
    // termp_lk_pre may leave an unprinted field; mdoc_lk_pre's authored
    // href remains typed metadata. br/Pp retire that buffer independently.
    for (control, visible_after) in [("", false), (".br\n", true), (".Pp\n", true)] {
        let query = round_trip(&format!(
            ".Lk https://ex.org \"\\p D\"\n{control}.No AFTER\n"
        ));
        let references = inventory(&query);
        assert_eq!(references.occurrences, ReferenceCount::Exact { value: 1 });
        assert_eq!(references.records.len(), 1);
        assert!(references.records[0].label.is_empty());
        let view = DocumentView::new(&query);
        for width in [20, 40, 80, 128, 240] {
            let rendered = view.render(width);
            assert_missing(&rendered, "https://ex.org");
            assert_missing(&rendered, "D");
            if visible_after {
                body_hit(&rendered, "AFTER");
            } else {
                assert_missing(&rendered, "AFTER");
            }
        }
        if visible_after {
            assert_pointer_and_copy(&query, "AFTER", None);
        }
        assert_missing(&portable(&query), "https://ex.org");
    }
    let query = round_trip(
        ".Lk https://ex.org \"\\p D\"\n.br\n.Lk https://ex.org \"\\p D\"\n.br\n.No AFTER\n",
    );
    let references = inventory(&query);
    assert_eq!(references.occurrences, ReferenceCount::Exact { value: 2 });
    assert_eq!(references.targets, ReferenceCount::Exact { value: 1 });
    assert_eq!(references.records.len(), 2);
    assert_ne!(references.records[0].origin, references.records[1].origin);
    assert!(
        references
            .records
            .iter()
            .all(|record| record.label.is_empty())
    );
    assert_pointer_and_copy(&query, "AFTER", None);
}

#[test]
fn delayed_accepted_glyph_keeps_its_font_and_stays_outside_the_next_link() {
    // term.c::encode1 stores P in the prior native owner before BACKBEFORE;
    // rejection of the next word cannot transfer P to the later Link owner.
    for body in [
        ".No \\zP\n.No \"\\p  D\"\n.No AFTER\n",
        ".Em \\zP\n.Lk https://other.example \"\\p  D\"\n.No AFTER\n",
    ] {
        let query = round_trip(body);
        let emphasis = body.starts_with(".Em");
        let view = DocumentView::new(&query);
        for width in [20, 40, 80, 128, 240] {
            let rendered = view.render(width);
            let p = body_hit(&rendered, "P");
            assert_missing(&rendered, "D");
            assert_missing(&rendered, "AFTER");
            assert_missing(&rendered, "https://other.example");
            let source_span = rendered.text.lines[p.row]
                .spans
                .iter()
                .find(|span| span.content.contains('P'))
                .unwrap();
            assert_eq!(
                source_span.style.add_modifier.contains(Modifier::ITALIC),
                emphasis
            );
        }
        assert_pointer_and_copy(&query, "P", None);
        body_hit(&portable(&query), "P");
        if emphasis {
            assert_eq!(
                inventory(&query).occurrences,
                ReferenceCount::Exact { value: 1 }
            );
        }
    }
}

#[test]
fn generated_link_words_do_not_repeat_an_already_projected_hard_row() {
    // termp_lk_pre executes label -> NOSPACE colon -> URI as one buffer.
    // Exact CVS output is D / : / URI AFTER; ordinary No is a negative case.
    let query = round_trip(".Lk https://ex.org \"D\\p \\p\"\n.No AFTER\n");
    let view = DocumentView::new(&query);
    for width in [40, 80, 128, 240] {
        let rendered = view.render(width);
        let d = body_hit(&rendered, "D");
        // The URI also contains ':'. Check the separately executed colon
        // row rather than counting every substring search hit in the body.
        let colon_row = d.row + 1;
        assert_eq!(rendered.text.lines[colon_row].to_string().trim(), ":");
        let uri = body_hit(&rendered, "https://ex.org");
        let after = body_hit(&rendered, "AFTER");
        assert_eq!(uri.row, colon_row + 1);
        assert_eq!(after.row, uri.row);
    }
    assert_pointer_and_copy(&query, "D", Some("https://ex.org"));
    assert_pointer_and_copy(&query, "AFTER", None);
    let plain = round_trip(".No \"D\\p \\p\"\n.No AFTER\n");
    let rendered = DocumentView::new(&plain).render(80);
    body_hit(&rendered, "D");
    assert_missing(&rendered, "AFTER");
}

#[test]
fn vertical_request_keeps_long_tag_blank_row_without_padding_the_short_tag() {
    // termp_pp_pre calls term_vspace, not term_newln; TAG overrun closes
    // its device row first, then emits an extra row. Short TAG differs.
    for (label, delta) in [("VERYVERYLONGLABEL", 2), ("LABEL", 1)] {
        let query = round_trip(&format!(
            ".Bl -tag -width 8n\n.It Xo\n.No {label}\n.Pp\n.No AFTER\n.Xc\n.No BodyWord\n.El\n"
        ));
        let view = DocumentView::new(&query);
        for width in [80, 128, 240] {
            let rendered = view.render(width);
            let head = body_hit(&rendered, label);
            let after = body_hit(&rendered, "AFTER");
            assert_eq!(after.row, head.row + delta, "{label}: {:?}", rendered.text);
            if delta == 2 {
                assert!(
                    rendered.text.lines[head.row + 1]
                        .to_string()
                        .trim()
                        .is_empty()
                );
            }
            body_hit(&rendered, "BodyWord");
        }
        assert_pointer_and_copy(&query, "AFTER", None);
        assert_eq!(portable(&query).search("AFTER").len(), 1);
    }
}
