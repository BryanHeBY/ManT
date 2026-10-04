//! Hard row origins remain presentation facts across source, wire, and visual consumers.
use super::*;

fn layout(rows: &[(u32, i32)]) -> mant_ir::InlineLayout {
    mant_ir::InlineLayout {
        row_hints: rows
            .iter()
            .map(|&(row, indent_columns)| mant_ir::RowLayoutHint {
                row,
                indent_columns,
            })
            .collect(),
    }
}

#[test]
fn native_definition_origins_survive_query_json_and_visual_copy() {
    // Exact inputs were run with registered pristine CVS ASCII, UTF-8 and
    // lint before adding this assertion. print_mdoc_node() handles NODE_LINE
    // before saving geometry; roff requests return without restoring offset,
    // while outer non-text scopes restore it before the last buffered word
    // flushes (mdoc_term.c:314-330, 393-397, 437-439).
    for (head, expected) in [
        (
            ".No Beta\n.No Gamma\n.No Delta\n",
            vec![("Alpha", 0), ("Beta", 6), ("Gamma", 6), ("Delta", 0)],
        ),
        (".No Beta\n", vec![("Alpha", 0), ("Beta", 0)]),
    ] {
        let source = format!(
            ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n.Bl -tag -width 4n\n.It Xo\n.No Alpha\n.nf\n{head}.Xc\n.No BodyWord\n.El\n"
        );
        let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let content: ResolvedContent = decoded.into();
        let plain = mant_render::render_query_text(&content);
        for (word, origin) in &expected {
            let row = plain
                .lines()
                .find(|row| row.trim() == *word)
                .expect("CLI row");
            assert_eq!(row, format!("{}{word}", " ".repeat(*origin)), "{plain}");
        }
        let body = plain.lines().find(|row| row.trim() == "BodyWord").unwrap();
        assert_eq!(body, "      BodyWord", "{plain}");
        let markdown = mant_codec::encode::render_markdown(&content);
        if expected.len() > 2 {
            assert!(
                markdown.contains("&#160;&#160;&#160;&#160;&#160;&#160;Beta"),
                "{markdown}"
            );
        }
        let view = DocumentView::new(&content);
        for width in [20, 80, 120] {
            let rendered = view.render(width);
            let area =
                ratatui::layout::Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
            let mut buffer = ratatui::buffer::Buffer::empty(area);
            ratatui::widgets::Widget::render(
                ratatui::widgets::Paragraph::new(rendered.text.clone()),
                area,
                &mut buffer,
            );
            let alpha = &rendered.search("Alpha")[0];
            for (word, origin) in &expected {
                let hit = &rendered.search(word)[0];
                assert_eq!(
                    hit.start_column,
                    alpha.start_column + origin,
                    "{word}, width={width}"
                );
                assert_eq!(
                    buffer[(
                        u16::try_from(hit.start_column).unwrap(),
                        u16::try_from(hit.row).unwrap()
                    )]
                        .symbol(),
                    &word[..1],
                );
                let copy = rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: hit.row,
                        column: 0,
                    },
                    focus: TextPosition {
                        row: hit.row,
                        column: hit.end_column - 1,
                    },
                });
                // Row hints no longer contribute copyable text; the existing
                // structural section origin remains part of visual copying.
                assert_eq!(
                    copy,
                    format!(
                        "{}{word}",
                        " ".repeat(hit.start_column.saturating_sub(*origin))
                    )
                );
            }
        }
    }
}

#[test]
fn first_named_origin_preserves_body_addresses_across_wire_and_visual_consumers() {
    // The exact first_named_origin input ran pristine CVS ASCII, UTF-8 and
    // HTML before these assertions. mdoc_term.c::print_mdoc_node saves and
    // restores a non-text scope's offset; roff_term.c::roff_term_pre_br runs
    // the BRIND transition; term.c::term_fill/term_field decide graph padding.
    // This label has relative origin zero. Only its BODY link has origin six.
    let source = ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh OPTIONS\n.Bl -tag -width 4n\n.It Xo\n.br\n.Fl alpha\n.Xc\n.Lk https://example.org BodyWord\n.El\n";
    let loaded = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&loaded)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
    let content: ResolvedContent = decoded.into();
    let document = content.document.as_ref().unwrap();
    let entries = mant_ir::content_entries(&document.sections[1].blocks);
    let entry = entries
        .iter()
        .find(|entry| entry.names().iter().any(|name| name == "-alpha"))
        .unwrap();
    let owner = entry.owner();
    let facts = owner.facts().unwrap();
    let binding = facts
        .name_bindings
        .iter()
        .find(|binding| facts.names[binding.name] == "-alpha")
        .unwrap();
    assert_eq!(binding.occurrences.len(), 1);
    let occurrence = &binding.occurrences[0];
    assert_eq!(
        mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
        "-alpha"
    );
    // Fl emits the hyphen before its child (mdoc_term.c::termp_fl_pre).
    // One name can therefore span multiple original leaves in the same term.
    let mut next_scalar = 0;
    for part in &occurrence.parts {
        let projected = mant_ir::project_content_slice(owner, part).unwrap();
        assert_eq!(projected.root, mant_ir::EntryInlineRoot::Term { index: 0 });
        assert_eq!(projected.chars.start, next_scalar);
        next_scalar = projected.chars.end;
    }
    assert_eq!(next_scalar, 6);
    let mant_ir::EntryOwner::Definition(item) = owner else {
        panic!("native tag owner");
    };
    assert_eq!(mant_ir::inline_plain_text(&item.terms[0]), "-alpha");
    assert_eq!(item.terms[0].inline_layout.row_indent(0), 0);
    assert_first_named_explanation(&content, &entry.content());
    assert_first_named_artifact(&content, facts);
    let view = DocumentView::new(&content);
    for width in [40, 80, 120] {
        assert_first_named_visual(&view.render(width), width, facts.id.as_str());
    }
}

fn assert_first_named_explanation(content: &ResolvedContent, original: &Block) {
    let explanation = mant_query::select_explanation(content, "-alpha").unwrap();
    let wire = serde_json::to_string(&explanation).unwrap();
    let restored: mant_protocol::QueryExplanation = serde_json::from_str(&wire).unwrap();
    assert_eq!(restored, explanation);
    let evidence = &restored.evidence[0];
    let Some(mant_protocol::ExplanationContent::Entry { block }) = &evidence.content else {
        panic!("complete source owner");
    };
    assert_eq!(block, original);
    let entry = evidence.entry.as_ref().unwrap();
    let binding = entry
        .name_bindings
        .iter()
        .find(|binding| entry.names[binding.name_index as usize] == "-alpha")
        .unwrap();
    assert_eq!(binding.occurrences.len(), 1);
    let mut selected = String::new();
    let mut next_scalar = 0;
    for range in &binding.occurrences[0].content {
        assert!(
            matches!(range, mant_protocol::ExplanationContentRange::DefinitionTerm {
            path, item_index: 0, term_index: 0, ..
        } if path.is_empty())
        );
        assert_eq!(range.char_range().start, next_scalar);
        next_scalar = range.char_range().end;
        let text = range.resolve(block).unwrap().safe_text();
        selected.extend(
            text.chars()
                .skip(range.char_range().start)
                .take(range.char_range().len()),
        );
    }
    assert_eq!(next_scalar, 6);
    assert_eq!(selected, "-alpha");
}

fn assert_first_named_artifact(content: &ResolvedContent, facts: &EntryFacts) {
    let artifact = mant_codec::encode::render_addressable_markdown(content);
    let mapped = artifact
        .nodes()
        .iter()
        .find(|mapped| {
            matches!(
                mapped.node(),
                mant_codec::encode::MarkdownNode::DocumentEntry { names, .. }
                    if names.iter().any(|name| name == "-alpha")
            )
        })
        .unwrap();
    let mant_codec::encode::MarkdownNode::DocumentEntry { owner, .. } = mapped.node() else {
        unreachable!("selected entry");
    };
    assert!(std::ptr::eq(owner.facts().unwrap(), facts));
    let rendered = &artifact.text()[mapped.range()];
    assert!(rendered.contains("-alpha"), "{rendered}");
    assert!(rendered.contains("BodyWord"), "{rendered}");
    assert!(rendered.contains("https://example.org"), "{rendered}");
}

fn assert_first_named_visual(rendered: &RenderedDocument, width: u16, owner_id: &str) {
    let alpha = &rendered.search("-alpha")[0];
    let body = &rendered.search("BodyWord")[0];
    assert_eq!(body.start_column, alpha.start_column + 6);
    assert_eq!(body.row, alpha.row + 1);
    assert_eq!(rendered.anchor_row(owner_id), Some(alpha.row));
    assert_eq!(
        rendered.link_target_at(body.row, body.start_column),
        Some(&LinkTarget::External(
            ExternalUri::parse("https://example.org").unwrap()
        ))
    );
    assert_eq!(
        rendered.link_target_at(body.row, body.start_column - 1),
        None
    );
    let area = ratatui::layout::Rect::new(0, 0, width, u16::try_from(rendered.row_count).unwrap());
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    ratatui::widgets::Widget::render(
        ratatui::widgets::Paragraph::new(rendered.text.clone()),
        area,
        &mut buffer,
    );
    for (hit, word) in [(alpha, "-alpha"), (body, "BodyWord")] {
        assert_eq!(
            buffer[(
                u16::try_from(hit.start_column).unwrap(),
                u16::try_from(hit.row).unwrap()
            )]
                .symbol(),
            &word[..1]
        );
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: hit.start_column,
                },
                focus: TextPosition {
                    row: hit.row,
                    column: hit.end_column - 1,
                },
            }),
            word
        );
    }
}

#[test]
fn structural_row_origins_leave_link_search_and_copy_coordinates_consistent() {
    let children = vec![
        Inline::Text {
            value: "Alpha".into(),
        },
        Inline::line_break(),
        Inline::Strong {
            children: vec![
                Inline::anchor_at("beta", None),
                Inline::Link {
                    target: mant_ir::LinkTarget::Section {
                        id: "destination".into(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "Beta".into(),
                    }],
                },
            ],
        },
        Inline::line_break(),
        Inline::Text {
            value: "Gamma".into(),
        },
        Inline::line_break(),
        Inline::Text {
            value: "Delta".into(),
        },
    ];
    assert_eq!(mant_ir::inline_scalar_len(&children), 22);
    for block in [
        Block::Paragraph {
            inline_layout: layout(&[(1, 6), (2, 6)]),
            children: children.clone(),
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Preformatted {
            inline_layout: layout(&[(1, 6), (2, 6)]),
            children: children.clone(),
            language: None,
            layout: LayoutHint::default(),
            source: None,
        },
        Block::DefinitionList {
            compact: true,
            declaration_groups: vec![],
            layout: LayoutHint::default(),
            source: None,
            items: vec![DefinitionItem {
                head_body_relation: mant_ir::HeadBodyRelation::Separate,
                source: None,
                entry: None,
                layout: mant_ir::DefinitionLayout::default(),
                terms: vec![mant_ir::DefinitionTerm {
                    content: children.clone(),
                    inline_layout: layout(&[(1, 6), (2, 6)]),
                }],
                description: vec![],
            }],
        },
    ] {
        let mut content = bundle();
        content.document.as_mut().unwrap().sections[0].blocks = vec![block];
        let wire = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
        let decoded: mant_protocol::QueryBundle = serde_json::from_str(&wire).unwrap();
        let content: ResolvedContent = decoded.into();
        let view = DocumentView::new(&content);
        for width in [40, 120] {
            let rendered = view.render(width);
            let beta = &rendered.search("Beta")[0];
            let gamma = &rendered.search("Gamma")[0];
            let delta = &rendered.search("Delta")[0];
            assert_eq!(beta.start_column, delta.start_column + 6, "width={width}");
            assert_eq!(gamma.start_column, delta.start_column + 6, "width={width}");
            assert_eq!(
                rendered.link_target_at(beta.row, beta.start_column),
                Some(&LinkTarget::Section("destination".into()))
            );
            assert!(rendered.link_target_at(beta.row, 0).is_none());
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: beta.row,
                        column: 0
                    },
                    focus: TextPosition {
                        row: beta.row,
                        column: beta.end_column - 1
                    },
                }),
                format!("{}Beta", " ".repeat(beta.start_column.saturating_sub(6)))
            );
        }
    }
}

#[test]
fn row_origins_compose_before_visible_padding_and_paragraph_continuations() {
    let mut builder = DocumentBuilder::new("origins".into(), None);
    builder.blocks(
        &[Block::Paragraph {
            inline_layout: layout(&[(1, 6)]),
            children: vec![
                Inline::Text {
                    value: "Alpha".into(),
                },
                Inline::line_break(),
                Inline::Text {
                    value: "Beta".into(),
                },
            ],
            layout: LayoutHint {
                indent_columns: -2,
                continuation_indent_columns: 1,
                ..Default::default()
            },
            source: None,
        }],
        0,
    );
    assert_eq!(builder.lines[0].indent, 0);
    assert_eq!(builder.lines[1].indent, 5);
    assert_eq!(builder.lines[1].continuation_indent, 5);
}

#[test]
fn nested_link_breaks_preserve_row_origins_and_reference_locations() {
    let nodes = vec![Inline::Link {
        target: mant_ir::LinkTarget::Section {
            id: "destination".into(),
        },
        title: None,
        children: vec![Inline::Emphasis {
            children: vec![
                Inline::Text {
                    value: "Alpha".into(),
                },
                Inline::line_break(),
                Inline::Text {
                    value: "Beta".into(),
                },
            ],
        }],
    }];
    let hints = layout(&[(1, 6)]);
    let lines = crate::document::inline::styled_reference_content_lines(
        mant_ir::InlineContentRef {
            content: &nodes,
            layout: &hints,
        },
        Style::default(),
        None,
        &[],
        false,
        &HashMap::default(),
    );
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[1].indent_columns, 6);
    assert_eq!(lines[1].links[0].start_scalar, 0);
    assert_eq!(lines[1].links[0].end_scalar, 4);
    let mut content = bundle();
    content.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        inline_layout: hints,
        children: nodes,
        layout: LayoutHint::default(),
        source: None,
    }];
    let rendered = DocumentView::new(&content).render(40);
    let alpha = &rendered.search("Alpha")[0];
    let hit = &rendered.search("Beta")[0];
    assert_eq!(
        (hit.start_column, hit.end_column),
        (alpha.start_column + 6, alpha.start_column + 10)
    );
    assert!(
        rendered
            .link_target_at(hit.row, hit.start_column - 1)
            .is_none()
    );
    assert_eq!(
        rendered.link_target_at(hit.row, hit.start_column),
        Some(&LinkTarget::Section("destination".into()))
    );
}

#[test]
fn first_and_soft_origins_keep_hanging_increment_through_resize() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Paragraph {
        children: vec![Inline::Text {
            value: "Alpha Beta Gamma\nDelta Epsilon".into(),
        }],
        inline_layout: layout(&[(0, 1), (1, -1)]),
        layout: LayoutHint {
            indent_columns: 2,
            continuation_indent_columns: 4,
            ..Default::default()
        },
        source: None,
    }];
    let view = DocumentView::new(&content);
    assert_eq!(
        (view.lines[0].indent, view.lines[0].continuation_indent),
        (3, 7)
    );
    assert_eq!(
        (view.lines[1].indent, view.lines[1].continuation_indent),
        (5, 5)
    );
    for width in [12, 80, 12] {
        let rendered = view.render(width);
        // The existing narrow-view policy translates both origins by one
        // cell so six columns remain available; their four-cell delta stays.
        assert_eq!(
            rendered.search("Alpha")[0].start_column,
            if width == 12 { 2 } else { 3 }
        );
        assert_eq!(rendered.search("Delta")[0].start_column, 5);
        if width == 12 {
            assert_eq!(rendered.search("Beta")[0].start_column, 6);
            assert_eq!(rendered.search("Gamma")[0].start_column, 6);
            assert_eq!(rendered.search("Epsilon")[0].start_column, 5);
        }
        let area = ratatui::layout::Rect::new(0, 0, width, rendered.row_count.try_into().unwrap());
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        ratatui::widgets::Widget::render(
            ratatui::widgets::Paragraph::new(rendered.text.clone()),
            area,
            &mut buffer,
        );
        for word in ["Alpha", "Beta", "Gamma", "Delta", "Epsilon"] {
            let hit = &rendered.search(word)[0];
            assert_eq!(
                buffer[(
                    hit.start_column.try_into().unwrap(),
                    hit.row.try_into().unwrap()
                )]
                    .symbol(),
                &word[..1]
            );
        }
    }
}

#[test]
fn heading_hints_do_not_become_text_or_trim_author_whitespace() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.heading = Some(mant_ir::Heading {
        content: vec![
            Inline::Strong {
                children: vec![Inline::Text {
                    value: " Alpha\u{a0} ".into(),
                }],
            },
            Inline::line_break(),
            Inline::anchor("heading-second"),
            Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "target".into(),
                },
                title: None,
                children: vec![Inline::Code {
                    value: " Beta  ".into(),
                }],
            },
        ],
        inline_layout: layout(&[(0, 4), (1, 2)]),
        source: None,
    });
    let view = DocumentView::new(&content);
    let rendered = view.render(80);
    for (word, origin, original) in [("Alpha", 4, " Alpha\u{a0} "), ("Beta", 2, " Beta  ")] {
        let hit = &rendered.search(word)[0];
        assert_eq!(hit.start_column, origin + 1);
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: hit.row,
                    column: 0
                },
                focus: TextPosition {
                    row: hit.row,
                    column: 79
                },
            }),
            original
        );
        assert_eq!(
            rendered.search(&format!("{}{}", " ".repeat(origin + 1), word)),
            []
        );
    }
    let beta = &rendered.search("Beta")[0];
    assert_eq!(rendered.anchor_row("heading-second"), Some(beta.row));
    assert!(rendered.link_target_at(beta.row, 0).is_none());
    assert_eq!(
        rendered.link_target_at(beta.row, beta.start_column),
        Some(&LinkTarget::Section("target".into()))
    );
}

#[test]
fn ordinary_open_tail_trims_once_and_hint_only_roots_stay_invisible() {
    // The exact A/blank/B no-fill input ran pristine CVS ASCII before this
    // model assertion. term.c::term_newln ends occupied output; term_vspace
    // independently completes blank rows. EOF literal settlement is the
    // existing reader contract; hints never create row content.
    for (value, paragraph_rows, literal_rows) in [("A\n", 1, 2), ("A\n\n", 2, 3), ("\n", 1, 2)] {
        for literal in [false, true] {
            let children = vec![Inline::Text {
                value: value.into(),
            }];
            let hints = layout(&[(0, 2)]);
            let block = if literal {
                Block::Preformatted {
                    children,
                    inline_layout: hints,
                    language: None,
                    layout: LayoutHint::default(),
                    source: None,
                }
            } else {
                Block::Paragraph {
                    children,
                    inline_layout: hints,
                    layout: LayoutHint::default(),
                    source: None,
                }
            };
            let mut builder = DocumentBuilder::new("tails".into(), None);
            builder.blocks(&[block], 0);
            assert_eq!(
                builder.lines.len(),
                if literal {
                    literal_rows
                } else {
                    paragraph_rows
                },
                "{value:?}"
            );
        }
    }
    let mut builder = DocumentBuilder::new("zero".into(), None);
    builder.blocks(
        &[Block::Paragraph {
            children: vec![Inline::anchor("empty-owner")],
            inline_layout: layout(&[(0, 6)]),
            layout: LayoutHint::default(),
            source: None,
        }],
        0,
    );
    let finished = builder.finish();
    assert!(finished.content.lines.is_empty());
    assert_eq!(finished.content.anchors.get("empty-owner"), Some(&0));

    let mut builder = DocumentBuilder::new("tail-target".into(), None);
    builder.blocks(
        &[
            Block::Paragraph {
                children: vec![
                    Inline::Text {
                        value: "A\n".into(),
                    },
                    Inline::anchor("open-tail"),
                ],
                inline_layout: layout(&[(1, 6)]),
                layout: LayoutHint::default(),
                source: None,
            },
            paragraph("NEXT"),
        ],
        0,
    );
    let finished = builder.finish();
    assert_eq!(finished.content.lines.len(), 2);
    assert_eq!(finished.content.anchors.get("open-tail"), Some(&1));
}

#[test]
fn empty_hard_rows_copy_no_structural_padding_and_authored_spaces_remain() {
    for literal in [false, true] {
        for authored_spaces in ["", "  "] {
            let mut content = bundle();
            let document = content.document.as_mut().unwrap();
            document.sections.clear();
            let children = vec![Inline::Text {
                value: format!("A\n{authored_spaces}\n"),
            }];
            let hints = layout(&[(1, 3)]);
            let geometry = LayoutHint {
                indent_columns: 2,
                ..Default::default()
            };
            document.blocks = vec![if literal {
                Block::Preformatted {
                    children,
                    inline_layout: hints,
                    language: None,
                    layout: geometry,
                    source: None,
                }
            } else {
                Block::Paragraph {
                    children,
                    inline_layout: hints,
                    layout: geometry,
                    source: None,
                }
            }];
            let rendered = DocumentView::new(&content).render(80);
            let blank_row = rendered.search("A")[0].row + 1;
            let copy = |column| {
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: blank_row,
                        column,
                    },
                    focus: TextPosition {
                        row: blank_row,
                        column: 79,
                    },
                })
            };
            if authored_spaces.is_empty() {
                assert_eq!(copy(0), "");
            } else {
                // The structural prefix remains part of visual copying when
                // source cells exist; the three hint cells are always omitted.
                assert_eq!(copy(0), "    ");
                assert_eq!(copy(5), authored_spaces);
            }
        }
    }
}

#[test]
fn empty_cell_hard_rows_do_not_copy_the_parent_column_origin() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![TableCell {
                blocks: vec![Block::Paragraph {
                    children: vec![Inline::Text {
                        value: "A\n\n".into(),
                    }],
                    inline_layout: layout(&[(1, 3)]),
                    layout: LayoutHint {
                        indent_columns: 2,
                        ..Default::default()
                    },
                    source: None,
                }],
                alignment: None,
                kind: mant_ir::TableCellKind::Text,
                column_span: 1,
                row_span: 1,
            }],
            kind: mant_ir::TableRowKind::Data,
        }],
        column_widths: vec![],
        layout: LayoutHint {
            indent_columns: 2,
            ..Default::default()
        },
        source: None,
    }];
    for width in [12, 80] {
        let rendered = DocumentView::new(&content).render(width);
        let blank_row = rendered.search("A")[0].row + 1;
        assert_eq!(
            rendered.selected_text(RenderedSelection {
                anchor: TextPosition {
                    row: blank_row,
                    column: 0,
                },
                focus: TextPosition {
                    row: blank_row,
                    column: 79,
                },
            }),
            "",
            "width={width}"
        );
    }
}

#[test]
fn marker_hint_padding_is_excluded_from_copy_without_touching_body_spaces() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![ListItem {
            blocks: vec![Block::Paragraph {
                children: vec![Inline::Text {
                    value: "BODY  \n\n".into(),
                }],
                inline_layout: layout(&[(0, 3)]),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::ListItemLayout::default(),
            entry: None,
            source: None,
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&content);
    assert_eq!(view.lines.len(), 2, "only the provisional tail is removed");
    let rendered = view.render(80);
    let body = &rendered.search("BODY")[0];
    assert_eq!(body.start_column, 5);
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: body.row,
                column: 0
            },
            focus: TextPosition {
                row: body.row,
                column: 79
            },
        }),
        "• BODY  "
    );
}

#[test]
fn shared_term_and_body_hints_keep_separate_copy_regions() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    document.blocks = vec![Block::DefinitionList {
        items: vec![DefinitionItem {
            head_body_relation: mant_ir::HeadBodyRelation::Shared {
                word_boundary: mant_ir::DefinitionWordBoundary::Separated,
            },
            terms: vec![mant_ir::DefinitionTerm {
                content: vec![Inline::Text {
                    value: "HEAD".into(),
                }],
                inline_layout: layout(&[(0, 2)]),
            }],
            description: vec![Block::Paragraph {
                children: vec![Inline::Link {
                    target: mant_ir::LinkTarget::Section {
                        id: "target".into(),
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "BODY  \n\n".into(),
                    }],
                }],
                inline_layout: layout(&[(0, 4)]),
                layout: LayoutHint::default(),
                source: None,
            }],
            layout: mant_ir::DefinitionLayout {
                body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
                body_indent_columns: 12,
                min_term_gap_columns: 1,
                spacing_before_lines: None,
            },
            entry: None,
            source: None,
        }],
        compact: true,
        declaration_groups: vec![],
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&content);
    assert_eq!(view.lines.len(), 2);
    let rendered = view.render(80);
    let body = &rendered.search("BODY")[0];
    assert_eq!(body.start_column, 16);
    assert!(rendered.link_target_at(body.row, 15).is_none());
    assert_eq!(
        rendered.link_target_at(body.row, 16),
        Some(&LinkTarget::Section("target".into()))
    );
    assert_eq!(
        rendered.selected_text(RenderedSelection {
            anchor: TextPosition {
                row: body.row,
                column: 0
            },
            focus: TextPosition {
                row: body.row,
                column: 79
            },
        }),
        "HEAD      BODY  "
    );
}

#[test]
fn table_placement_moves_hint_copy_regions_with_their_cells() {
    let mut content = bundle();
    let document = content.document.as_mut().unwrap();
    document.sections.clear();
    let cell = |word: &str, correction| TableCell {
        blocks: vec![Block::Paragraph {
            children: vec![Inline::Link {
                target: mant_ir::LinkTarget::Section {
                    id: "target".into(),
                },
                title: None,
                children: vec![Inline::Text {
                    value: format!(" {word}  "),
                }],
            }],
            inline_layout: layout(&[(0, correction)]),
            layout: LayoutHint::default(),
            source: None,
        }],
        alignment: None,
        kind: mant_ir::TableCellKind::Text,
        column_span: 1,
        row_span: 1,
    };
    document.blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![cell("LEFT", 3), cell("RIGHT", 1)],
            kind: mant_ir::TableRowKind::Data,
        }],
        column_widths: vec![],
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&content);
    for width in [12, 80, 12] {
        let rendered = view.render(width);
        for (word, correction) in [("LEFT", 3), ("RIGHT", 1)] {
            let hit = &rendered.search(word)[0];
            let original = format!(" {word}  ");
            assert_eq!(
                rendered.selected_text(RenderedSelection {
                    anchor: TextPosition {
                        row: hit.row,
                        column: hit.start_column - correction - 1
                    },
                    focus: TextPosition {
                        row: hit.row,
                        column: hit.end_column + 1
                    },
                }),
                original
            );
            assert_eq!(
                rendered.link_target_at(hit.row, hit.start_column),
                Some(&LinkTarget::Section("target".into()))
            );
        }
    }
}
