//! Table reading roles finish before syntax bytes and owner coordinates.
use super::super::{Block, Inline, MarkdownOptions, paragraph};
use super::fixtures::{
    assert_mapped_readback, assert_readback, cell_blocks, literal, owned, table,
};

fn owned_block(block: Block, id: &str) -> Block {
    let mut owner = owned("", id);
    let Block::List { items, .. } = &mut owner else {
        unreachable!();
    };
    items[0].blocks = vec![block];
    owner
}

#[test]
fn accepted_table_owner_ranges_follow_shrinking_literal_before_second_owner() {
    // MAP01: the pre-fix fence normalized CRLF only after mapping both
    // owners. A prefix-only rebase leaves the later owner's range one byte
    // beyond its actual final text. The Code/literal contract is CRLF -> LF.
    let block = table(vec![cell_blocks(
        vec![
            owned_block(literal("FIRST\r\nZ"), "first"),
            owned("SECOND", "second"),
        ],
        false,
    )]);
    assert_mapped_readback(
        &block,
        "FIRST\nZ\nSECOND",
        &[("first", "FIRST\nZ"), ("second", "SECOND")],
    );
}

#[test]
fn table_reading_text_and_code_keep_independent_line_ending_roles() {
    let mut configurations = 0;
    for (value, reading, code) in [
        ("A\rB", "A�B", "A\nB"),
        ("A\nB", "A\nB", "A\nB"),
        ("A\r\nB", "A�\nB", "A\nB"),
        ("\r", "�", "\n"),
        ("A\r", "A�", "A\n"),
        ("\rA", "�A", "\nA"),
        ("\r\r", "��", "\n\n"),
        ("A\r\n", "A�\n", "A\n"),
        ("A\tB", "A\tB", "A\tB"),
        ("A\x0cB", "A B", "A B"),
        ("A\x1bB", "A B", "A B"),
        ("A\u{81}B", "A  B", "A  B"),
        ("A\u{a0}B", "A\u{a0}B", "A\u{a0}B"),
        ("A\u{2028}B", "A\u{2028}B", "A\u{2028}B"),
        ("A\u{2029}B", "A\u{2029}B", "A\u{2029}B"),
        ("A\u{200b}B", "A\u{200b}B", "A\u{200b}B"),
        ("A\u{feff}B", "A\u{feff}B", "A\u{feff}B"),
        ("界😀e\u{301}\rB", "界😀e\u{301}�B", "界😀e\u{301}\nB"),
    ] {
        for (block, role) in role_blocks(value) {
            let expected = match role {
                ExpectedRole::Reading => reading,
                ExpectedRole::Code => code,
                // Unsupported already trims edge LF before flattening; its
                // old block policy is distinct from scalar-role admission.
                ExpectedRole::Fallback => reading.trim_matches('\n'),
            };
            assert_two_reads(table(vec![cell_blocks(vec![block], false)]), expected);
            configurations += 1;
        }
    }
    assert_eq!(configurations, 126);
}

enum ExpectedRole {
    Reading,
    Code,
    Fallback,
}

fn role_blocks(value: &str) -> [(Block, ExpectedRole); 7] {
    let text = || Inline::Text {
        value: value.into(),
    };
    let inline = |node| paragraph(vec![node]);
    [
        (inline(text()), ExpectedRole::Reading),
        (
            inline(Inline::Strong {
                children: vec![text()],
            }),
            ExpectedRole::Reading,
        ),
        (
            inline(Inline::Emphasis {
                children: vec![text()],
            }),
            ExpectedRole::Reading,
        ),
        (
            inline(Inline::Link {
                target: mant_ir::LinkTarget::External {
                    uri: "https://example.test".into(),
                },
                title: None,
                children: vec![text()],
            }),
            ExpectedRole::Reading,
        ),
        (
            inline(Inline::Code {
                value: value.into(),
            }),
            ExpectedRole::Code,
        ),
        (literal(value), ExpectedRole::Code),
        (
            Block::Unsupported {
                name: None,
                text: value.into(),
                layout: mant_ir::LayoutHint::default(),
                source: None,
            },
            ExpectedRole::Fallback,
        ),
    ]
}

fn assert_two_reads(original: Block, expected: &str) {
    assert_readback(&original, expected);
    let mut content = vec![original];
    for _ in 0..2 {
        let markdown = super::super::render_blocks_fragment(
            &content,
            super::super::MarkdownFragmentOptions::default(),
        )
        .join("\n\n");
        let parsed = super::super::parse_content(&markdown, None).unwrap();
        let [Block::Preformatted { children, .. }] =
            parsed.document.as_ref().unwrap().blocks.as_slice()
        else {
            panic!("one accepted table fence");
        };
        assert_eq!(mant_ir::inline_plain_text(children), expected);
        content = parsed.document.unwrap().blocks;
    }
}

#[test]
fn growing_reading_payload_maps_both_owners_and_variable_fence_prefixes() {
    for (source, projected) in [
        ("FIRST\r界😀", "FIRST�界😀"),
        ("FIRST````\rZ", "FIRST````�Z"),
    ] {
        let block = table(vec![cell_blocks(
            vec![owned(source, "first"), owned("SECOND", "second")],
            false,
        )]);
        let expected = format!("{projected}; SECOND");
        let rendered = super::blocks::render_blocks_with_entries(
            std::slice::from_ref(&block),
            MarkdownOptions::default(),
            true,
        );
        assert_eq!(rendered.entries.len(), 2);
        for (id, value) in [("first", projected), ("second", "SECOND")] {
            let owner = rendered
                .entries
                .iter()
                .find(|entry| entry.owner.facts().unwrap().id.as_str() == id)
                .unwrap();
            assert_eq!(rendered.text.get(owner.start..owner.end), Some(value));
            assert!(owner.start <= owner.end && owner.end <= rendered.text.len());
        }
        assert_eq!(super::fixtures::literal_readback(&block), expected);
        assert_eq!(
            serde_json::from_str::<Block>(&serde_json::to_string(&block).unwrap()).unwrap(),
            block
        );
    }
}

#[test]
fn literal_crlf_fragmentation_does_not_consume_an_explicit_hard_break() {
    let content = |children| Block::Preformatted {
        children,
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: mant_ir::LayoutHint::default(),
        source: None,
    };
    for (children, expected) in [
        (
            vec![
                Inline::Text {
                    value: "A\r".into(),
                },
                Inline::Strong {
                    children: vec![Inline::Text {
                        value: "\nB".into(),
                    }],
                },
            ],
            "A\nB",
        ),
        (
            vec![
                Inline::Text {
                    value: "A\r".into(),
                },
                Inline::LineBreak {},
                Inline::Text { value: "B".into() },
            ],
            "A\n\nB",
        ),
    ] {
        assert_readback(
            &table(vec![cell_blocks(vec![content(children)], false)]),
            expected,
        );
    }
}

#[test]
fn canonicalized_code_tail_never_reuses_the_previous_row_origin() {
    for (value, hint_row, expected) in [("A\r", 0, "   A\n | B"), ("A\r\n", 1, "A\n    | B")] {
        let mut first = literal(value);
        let Block::Preformatted { inline_layout, .. } = &mut first else {
            unreachable!();
        };
        inline_layout.row_hints.push(mant_ir::RowLayoutHint {
            row: hint_row,
            indent_columns: 3,
            ..Default::default()
        });
        assert_two_reads(
            table(vec![
                cell_blocks(vec![first], false),
                super::fixtures::cell("B", false, 0),
            ]),
            expected,
        );
    }
}

#[test]
fn role_normalization_keeps_accepted_cell_closures_and_gap_rows() {
    // Pure IR: normalization resolves only the scalar's role. The accepted
    // open tail, breakAfter and completed gap budgets keep their existing
    // table_boundaries/nested_rows spelling without re-executing a formatter.
    for code in [false, true] {
        for gap in [0, 1, 2] {
            for closed in [false, true] {
                let first = if code {
                    literal("A\r\n")
                } else {
                    paragraph(vec![Inline::Text {
                        value: "A\r\n".into(),
                    }])
                };
                let mut blocks = vec![first];
                if gap > 0 {
                    blocks.push(super::fixtures::gap(gap));
                }
                let prefix = if code { "A" } else { "A�" };
                let seam = if gap > 0 {
                    "\n".repeat(usize::from(gap) + 1 + usize::from(code))
                } else if closed {
                    "\n\n".into()
                } else {
                    "\n | ".into()
                };
                assert_two_reads(
                    table(vec![
                        cell_blocks(blocks, closed),
                        super::fixtures::cell("B", false, 0),
                    ]),
                    &format!("{prefix}{seam}B"),
                );
            }
        }
    }
}

#[test]
fn normalized_definition_words_keep_the_selected_shared_relation() {
    for (relation, seam) in [
        (mant_ir::HeadBodyRelation::joined(), ""),
        (mant_ir::HeadBodyRelation::separated(), " "),
        (mant_ir::HeadBodyRelation::Separate, "\n"),
    ] {
        for nested in [false, true] {
            let mut definition = super::fixtures::wrap(
                paragraph(vec![Inline::Text {
                    value: "B\rC".into(),
                }]),
                "definition",
            );
            let Block::DefinitionList { items, .. } = &mut definition else {
                unreachable!();
            };
            items[0].terms = vec![
                vec![Inline::Text {
                    value: "H\r".into(),
                }]
                .into(),
            ];
            items[0].head_body_relation = relation;
            if nested {
                definition = super::fixtures::wrap(definition, "table");
            }
            assert_two_reads(
                table(vec![cell_blocks(vec![definition], false)]),
                &format!("H�{seam}B�C"),
            );
        }
    }
}

#[test]
fn public_artifact_ranges_borrow_two_owners_after_growth_and_shrinkage() {
    for (mut first, accepted) in [
        (
            paragraph(vec![Inline::Text {
                value: "FIRST\r界😀".into(),
            }]),
            "FIRST�界😀",
        ),
        (literal("FIRST\r\nZ"), "FIRST\nZ"),
    ] {
        match &mut first {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                children.insert(0, Inline::anchor("coordinate-target"));
            }
            _ => unreachable!(),
        }
        for anchors in [false, true] {
            let mut query = super::super::parse_content("# Coordinates\n", None).unwrap();
            query.document.as_mut().unwrap().blocks = vec![table(vec![cell_blocks(
                vec![
                    owned_block(first.clone(), "first"),
                    owned("SECOND", "second"),
                ],
                false,
            )])];
            assert_eq!(
                mant_ir::validate_document(query.document.as_ref().unwrap()),
                []
            );
            let original = query.clone();
            let options = MarkdownOptions {
                preserve_anchors: anchors,
                ..MarkdownOptions::default()
            };
            let artifact = crate::encode::render_addressable_markdown_with_options(&query, options);
            assert_eq!(
                artifact.text(),
                super::super::render_markdown_with_options(&query, options)
            );
            assert_eq!(
                artifact.text().matches("id=\"coordinate-target\"").count(),
                usize::from(anchors),
                "navigation syntax rebases both accepted owner intervals"
            );
            let mut count = 0;
            for node in artifact.nodes() {
                let super::super::MarkdownNode::DocumentEntry { owner, .. } = node.node() else {
                    continue;
                };
                let id = owner.facts().unwrap().id.as_str();
                let expected = if id == "first" {
                    accepted
                } else {
                    assert_eq!(id, "second");
                    "SECOND"
                };
                let range = node.range();
                assert!(range.start <= range.end && range.end <= artifact.text().len());
                assert!(artifact.text().is_char_boundary(range.start));
                assert!(artifact.text().is_char_boundary(range.end));
                assert_eq!(artifact.text().get(range), Some(expected));
                count += 1;
            }
            assert_eq!(count, 2);
            assert_eq!(
                query, original,
                "checked projection does not rewrite the IR"
            );
        }
    }
}
