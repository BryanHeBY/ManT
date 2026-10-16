//! Independent inputs and assertions for table consumer regression axes.

use mant_ir::ResolvedContent;
use mant_ui::{DocumentView, RenderedDocument};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};
use serde_json::{Value, json};
use unicode_width::UnicodeWidthStr;

pub fn paragraph(value: &str) -> Value {
    json!({"type":"paragraph","children":[{"type":"text","value":value}]})
}

pub fn cell(value: &str) -> Value {
    json!({"blocks":[paragraph(value)]})
}

pub fn query(block: Value) -> ResolvedContent {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../../../tests/contracts/minimal-query-v0.13.json"
    ))
    .unwrap();
    value.as_object_mut().unwrap().remove("tldr");
    value["label"] = json!("table-probe");
    value["document"]["meta"] = json!({"title":"TABLE-PROBE"});
    value["document"]["sections"] = json!([]);
    value["document"]["blocks"] = Value::Array(vec![block]);
    let query: mant_protocol::QueryBundle =
        serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();
    query.into()
}

pub fn restored(value: &ResolvedContent) -> ResolvedContent {
    let wire = mant_render::render_query_json(value, false).unwrap();
    let query: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored: ResolvedContent = query.into();
    assert_eq!(
        &restored, value,
        "actual serialized JSON must retain table facts"
    );
    restored
}

pub fn rows(rendered: &RenderedDocument) -> Vec<String> {
    rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect()
}

pub fn cli(value: &ResolvedContent, width: u16) -> String {
    mant_render::render_query_text_at_width(value, usize::from(width), |_, text| text.to_owned())
}

pub fn assert_markers(value: &ResolvedContent, width: u16, markers: &[&str]) {
    let rendered = DocumentView::new(value).render(width);
    let text = cli(value, width);
    let decorated =
        mant_render::render_query_text_at_width(value, usize::from(width), |_, text| {
            format!("\u{1b}[1m{text}\u{1b}[0m")
        });
    assert_eq!(
        decorated.replace("\u{1b}[1m", "").replace("\u{1b}[0m", ""),
        text
    );
    for marker in markers {
        assert_eq!(
            rendered.search(marker).len(),
            1,
            "{width}, {marker}: {:?}",
            rendered.text
        );
        // A marker may soft-wrap at narrow widths. Search uses source scalars;
        // the CLI still must retain its full letter sequence in reading order.
        let joined = text.lines().collect::<String>();
        assert_eq!(
            joined.matches(marker).count(),
            1,
            "{width}, {marker}: {text:?}"
        );
    }
}

/// Check glyph placement in Ratatui's actual Buffer, not just a text snapshot.
pub fn assert_buffer(rendered: &RenderedDocument, width: u16, markers: &[&str]) {
    let height = u16::try_from(rendered.row_count).unwrap();
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    for marker in markers {
        let hit = &rendered.search(marker)[0];
        assert_eq!(
            buffer[(
                u16::try_from(hit.start_column).unwrap(),
                u16::try_from(hit.row).unwrap()
            )]
                .symbol(),
            mant_render::cells::graphemes(marker).next().unwrap().text(),
            "{width}, {marker}"
        );
    }
    for row in &rendered.text.lines {
        assert!(
            row.to_string().width() <= usize::from(width),
            "row exceeds viewport {width}: {row:?}"
        );
    }
}

/// Border glyphs are presentation, and cannot be found in authoritative search.
pub fn assert_no_decoration_search(rendered: &RenderedDocument) {
    for decoration in ["│", "║", "┌", "╔", "─", "═"] {
        assert!(
            rendered.search(decoration).is_empty(),
            "decoration became source text: {decoration}"
        );
    }
}

/// Return a complete frame's left/right cell boundaries and top/bottom rows.
pub fn frame(rows: &[String], double: bool) -> (usize, usize, usize, usize) {
    let (tl, tr, bl, br) = if double {
        ('╔', '╗', '╚', '╝')
    } else {
        ('┌', '┐', '└', '┘')
    };
    let top = rows
        .iter()
        .position(|row| row.contains(tl))
        .expect("outer top frame");
    let bottom = rows
        .iter()
        .rposition(|row| row.contains(bl))
        .expect("outer bottom frame");
    let left = rows[top].chars().position(|c| c == tl).unwrap();
    let right = rows[top].chars().position(|c| c == tr).unwrap();
    assert_eq!(rows[bottom].chars().position(|c| c == bl), Some(left));
    assert_eq!(rows[bottom].chars().position(|c| c == br), Some(right));
    assert!(bottom > top);
    (left, right, top, bottom)
}
