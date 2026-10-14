//! Source-neutral presentation at marker and definition ownership seams.
use super::super::{
    Block, DefinitionItem, DocumentView, Inline, LayoutHint, LinkTarget, ListItem, ListKind,
    RenderedDocument, RenderedSelection, ResolvedContent, TextPosition, bundle,
};
use mant_ir::{
    DefinitionLayout, DefinitionTerm, HeadBodyRelation, InlineLayout, RowLayoutHint, TextAlignment,
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Modifier,
    widgets::{Paragraph, Widget},
};

fn positioned_paragraph(hint: RowLayoutHint) -> Block {
    Block::Paragraph {
        children: vec![
            Inline::anchor("body-anchor"),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "target".into(),
                },
                title: None,
                children: vec![Inline::Emphasis {
                    children: vec![Inline::Text {
                        value: "BODY".into(),
                    }],
                }],
            },
        ],
        inline_layout: InlineLayout {
            row_hints: (!hint.is_empty()).then_some(hint).into_iter().collect(),
        },
        layout: LayoutHint::default(),
        source: None,
    }
}

fn query(block: Block) -> ResolvedContent {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![block];
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
    let restored: ResolvedContent = serde_json::from_str::<mant_protocol::QueryBundle>(&wire)
        .unwrap()
        .into();
    assert_eq!(restored.document, content.document);
    restored
}

fn origin(alignment: TextAlignment, width: usize, start: usize, length: usize) -> usize {
    let free = width - start - length;
    start
        + match alignment {
            TextAlignment::Left => 0,
            TextAlignment::Center => free / 2,
            TextAlignment::Right => free,
        }
}

fn assert_row(rendered: &RenderedDocument, word: &str, expected: &str, width: u16) {
    let hits = rendered.search(word);
    let hit = &hits[0];
    assert_eq!(hits.len(), 1);
    let expected_column = expected.find(word).unwrap();
    assert_eq!(
        hit.start_column,
        expected[..expected_column].chars().count()
    );
    let area = Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
    let mut buffer = Buffer::empty(area);
    Paragraph::new(rendered.text.clone()).render(area, &mut buffer);
    for (column, character) in expected.chars().enumerate() {
        assert_eq!(
            buffer[(column.try_into().unwrap(), hit.row.try_into().unwrap())].symbol(),
            character.to_string()
        );
    }
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
        word
    );
    let style = buffer[(
        hit.start_column.try_into().unwrap(),
        hit.row.try_into().unwrap(),
    )]
        .modifier;
    assert!(style.contains(if word == "HEAD" {
        Modifier::BOLD
    } else {
        Modifier::ITALIC
    }));
    if word == "BODY" {
        assert_eq!(rendered.anchor_row("body-anchor"), Some(hit.row));
        assert_eq!(
            rendered.link_target_at(hit.row, hit.start_column),
            Some(&LinkTarget::Section("target".into()))
        );
        assert!(
            rendered
                .link_target_at(hit.row, hit.start_column - 1)
                .is_none()
        );
    }
}

fn definition(relation: HeadBodyRelation, alignment: TextAlignment, aligned_head: bool) -> Block {
    Block::DefinitionList {
        compact: true,
        declaration_groups: vec![],
        items: vec![DefinitionItem {
            terms: vec![DefinitionTerm {
                content: vec![Inline::Strong {
                    children: vec![Inline::Text {
                        value: "HEAD".into(),
                    }],
                }],
                inline_layout: InlineLayout {
                    row_hints: vec![RowLayoutHint {
                        first_line_indent_columns: 2,
                        alignment: if aligned_head {
                            alignment
                        } else {
                            TextAlignment::Left
                        },
                        ..Default::default()
                    }],
                },
            }],
            description: vec![positioned_paragraph(RowLayoutHint {
                alignment,
                ..Default::default()
            })],
            head_body_relation: relation,
            layout: DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::AfterTerm,
                ..Default::default()
            },
            entry: None,
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn definition_alignment_uses_final_shared_row_in_cli_and_buffer() {
    for width in [20_u16, 39, 40, 79] {
        for alignment in [
            TextAlignment::Left,
            TextAlignment::Center,
            TextAlignment::Right,
        ] {
            for relation in [
                HeadBodyRelation::Separate,
                HeadBodyRelation::joined(),
                HeadBodyRelation::separated(),
            ] {
                for aligned_head in [false, true] {
                    let content = query(definition(relation, alignment, aligned_head));
                    let rendered = DocumentView::new(&content).render(width);
                    let cli = mant_render::render_query_text_at_width(
                        &content,
                        usize::from(width),
                        |_, value| value.into(),
                    );
                    if relation == HeadBodyRelation::Separate {
                        let head = format!(
                            "{}HEAD",
                            " ".repeat(origin(
                                if aligned_head {
                                    alignment
                                } else {
                                    TextAlignment::Left
                                },
                                usize::from(width),
                                2,
                                4
                            ))
                        );
                        let body = format!(
                            "{}BODY",
                            " ".repeat(origin(alignment, usize::from(width), 4, 4))
                        );
                        assert_row(&rendered, "HEAD", &head, width);
                        assert_row(&rendered, "BODY", &body, width);
                        assert!(cli.ends_with(&format!("{head}\n{body}")));
                    } else {
                        let words = if relation.joins_without_separator() {
                            "HEADBODY"
                        } else {
                            "HEAD BODY"
                        };
                        let row = format!(
                            "{}{words}",
                            " ".repeat(origin(alignment, usize::from(width), 2, words.len()))
                        );
                        assert_row(&rendered, "HEAD", &row, width);
                        assert_row(&rendered, "BODY", &row, width);
                        assert!(cli.ends_with(&row));
                    }
                }
            }
        }
    }
}

#[test]
fn aligned_list_keeps_marker_fixed_and_padding_out_of_search_and_copy() {
    for width in [20_u16, 39, 40, 79] {
        for alignment in [
            TextAlignment::Left,
            TextAlignment::Center,
            TextAlignment::Right,
        ] {
            for delta in [0, 2, 4] {
                for (kind, marker) in [
                    (ListKind::Bullet, "• "),
                    (ListKind::Dash, "- "),
                    (ListKind::Ordered { start: Some(1) }, "1. "),
                ] {
                    let content = query(Block::List {
                        kind,
                        compact: true,
                        items: vec![ListItem {
                            blocks: vec![positioned_paragraph(RowLayoutHint {
                                first_line_indent_columns: delta,
                                alignment,
                                ..Default::default()
                            })],
                            entry: None,
                            source: None,
                            layout: mant_ir::ListItemLayout::default(),
                        }],
                        layout: LayoutHint::default(),
                        source: None,
                    });
                    let marker_width = marker.chars().count();
                    let start = origin(
                        alignment,
                        usize::from(width),
                        marker_width + usize::try_from(delta).unwrap(),
                        4,
                    );
                    let row = format!("{marker}{}BODY", " ".repeat(start - marker_width));
                    let rendered = DocumentView::new(&content).render(width);
                    assert_row(&rendered, "BODY", &row, width);
                    let hit = &rendered.search("BODY")[0];
                    let copied = rendered.selected_text(RenderedSelection {
                        anchor: TextPosition {
                            row: hit.row,
                            column: 0,
                        },
                        focus: TextPosition {
                            row: hit.row,
                            column: usize::from(width) - 1,
                        },
                    });
                    assert_eq!(copied, format!("{marker}BODY"));
                    assert!(
                        mant_render::render_query_text_at_width(
                            &content,
                            usize::from(width),
                            |_, value| value.into()
                        )
                        .ends_with(&row)
                    );
                }
            }
        }
    }
}
