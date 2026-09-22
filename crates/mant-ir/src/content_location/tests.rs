//! Typed address, source mapping and bounded resolver regressions.
use super::*;
use crate::{Block, Document, EntryContentSlice, EntryInlineRoot, Inline};
use serde_json::json;

fn document() -> Document {
    serde_json::from_value(json!({
        "parser":null,"sources":[{"key":1,"identity":{"kind":"anonymous","name":"test"},"format":"markdown","decodedByteLength":0,"coordinates":{"kind":"decoded-utf8-bytes"}}],"rootSource":1,"meta":{},
        "contentStore": {
            "owners":[{"key":1,"kind":"content","roots":[1,2,3,4],"provenance":{"kind":"unknown"}}],
            "roots":[
                {"key":1,"owner":1,"kind":"heading","atoms":[],"points":[],"provenance":{"kind":"unknown"}},
                {"key":2,"owner":1,"kind":"term","atoms":[1],"points":[],"provenance":{"kind":"unknown"}},
                {"key":3,"owner":1,"kind":"body","atoms":[2],"points":[],"provenance":{"kind":"unknown"}},
                {"key":4,"owner":1,"kind":"heading","atoms":[3],"points":[],"provenance":{"kind":"unknown"}}
            ],
            "atoms":[
                {"key":1,"root":2,"owner":1,"kind":"text","text":"é名","style":{"literal":true},"link":2,"provenance":{"kind":"unknown"}},
                {"key":2,"root":3,"owner":1,"kind":"text","text":"body","provenance":{"kind":"unknown"}},
                {"key":3,"root":4,"owner":1,"kind":"text","text":"Part","provenance":{"kind":"unknown"}}
            ],
            "points":[],
            "links":[
                {"key":1,"owner":1,"target":{"kind":"document","name":"index"},"label":[],"provenance":{"kind":"unknown"}},
                {"key":2,"owner":1,"target":{"kind":"document","name":"term"},"label":[{"kind":"content","content":{"atom":1,"bytes":{"start":0,"end":5}}}],"provenance":{"kind":"unknown"}}
            ]
        },
        "heading":{"content":[{"type":"link","occurrence":1,"children":[]}]},
        "blocks":[{"type":"definition-list","items":[{
            "entry":null,"terms":[[{"type":"link","occurrence":2,"children":[{"type":"code","content":{"atom":1,"bytes":{"start":0,"end":5}}}]}]],
            "description":[{"type":"paragraph","children":[{"type":"text","content":{"atom":2,"bytes":{"start":0,"end":4}}}]}]
        }]}],"sections":[{"id":"part","heading":{"content":[{"type":"text","content":{"atom":3,"bytes":{"start":0,"end":4}}}]},"blocks":[],"children":[]}]
    })).unwrap()
}

#[test]
fn typed_addresses_resolve_empty_labels_terms_and_owner_slices() {
    let document = document();
    let heading = ContentLocation::DocumentHeading { path: vec![0] };
    assert!(
        matches!(heading.resolve_link(&document), Some(Inline::Link { children, .. }) if children.is_empty())
    );
    assert!(
        ContentLocation::DocumentHeading { path: vec![] }
            .resolve_link(&document)
            .is_none()
    );
    let steps = [ContentBlockStep::Block { index: 0 }];
    let owner = EntryOwnerLocationRef {
        sections: &[],
        blocks: &steps,
        item_index: 0,
    };
    let term = owner
        .map_slice(
            &document,
            &EntryContentSlice {
                root: EntryInlineRoot::Term { index: 0 },
                path: vec![0, 0],
                bytes: Some(0..2),
            },
        )
        .unwrap();
    let Some([inline]) = term.resolve(&document) else {
        panic!("term slice resolves to one code leaf");
    };
    assert!(matches!(
        document.content().inline(inline),
        Ok(crate::InlineView::Code("é名"))
    ));
    assert!(
        owner
            .map_slice(
                &document,
                &EntryContentSlice {
                    root: EntryInlineRoot::Term { index: 0 },
                    path: vec![0, 0],
                    bytes: Some(0..1)
                }
            )
            .is_none()
    );
    let body = owner
        .map_slice(
            &document,
            &EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![0],
                bytes: None,
            },
        )
        .unwrap();
    let Some([inline]) = body.resolve(&document) else {
        panic!("body slice resolves to one text leaf");
    };
    assert!(matches!(
        document.content().inline(inline),
        Ok(crate::InlineView::Text("body"))
    ));
}

#[test]
fn compact_encoding_budget_is_exact_and_checked_before_materialization() {
    for location in [
        ContentLocation::DocumentHeading {
            path: vec![0, u32::MAX],
        },
        ContentLocation::SectionHeading {
            sections: vec![3, 14],
            path: vec![2],
        },
        ContentLocation::Content {
            sections: vec![],
            blocks: vec![
                ContentBlockStep::Block { index: 123 },
                ContentBlockStep::ListItem { index: 4 },
                ContentBlockStep::Block { index: 5 },
                ContentBlockStep::TableCell { row: 6, column: 7 },
                ContentBlockStep::Block { index: 8 },
                ContentBlockStep::DefinitionItem { index: 9 },
                ContentBlockStep::Block { index: 10 },
            ],
            root: ContentInlineRoot::DefinitionTerm {
                item_index: 123,
                term_index: 12,
            },
            path: vec![4, 3],
        },
        ContentLocation::Content {
            sections: vec![0],
            blocks: vec![ContentBlockStep::Block { index: 0 }],
            root: ContentInlineRoot::Inlines,
            path: vec![],
        },
    ] {
        assert_eq!(
            location.as_ref().encoded_len(),
            serde_json::to_vec(&location).unwrap().len()
        );
        assert_eq!(location.as_ref().to_owned(), Some(location));
    }
    let huge = vec![u32::MAX; MAX_CONTENT_DEPTH + 1];
    assert!(
        ContentLocationRef::DocumentHeading { path: &huge }
            .to_owned()
            .is_none()
    );
    let huge = vec![ContentBlockStep::DefinitionItem { index: u32::MAX }; MAX_CONTENT_DEPTH];
    let oversized = ContentLocationRef::Content {
        sections: &[],
        blocks: &huge,
        root: ContentInlineRoot::Inlines,
        path: &[],
    };
    assert!(oversized.encoded_len() > MAX_CONTENT_LOCATION_BYTES);
    assert!(oversized.to_owned().is_none());
}

#[test]
fn wrong_container_and_out_of_bounds_addresses_never_fall_back() {
    let document = document();
    let good = ContentLocation::Content {
        sections: vec![],
        blocks: vec![ContentBlockStep::Block { index: 0 }],
        root: ContentInlineRoot::DefinitionTerm {
            item_index: 0,
            term_index: 0,
        },
        path: vec![0],
    };
    assert!(good.resolve_link(&document).is_some());
    for (blocks, root, path) in [
        (
            vec![ContentBlockStep::ListItem { index: 0 }],
            ContentInlineRoot::Inlines,
            vec![0],
        ),
        (
            vec![ContentBlockStep::Block { index: 0 }],
            ContentInlineRoot::Inlines,
            vec![0],
        ),
        (
            vec![
                ContentBlockStep::Block { index: 0 },
                ContentBlockStep::ListItem { index: 0 },
                ContentBlockStep::Block { index: 0 },
            ],
            ContentInlineRoot::Inlines,
            vec![0],
        ),
        (
            vec![ContentBlockStep::Block { index: 0 }],
            ContentInlineRoot::DefinitionTerm {
                item_index: 1,
                term_index: 0,
            },
            vec![0],
        ),
        (
            vec![ContentBlockStep::Block { index: 0 }],
            ContentInlineRoot::DefinitionTerm {
                item_index: 0,
                term_index: 0,
            },
            vec![0, 0, 0],
        ),
    ] {
        assert!(
            ContentLocation::Content {
                sections: vec![],
                blocks,
                root,
                path
            }
            .resolve(&document)
            .is_none()
        );
    }
    assert!(
        ContentLocation::SectionHeading {
            sections: vec![],
            path: vec![0]
        }
        .resolve(&document)
        .is_none()
    );
    assert!(
        ContentLocation::SectionHeading {
            sections: vec![99],
            path: vec![0]
        }
        .resolve(&document)
        .is_none()
    );
    let encoded = serde_json::to_value(good).unwrap();
    let mut unknown = encoded.clone();
    unknown["guess"] = json!(true);
    assert!(serde_json::from_value::<ContentLocation>(unknown).is_err());
    let mut negative = encoded;
    negative["path"] = json!([-1]);
    assert!(serde_json::from_value::<ContentLocation>(negative).is_err());
    assert!(
        serde_json::from_value::<ContentInlineRoot>(json!({"kind":"inlines","unknown":true}))
            .is_err()
    );
}

#[test]
fn shared_block_resolver_preserves_response_pair_depth_contract() {
    let mut block = Block::Paragraph {
        children: vec![],
        layout: crate::LayoutHint::default(),
        source: None,
    };
    let mut path = Vec::new();
    for _ in 0..=MAX_CONTENT_DEPTH {
        block = Block::List {
            kind: crate::ListKind::Bullet,
            compact: true,
            layout: crate::LayoutHint::default(),
            source: None,
            items: vec![crate::ListItem {
                layout: crate::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![block],
            }],
        };
        path.extend([
            ContentBlockStep::ListItem { index: 0 },
            ContentBlockStep::Block { index: 0 },
        ]);
    }
    assert!(matches!(
        resolve_block_descendant(&block, &path),
        Some(Block::Paragraph { .. })
    ));
    path.extend([
        ContentBlockStep::ListItem { index: 0 },
        ContentBlockStep::Block { index: 0 },
    ]);
    assert!(resolve_block_descendant(&block, &path).is_none());
}

#[test]
fn entry_local_mapping_checks_the_combined_path_before_fixed_scratch_growth() {
    let mut document = document();
    let Block::DefinitionList { items, .. } = &document.blocks[0] else {
        unreachable!();
    };
    let term = items[0].terms[0].clone();
    let description = items[0].description.clone();
    let mut block = Block::DefinitionList {
        declaration_groups: Vec::new(),
        compact: true,
        layout: crate::LayoutHint::default(),
        source: None,
        items: vec![crate::DefinitionItem {
            terms: vec![term],
            description,
            entry: None,
            layout: crate::DefinitionLayout::default(),
            source: None,
        }],
    };
    let mut path = vec![ContentBlockStep::Block { index: 0 }];
    for _ in 0..127 {
        block = Block::List {
            kind: crate::ListKind::Bullet,
            compact: true,
            layout: crate::LayoutHint::default(),
            source: None,
            items: vec![crate::ListItem {
                blocks: vec![block],
                entry: None,
                layout: crate::ListItemLayout::default(),
                source: None,
            }],
        };
        path.extend([
            ContentBlockStep::ListItem { index: 0 },
            ContentBlockStep::Block { index: 0 },
        ]);
    }
    document.blocks = vec![block];
    let owner = EntryOwnerLocationRef {
        sections: &[],
        blocks: &path,
        item_index: 0,
    };
    assert!(owner.resolve(&document).is_some());
    let term = owner
        .map_slice(
            &document,
            &EntryContentSlice {
                root: EntryInlineRoot::Term { index: 0 },
                path: vec![0],
                bytes: None,
            },
        )
        .unwrap();
    assert_eq!(term.as_ref().depth(), MAX_CONTENT_DEPTH);
    let Some([Inline::Link { children, .. }]) = term.resolve(&document) else {
        panic!("term resolves to its retained link wrapper");
    };
    assert!(matches!(
        document.content().inline(&children[0]),
        Ok(crate::InlineView::Code("é名"))
    ));
    // A body root adds item + block coordinates, exceeding the combined cap
    // even though the selected owner and its local slice are each valid.
    assert!(
        owner
            .map_slice(
                &document,
                &EntryContentSlice {
                    root: EntryInlineRoot::Block { index: 0 },
                    path: vec![0],
                    bytes: None,
                }
            )
            .is_none()
    );
}
