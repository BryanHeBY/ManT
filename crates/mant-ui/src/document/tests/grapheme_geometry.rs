//! Independent terminal-cell oracles, including real Ratatui grapheme output.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

fn document(children: Vec<Inline>) -> ResolvedContent {
    let mut bundle = bundle();
    let doc = bundle.document.as_mut().unwrap();
    doc.sections.clear();
    doc.blocks = vec![Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    }];
    bundle
}

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn buffer(line: &Line<'_>, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, 1);
    let mut buffer = Buffer::empty(area);
    Paragraph::new(line.clone()).render(area, &mut buffer);
    buffer
}

#[test]
fn joined_emoji_search_highlight_copy_and_link_use_real_glyph_cells() {
    let bundle = document(vec![
        text("界"),
        Inline::Link {
            target: mant_ir::LinkTarget::Section {
                id: "destination".into(),
            },
            title: None,
            children: vec![text("👩‍💻")],
        },
        text("Z"),
    ]);
    let rendered = DocumentView::new(&bundle).render(20);
    let emoji = rendered.search("👩‍💻");
    assert_eq!(emoji.len(), 1);
    assert_eq!((emoji[0].start_column, emoji[0].end_column), (2, 4));
    assert!(emoji[0].additional_fragments.is_empty());
    let component = rendered.search("💻");
    assert_eq!((component[0].start_column, component[0].end_column), (2, 4));
    let row = emoji[0].row;
    let z = rendered.search("Z");
    assert_eq!((z[0].start_column, z[0].end_column), (4, 5));
    for column in [2, 3] {
        assert_eq!(
            rendered.link_target_at(row, column),
            Some(&LinkTarget::Section("destination".into()))
        );
        let selection = RenderedSelection::new(TextPosition { row, column });
        assert_eq!(rendered.selected_text(selection), "👩‍💻");
    }
    assert!(rendered.link_target_at(row, 4).is_none());
    let selected = rendered.viewport_text(
        row,
        1,
        &[],
        None,
        Some(RenderedSelection::new(TextPosition { row, column: 3 })),
    );
    let selected = buffer(&selected.lines[0], 20);
    assert_eq!(selected[(2, 0)].symbol(), "👩‍💻");
    assert_eq!(selected[(2, 0)].bg, theme::SELECTED);
    assert_ne!(selected[(4, 0)].bg, theme::SELECTED);
    let highlighted = rendered.highlighted_text(&emoji, Some(0));
    let highlighted = buffer(&highlighted.lines[row], 20);
    assert_eq!(highlighted[(2, 0)].symbol(), "👩‍💻");
    assert_eq!(highlighted[(2, 0)].bg, theme::SEARCH_ACTIVE);
    assert_ne!(highlighted[(4, 0)].bg, theme::SEARCH_ACTIVE);
}

#[test]
fn wrapping_and_one_column_replacement_never_split_a_cluster() {
    for glyph in ["👩‍💻", "🇺🇳", "✈️", "e\u{301}", "界"] {
        let bundle = document(vec![text(&format!("{glyph}Z"))]);
        for width in [1, 2, 3, 20] {
            let rendered = DocumentView::new(&bundle).render(width);
            let found = rendered.search(glyph);
            assert_eq!(found.len(), 1, "{glyph}, width={width}");
            assert!(found[0].additional_fragments.is_empty());
            let line = &rendered.text.lines[found[0].row];
            let actual = buffer(line, width);
            let glyph_width = mant_render::cells::graphemes(glyph)
                .next()
                .unwrap()
                .columns();
            assert_eq!(
                actual[(0, 0)].symbol(),
                if glyph_width > usize::from(width) {
                    "�"
                } else {
                    glyph
                }
            );
            assert!(found[0].end_column <= usize::from(width));
            assert!(
                rendered
                    .text
                    .lines
                    .iter()
                    .all(|line| line.width() <= usize::from(width))
            );
        }
    }
}

#[test]
fn source_style_boundaries_do_not_split_graphemes_or_move_following_links() {
    let bundle = document(vec![
        Inline::Strong {
            children: vec![text("👩")],
        },
        text("‍💻"),
        Inline::Link {
            target: mant_ir::LinkTarget::Section { id: "z".into() },
            title: None,
            children: vec![text("Z")],
        },
    ]);
    let rendered = DocumentView::new(&bundle).render(3);
    let found = rendered.search("Z");
    assert_eq!((found[0].start_column, found[0].end_column), (2, 3));
    assert_eq!(
        rendered.link_target_at(found[0].row, 2),
        Some(&LinkTarget::Section("z".into()))
    );
    let actual = buffer(&rendered.text.lines[found[0].row], 3);
    assert_eq!(actual[(0, 0)].symbol(), "👩‍💻");
    assert!(actual[(0, 0)].modifier.contains(Modifier::BOLD));
    assert_eq!(actual[(2, 0)].symbol(), "Z");
}

#[test]
fn tab_stops_and_arabic_neighbors_follow_terminal_grapheme_charges() {
    for (source, column) in [("👩‍💻\tZ", 8), ("لاZ", 2)] {
        let bundle = document(vec![text(source)]);
        let rendered = DocumentView::new(&bundle).render(20);
        let found = rendered.search("Z");
        assert_eq!(found[0].start_column, column);
        assert_eq!(
            buffer(&rendered.text.lines[found[0].row], 20)[(u16::try_from(column).unwrap(), 0)]
                .symbol(),
            "Z"
        );
    }
}

#[test]
fn table_padding_and_following_cells_share_the_actual_glyph_width() {
    let bundle = mant_loader::load_markdown_text(
        "# Probe\n\n| A | B |\n| --- | --- |\n| لا | Z |\n| 👩‍💻 | X |\n",
        None,
    )
    .unwrap();
    for width in [5, 20, 40] {
        let rendered = DocumentView::new(&bundle).render(width);
        for token in ["Z", "X"] {
            let found = rendered.search(token);
            assert_eq!(found[0].start_column, 4);
            let actual = buffer(&rendered.text.lines[found[0].row], width);
            assert_eq!(actual[(4, 0)].symbol(), token);
        }
    }
}
