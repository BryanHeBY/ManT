use super::*;
use mant_protocol::QueryBundle;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

fn contains_multiline_unsupported(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(fields) => {
            (fields.get("type").and_then(serde_json::Value::as_str) == Some("unsupported")
                && fields
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|text| text.contains("LINE_A\nLINE_B")))
                || fields.values().any(contains_multiline_unsupported)
        }
        serde_json::Value::Array(items) => items.iter().any(contains_multiline_unsupported),
        _ => false,
    }
}

#[test]
fn deep_literal_fallback_keeps_authored_rows_through_json_and_terminal_buffer() {
    // CVS mdoc_macro.c enables ROFF_NOFILL for Bd -literal; mdoc_term.c::
    // print_mdoc_node() emits term_newln() for each NODE_NOFILL | NODE_LINE.
    // The exact source was checked with the pinned mandoc -Tutf8 before this
    // assertion: LINE_A and LINE_B occupy distinct hard lines.
    let mut source = String::from(".Dd September 28, 2026\n.Dt REVIEW 7\n.Os\n.Sh DESCRIPTION\n");
    source.push_str(&".Bl -tag -width key\n.It key\n".repeat(36));
    source.push_str(".Bd -literal\nLINE_A\nLINE_B\n.Ed\n");
    source.push_str(&".El\n".repeat(36));

    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).expect("lower deep literal");
    let wire = serde_json::to_string(&QueryBundle::from(&loaded)).expect("serialize query");
    let value = serde_json::from_str(&wire).expect("inspect query JSON");
    assert!(contains_multiline_unsupported(&value), "{wire}");
    let decoded: QueryBundle = serde_json::from_str(&wire).expect("decode query JSON text");
    let query: ResolvedContent = decoded.into();

    for width in [80, 120, 240] {
        let rendered = DocumentView::new(&query).render(width);
        let first = rendered.search("LINE_A");
        let second = rendered.search("LINE_B");
        assert_eq!(first.len(), 1, "width={width}");
        assert_eq!(second.len(), 1, "width={width}");
        let first = &first[0];
        let second = &second[0];
        assert_eq!(second.row, first.row + 1, "width={width}");
        assert_eq!(second.start_column, first.start_column, "width={width}");
        assert_eq!(first.end_column - first.start_column, 6);
        assert_eq!(second.end_column - second.start_column, 6);
        assert_eq!(rendered.text.lines[first.row].to_string().trim(), "LINE_A");
        assert_eq!(rendered.text.lines[second.row].to_string().trim(), "LINE_B");
        assert!(
            rendered
                .text
                .lines
                .iter()
                .all(|line| !line.to_string().contains('�'))
        );

        let selected = rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: first.row,
                column: first.start_column,
            },
            focus: TextPosition {
                row: second.row,
                column: second.end_column - 1,
            },
        });
        assert_eq!(
            selected.lines().map(str::trim).collect::<Vec<_>>(),
            ["LINE_A", "LINE_B"]
        );

        let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
        let mut buffer = Buffer::empty(area);
        Paragraph::new(rendered.text).render(area, &mut buffer);
        for (row, expected) in [(first.row, "LINE_A"), (second.row, "LINE_B")] {
            let row = u16::try_from(row).expect("bounded test document rows");
            let visible = (0..width)
                .map(|column| buffer[(column, row)].symbol())
                .collect::<String>();
            assert_eq!(visible.trim(), expected, "width={width}, row={row}");
        }
    }
}

#[test]
fn unsupported_hard_breaks_do_not_weaken_terminal_control_masking() {
    let mut content = bundle();
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::Unsupported {
        name: None,
        text: "FIRST\nSECOND\u{1b}THIRD".to_owned(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&content).render(80);
    let first = &rendered.search("FIRST")[0];
    let second = &rendered.search("SECOND�THIRD")[0];
    assert_eq!(second.row, first.row + 1);
    assert_eq!(rendered.text.lines[first.row].to_string().trim(), "FIRST");
    assert_eq!(
        rendered.text.lines[second.row].to_string().trim(),
        "SECOND�THIRD"
    );
    assert_eq!(rendered.search("SECOND\u{1b}THIRD").len(), 0);
}
