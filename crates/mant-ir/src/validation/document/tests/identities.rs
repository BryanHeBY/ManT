//! Existing identities invariant contracts.
use super::*;

#[test]
fn reports_duplicate_and_empty_section_identities() {
    let diagnostics = validate_document(&document(
        vec![section(""), section("duplicate"), section("duplicate")],
        Vec::new(),
    ));
    let codes = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"ir.empty-identity"));
    assert!(codes.contains(&"ir.duplicate-identity"));
}

#[test]
fn accepts_links_to_sections_and_inline_anchors() {
    let link = |id: &str| Inline::Link {
        target: LinkTarget::Section { id: id.into() },
        title: None,
        children: vec![Inline::Text {
            value: id.to_owned(),
        }],
    };
    let blocks = vec![Block::Paragraph {
        inline_layout: crate::InlineLayout::default(),
        children: vec![
            Inline::anchor("anchor"),
            link("section"),
            link("anchor"),
            link("missing"),
        ],
        layout: LayoutHint::default(),
        source: None,
    }];
    let diagnostics = validate_document(&document(vec![section("section")], blocks));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code.as_deref(),
        Some("ir.dangling-section-link")
    );
}

#[test]
fn fragment_aliases_keep_source_spelling_but_must_resolve_uniquely() {
    let mut first = section("first");
    first.fragment_aliases = vec!["Mixed.Target".into(), "--option".into()];
    let diagnostics = validate_document(&document(vec![first.clone()], Vec::new()));
    assert_eq!(diagnostics.len(), 0);

    let mut second = section("second");
    second.fragment_aliases = vec!["Mixed.Target".into(), "bad fragment".into()];
    let diagnostics = validate_document(&document(vec![first, second], Vec::new()));
    let codes = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect::<Vec<_>>();
    assert!(codes.contains(&"ir.invalid-fragment-alias"));
    assert!(codes.contains(&"ir.ambiguous-fragment-alias"));
}

#[test]
fn reports_invalid_ids_role_collisions_ranges_tables_and_uris() {
    let source = SourceSpan {
        byte_range: Some(TextRange {
            start: TextSize::new(9),
            end: TextSize::new(3),
        }),
        line: 0,
        column: 0,
        end_line: Some(0),
        end_column: Some(0),
    };
    let shared: NodeId = "Bad ID".into();
    let section = Section {
        id: shared.clone(),
        fragment_aliases: Vec::new(),
        heading: "invalid".into(),
        spacing_before_lines: 0,
        blocks: vec![Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                head_body_relation: HeadBodyRelation::from(false),
                source: None,
                entry: Some(EntryFacts {
                    name_bindings: Vec::new(),
                    alias_groups: Vec::new(),
                    alias_of: None,
                    forms: Vec::new(),
                    id: shared.clone(),
                    kind: EntryKind::Term,
                    case: NameCase::Sensitive,
                    names: vec!["term".to_owned()],
                    value_domain: None,
                }),
                terms: vec![vec![Inline::anchor(shared.clone())].into()],
                description: Vec::new(),
                layout: crate::DefinitionLayout {
                    body_alignment: crate::DefinitionBodyAlignment::Indented,
                    spacing_before_lines: None,
                    ..Default::default()
                },
            }],
            compact: true,
            layout: LayoutHint::default(),
            source: Some(source),
        }],
        children: Vec::new(),
        source: None,
    };
    let blocks = vec![
        invalid_uri_paragraph(),
        Block::Table {
            column_preferences: crate::ColumnPreferences::default(),
            rows: vec![TableRow {
                kind: crate::TableRowKind::Data,
                cells: vec![TableCell {
                    break_after: false,
                    kind: crate::TableCellKind::Text,
                    blocks: Vec::new(),
                    column_span: 0,
                    row_span: 0,
                    alignment: None,
                }],
            }],
            layout: LayoutHint::default(),
            source: None,
        },
    ];

    let diagnostics = validate_document(&document(vec![section], blocks));
    let codes = diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.code.as_deref())
        .collect::<Vec<_>>();
    for expected in [
        "ir.invalid-identity",
        "ir.identity-role-collision",
        "ir.invalid-source-position",
        "ir.reverse-source-range",
        "ir.invalid-table-span",
        "ir.invalid-external-uri",
        "ir.invalid-email-address",
    ] {
        assert!(codes.contains(&expected), "missing {expected}: {codes:?}");
    }
}

fn invalid_uri_paragraph() -> Block {
    Block::Paragraph {
        inline_layout: crate::InlineLayout::default(),
        children: vec![
            Inline::Link {
                target: LinkTarget::External {
                    uri: "relative target".to_owned(),
                },
                title: None,
                children: Vec::new(),
            },
            Inline::Link {
                target: LinkTarget::Email {
                    address: "missing-domain".to_owned(),
                },
                title: None,
                children: Vec::new(),
            },
        ],
        layout: LayoutHint::default(),
        source: None,
    }
}
