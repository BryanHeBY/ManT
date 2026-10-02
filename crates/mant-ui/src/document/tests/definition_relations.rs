//! Word ownership and first-row alignment use the same source-neutral policy.

use super::*;
use mant_ir::{DefinitionBodyAlignment, DefinitionLayout, HeadBodyRelation};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    widgets::{Paragraph, Widget},
};

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn query(
    relation: HeadBodyRelation,
    literal: bool,
    multi_head: bool,
    origin: i32,
) -> ResolvedContent {
    let mut query = bundle();
    let mut head = Vec::new();
    if multi_head {
        head.extend([text("HEAD"), Inline::line_break_indented(2)]);
    }
    head.push(Inline::Strong {
        children: vec![text("中e\u{301}")],
    });
    let children = vec![
        Inline::Link {
            target: mant_ir::LinkTarget::External {
                uri: "https://ex.org".into(),
            },
            title: None,
            children: vec![Inline::Code {
                value: "BODY".into(),
            }],
        },
        Inline::line_break_indented(1),
        Inline::Emphasis {
            children: vec![text("CONT")],
        },
    ];
    let layout = LayoutHint {
        indent_columns: -2,
        continuation_indent_columns: 2,
        ..Default::default()
    };
    let body = if literal {
        Block::Preformatted {
            children,
            layout,
            language: None,
            source: None,
        }
    } else {
        Block::Paragraph {
            children,
            layout,
            source: None,
        }
    };
    let document = query.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::DefinitionList {
        declaration_groups: vec![],
        compact: true,
        items: vec![DefinitionItem {
            terms: vec![head],
            description: vec![body],
            entry: None,
            source: None,
            layout: DefinitionLayout {
                head_body_relation: relation,
                body_indent_columns: 12,
                min_term_gap_columns: 2,
                spacing_before_lines: None,
            },
        }],
        layout: LayoutHint {
            indent_columns: origin,
            ..Default::default()
        },
        source: None,
    }];
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&query)).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into();
    assert_eq!(restored.document, query.document);
    restored
}

fn assert_word_cells(
    rendered: &RenderedDocument,
    word: &str,
    width: u16,
    target: Option<&LinkTarget>,
) {
    let hits = rendered.search(word);
    let [hit] = hits.as_slice() else {
        panic!("one word {word}");
    };
    assert_eq!(rendered.link_target_at(hit.row, hit.start_column), target);
    let copy = rendered.selected_text(RenderedSelection {
        anchor: TextPosition {
            row: hit.row,
            column: hit.start_column,
        },
        focus: TextPosition {
            row: hit.row,
            column: hit.end_column - 1,
        },
    });
    assert_eq!(copy, word);
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    let mut column = hit.start_column;
    for glyph in mant_render::cells::graphemes(word) {
        assert_eq!(
            buffer[(column.try_into().unwrap(), hit.row.try_into().unwrap())].symbol(),
            glyph.text()
        );
        column += glyph.columns();
    }
    let style = buffer[(
        hit.start_column.try_into().unwrap(),
        hit.row.try_into().unwrap(),
    )]
        .modifier;
    if word == "中e\u{301}" {
        assert!(style.contains(Modifier::BOLD));
    }
    if word == "CONT" {
        assert!(style.contains(Modifier::ITALIC));
    }
}

fn logical_line<'view>(view: &'view DocumentView, word: &str) -> &'view LogicalLine {
    view.lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.content.contains(word)))
        .unwrap()
}

#[test]
fn native_capacity_and_styled_markdown_seams_keep_cells_copy_and_resize() {
    // These 48 exact sources are a subset of the 75 pristine five-profile
    // capacity fixture in engine's definition_consumers. term_flushln's final
    // minbl/viscol, including equality, controls words independently of style.
    const HEADER: &str =
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";
    for capacity in [3, 4, 5] {
        let head_word = "X".repeat(capacity);
        for head in ["No", "Sy", "Em", "Li"] {
            for body in ["No", "Sy", "Em", "Li"] {
                let source = format!(
                    "{HEADER}.Bl -hang -width 2n\n.It Xo\n.sp\n.{head} {head_word}\n.Xc\n.{body} BODY\n.El\n.Sh NEXT\n.No END\n"
                );
                let native = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
                let wire =
                    serde_json::to_string(&mant_protocol::QueryBundle::from(&native)).unwrap();
                let restored: ResolvedContent =
                    serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
                        .unwrap()
                        .into();
                assert_eq!(native, restored);
                let markdown = mant_codec::encode::render_markdown(&restored);
                let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
                let expected = if capacity < 4 {
                    format!("{head_word} BODY")
                } else {
                    format!("{head_word}BODY")
                };
                for content in [&native, &restored, &imported] {
                    let view = DocumentView::new(content);
                    let first = view.render(20).text;
                    for width in [20, 40, 78, 120, 20] {
                        let rendered = view.render(width);
                        assert_word_cells(&rendered, &head_word, width, None);
                        assert_word_cells(&rendered, "BODY", width, None);
                        let seams = rendered.search(&expected);
                        let [seam] = seams.as_slice() else {
                            panic!("one intact seam: {head}/{body}: {expected}");
                        };
                        let copied = rendered.selected_text(RenderedSelection {
                            anchor: TextPosition {
                                row: seam.row,
                                column: seam.start_column,
                            },
                            focus: TextPosition {
                                row: seam.row,
                                column: seam.end_column - 1,
                            },
                        });
                        assert_eq!(copied, expected);
                        let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
                        let mut buffer = Buffer::empty(area);
                        Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
                        for (word, carrier) in [(&head_word[..], head), ("BODY", body)] {
                            let hit = &rendered.search(word)[0];
                            let modifiers = buffer[(
                                hit.start_column.try_into().unwrap(),
                                hit.row.try_into().unwrap(),
                            )]
                                .modifier;
                            assert_eq!(
                                modifiers.contains(Modifier::BOLD),
                                carrier == "Sy",
                                "{head}/{body}/{carrier}"
                            );
                            assert_eq!(
                                modifiers.contains(Modifier::ITALIC),
                                carrier == "Em",
                                "{head}/{body}/{carrier}"
                            );
                        }
                    }
                    assert_eq!(view.render(20).text, first);
                }
            }
        }
    }
}

#[test]
fn all_shared_policies_keep_native_words_origins_links_and_copy_after_resize() {
    // Constructed public IR proves four independent Shared combinations.
    // It does not claim the retired JoinedNoSpace variant was reachable
    // from native source. Relative child and parent origins compose once.
    for alignment in [
        DefinitionBodyAlignment::AfterTerm,
        DefinitionBodyAlignment::Indented,
    ] {
        for joined in [false, true] {
            let relation = if joined {
                HeadBodyRelation::joined(alignment)
            } else {
                HeadBodyRelation::separated(alignment)
            };
            for literal in [false, true] {
                for multi_head in [false, true] {
                    for origin in [0, 3] {
                        let content = query(relation, literal, multi_head, origin);
                        let before = content.clone();
                        let view = DocumentView::new(&content);
                        let initial = view.render(20).text;
                        let term_indent = usize::from(multi_head) * 2;
                        let logical_head_origin = usize::try_from(origin).unwrap() + term_indent;
                        let logical_continuation_origin = usize::try_from(origin).unwrap() + 13;
                        assert_eq!(
                            logical_line(&view, "中e\u{301}").indent,
                            logical_head_origin
                        );
                        assert_eq!(
                            logical_line(&view, "中e\u{301}").continuation_indent,
                            usize::try_from(origin).unwrap() + 12
                        );
                        assert_eq!(
                            logical_line(&view, "CONT").indent,
                            logical_continuation_origin
                        );
                        let gap = if joined {
                            0
                        } else if alignment == DefinitionBodyAlignment::AfterTerm {
                            2
                        } else {
                            7 - term_indent
                        };
                        for width in [20, 40, 78, 120, 20] {
                            let rendered = view.render(width);
                            let head = rendered.search("中e\u{301}")[0].clone();
                            let body = rendered.search("BODY")[0].clone();
                            assert_eq!(body.row, head.row);
                            // wrap.rs::readable_origins reserves half of a
                            // 20-column viewport for text, so these large
                            // origins reduce before cells are rendered. The
                            // pre-wrap coordinates above remain unchanged.
                            let head_origin = if width == 20 { 0 } else { logical_head_origin };
                            let continuation_origin = if width == 20 {
                                10
                            } else {
                                logical_continuation_origin
                            };
                            assert_eq!(head.start_column, head_origin);
                            assert_eq!(body.start_column, head.start_column + 3 + gap);
                            let continuation = &rendered.search("CONT")[0];
                            assert_eq!(continuation.row, body.row + 1);
                            assert_eq!(continuation.start_column, continuation_origin);
                            assert_word_cells(&rendered, "中e\u{301}", width, None);
                            assert_word_cells(
                                &rendered,
                                "BODY",
                                width,
                                Some(&LinkTarget::External(
                                    ExternalUri::parse("https://ex.org").unwrap(),
                                )),
                            );
                            assert_word_cells(&rendered, "CONT", width, None);
                        }
                        assert_eq!(view.render(20).text, initial);
                        assert_eq!(content, before);
                    }
                }
            }
        }
    }
}

#[test]
fn separate_rows_and_explicit_leading_spacing_prevent_sharing() {
    for literal in [false, true] {
        for spacing in [0, 1] {
            let relation = if spacing == 0 {
                HeadBodyRelation::Separate
            } else {
                HeadBodyRelation::joined(DefinitionBodyAlignment::Indented)
            };
            let mut content = query(relation, literal, false, 3);
            let Block::DefinitionList { items, .. } =
                &mut content.document.as_mut().unwrap().blocks[0]
            else {
                panic!("definition")
            };
            let (Block::Paragraph { layout, .. } | Block::Preformatted { layout, .. }) =
                &mut items[0].description[0]
            else {
                panic!("inline body")
            };
            layout.spacing_before_lines = spacing;
            let view = DocumentView::new(&content);
            let logical_body = logical_line(&view, "BODY");
            assert_eq!(logical_body.indent, 13);
            assert_eq!(
                logical_body.continuation_indent,
                if literal { 13 } else { 15 }
            );
            for width in [20, 40, 78, 120, 20] {
                let rendered = view.render(width);
                let head = &rendered.search("中e\u{301}")[0];
                let body = &rendered.search("BODY")[0];
                assert_eq!(body.row, head.row + 1 + usize::from(spacing));
                // A paragraph keeps its additional continuation displacement;
                // a standalone literal has one code-row origin. The existing
                // narrow-view reduction therefore moves them by 5 or 3 cells.
                let body_origin = if width == 20 {
                    if literal { 10 } else { 8 }
                } else {
                    13
                };
                assert_eq!(body.start_column, body_origin);
                assert_word_cells(
                    &rendered,
                    "BODY",
                    width,
                    Some(&LinkTarget::External(
                        ExternalUri::parse("https://ex.org").unwrap(),
                    )),
                );
            }
        }
    }
}
