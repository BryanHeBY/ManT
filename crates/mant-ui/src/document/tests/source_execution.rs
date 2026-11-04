//! Source execution rows survive wire decoding and final terminal painting.

use super::*;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use serde_json::Value;

fn paint_rows(content: &ResolvedContent, width: u16, heading: &str) -> Vec<String> {
    let view = DocumentView::new(content);
    let rendered = view.render(width);
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    ratatui::widgets::Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    let rows: Vec<_> = (0..area.height)
        .map(|row| {
            (0..width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>()
                // This fixture cohort has no authored edge blanks. Only
                // responsive row origins and untouched terminal fill cells
                // are excluded; internal spaces and empty rows stay exact.
                .trim_matches(' ')
                .to_owned()
        })
        .collect();
    let start = rows.iter().position(|row| row == heading).unwrap() + 1;
    let end = rows.iter().position(|row| row == "NEXT").unwrap();
    let mut body = rows[start..end].to_vec();
    // The view owns section presentation gaps. The fixture compares every
    // interior native hard/empty row, independently of that outer gap.
    while body.last().is_some_and(String::is_empty) {
        body.pop();
    }
    body
}

fn portable_rows(
    case: &Value,
    content: &ResolvedContent,
    heading: &str,
    native_rows: &[String],
) -> Vec<String> {
    let Some(rows) = case.get("portable_rows") else {
        return native_rows.to_vec();
    };
    // Direct declarations are separate semantic owners from preceding and
    // following prose. Canonical Markdown retains those paragraphs; they
    // are not extra native rows. Verify the complete original owners first.
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == heading)
        .unwrap();
    let actual_owners: Vec<_> = section
        .blocks
        .iter()
        .map(|block| {
            let Block::Paragraph { children, .. } = block else {
                panic!("{}: paragraph owner", case["id"])
            };
            mant_ir::inline_plain_text(children)
        })
        .collect();
    let expected_owners: Vec<_> = case["portable_owner_texts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|text| text.as_str().unwrap())
        .collect();
    assert_eq!(actual_owners, expected_owners, "{}", case["id"]);
    rows.as_array()
        .unwrap()
        .iter()
        .map(|row| row.as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn native_node_lifecycles_reach_complete_buffers_and_markdown_readback() {
    // Exact sources ran the pristine ASCII/UTF-8/HTML/tree/lint oracle before
    // these assertions. mdoc_term.c::synopsis_pre, termp_nm_pre, D1/Dl's
    // termp_bl_post and termp_sm_pre execute in the same live term_word flow.
    let fixture: Value = serde_json::from_str(include_str!("source_execution/cases.json")).unwrap();
    assert_eq!(
        fixture["oracle_sha256"],
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    let mut differences = Vec::new();
    for case in fixture["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let source = case["source"].as_str().unwrap();
        let expected: Vec<_> = case["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row.as_str().unwrap().to_owned())
            .collect();
        let heading = case["heading"].as_str().unwrap_or("SYNOPSIS");
        let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let decoded: ResolvedContent = decoded.into();
        for width in [40, 78] {
            let actual = paint_rows(&decoded, width, heading);
            if actual != expected {
                differences.push(format!("{id}/{width}: {actual:?} != {expected:?}"));
            }
        }
        let markdown = mant_codec::encode::render_markdown(&decoded);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let readback_section = readback
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == heading)
            .unwrap();
        let list_marker = if case["context"] == "head" {
            let [
                Block::List {
                    kind: mant_ir::ListKind::Bullet,
                    items,
                    ..
                },
            ] = readback_section.blocks.as_slice()
            else {
                panic!("{id}: original definition has one actual exported list owner")
            };
            assert_eq!(items.len(), 1, "{id}");
            true
        } else {
            false
        };
        let portable_rows = portable_rows(case, &decoded, heading, &expected);
        for width in [40, 78] {
            let mut actual = paint_rows(&readback, width, heading);
            if list_marker {
                actual[0] = actual[0].strip_prefix("• ").unwrap().to_owned();
            }
            if actual != portable_rows {
                differences.push(format!(
                    "{id}/readback/{width}: {actual:?} != {portable_rows:?}"
                ));
            }
        }
        assert_eq!(
            serde_json::to_string(&mant_protocol::QueryBundle::from(&decoded)).unwrap(),
            wire,
            "{id}: painting changed content ownership"
        );
    }
    assert_eq!(differences, Vec::<String>::new());
}
