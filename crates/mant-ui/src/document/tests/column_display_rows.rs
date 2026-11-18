//! Empty native display columns keep their completed row in the real Buffer.

use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

#[test]
fn empty_column_display_anchors_do_not_change_terminal_pixels_or_hard_rows() {
    let header = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";
    // The same fixed-CVS sources are exercised by the engine's
    // column_display_rows.rs; the plain No word leaves the field open.
    for (display, row_delta) in [
        (".D1 \\& Ta THIRD\n", 1),
        (".Dl \\& Ta THIRD\n", 1),
        (".Bd -literal\n\\&\n.Ed\n.Ta THIRD\n", 1),
        (".No \\& Ta THIRD\n", 0),
    ] {
        for width in [18, 78] {
            let mut prior = None;
            for tag in ["", ".Tg Mixed.Target\n"] {
                let source = format!(
                    "{header}.Bl -column aa bb cc\n.It FIRST Ta \\&\n{tag}{display}.El\n.Sh NEXT\n.No SKIP\n"
                );
                let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                let wire =
                    serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
                let query: ResolvedContent =
                    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
                        .unwrap()
                        .into();
                let view = DocumentView::new(&query);
                let rendered = view.render(width);
                let first = &rendered.search("FIRST")[0];
                let third = &rendered.search("THIRD")[0];
                let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
                let mut buffer = Buffer::empty(area);
                Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
                for (hit, word) in [(first, "FIRST"), (third, "THIRD")] {
                    let painted: String = (hit.start_column..hit.end_column)
                        .map(|col| {
                            buffer[(col.try_into().unwrap(), hit.row.try_into().unwrap())].symbol()
                        })
                        .collect();
                    assert_eq!(painted, word, "{source}: width {width}");
                }
                // Even the open-word control stacks at the narrow viewport.
                // The wide rendering distinguishes native closure from
                // viewport fallback, which only changes final placement.
                let visible_delta = if width == 18 { 1 } else { row_delta };
                assert_eq!(
                    third.row - first.row,
                    visible_delta,
                    "{source}: width {width}"
                );
                let terminal = (0..area.height)
                    .map(|row| {
                        (0..width)
                            .map(|col| buffer[(col, row)].symbol())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>();
                if let Some(plain) = &prior {
                    assert_eq!(&terminal, plain, "{source}: width {width}");
                    assert!(view.validate_fragment("Mixed.Target").is_ok(), "{source}");
                    let anchor = rendered.anchor_row("mixed-target").unwrap();
                    assert!(
                        (first.row..=third.row).contains(&anchor),
                        "{source}: width {width}"
                    );
                } else {
                    assert!(view.validate_fragment("Mixed.Target").is_err(), "{source}");
                    prior = Some(terminal);
                }
            }
        }
    }
}
