//! Existing regressions grouped by layout behavior; expected values remain independent.
use super::*;

#[test]
fn preformatted_rows_share_one_full_width_surface() {
    let mut builder = DocumentBuilder::new("demo".to_owned(), None);
    builder.inline_lines_with_surface(
        &[
            Inline::Text {
                value: "short".to_owned(),
            },
            Inline::LineBreak,
            Inline::Text {
                value: "longer code".to_owned(),
            },
        ],
        3,
        Style::default().fg(theme::TEXT),
        LineSurface::Code,
    );

    let rows = builder
        .lines
        .iter()
        .flat_map(|line| wrap_line(line, 40))
        .collect::<Vec<_>>();

    assert_eq!(rows.len(), 2);
    assert!(rows[0].to_string().starts_with("   short"));
    assert!(rows[1].to_string().starts_with("   longer code"));
    assert_eq!(UnicodeWidthStr::width(rows[0].to_string().as_str()), 40);
    assert_eq!(UnicodeWidthStr::width(rows[1].to_string().as_str()), 40);
    assert_eq!(rows[0].spans[0].style.bg, None);
    assert_eq!(
        rows[0].spans.last().and_then(|span| span.style.bg),
        Some(theme::SURFACE)
    );
}

#[test]
fn preformatted_character_wrapping_preserves_significant_spaces() {
    let line = LogicalLine::plain(2, "ab  cd", Style::default())
        .surface(LineSurface::Code)
        .wrap_mode(WrapMode::Character);
    let rows = wrap_line(&line, 7);

    assert_eq!(&rows[0].to_string()[..7], "  ab  c");
    assert!(rows[1].to_string().starts_with("  d"));
}

#[test]
fn indented_continuation_without_spacing_follows_its_lead_row() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        paragraph("alternate object database"),
        Block::Paragraph {
            children: vec![Inline::Text {
                value: "Via the alternates mechanism, a repository can inherit objects.".to_owned(),
            }],
            layout: LayoutHint {
                indent_columns: 4,
                spacing_before_lines: 0,
            },
            source: None,
        },
    ];

    let rendered = DocumentView::new(&bundle).render(100);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let term = rows
        .iter()
        .position(|row| row.trim() == "alternate object database")
        .expect("visible glossary term");

    assert!(
        rows.get(term + 1)
            .is_some_and(|row| row.trim_start().starts_with("Via the alternates mechanism")),
        "an indented continuation with zero spacing must occupy the next row: {rows:?}",
    );
}

#[test]
fn wrapped_rows_preserve_their_indent() {
    let line = LogicalLine::plain(3, "abcdefgh", Style::default());
    let rows = wrap_line(&line, 7);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].to_string(), "   abcd");
    assert_eq!(rows[1].to_string(), "   efgh");
}

#[test]
fn wrapping_prefers_word_boundaries() {
    let line = LogicalLine::plain(2, "alpha beta", Style::default());
    let rows = wrap_line(&line, 8);

    assert_eq!(rows[0].to_string(), "  alpha");
    assert_eq!(rows[1].to_string(), "  beta");
}

#[test]
fn code_surfaces_fill_the_document_width_after_the_body_indent() {
    let line = LogicalLine::plain(3, "code", Style::default()).surface(LineSurface::Code);
    let rows = wrap_line(&line, 12);

    assert_eq!(UnicodeWidthStr::width(rows[0].to_string().as_str()), 12);
    assert_eq!(rows[0].spans[0].content, "   ");
    assert_eq!(rows[0].spans[0].style.bg, None);
    assert_eq!(rows[0].spans[1].style.bg, Some(theme::SURFACE));
    assert_eq!(rows[0].spans.last().expect("surface fill").content, "     ");
}

#[test]
fn bullet_lists_share_the_first_row_and_use_a_hanging_indent() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::List {
        kind: ListKind::Bullet,
        start: None,
        compact: true,
        items: vec![ListItem {
            entry: None,
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "alpha beta gamma".to_owned(),
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];

    let rendered = DocumentView::new(&bundle).render(16);
    let rows = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    assert_eq!(rows[1], "   • alpha beta");
    assert_eq!(rows[2], "     gamma");
}

#[test]
fn adjacent_blocks_add_only_explicit_vertical_space() {
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![
        paragraph("before"),
        Block::Preformatted {
            children: vec![Inline::Text {
                value: "display".to_owned(),
            }],
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
        paragraph("after"),
        Block::VerticalSpace {
            lines: 1,
            source: None,
        },
        paragraph("spaced"),
    ];

    let rows = DocumentView::new(&bundle)
        .render(80)
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let before = rows.iter().position(|row| row.contains("before")).unwrap();
    let display = rows.iter().position(|row| row.contains("display")).unwrap();
    let after = rows.iter().position(|row| row.contains("after")).unwrap();
    let spaced = rows.iter().position(|row| row.contains("spaced")).unwrap();

    assert_eq!(display, before + 1);
    assert_eq!(after, display + 1);
    assert_eq!(spaced, after + 2);
}

#[test]
fn ordered_list_markers_saturate_instead_of_overflowing() {
    let mut bundle = bundle();
    let paragraph = |value: &str| Block::Paragraph {
        children: vec![Inline::Text {
            value: value.to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    };
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::List {
        kind: ListKind::Ordered,
        start: Some(u64::MAX),
        compact: true,
        items: vec![
            ListItem {
                entry: None,
                blocks: vec![paragraph("first")],
            },
            ListItem {
                entry: None,
                blocks: vec![paragraph("second")],
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }];

    let output = DocumentView::new(&bundle).render(80).text.to_string();
    assert_eq!(output.matches("18446744073709551615. ").count(), 2);
}
