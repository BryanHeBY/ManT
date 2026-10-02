//! Actual buffers, selection and activation after exported hard rows are read.
use super::*;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    widgets::{Paragraph, Widget},
};

const HEADER: &str =
    ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn source(container: &str, carrier: &str, word: &str) -> String {
    let target = if carrier == "Lk" {
        " https://ex.org"
    } else {
        ""
    };
    let body = format!(".{carrier}{target} \"{word}\"\n");
    let body = match container {
        "tag" | "hang" => {
            format!(".Bl -{container} -width 8n\n.It Xo\n{body}.Xc\n.No BodyWord\n.El\n")
        }
        "column" => format!(".Bl -column \"xxxxxxxx\" \"xxxx\"\n.It Xo\n{body}.Xc Ta RIGHT\n.El\n"),
        "literal" => format!(".Bd -literal -compact\n{body}.Ed\n"),
        _ => body,
    };
    format!("{HEADER}{body}.Sh NEXT\n.No END\n")
}

fn buffer(rendered: &RenderedDocument, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    buffer
}

fn assert_word(
    rendered: &RenderedDocument,
    width: u16,
    word: &str,
    context: &str,
) -> RenderedSearchMatch {
    let buffer = buffer(rendered, width);
    let hits = rendered
        .search(word)
        .into_iter()
        .filter(|hit| {
            let row: u16 = hit.row.try_into().unwrap();
            let adjacent = |column: usize| {
                u16::try_from(column)
                    .ok()
                    .filter(|column| *column < width)
                    .and_then(|column| buffer[(column, row)].symbol().chars().next())
                    .is_some_and(char::is_alphanumeric)
            };
            !adjacent(hit.end_column) && (hit.start_column == 0 || !adjacent(hit.start_column - 1))
        })
        .collect::<Vec<_>>();
    let [hit] = hits.as_slice() else {
        panic!(
            "one {word} hit at width {width}: {hits:?}\n{context}\n{}",
            rendered.text
        )
    };
    assert!(hit.additional_fragments.is_empty(), "short label {word}");
    assert_eq!(
        buffer[(
            hit.start_column.try_into().unwrap(),
            hit.row.try_into().unwrap()
        )]
            .symbol(),
        &word[..1]
    );
    let selected = rendered.selected_text(RenderedSelection {
        anchor: TextPosition {
            row: hit.row,
            column: hit.start_column,
        },
        focus: TextPosition {
            row: hit.row,
            column: hit.end_column - 1,
        },
    });
    assert_eq!(selected, word);
    let viewport = rendered.viewport_text(hit.row, 1, &[], None, None);
    assert_eq!(viewport.lines.len(), 1);
    assert!(viewport.lines[0].to_string().contains(word));
    hit.clone()
}

fn assert_rendered_carrier(
    view: &DocumentView,
    width: u16,
    container: &str,
    carrier: &str,
    topology: usize,
    markdown: &str,
) {
    let rendered = view.render(width);
    assert!(
        rendered.text.lines.iter().all(|row| {
            let text = row.to_string();
            !text.contains("<br>") && !text.contains("<br />")
        }),
        "{container}/{carrier}/{topology}: {markdown}"
    );
    let label = if topology == 4 { "B" } else { "AFTER" };
    let next = assert_word(&rendered, width, label, markdown);
    if !matches!(container, "literal" | "column") {
        let cells = buffer(&rendered, width);
        let cell = &cells[(
            next.start_column.try_into().unwrap(),
            next.row.try_into().unwrap(),
        )];
        assert_eq!(cell.modifier.contains(Modifier::BOLD), carrier == "Sy");
        assert_eq!(
            cell.modifier.contains(Modifier::ITALIC),
            matches!(carrier, "Em" | "Lk")
        );
    }
    if topology == 2 || topology == 4 {
        let previous = rendered
            .search("A")
            .into_iter()
            .filter(|hit| hit.row < next.row)
            .max_by_key(|hit| hit.row)
            .unwrap();
        assert_eq!(
            next.row - previous.row,
            if topology == 2 { 2 } else { 1 },
            "{container}/{carrier}/{topology}/width={width}: {markdown}"
        );
    }
    if carrier == "Lk" && !matches!(container, "literal" | "column") {
        let target = LinkTarget::External(ExternalUri::parse("https://ex.org").unwrap());
        assert_eq!(
            rendered.link_target_at(next.row, next.start_column),
            Some(&target),
            "{container}/{carrier}/{topology}/width={width}: {markdown}"
        );
    }
}

fn assert_reader_view(content: &ResolvedContent, container: &str, carrier: &str, topology: usize) {
    let markdown = mant_codec::encode::render_markdown_with_options(
        content,
        mant_codec::encode::MarkdownOptions::default(),
    );
    let parsed = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let before = parsed.clone();
    let view = DocumentView::new(&parsed);
    let first = view.render(20).text;
    for width in [20, 40, 78, 120, 20] {
        assert_rendered_carrier(&view, width, container, carrier, topology, &markdown);
        if topology < 2 {
            let mut shortened = parsed.clone();
            assert!(remove_leading_hard_row(first_description_children(
                &mut shortened
            )));
            let shortened = DocumentView::new(&shortened).render(width);
            let full = view.render(width);
            let complete = assert_word(&full, width, "AFTER", &markdown);
            let missing = assert_word(&shortened, width, "AFTER", &markdown);
            assert_eq!(
                full.logical_rows.len(),
                shortened.logical_rows.len() + 1,
                "leading logical row lost: {markdown}"
            );
            // With a narrow bullet/label origin, removing an authored row
            // can make its ten-cell prefix soft-wrap the label instead. At
            // 40+ cells the whole bounded prefix and label fit in this core;
            // the physical hard-row delta is then independently addressable.
            if width >= 40 {
                assert_eq!(
                    complete.row,
                    missing.row + 1,
                    "leading row lost: {markdown}"
                );
            }
        }
    }
    assert_eq!(view.render(20).text, first, "resize changed content");
    assert_eq!(parsed, before, "rendering changed logical content");
}

fn first_description_children(content: &mut ResolvedContent) -> &mut Vec<Inline> {
    fn children(block: &mut Block) -> &mut Vec<Inline> {
        match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => children,
            Block::List { items, .. } => children(&mut items[0].blocks[0]),
            other => panic!("reader container: {other:?}"),
        }
    }
    let section = content
        .document
        .as_mut()
        .unwrap()
        .sections
        .iter_mut()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    children(&mut section.blocks[0])
}

fn remove_leading_hard_row(children: &mut Vec<Inline>) -> bool {
    for (index, child) in children.iter_mut().enumerate() {
        match child {
            Inline::LineBreak { .. } => {
                children.remove(index);
                return true;
            }
            Inline::Text { value } | Inline::Code { value } if value.starts_with('\n') => {
                value.remove(0);
                return true;
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if remove_leading_hard_row(children) {
                    return true;
                }
            }
            Inline::Anchor { .. } => {}
            _ => return false,
        }
    }
    false
}

#[test]
fn hundred_roff_carriers_keep_visible_hard_rows_through_reader_buffer_and_resize() {
    // Byte-identical to engine's 100 complete markdown_hard_rows/cases.json
    // sources; each ran pristine CVS's five profiles before assertions.
    // term.c::ESCAPE_BREAK buffers an ordered hard-row request; semantic
    // wrappers do not turn it into a literal HTML tag.
    let words = [
        r"\&\p AFTER",
        r"\&\p \&\p AFTER",
        r"A\p \&\p AFTER",
        r"AFTER\p",
        r"A\p B",
    ];
    for container in ["paragraph", "tag", "hang", "column", "literal"] {
        for carrier in ["No", "Em", "Sy", "Lk"] {
            for (topology, word) in words.iter().enumerate() {
                let original =
                    mant_loader::load_roff_bytes(source(container, carrier, word).as_bytes())
                        .unwrap();
                let wire =
                    serde_json::to_string(&mant_protocol::QueryBundle::from(&original)).unwrap();
                let content: ResolvedContent =
                    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
                        .unwrap()
                        .into();
                assert_reader_view(&content, container, carrier, topology);
            }
        }
    }
}

#[test]
fn canonical_unicode_rows_keep_two_occurrences_copy_and_safe_activation() {
    let markdown = "# Tool\n\n## TEXT\n\n<br />\n[中e\u{301}🦀](https://example.org)<br>\n<br>\n[中e\u{301}🦀](https://example.org)\n";
    let parsed = mant_loader::load_markdown_text(markdown, Some("unicode.md".into())).unwrap();
    let view = DocumentView::new(&parsed);
    for width in [20, 40, 78, 120, 20] {
        let rendered = view.render(width);
        let hits = rendered.search("中e\u{301}🦀");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[1].row - hits[0].row, 2);
        let buffer = buffer(&rendered, width);
        for hit in &hits {
            assert_eq!(hit.end_column - hit.start_column, 5);
            let row = hit.row.try_into().unwrap();
            let column: u16 = hit.start_column.try_into().unwrap();
            assert_eq!(buffer[(column, row)].symbol(), "中");
            assert_eq!(buffer[(column + 2, row)].symbol(), "e\u{301}");
            assert_eq!(buffer[(column + 3, row)].symbol(), "🦀");
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: hit.row,
                        column: hit.start_column
                    },
                    focus: TextPosition {
                        row: hit.row,
                        column: hit.end_column - 1
                    },
                }),
                "中e\u{301}🦀"
            );
            assert!(
                matches!(rendered.link_target_at(hit.row, hit.start_column), Some(LinkTarget::External(uri))
                if uri.as_str() == "https://example.org")
            );
        }
    }
}

#[test]
fn canonical_prefix_does_not_activate_mixed_html_or_hostile_targets() {
    for suffix in [
        "<script>alert(1)</script>",
        "<a href=\"https://example.org\">literal</a>",
        "<br class=x>",
        "[bad](javascript:alert%281%29)",
        "[bad](file:///etc/passwd)",
    ] {
        let source = format!("# Tool\n\n## TEXT\n\n<br />\n{suffix}\n");
        let parsed = mant_loader::load_markdown_text(&source, None).unwrap();
        let rendered = DocumentView::new(&parsed).render(120);
        assert!(
            rendered.links.is_empty(),
            "hostile target activated: {suffix}"
        );
        if suffix.starts_with('<') {
            assert!(
                !rendered.search("<br />").is_empty(),
                "raw mixed block must remain source"
            );
        }
    }
}

fn mutation_view(children: Vec<Inline>) -> RenderedDocument {
    let mut content = bundle();
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children,
        layout: LayoutHint::default(),
        source: None,
    }];
    DocumentView::new(&content).render(20)
}

#[test]
fn buffer_and_activation_checks_detect_line_font_and_link_mutations() {
    let link = |target: &str| Inline::Link {
        target: mant_ir::LinkTarget::External { uri: target.into() },
        title: None,
        children: vec![Inline::Text {
            value: "LABEL".into(),
        }],
    };
    let children = vec![
        Inline::line_break(),
        Inline::Strong {
            children: vec![link("https://example.org")],
        },
    ];
    let good = mutation_view(children.clone());
    let good_buffer = buffer(&good, 20);
    let mut wrong_row = children.clone();
    wrong_row.remove(0);
    assert_ne!(buffer(&mutation_view(wrong_row), 20), good_buffer);
    let mut wrong_font = children.clone();
    wrong_font[1] = link("https://example.org");
    assert_ne!(buffer(&mutation_view(wrong_font), 20), good_buffer);
    let hit = &good.search("LABEL")[0];
    let expected = good.link_target_at(hit.row, hit.start_column).unwrap();
    let wrong_target = mutation_view(vec![
        Inline::line_break(),
        Inline::Strong {
            children: vec![link("https://wrong.example")],
        },
    ]);
    assert_ne!(
        wrong_target.link_target_at(hit.row, hit.start_column),
        Some(expected)
    );
    for unsafe_target in [
        "https://example.org/\u{1b}",
        "javascript:alert(1)",
        "file:///etc/passwd",
    ] {
        let rejected = mutation_view(vec![Inline::line_break(), link(unsafe_target)]);
        assert_eq!(rejected.search("LABEL").len(), 1, "label disappeared");
        assert!(rejected.links.is_empty(), "activated {unsafe_target:?}");
    }
}
