//! Clickable reference labels and copy positions after authored leading rows.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

fn children(query: &mut ResolvedContent) -> &mut Vec<Inline> {
    let block = &mut query.document.as_mut().unwrap().sections[0].blocks[0];
    let block = match block {
        Block::List { items, .. } => &mut items[0].blocks[0],
        block => block,
    };
    let Block::Paragraph { children, .. } = block else {
        panic!("reference label paragraph")
    };
    children
}

fn assert_view(query: &ResolvedContent, control: &ResolvedContent, label: &str, width: u16) {
    let rendered = DocumentView::new(query).render(width);
    let baseline = DocumentView::new(control).render(width);
    let hits = rendered.search(label);
    let [hit] = hits.as_slice() else {
        panic!("one label: {hits:?}")
    };
    assert_eq!(hit.row, baseline.search(label)[0].row + 1);
    let target = LinkTarget::External(ExternalUri::parse("https://example.org").unwrap());
    assert_eq!(
        rendered.link_target_at(hit.row, hit.start_column),
        Some(&target)
    );
    let selection = rendered.selected_text(RenderedSelection {
        anchor: TextPosition {
            row: hit.row,
            column: hit.start_column,
        },
        focus: TextPosition {
            row: hit.row,
            column: hit.end_column - 1,
        },
    });
    assert_eq!(selection, label);
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text).render(area, &mut buffer);
    assert_eq!(
        buffer[(
            hit.start_column.try_into().unwrap(),
            hit.row.try_into().unwrap()
        )]
            .symbol(),
        &label[..1]
    );
}

#[test]
fn leading_rows_keep_reference_labels_clickable_after_json_and_resize() {
    for (reference, label) in [
        ("[hello][ref]", "hello"),
        ("[ref][]", "ref"),
        ("[ref]", "ref"),
    ] {
        for before in [false, true] {
            for list in [false, true] {
                for newline in ["\n", "\r\n", "\r"] {
                    let definitions = "[ref]: https://example.org \"title\"\n\n";
                    let (prefix, suffix) = if before {
                        (definitions, "")
                    } else {
                        ("", definitions)
                    };
                    let body = if list {
                        format!("- <br />\n  {reference}\n")
                    } else {
                        format!("<br />\n{reference}\n")
                    };
                    let source = format!("# TEST\n\n## DESCRIPTION\n\n{prefix}{body}\n{suffix}")
                        .replace('\n', newline);
                    let query = mant_loader::load_markdown_text(&source, None).unwrap();
                    let json =
                        serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
                    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
                    let query: ResolvedContent = decoded.into();
                    let mut control = query.clone();
                    assert!(matches!(
                        children(&mut control).remove(0),
                        Inline::LineBreak { .. }
                    ));
                    for width in [20, 80, 120, 20] {
                        assert_view(&query, &control, label, width);
                    }
                }
            }
        }
    }
}
