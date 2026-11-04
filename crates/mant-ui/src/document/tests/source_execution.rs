//! Source execution rows survive wire decoding and final terminal painting.

use super::*;
use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};
use serde_json::Value;

fn paint_rows(
    content: &ResolvedContent,
    width: u16,
    heading: &str,
    exact_word_observation: bool,
) -> Vec<String> {
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
                .trim_end_matches(' ')
                .to_owned()
        })
        .collect();
    let start = rows.iter().position(|row| row == heading).unwrap() + 1;
    let end = rows.iter().position(|row| row == "NEXT").unwrap();
    let mut body = rows[start..end].to_vec();
    if exact_word_observation {
        // The recorded direct Fn/Fo cohort starts with PREFIX/PREFIXX. Remove
        // that shared viewport origin, not the native word SP before SUFFIX.
        let first = body.iter().find(|row| !row.is_empty()).unwrap();
        let origin = " ".repeat(first.len() - first.trim_start_matches(' ').len());
        for row in &mut body {
            if !row.is_empty() {
                *row = row.strip_prefix(&origin).unwrap().to_owned();
            }
        }
    } else {
        // Keep the older cohort's documented row-origin observation policy.
        // Exact word cells in source-bound direct-flush cases are checked above.
        for row in &mut body {
            *row = row.trim_start_matches(' ').to_owned();
        }
    }
    // The view owns section presentation gaps. The fixture compares every
    // interior native hard/empty row, independently of that outer gap.
    while body.last().is_some_and(String::is_empty) {
        body.pop();
    }
    body
}

fn apply_word_observations(fixture: &mut Value) -> std::collections::BTreeSet<String> {
    let observations: Value =
        serde_json::from_str(include_str!("source_execution/word_observations.json")).unwrap();
    assert_eq!(observations["oracle_sha256"], fixture["oracle_sha256"]);
    assert_eq!(observations["cases"].as_array().unwrap().len(), 8);
    let mut applied = std::collections::BTreeSet::new();
    for observation in observations["cases"].as_array().unwrap() {
        let id = observation["id"].as_str().unwrap();
        assert!(applied.insert(id.to_owned()));
        let case = fixture["cases"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|case| case["id"] == id)
            .unwrap();
        assert_eq!(case["source"], observation["source"], "{id}: exact source");
        assert_eq!(observation["profiles"].as_object().unwrap().len(), 5);
        case["rows"] = observation["reading_rows"].clone();
        case["portable_rows"] = observation["portable_rows"].clone();
        case["portable_owner_texts"] = observation["portable_owner_texts"].clone();
    }
    applied
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
    let mut fixture: Value =
        serde_json::from_str(include_str!("source_execution/cases.json")).unwrap();
    // termp_fn_pre's pretty tail calls term_flushln, not term_newln. The
    // latter alone selects NOSPACE (term.c:474-481,573-589). Fresh five-profile
    // runs precede these eight source-bound corrections; the old JSON is intact.
    let exact_word_cases = apply_word_observations(&mut fixture);
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
            let actual = paint_rows(&decoded, width, heading, exact_word_cases.contains(id));
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
            let mut actual = paint_rows(&readback, width, heading, exact_word_cases.contains(id));
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
