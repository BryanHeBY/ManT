//! Exact soft seams, alignment and decoration through both reading consumers.
use crate::table_matrix::*;
use mant_ui::DocumentView;
use serde_json::json;

#[test]
fn exact_word_widths_do_not_add_blank_rows_or_shift_cell_alignment() {
    let mut configurations = 0;
    for word in ["aaa", "long", "fives"] {
        for separator in [" ", "   ", "\t"] {
            let body = format!("{word}{separator}{word}{separator}{word}");
            for width in [3, 4, 5, 6] {
                for alignment in ["left", "center", "right"] {
                    for kind in ["text", "strong", "link"] {
                        let children = match kind {
                            "strong" => {
                                json!([{"type":"strong","children":[{"type":"text","value":body}]}])
                            }
                            "link" => {
                                json!([{"type":"link","target":{"kind":"external","uri":"https://example.test"},"children":[{"type":"text","value":body}]}])
                            }
                            _ => json!([{"type":"text","value":body}]),
                        };
                        let value = restored(&query(json!({"type":"table","alignment":"left",
                            "borders":{"outer":"single"},"rows":[{"cells":[{
                                "preferredWidthColumns":width,"alignment":alignment,
                                "blocks":[{"type":"paragraph","children":children}]}]}]})));
                        let slot = width.max(word.len());
                        let extra = match alignment {
                            "center" => (slot - word.len()) / 2,
                            "right" => slot - word.len(),
                            _ => 0,
                        };
                        let content = format!(
                            "│ {}{word}{} │",
                            " ".repeat(extra),
                            " ".repeat(slot - extra - word.len())
                        );
                        let expected = [
                            format!("┌{}┐", "─".repeat(slot + 2)),
                            content.clone(),
                            content.clone(),
                            content,
                            format!("└{}┘", "─".repeat(slot + 2)),
                        ];
                        for (open, close) in [
                            ("", ""),
                            ("\x1b[1m", "\x1b[0m"),
                            ("\x1b]8;;https://example.test\x1b\\", "\x1b]8;;\x1b\\"),
                        ] {
                            let text =
                                mant_render::render_query_text_at_width(&value, 40, |_, text| {
                                    format!("{open}{text}{close}")
                                });
                            let plain = text.replace(open, "").replace(close, "");
                            let lines = plain.lines().map(str::to_owned).collect::<Vec<_>>();
                            let (_, _, top, bottom) = frame(&lines, false);
                            assert_eq!(
                                &lines[top..=bottom],
                                expected,
                                "{body:?}/{width}/{alignment}/{kind}"
                            );
                        }
                        let rendered = DocumentView::new(&value).render(40);
                        let lines = rows(&rendered);
                        let (_, _, top, bottom) = frame(&lines, false);
                        assert_eq!(
                            &lines[top..=bottom],
                            expected,
                            "TUI {body:?}/{width}/{alignment}/{kind}"
                        );
                        assert_eq!(rendered.search(word).len(), 3);
                        assert_no_decoration_search(&rendered);
                        configurations += 1;
                    }
                }
            }
        }
    }
    assert_eq!(configurations, 324);
}

#[test]
fn authored_hard_rows_and_nonbreaking_separators_survive_soft_placement() {
    for space in ['\u{a0}', '\u{2007}', '\u{202f}'] {
        let body = format!("AA{space}BBBB");
        let value = query(
            json!({"type":"table","alignment":"left","borders":{"outer":"single"},
            "rows":[{"cells":[{"preferredWidthColumns":2,"blocks":[paragraph(&body)]}]}]}),
        );
        for width in [3, 4, 5, 8, 40] {
            let rendered = DocumentView::new(&value).render(width);
            let text = cli(&value, width);
            assert_eq!(
                text.matches(space).count(),
                1,
                "CLI {body:?}/{width}: {text:?}"
            );
            assert_eq!(
                rows(&rendered).join("\n").matches(space).count(),
                1,
                "TUI {body:?}/{width}"
            );
            assert_eq!(rendered.search(&body).len(), 1, "{body:?}/{width}");
            if width == 40 {
                assert!(text.contains(&body));
                assert!(rows(&rendered).iter().any(|row| row.contains(&body)));
            }
        }
    }
    let value = query(
        json!({"type":"table","alignment":"left","borders":{"outer":"single"},
        "rows":[{"cells":[{"preferredWidthColumns":4,"blocks":[{"type":"paragraph","children":[
            {"type":"text","value":"long word"},{"type":"line-break"},{"type":"line-break"},{"type":"text","value":"here"}]}]}]}]}),
    );
    let expected = [
        "┌──────┐",
        "│ long │",
        "│ word │",
        "│      │",
        "│ here │",
        "└──────┘",
    ];
    for lines in [
        cli(&value, 40)
            .lines()
            .map(str::to_owned)
            .collect::<Vec<_>>(),
        rows(&DocumentView::new(&value).render(40)),
    ] {
        let (_, _, top, bottom) = frame(&lines, false);
        assert_eq!(&lines[top..=bottom], expected);
    }
}
