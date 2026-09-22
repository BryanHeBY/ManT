//! Existing regressions grouped by search behavior; expected values remain independent.
use super::*;

#[test]
fn narrow_view_reduces_only_presentation_indent_and_keeps_link_search_copy_cells() {
    let mut bundle = bundle();
    let target = LinkTarget::Section("options".into());
    bundle.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: vec![crate::test_content::link(
            mant_ir::LinkTarget::Section {
                id: "options".into(),
            },
            None,
            vec![crate::test_content::text("abcdefghijklmnopqrstuvwx")],
        )],
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
            view.link_target_at(&rendered, hit.row, hit.start_column),
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
        children: vec![crate::test_content::text(
            "Use --acls, then repeat --acls.".to_owned(),
        )],
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
            children: vec![crate::test_content::text(text.to_owned())],
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
        children: vec![crate::test_content::text(
            "alpha searchable phrase omega".to_owned(),
        )],
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
        children: vec![crate::test_content::text("Relative inset end".to_owned())],
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
        children: vec![crate::test_content::text("abcdefghijklmnop".to_owned())],
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
        children: vec![crate::test_content::text("supercalifragilistic".to_owned())],
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&bundle).render(10);

    assert_eq!(rendered.search("fragilistic").len(), 1);
}

#[test]
#[allow(clippy::too_many_lines)]
fn profile_glyphs_keep_logical_search_and_link_cells_across_wrapping() {
    // Fixed CVS oracle for `.TH DISPLAY 1`, `.SH TEST`,
    // `left\[em]right`, and `left\~right`: term.c::term_word emits
    // one logical scalar for each escape; ASCII projects them as `--`
    // and a plain space, while UTF-8 prints em dash and NBSP.
    let mut bundle = bundle();
    let left_dash = crate::test_content::text("left");
    let dash = crate::test_content::text("—");
    let dash_atom = match &dash {
        Inline::Text { content } => content.atom,
        _ => unreachable!(),
    };
    let right_dash = crate::test_content::text("right");
    let line_break = crate::test_content::line_break();
    let left_nbsp = crate::test_content::text("left");
    let nbsp = crate::test_content::text("\u{a0}");
    let nbsp_atom = match &nbsp {
        Inline::Text { content } => content.atom,
        _ => unreachable!(),
    };
    let right_nbsp = crate::test_content::text("right");
    let target = LinkTarget::Section("description".to_owned());
    let linked = crate::test_content::link(
        mant_ir::LinkTarget::Section {
            id: "description".into(),
        },
        None,
        vec![dash],
    );
    let document = bundle.document.as_mut().expect("document");
    document.sources[0].identity = SourceIdentity::Anonymous {
        name: "profile-projection".to_owned(),
    };
    document.sections[0].blocks = vec![Block::Paragraph {
        children: vec![
            left_dash, linked, right_dash, line_break, left_nbsp, nbsp, right_nbsp,
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    crate::test_content::sync_document(document);
    for (key, expected, projected) in [(dash_atom, "—", "--"), (nbsp_atom, "\u{a0}", " ")] {
        let atom = &mut document.content_store.atoms[(key.get() - 1) as usize];
        match &mut atom.kind {
            mant_ir::ContentAtomKind::Text {
                text,
                display_override,
            } => {
                assert_eq!(text, expected);
                *display_override = Some(projected.to_owned());
            }
            _ => unreachable!(),
        }
    }

    let view = DocumentView::new(&bundle);
    assert!(
        view.lines
            .iter()
            .any(|line| !line.glyph_projections.is_empty())
    );
    for width in [6, 80] {
        let rendered = view.render(width);
        assert_eq!(rendered.search("left—right").len(), 1, "width={width}");
        assert_eq!(rendered.search("left--right").len(), 0, "width={width}");
        assert_eq!(rendered.search("left\u{a0}right").len(), 1, "width={width}");
        assert_eq!(rendered.search("left right").len(), 0, "width={width}");
    }
    let rendered = view.render(80);
    let rows: Vec<_> = rendered
        .text
        .lines
        .iter()
        .map(ToString::to_string)
        .collect();
    assert!(
        rows.iter().any(|row| row.contains("left--right")),
        "{rows:?}"
    );
    assert!(
        rows.iter().any(|row| row.contains("left right")),
        "{rows:?}"
    );
    let dash_hit = rendered.search("—").pop().expect("logical dash hit");
    assert_eq!(dash_hit.end_column - dash_hit.start_column, 2);
    for column in dash_hit.start_column..dash_hit.end_column {
        assert_eq!(
            view.link_target_at(&rendered, dash_hit.row, column),
            Some(&target)
        );
    }
    assert!(rendered.text.lines[dash_hit.row].spans.iter().any(|span| {
        span.content.contains("--") && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));
    assert_eq!(
        rendered.selected_text(crate::document::RenderedSelection {
            anchor: crate::document::TextPosition {
                row: dash_hit.row,
                column: dash_hit.start_column,
            },
            focus: crate::document::TextPosition {
                row: dash_hit.row,
                column: dash_hit.end_column - 1,
            },
        }),
        "--",
        "visual-cell selection copies the displayed profile glyphs"
    );
}

#[test]
fn projected_scalars_in_one_combining_grapheme_keep_one_visual_hit_region() {
    // Fixed CVS with `.TH X 1`, `.SH NAME`, `X \[em]́ Y` prints
    // `X --<?> Y` in ASCII and `X —́ Y` in UTF-8. Pinned
    // term.c::term_word emits one logical scalar before each ASCII profile
    // projection; the terminal still shapes the joined logical grapheme.
    let mut bundle = bundle();
    let left = crate::test_content::text("X");
    let dash = crate::test_content::text("—");
    let mark = crate::test_content::text("\u{0301}");
    let atom = |inline: &Inline| match inline {
        Inline::Text { content } => content.atom,
        _ => unreachable!(),
    };
    let dash_atom = atom(&dash);
    let mark_atom = atom(&mark);
    let linked = crate::test_content::link(
        mant_ir::LinkTarget::Section {
            id: "description".into(),
        },
        None,
        vec![dash, mark],
    );
    let right = crate::test_content::text("Y");
    let document = bundle.document.as_mut().expect("document");
    document.sources[0].identity = SourceIdentity::Anonymous {
        name: "combining-profile-projection".into(),
    };
    document.sections[0].blocks = vec![Block::Paragraph {
        children: vec![left, linked, right],
        layout: LayoutHint::default(),
        source: None,
    }];
    crate::test_content::sync_document(document);
    for (key, expected, glyphs) in [(dash_atom, "—", "--"), (mark_atom, "\u{0301}", "<?>")] {
        let atom = &mut document.content_store.atoms[(key.get() - 1) as usize];
        let mant_ir::ContentAtomKind::Text {
            text,
            display_override,
        } = &mut atom.kind
        else {
            unreachable!();
        };
        assert_eq!(text, expected);
        *display_override = Some(glyphs.into());
    }
    mant_ir::validate_content_store(&document.content_store).unwrap();

    let view = DocumentView::new(&bundle);
    let target = LinkTarget::Section("description".into());
    for width in [20, 80] {
        let rendered = view.render(width);
        let hit = rendered.search("—́").pop().expect("logical grapheme hit");
        assert_eq!(hit.end_column - hit.start_column, 5, "width={width}");
        assert_eq!(rendered.search("--<?>").len(), 0, "width={width}");
        for column in hit.start_column..hit.end_column {
            assert_eq!(
                view.link_target_at(&rendered, hit.row, column),
                Some(&target)
            );
        }
        assert_eq!(
            rendered.selected_text(crate::document::RenderedSelection {
                anchor: crate::document::TextPosition {
                    row: hit.row,
                    column: hit.start_column,
                },
                focus: crate::document::TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1,
                },
            }),
            "--<?>",
            "visual selection copies the complete projected grapheme"
        );
        assert!(rendered.text.lines[hit.row].to_string().contains("--<?>"));
    }
}
