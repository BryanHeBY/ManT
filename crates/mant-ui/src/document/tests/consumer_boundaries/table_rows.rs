//! Early and late column fallback preserve the same physical cell receipts.

use super::super::*;
use super::fixtures::{
    assert_word, cell, clipped, copy_cells, json_content, linked, literal_nodes, paragraph_nodes,
    table, text,
};
use mant_ir::ColumnPreferences;

#[derive(Clone, Copy, Debug)]
enum Middle {
    Absent,
    Paragraph,
    ParagraphText,
    Literal,
    LiteralAnchor,
    LiteralText,
    Spaces,
    Hard,
    Gap,
}

impl Middle {
    fn blocks(self) -> Vec<Block> {
        match self {
            Self::Absent => vec![],
            Self::Paragraph => vec![paragraph_nodes(vec![])],
            Self::ParagraphText => vec![paragraph_nodes(vec![text("")])],
            Self::Literal => vec![literal_nodes(vec![])],
            Self::LiteralAnchor => vec![literal_nodes(vec![Inline::anchor("middle")])],
            Self::LiteralText => vec![literal_nodes(vec![Inline::anchor("middle"), text("")])],
            Self::Spaces => vec![paragraph_nodes(vec![Inline::anchor("middle"), text("  ")])],
            Self::Hard => vec![paragraph_nodes(vec![
                Inline::anchor("middle"),
                Inline::LineBreak {},
            ])],
            Self::Gap => vec![Block::VerticalSpace {
                lines: 1,
                source: None,
            }],
        }
    }

    const fn fallback_rows(self, closed: bool) -> usize {
        match self {
            Self::LiteralText => {
                if closed {
                    0
                } else {
                    1
                }
            }
            Self::Spaces | Self::Gap => 1,
            Self::Hard => {
                if closed {
                    2
                } else {
                    1
                }
            }
            _ => 0,
        }
    }

    const fn normal_distance(self, closed: bool) -> usize {
        match self {
            Self::Hard => {
                if closed {
                    2
                } else {
                    1
                }
            }
            Self::Gap => 1,
            _ => {
                if closed {
                    1
                } else {
                    0
                }
            }
        }
    }
}

fn table_content(
    middle: Middle,
    closed: bool,
    preferences: ColumnPreferences,
    origin: i32,
) -> ResolvedContent {
    json_content(vec![table(
        vec![
            cell(
                vec![paragraph_nodes(vec![Inline::anchor("first"), text("A")])],
                false,
            ),
            cell(middle.blocks(), closed),
            cell(
                vec![paragraph_nodes(vec![Inline::anchor("last"), linked("C")])],
                false,
            ),
        ],
        preferences,
        origin,
    )])
}

fn assert_fallback(
    content: &ResolvedContent,
    middle: Middle,
    closed: bool,
    origin: i32,
    width: u16,
) -> Vec<String> {
    let rendered = DocumentView::new(content).render(width);
    let prefix = clipped(origin).min(usize::from(width) - (usize::from(width) / 2).min(16));
    let mut expected = vec![format!("{}A", " ".repeat(prefix))];
    for _ in 0..middle.fallback_rows(closed) {
        let author = if matches!(middle, Middle::Spaces) {
            format!("{}  ", " ".repeat(prefix))
        } else {
            String::new()
        };
        expected.push(author);
    }
    expected.push(format!("{}C", " ".repeat(prefix)));
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    assert_eq!(rows, expected, "{middle:?}/{closed}/{origin}/{width}");
    assert_eq!(rendered.row_count, expected.len());
    let first = assert_word(&rendered, "A", false);
    let last = assert_word(&rendered, "C", true);
    assert_eq!((first.row, last.row), (0, expected.len() - 1));
    assert_eq!(rendered.anchor_row("first"), Some(first.row));
    assert_eq!(rendered.anchor_row("last"), Some(last.row));
    for row in 1..last.row {
        let expected = if matches!(middle, Middle::Spaces) {
            format!("{}  ", " ".repeat(prefix))
        } else {
            String::new()
        };
        assert_eq!(
            copy_cells(&rendered, row, 0, usize::from(width) - 1),
            expected
        );
    }
    assert_middle_anchor(&rendered, middle, closed, last.row);
    rows
}

fn assert_middle_anchor(rendered: &RenderedDocument, middle: Middle, closed: bool, last: usize) {
    let row = match middle {
        Middle::LiteralAnchor => {
            if closed {
                0
            } else {
                last
            }
        }
        Middle::LiteralText => usize::from(!closed),
        Middle::Spaces | Middle::Hard => 1,
        _ => return,
    };
    assert_eq!(
        rendered.anchor_row("middle"),
        Some(row),
        "{middle:?}/{closed}"
    );
}

#[test]
fn late_and_early_column_fallbacks_preserve_identical_empty_cell_receipts() {
    // Change only preferences: both fallbacks consume these same three cells.
    // The wide late case reaches place_at's generated-padding bound after
    // wrapping; 257 declarations reject before wrapping. Neither may cache
    // a synthetic empty row as accepted content on the other route.
    for middle in [
        Middle::Absent,
        Middle::Paragraph,
        Middle::ParagraphText,
        Middle::Literal,
        Middle::LiteralAnchor,
        Middle::LiteralText,
        Middle::Spaces,
        Middle::Hard,
        Middle::Gap,
    ] {
        for closed in [false, true] {
            for origin in [-3, 0, 4096] {
                let late = table_content(
                    middle,
                    closed,
                    ColumnPreferences {
                        widths: vec![4096, 4],
                        ..Default::default()
                    },
                    origin,
                );
                let early = table_content(
                    middle,
                    closed,
                    ColumnPreferences {
                        widths: vec![4; 257],
                        ..Default::default()
                    },
                    origin,
                );
                for width in [12, 80, u16::MAX] {
                    let late = assert_fallback(&late, middle, closed, origin, width);
                    let early = assert_fallback(&early, middle, closed, origin, width);
                    assert_eq!(late, early, "{middle:?}/{closed}/{origin}/{width}");
                }
            }
        }
    }
}

#[test]
fn normal_column_placement_keeps_empty_rows_distinct_from_author_spaces() {
    for middle in [
        Middle::Absent,
        Middle::Paragraph,
        Middle::ParagraphText,
        Middle::Literal,
        Middle::LiteralAnchor,
        Middle::LiteralText,
        Middle::Spaces,
        Middle::Hard,
        Middle::Gap,
    ] {
        for closed in [false, true] {
            let content = table_content(
                middle,
                closed,
                ColumnPreferences {
                    widths: vec![4, 4],
                    ..Default::default()
                },
                0,
            );
            let rendered = DocumentView::new(&content).render(80);
            let first = assert_word(&rendered, "A", false);
            let last = assert_word(&rendered, "C", true);
            let distance = middle.normal_distance(closed);
            assert_eq!(
                (first.row, last.row, last.start_column),
                (0, distance, 12),
                "{middle:?}/{closed}"
            );
            assert_eq!(rendered.row_count, distance + 1, "{middle:?}/{closed}");
            assert_eq!(rendered.anchor_row("last"), Some(last.row));
            if matches!(middle, Middle::Spaces) {
                assert_eq!(copy_cells(&rendered, 0, 6, 7), "  ");
            }
            if matches!(
                middle,
                Middle::LiteralAnchor | Middle::LiteralText | Middle::Spaces | Middle::Hard
            ) {
                assert_eq!(rendered.anchor_row("middle"), Some(0));
            }
            for row in 1..last.row {
                assert_eq!(copy_cells(&rendered, row, 0, 79), "");
            }
        }
    }
}
