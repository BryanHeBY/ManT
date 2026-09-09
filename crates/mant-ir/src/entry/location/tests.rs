use super::*;
use crate::{
    DefinitionItem, DefinitionLayout, EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot,
    EntryKind, EntryNameBinding, EntryNameEvidence, Inline, LayoutHint, ListItem, ListItemLayout,
    ListKind, NameCase, TableCell, TableRow,
};

fn facts(name: &str, root: EntryInlineRoot) -> EntryFacts {
    let form = EntryForm {
        parts: vec![EntryContentSlice {
            root,
            path: vec![0],
            bytes: None,
        }],
    };
    EntryFacts {
        id: name.into(),
        kind: EntryKind::Command,
        case: NameCase::Sensitive,
        names: vec![name.into()],
        forms: vec![form.clone()],
        name_bindings: vec![EntryNameBinding {
            name: 0,
            evidence: EntryNameEvidence::Declared,
            occurrences: vec![form],
        }],
        alias_groups: Vec::new(),
        alias_of: None,
        value_domain: None,
    }
}

fn definition(name: &str, children: Vec<Block>) -> DefinitionItem {
    DefinitionItem {
        terms: vec![vec![Inline::Code { value: name.into() }]],
        description: children,
        entry: Some(facts(name, EntryInlineRoot::Term { index: 0 })),
        layout: DefinitionLayout::default(),
        source: None,
    }
}

fn definitions(items: Vec<DefinitionItem>) -> Block {
    Block::DefinitionList {
        declaration_groups: Vec::new(),
        items,
        compact: true,
        layout: LayoutHint {
            indent_columns: 4,
            ..Default::default()
        },
        source: Some(SourceSpan {
            byte_range: None,
            line: 3,
            column: 1,
            end_line: Some(7),
            end_column: None,
        }),
    }
}

fn item(name: &str) -> ListItem {
    ListItem {
        entry: Some(facts(name, EntryInlineRoot::Block { index: 0 })),
        blocks: vec![Block::Paragraph {
            children: vec![Inline::Code { value: name.into() }],
            layout: LayoutHint::default(),
            source: None,
        }],
        layout: ListItemLayout {
            spacing_before_lines: Some(2),
        },
        source: None,
    }
}

#[test]
fn borrowed_serialization_matches_owned_excerpts_and_preserves_ordinals() {
    let blocks = vec![
        definitions(vec![
            definition("first", Vec::new()),
            definition("second", Vec::new()),
        ]),
        Block::List {
            kind: ListKind::Ordered { start: Some(7) },
            items: vec![item("third"), item("fourth")],
            compact: false,
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    let entries = content_entries(&blocks);
    assert_eq!(entries.len(), 4);
    for entry in &entries {
        assert_eq!(
            serde_json::to_value(entry).unwrap(),
            serde_json::to_value(entry.content()).unwrap()
        );
        assert_eq!(
            entry.content().entry_owner().unwrap().facts(),
            entry.owner().facts()
        );
        assert_eq!(entry.names(), entry.owner().facts().unwrap().names);
    }
    let Block::List { kind, items, .. } = entries[3].content() else {
        panic!("list excerpt")
    };
    assert_eq!(kind, ListKind::Ordered { start: Some(8) });
    assert_eq!(items.len(), 1);
    assert_eq!(entries[3].item_index(), 1);
    assert_eq!(entries[0].source(), None);
}

#[test]
fn transparent_table_and_list_paths_keep_nested_semantic_coordinates() {
    use ContentBlockStep::{Block as B, DefinitionItem as D, ListItem as L, TableCell as T};
    let mut transparent = item("unused");
    transparent.entry = None;
    transparent.blocks = vec![Block::Table {
        rows: vec![TableRow {
            cells: vec![TableCell {
                blocks: vec![definitions(vec![
                    definition(
                        "parent",
                        vec![definitions(vec![definition("child", Vec::new())])],
                    ),
                    definition("sibling", Vec::new()),
                ])],
                column_span: 1,
                row_span: 1,
                alignment: None,
            }],
        }],
        layout: LayoutHint::default(),
        source: None,
    }];
    let blocks = vec![Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![transparent],
        layout: LayoutHint::default(),
        source: None,
    }];
    let entries = content_entries(&blocks);
    let base = vec![
        B { index: 0 },
        L { index: 0 },
        B { index: 0 },
        T { row: 0, column: 0 },
        B { index: 0 },
    ];
    assert_eq!(
        entries
            .iter()
            .map(ContentEntry::indices)
            .collect::<Vec<_>>(),
        vec![&[1][..], &[1, 1][..], &[2][..]]
    );
    assert_eq!(entries[0].block_path(), base);
    let mut nested = base.clone();
    nested.extend([D { index: 0 }, B { index: 0 }]);
    assert_eq!(entries[1].block_path(), nested);
    assert_eq!(entries[2].block_path(), base);
    assert_eq!(entries[1].ancestors().len(), 1);
    assert_eq!(
        entries[1].ancestors()[0].facts().unwrap().id.as_str(),
        "parent"
    );
    assert!(entries[2].ancestors().is_empty());
    for entry in entries {
        let block = crate::resolve_content_block(&blocks, entry.block_path()).unwrap();
        assert!(std::ptr::eq(block, entry.container));
    }
}

#[test]
fn invalid_names_remain_addressable_and_location_only_scan_skips_names() {
    let valid = definition("valid", Vec::new());
    let mut invalid = definition(
        "invalid",
        vec![definitions(vec![definition("child", Vec::new())])],
    );
    invalid.entry.as_mut().unwrap().name_bindings.clear();
    invalid.source = Some(SourceSpan {
        byte_range: None,
        line: 8,
        column: 2,
        end_line: None,
        end_column: None,
    });
    let blocks = vec![definitions(vec![valid, invalid])];
    let detailed = content_entries(&blocks);
    let locations = content_entry_locations(&blocks);
    assert_eq!(detailed.len(), 3);
    assert_eq!(detailed[0].names(), ["valid"]);
    assert!(detailed[1].names().is_empty());
    assert_eq!(detailed[2].names(), ["child"]);
    assert_eq!(detailed[1].source().unwrap().line, 8);
    for (detail, location) in detailed.iter().zip(&locations) {
        assert!(location.names().is_empty());
        assert_eq!(detail.indices(), location.indices());
        assert_eq!(detail.block_path(), location.block_path());
        assert_eq!(detail.owner().facts(), location.owner().facts());
    }
    assert_eq!(locations[2].indices(), [2, 1]);
}

#[test]
fn borrowed_locations_and_semantic_index_share_root_and_section_owners() {
    let mut transparent = definition(
        "unused",
        vec![definitions(vec![definition(
            "parent",
            vec![definitions(vec![definition("child", Vec::new())])],
        )])],
    );
    transparent.entry = None;
    let blocks = vec![definitions(vec![transparent])];
    let document = crate::Document {
        heading: None,
        parser: None,
        source: crate::DocumentSource {
            format: crate::SourceFormat::Markdown,
            path: None,
        },
        meta: crate::DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: blocks.clone(),
        sections: vec![crate::Section {
            id: "section".into(),
            heading: crate::Heading {
                content: vec![Inline::Text {
                    value: "Section".into(),
                }],
                source: None,
            },
            fragment_aliases: Vec::new(),
            spacing_before_lines: 0,
            blocks,
            children: Vec::new(),
            source: None,
        }],
    };
    let index = crate::SemanticIndex::build(&document);
    for (section, blocks) in [
        (None, document.blocks.as_slice()),
        (Some(&[1][..]), document.sections[0].blocks.as_slice()),
    ] {
        for entry in content_entry_locations(blocks) {
            let path = crate::OutlinePath::nested_entry(section, entry.indices()).unwrap();
            assert_eq!(
                index.owner_at(&path),
                Some(&crate::ContentReveal::Owner {
                    sections: section.map_or(Vec::new(), |_| vec![0]),
                    blocks: entry.block_path().to_vec(),
                    item_index: u32::try_from(entry.item_index()).unwrap(),
                })
            );
        }
    }
}
