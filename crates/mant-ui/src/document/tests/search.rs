//! Existing regressions grouped by search behavior; expected values remain independent.
use super::*;

#[test]
fn narrow_view_reduces_only_presentation_indent_and_keeps_link_search_copy_cells() {
    let mut bundle = bundle();
    let target = LinkTarget::Section("options".into());
    bundle.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Link {
            target: mant_ir::LinkTarget::Section {
                id: "options".into(),
            },
            title: None,
            children: vec![Inline::Text {
                value: "abcdefghijklmnopqrstuvwx".into(),
            }],
        }],
        layout: LayoutHint {
            indent_columns: 4096,
            continuation_indent_columns: 4,
            ..Default::default()
        },
        source: None,
    }];
    let before = bundle.clone();
    let view = DocumentView::new(&bundle);
    for width in [1, 2, 40, 80, 120] {
        let rendered = view.render(width);
        let hits = rendered.search("abcdefghijklmnopqrstuvwx");
        assert_eq!(hits.len(), 1, "width={width}");
        let hit = &hits[0];
        if width >= 40 {
            assert!(
                hit.additional_fragments.len() <= 1,
                "readable area collapsed: width={width}"
            );
        }
        let (end_row, end_column) = hit
            .additional_fragments
            .last()
            .map_or((hit.row, hit.end_column), |f| (f.row, f.end_column));
        let selected = rendered.selected_text(crate::document::RenderedSelection {
            anchor: crate::document::TextPosition {
                row: hit.row,
                column: hit.start_column,
            },
            focus: crate::document::TextPosition {
                row: end_row,
                column: end_column - 1,
            },
        });
        // Selection is explicitly visual-cell copying: continuation padding
        // and visual newlines are retained, not claimed to be source export.
        assert_eq!(
            selected.lines().map(str::trim_start).collect::<String>(),
            "abcdefghijklmnopqrstuvwx"
        );
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&target)
        );
        assert_eq!(view.render(width).text, rendered.text);
    }
    assert_eq!(bundle, before, "resize must not alter logical IR");
}

#[test]
fn rendered_search_finds_literal_options_and_decorates_every_match() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "Use --acls, then repeat --acls.".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(42);

    let matches = rendered.search("--ACLS");
    let highlighted = rendered.highlighted_text(&matches, Some(1));

    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].row, 1);
    assert!(
        highlighted.lines[1]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_MATCH))
    );
    assert!(
        highlighted.lines[1]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
    );

    let viewport = rendered.viewport_text(1, 1, &matches, Some(1), None);
    assert_eq!(viewport.lines.len(), 1);
    assert!(
        viewport.lines[0]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_MATCH))
    );
    assert!(
        viewport.lines[0]
            .spans
            .iter()
            .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
    );
}

#[test]
fn unicode_search_uses_the_same_transform_before_and_after_visual_wrapping() {
    for (text, query) in [
        ("ΟΣ", "ΟΣ"),
        ("ΟΣ", "οσ"),
        ("ος", "ος"),
        ("İstanbul", "İSTANBUL"),
        ("İstanbul", "i\u{307}stanbul"),
    ] {
        let mut bundle = bundle();
        bundle.document.as_mut().unwrap().sections[0].blocks = vec![Block::Preformatted {
            children: vec![Inline::Text {
                value: text.to_owned(),
            }],
            language: None,
            layout: LayoutHint::default(),
            source: None,
        }];
        let view = DocumentView::new(&bundle);
        for width in [1, 2, 80] {
            let rendered = view.render(width);
            let matches = rendered.search(query);
            assert_eq!(matches.len(), 1, "{text:?}/{query:?}, width {width}");
            let hit = &matches[0];
            let columns = hit.end_column - hit.start_column
                + hit
                    .additional_fragments
                    .iter()
                    .map(|f| f.end_column - f.start_column)
                    .sum::<usize>();
            assert_eq!(
                columns,
                text.width(),
                "fold expansion changed the highlight range"
            );
        }
    }
}

#[test]
fn search_matches_one_logical_phrase_across_soft_wrapping() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "alpha searchable phrase omega".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(15);

    let matches = rendered.search("searchable phrase");
    let highlighted = rendered.highlighted_text(&matches, Some(0));

    assert_eq!(matches.len(), 1);
    assert!(!matches[0].additional_fragments.is_empty());
    let highlighted_rows = highlighted
        .lines
        .iter()
        .filter(|line| {
            line.spans
                .iter()
                .any(|span| span.style.bg == Some(theme::SEARCH_ACTIVE))
        })
        .count();
    assert_eq!(highlighted_rows, 2);
}

#[test]
fn search_preserves_a_space_wrapped_exactly_after_the_row_boundary() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "Relative inset end".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(11);

    let matches = rendered.search("Relative inset end");

    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].additional_fragments.len(), 2);
}

#[test]
fn character_wrapped_code_remains_contiguous_for_search() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Preformatted {
        children: vec![Inline::Text {
            value: "abcdefghijklmnop".to_owned(),
        }],
        language: None,
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&bundle);
    for width in [1, 2, 4, 10] {
        let rendered = view.render(width);
        let matches = rendered.search("ghijkl");

        assert_eq!(matches.len(), 1, "lost code at width {width}");
        assert!(
            !matches[0].additional_fragments.is_empty(),
            "code did not wrap at width {width}"
        );
    }
}

#[test]
fn forced_word_splitting_does_not_insert_a_search_space() {
    let mut bundle = bundle();
    bundle.document.as_mut().expect("document").sections[0].blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "supercalifragilistic".to_owned(),
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(10);

    assert_eq!(rendered.search("fragilistic").len(), 1);
}
