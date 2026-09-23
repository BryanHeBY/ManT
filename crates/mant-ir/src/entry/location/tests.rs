use super::*;
use crate::{
    DefinitionItem, DefinitionLayout, EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot,
    EntryKind, EntryNameBinding, EntryNameEvidence, LayoutHint, ListItem, ListItemLayout, ListKind,
    NameCase, TableCell, TableRow,
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

fn definition(
    fixture: &mut crate::test_support::ContentFixture,
    name: &str,
    children: Vec<Block>,
) -> DefinitionItem {
    DefinitionItem {
        terms: vec![vec![fixture.code(name)]],
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
            source: crate::SourceKey::FIRST,
            byte_range: None,
            line: 3,
            column: 1,
            end_line: Some(7),
            end_column: None,
        }),
    }
}

fn item(fixture: &mut crate::test_support::ContentFixture, name: &str) -> ListItem {
    ListItem {
        entry: Some(facts(name, EntryInlineRoot::Block { index: 0 })),
        blocks: vec![Block::Paragraph {
            children: vec![fixture.code(name)],
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
    let mut fixture = crate::test_support::ContentFixture::body();
    let blocks = vec![
        definitions(vec![
            definition(&mut fixture, "first", Vec::new()),
            definition(&mut fixture, "second", Vec::new()),
        ]),
        Block::List {
            kind: ListKind::Ordered { start: Some(7) },
            items: vec![item(&mut fixture, "third"), item(&mut fixture, "fourth")],
            compact: false,
            layout: LayoutHint::default(),
            source: None,
        },
    ];
    let entries = content_entries(fixture.content(), &blocks);
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
    let mut fixture = crate::test_support::ContentFixture::body();
    let mut transparent = item(&mut fixture, "unused");
    let child = definition(&mut fixture, "child", Vec::new());
    let parent = definition(&mut fixture, "parent", vec![definitions(vec![child])]);
    let sibling = definition(&mut fixture, "sibling", Vec::new());
    transparent.entry = None;
    transparent.blocks = vec![Block::Table {
        fixed_view: None,
        rows: vec![TableRow {
            kind: crate::TableRowKind::Data,
            cells: vec![TableCell {
                kind: crate::TableCellKind::Text,
                blocks: vec![definitions(vec![parent, sibling])],
                point: None,
                column_span: 1,
                row_span: 1,
                alignment: None,
                source: None,
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
    let entries = content_entries(fixture.content(), &blocks);
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
    let mut fixture = crate::test_support::ContentFixture::body();
    let valid = definition(&mut fixture, "valid", Vec::new());
    let child = definition(&mut fixture, "child", Vec::new());
    let mut invalid = definition(&mut fixture, "invalid", vec![definitions(vec![child])]);
    invalid.entry.as_mut().unwrap().name_bindings.clear();
    invalid.source = Some(SourceSpan {
        source: crate::SourceKey::FIRST,
        byte_range: None,
        line: 8,
        column: 2,
        end_line: None,
        end_column: None,
    });
    let blocks = vec![definitions(vec![valid, invalid])];
    let detailed = content_entries(fixture.content(), &blocks);
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
    let mut fixture = crate::test_support::ContentFixture::body();
    let child = definition(&mut fixture, "child", Vec::new());
    let parent = definition(&mut fixture, "parent", vec![definitions(vec![child])]);
    let mut transparent = definition(&mut fixture, "unused", vec![definitions(vec![parent])]);
    transparent.entry = None;
    let blocks = vec![definitions(vec![transparent])];
    let document = crate::Document {
        content_store: fixture.finish(),
        heading: None,
        parser: None,
        sources: vec![crate::SourceRecord {
            key: crate::SourceKey::FIRST,
            identity: crate::SourceIdentity::Anonymous {
                name: "test".to_owned(),
            },
            format: crate::SourceFormat::Markdown,
            decoded_byte_length: 0,
            content_sha256: None,
            coordinates: crate::SourceCoordinates::DecodedUtf8Bytes,
        }],
        root_source: crate::SourceKey::FIRST,
        meta: crate::DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks: blocks.clone(),
        sections: vec![crate::Section {
            id: "section".into(),
            heading: crate::Heading {
                content: Vec::new(),
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
