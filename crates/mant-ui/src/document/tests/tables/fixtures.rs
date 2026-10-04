//! Shared table builders and source-coordinate assertions.
use super::super::*;

pub(super) fn assert_padding_hit_coordinates(
    rendered: &RenderedDocument,
    hit: &RenderedSearchMatch,
    target: &LinkTarget,
) {
    let fragments = std::iter::once((hit.row, hit.start_column, hit.end_column)).chain(
        hit.additional_fragments
            .iter()
            .map(|fragment| (fragment.row, fragment.start_column, fragment.end_column)),
    );
    let mut copied = String::new();
    for (row, start, end) in fragments {
        let text = rendered.text.lines[row].to_string();
        // Search, clickable range and rendered glyph columns agree even when
        // Unicode makes a byte index differ from a display-cell coordinate.
        let fragment = rendered.selected_text(RenderedSelection {
            anchor: TextPosition { row, column: start },
            focus: TextPosition {
                row,
                column: end - 1,
            },
        });
        let byte = text.find(&fragment).unwrap();
        assert_eq!(start, mant_ir::geometry::text_width(&text[..byte]));
        assert_eq!(rendered.link_target_at(row, start), Some(target));
        assert_eq!(rendered.link_target_at(row, end - 1), Some(target));
        copied.push_str(&fragment);
    }
    assert_eq!(copied, "CLICK");
}

pub(super) fn declared_preferences(widths: &[u16]) -> mant_ir::ColumnPreferences {
    if widths.is_empty() {
        return mant_ir::ColumnPreferences::default();
    }
    mant_ir::ColumnPreferences {
        widths: widths.to_vec(),
        gap_columns: match widths.len() {
            n if n < 5 => 4,
            5 => 3,
            _ => 1,
        },
        advance_limit_columns: Some(256),
        extra_width_columns: Some(10),
    }
}

pub(super) fn assert_field_roundtrip(
    query: &ResolvedContent,
    shared: bool,
    column: usize,
    (indent, correction, nested): (i32, i32, bool),
) {
    let wire = mant_render::render_query_json(query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let restored = restored.into();
    let rendered = DocumentView::new(&restored).render(u16::MAX);
    let first = rendered.search("LEFT");
    let body = rendered.search("FIELD_B");
    assert_eq!(body.len(), 1);
    assert_eq!(first.len(), 1);
    let body = &body[0];
    assert_eq!(body.start_column, column, "{indent}/{correction}/{nested}");
    assert_eq!(
        body.row == first[0].row,
        shared,
        "{indent}/{correction}/{nested}"
    );
    assert_eq!(rendered.anchor_row("field-owner"), Some(body.row));
    assert!(rendered.link_target_at(body.row, column).is_some());
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: body.row,
                column
            },
            focus: TextPosition {
                row: body.row,
                column: body.end_column
            },
        }),
        "FIELD_B"
    );
}

pub(super) fn generic_linked_cell(
    value: &str,
    indent: i32,
    alignment: mant_ir::TableAlignment,
) -> TableCell {
    TableCell {
        break_after: false,
        blocks: vec![Block::Paragraph {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://e.example/generic".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: value.into(),
                }],
            }],
            inline_layout: mant_ir::InlineLayout::default(),
            layout: LayoutHint {
                indent_columns: indent,
                ..Default::default()
            },
            source: None,
        }],
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
        alignment: Some(alignment),
    }
}
