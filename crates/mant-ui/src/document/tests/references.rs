//! Reference inventory and exact source-row geometry remain independent of entries.
use super::*;

fn document_link(label: &str, fragment: Option<&str>) -> Inline {
    Inline::Link {
        target: mant_ir::LinkTarget::Document {
            name: "target".into(),
            fragment: fragment.map(str::to_owned),
        },
        title: None,
        children: vec![Inline::Text {
            value: label.into(),
        }],
    }
}

fn linked_block(prefix: &str, label: &str) -> Block {
    Block::Paragraph {
        children: vec![
            Inline::Text {
                value: prefix.into(),
            },
            document_link(label, None),
        ],
        layout: LayoutHint::default(),
        source: None,
    }
}

#[test]
fn references_group_full_targets_without_promoting_entries_or_rewriting_body() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: vec![
            document_link("first", None),
            Inline::Text {
                value: " then ".into(),
            },
            document_link("second", None),
            document_link("fragment", Some("part")),
            document_link("", None),
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let before = query.clone();
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 4);
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| matches!(node.kind, NavKind::Entry(_)))
            .count(),
        0
    );
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        4
    );
    assert!(
        view.navigation()
            .iter()
            .any(|node| node.title.ends_with("3 locations"))
    );
    assert!(
        view.references
            .iter()
            .any(|reference| reference.label.contains("unlabelled"))
    );
    for width in [12, 40, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            assert!(
                rendered.anchor_row(&reference.id).is_some(),
                "{reference:?}"
            );
        }
        assert!(!rendered.text.to_string().contains("unlabelled"));
        assert!(!rendered.text.to_string().contains("DOCUMENT REFERENCES"));
    }
    assert_eq!(query, before);
    let second = DocumentView::new(&query);
    assert_eq!(
        view.references
            .iter()
            .map(|r| (&r.id, &r.location))
            .collect::<Vec<_>>(),
        second
            .references
            .iter()
            .map(|r| (&r.id, &r.location))
            .collect::<Vec<_>>()
    );
}

#[test]
fn reference_origins_follow_actual_occurrence_through_wrapping_and_table_stacking() {
    let mut query = bundle();
    let document = query.document.as_mut().unwrap();
    document.heading = Some(mant_ir::Heading {
        content: vec![document_link("ROOTLINK", None)],
        source: None,
    });
    document.sections[0].heading = mant_ir::Heading {
        content: vec![document_link("HEADLINK", None)],
        source: None,
    };
    document.sections[0].blocks = vec![
        linked_block("日本 e\u{301}\tbefore before before ", "BODYLINK"),
        Block::List {
            kind: mant_ir::ListKind::Bullet,
            compact: true,
            items: vec![ListItem {
                blocks: vec![linked_block("list prefix ", "LISTLINK")],
                entry: None,
                layout: mant_ir::ListItemLayout::default(),
                source: None,
            }],
            layout: LayoutHint::default(),
            source: None,
        },
        Block::Table {
            rows: vec![TableRow {
                cells: vec![
                    TableCell {
                        blocks: vec![linked_block("left ", "CELLLINK")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                    TableCell {
                        blocks: vec![paragraph("right column has substantial wrapped content")],
                        column_span: 1,
                        row_span: 1,
                        alignment: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 5);
    for width in [8, 12, 24, 80] {
        let rendered = view.render(width);
        for reference in &view.references {
            let found = rendered.search(&reference.label);
            assert!(
                !found.is_empty(),
                "width={width} {reference:?}\n{}",
                rendered.text
            );
            assert_eq!(
                rendered.anchor_row(&reference.id),
                Some(found[0].row),
                "width={width} {reference:?}\n{}",
                rendered.text
            );
        }
    }
}

#[test]
fn bounded_inventory_exposes_truncation_without_removing_body_links() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![Block::Paragraph {
        children: (0..1002).map(|_| document_link("x", None)).collect(),
        layout: LayoutHint::default(),
        source: None,
    }];
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 1000);
    assert!(
        view.navigation()
            .iter()
            .any(|node| node.kind == NavKind::ReferenceNotice)
    );
    assert_eq!(view.render(80).text.to_string().matches('x').count(), 1002);
}

#[test]
fn empty_only_reference_keeps_a_reveal_location_without_manufactured_text() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks =
        vec![linked_block("", ""), paragraph("AFTER")];
    let view = DocumentView::new(&query);
    let rendered = view.render(80);
    assert_eq!(view.references.len(), 1);
    assert_eq!(
        rendered.anchor_row(&view.references[0].id),
        Some(rendered.search("AFTER")[0].row)
    );
    assert!(!rendered.text.to_string().contains("target"));
}

#[test]
fn definition_term_and_run_in_description_keep_separate_source_origins() {
    for inline_term in [false, true] {
        let mut query = bundle();
        query.document.as_mut().unwrap().sections[0].blocks = vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                terms: vec![vec![
                    Inline::Text {
                        value: "日本 ".into(),
                    },
                    document_link("TERM", None),
                ]],
                description: vec![linked_block("body prefix ", "TAILREF")],
                entry: None,
                source: None,
                layout: mant_ir::DefinitionLayout {
                    inline_term,
                    ..Default::default()
                },
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        }];
        let view = DocumentView::new(&query);
        assert_eq!(view.references.len(), 2);
        for width in [8, 12, 40, 80] {
            let rendered = view.render(width);
            for reference in &view.references {
                assert_eq!(
                    rendered.anchor_row(&reference.id),
                    Some(rendered.search(&reference.label)[0].row),
                    "inline={inline_term} width={width}\n{}",
                    rendered.text
                );
            }
        }
    }
}

#[test]
fn duplicate_invalid_owner_ids_do_not_duplicate_or_misassign_references() {
    let mut query = bundle();
    query.document.as_mut().unwrap().sections[0].blocks = vec![linked_block("", "FIRST")];
    let mut second = query.document.as_ref().unwrap().sections[0].clone();
    second.blocks = vec![linked_block("", "SECOND")];
    query.document.as_mut().unwrap().sections.push(second);
    let view = DocumentView::new(&query);
    assert_eq!(view.references.len(), 2);
    assert_eq!(
        view.navigation()
            .iter()
            .filter(|node| node.kind == NavKind::Reference)
            .count(),
        2
    );
    let rendered = view.render(80);
    for reference in &view.references {
        assert_eq!(
            rendered.anchor_row(&reference.id),
            Some(rendered.search(&reference.label)[0].row)
        );
    }
}
