//! Source-neutral TAB placement: measured columns equal the actual painted rows.
use super::table_frames::{block, cell, data};
use super::{Block, BlockRenderer, Inline, LayoutHint};
use mant_ir::{LinkTarget, TableAlignment, TableBorderStyle, TableBorders};
use unicode_width::UnicodeWidthStr;

const OSC_OPEN: &str = "\x1b]8;;https://example.test\x1b\\";
const OSC_CLOSE: &str = "\x1b]8;;\x1b\\";

fn framed(mut table: Block, origin: i32) -> Block {
    if let Block::Table {
        borders, layout, ..
    } = &mut table
    {
        *borders = TableBorders {
            outer: Some(TableBorderStyle::Single),
            ..TableBorders::default()
        };
        layout.indent_columns = origin;
    }
    table
}

fn rendered(table: &Block, width: usize, paint: usize) -> String {
    let decorate = |_: crate::TextPresentation, source: &str| {
        let (open, close) = match paint {
            1 => ("\x1b[1m", "\x1b[0m"),
            2 => (OSC_OPEN, OSC_CLOSE),
            _ => ("", ""),
        };
        // Match the production decorator's exact authored edge ownership.
        let core = source.trim();
        let start = source.len() - source.trim_start().len();
        format!(
            "{}{open}{core}{close}{}",
            &source[..start],
            &source[start + core.len()..]
        )
    };
    BlockRenderer {
        width: Some(width),
        names: None,
        locations: None,
        decorate: &decorate,
    }
    .render_blocks(std::slice::from_ref(table), 0)
}

fn plain(source: &str) -> String {
    source
        .replace("\x1b[1m", "")
        .replace("\x1b[0m", "")
        .replace(OSC_OPEN, "")
        .replace(OSC_CLOSE, "")
}

fn assert_frame_columns(source: &str) {
    assert!(
        !source.contains('\t'),
        "terminal TAB escaped local planning: {source:?}"
    );
    let widths: Vec<_> = source.lines().map(UnicodeWidthStr::width).collect();
    assert!(
        widths.iter().all(|width| *width == widths[0]),
        "unequal frame columns {widths:?}:\n{source}"
    );
}

#[test]
fn tab_in_a_preferred_width_cell_wraps_before_painting_the_outer_frame() {
    // Pure valid IR: the local TAB advances abc from column 3 to column 8.
    // The seven-column owner therefore wraps at that exact internal seam.
    let mut owner = cell("abc\tdef", None);
    owner.preferred_width_columns = Some(7);
    let table = framed(
        block(vec![data(vec![owner])], Some(TableAlignment::Left)),
        0,
    );
    let expected = "┌─────────┐\n│ abc     │\n│ def     │\n└─────────┘";
    for paint in 0..3 {
        let output = rendered(&table, 20, paint);
        assert_eq!(plain(&output), expected);
        assert_frame_columns(&plain(&output));
    }
}

#[test]
fn tab_phases_origins_alignment_and_styles_share_one_cell_geometry() {
    for prefix in [0, 1, 7, 8, 9] {
        for (source, indent) in [("\ta\t", 0), ("a\t\tb ", 3), ("中e\u{301}\t界", 1)] {
            for origin in [0, 4] {
                for alignment in [
                    TableAlignment::Left,
                    TableAlignment::Center,
                    TableAlignment::Right,
                ] {
                    let mut owner = cell("", Some(alignment));
                    owner.blocks = vec![Block::Paragraph {
                        children: vec![
                            Inline::Text {
                                value: "x".repeat(prefix),
                            },
                            Inline::Strong {
                                children: vec![Inline::Text {
                                    value: source.into(),
                                }],
                            },
                            Inline::Link {
                                target: LinkTarget::External {
                                    uri: "https://example.test".into(),
                                },
                                children: vec![Inline::Text {
                                    value: "\tZ".into(),
                                }],
                                title: None,
                            },
                        ],
                        inline_layout: mant_ir::InlineLayout::default(),
                        layout: LayoutHint {
                            indent_columns: indent,
                            ..LayoutHint::default()
                        },
                        source: None,
                    }];
                    owner.preferred_width_columns = Some(10);
                    let table = framed(
                        block(vec![data(vec![owner])], Some(TableAlignment::Left)),
                        origin,
                    );
                    let before = serde_json::to_string(&table).unwrap();
                    for width in [20, 80] {
                        let expected = rendered(&table, width, 0);
                        assert_frame_columns(&expected);
                        for paint in [1, 2] {
                            let output = rendered(&table, width, paint);
                            assert_eq!(plain(&output), expected);
                            assert_eq!(
                                output.matches(OSC_OPEN).count(),
                                output.matches(OSC_CLOSE).count()
                            );
                            assert_frame_columns(&plain(&output));
                        }
                    }
                    assert_eq!(serde_json::to_string(&table).unwrap(), before);
                }
            }
        }
    }
}

#[test]
fn tabs_remain_local_through_spans_nested_cells_and_stacked_fallback() {
    for span in [1, 2] {
        let mut owner = cell("abc\tdef\tZ", Some(TableAlignment::Center));
        owner.column_span = span;
        owner.preferred_width_columns = Some(7);
        let table = framed(
            block(
                vec![data(vec![cell("LEFT", None), owner, cell("RIGHT", None)])],
                Some(TableAlignment::Left),
            ),
            3,
        );
        for width in [14, 28, 80] {
            let expected = rendered(&table, width, 0);
            assert_frame_columns(&expected);
            for paint in [1, 2] {
                assert_eq!(plain(&rendered(&table, width, paint)), expected);
            }
        }
    }
    let inner = framed(
        block(
            vec![data(vec![cell("a\tb\t", None)])],
            Some(TableAlignment::Left),
        ),
        1,
    );
    let mut owner = cell("", None);
    owner.blocks = vec![inner];
    let table = framed(
        block(
            vec![data(vec![cell("LEFT", None), owner])],
            Some(TableAlignment::Left),
        ),
        2,
    );
    for paint in 0..3 {
        let output = plain(&rendered(&table, 80, paint));
        assert_frame_columns(&output);
    }
}

#[test]
fn declared_fields_keep_tab_tail_significant_after_display_expansion() {
    let table = super::declared_column_table(&[4, 4], &["A\t ", "B"]);
    for paint in 0..3 {
        let output = plain(&rendered(&table, 80, paint));
        assert!(!output.contains('\t'));
        // The fixture declares width four plus a four-column field gap.
        assert_eq!(output, "A        \n        B");
    }
}
