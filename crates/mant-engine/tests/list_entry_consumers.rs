//! Entry projections must borrow ordinary content, not reconstruct definitions.
#[path = "../src/semantic_test_read.rs"]
mod semantic_read;
use mant_codec::encode::render_markdown;
use mant_ir::{
    Block, ContentAtom, ContentAtomKey, ContentAtomKind, ContentByteRange, ContentOwner,
    ContentOwnerKey, ContentOwnerKind, ContentRef, ContentRoot, ContentRootKey, ContentRootKind,
    ContentStore, ContentStyle, EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot,
    EntryKind, Inline, LayoutHint, ListItem, ListKind, NameCase, Provenance, ResolvedContent,
};
use mant_loader::load_markdown_text;
use mant_protocol::{EntryProjection, ExcerptSelection, OutlineNode};
use mant_query::build_outline_projection;
use mant_render::{render_excerpt_markdown, render_excerpt_text, render_query_text};

fn item(store: &mut ContentStore, name: &str, payload: &str, entry: bool) -> ListItem {
    let owner = ContentOwnerKey::new(u32::try_from(store.owners.len() + 1).unwrap()).unwrap();
    let root = ContentRootKey::new(u32::try_from(store.roots.len() + 1).unwrap()).unwrap();
    let code_atom = ContentAtomKey::new(u32::try_from(store.atoms.len() + 1).unwrap()).unwrap();
    let text_atom = ContentAtomKey::new(u32::try_from(store.atoms.len() + 2).unwrap()).unwrap();
    let suffix = format!(" — {payload}: punctuation | stays.");
    store.owners.push(ContentOwner {
        key: owner,
        kind: ContentOwnerKind::ListItem,
        roots: vec![root],
        provenance: Provenance::Unknown,
    });
    store.roots.push(ContentRoot {
        key: root,
        owner,
        kind: ContentRootKind::Body,
        atoms: vec![code_atom, text_atom],
        points: Vec::new(),
        provenance: Provenance::Unknown,
    });
    for (key, value, literal) in [
        (code_atom, name.to_owned(), true),
        (text_atom, suffix.clone(), false),
    ] {
        store.atoms.push(ContentAtom {
            key,
            root,
            owner,
            kind: ContentAtomKind::Text {
                text: value,
                display_override: None,
            },
            style: ContentStyle {
                literal,
                ..ContentStyle::default()
            },
            role: None,
            link: None,
            provenance: Provenance::Unknown,
        });
    }
    let code = ContentRef {
        atom: code_atom,
        bytes: ContentByteRange {
            start: 0,
            end: u32::try_from(name.len()).unwrap(),
        },
    };
    let prose = ContentRef {
        atom: text_atom,
        bytes: ContentByteRange {
            start: 0,
            end: u32::try_from(suffix.len()).unwrap(),
        },
    };
    ListItem {
        layout: mant_ir::ListItemLayout::default(),
        source: None,
        entry: entry.then(|| EntryFacts {
            id: name.into(),
            kind: EntryKind::Command,
            case: NameCase::Sensitive,
            names: vec![name.into()],
            value_domain: None,
            alias_groups: Vec::new(),
            alias_of: None,
            name_bindings: vec![mant_ir::EntryNameBinding {
                name: 0,
                evidence: mant_ir::EntryNameEvidence::Declared,
                occurrences: vec![EntryForm {
                    parts: vec![EntryContentSlice {
                        root: EntryInlineRoot::Block { index: 0 },
                        path: vec![0],
                        bytes: None,
                    }],
                }],
            }],
            forms: vec![EntryForm {
                parts: vec![EntryContentSlice {
                    root: EntryInlineRoot::Block { index: 0 },
                    path: vec![0],
                    bytes: None,
                }],
            }],
        }),
        blocks: vec![Block::Paragraph {
            children: vec![
                Inline::Code { content: code },
                Inline::Text { content: prose },
            ],
            layout: LayoutHint::default(),
            source: None,
        }],
    }
}

fn query(annotated: bool) -> ResolvedContent {
    let mut query = load_markdown_text("# Example\n", None).unwrap();
    let document = query.document.as_mut().unwrap();
    let intro = item(&mut document.content_store, "intro", "FIRST", false);
    let run = item(&mut document.content_store, "run", "SECOND", annotated);
    document.blocks = vec![Block::List {
        kind: ListKind::Ordered { start: Some(7) },
        compact: false,
        items: vec![intro, run],
        layout: LayoutHint {
            indent_columns: 2,
            ..LayoutHint::default()
        },
        source: None,
    }];
    query
}

#[test]
fn ordinary_owner_navigation_and_excerpts_preserve_the_original_item() {
    let query = query(true);
    let original = query.document.as_ref().unwrap().clone();
    assert!(mant_ir::validate_document(&original).is_empty());
    assert_eq!(
        render_query_text(&query),
        render_query_text(&self::query(false))
    );
    assert_eq!(
        render_markdown(&query),
        render_markdown(&self::query(false))
    );
    let outline = build_outline_projection(
        &query,
        EntryProjection::All,
        Some(mant_protocol::ContentSelector::id("run")),
    )
    .unwrap();
    assert!(
        matches!(&outline.nodes[..], [OutlineNode::DocumentEntry { path, id, .. }]
        if id == "run" && path.as_ref() == "root/e1")
    );
    for selector in [
        mant_protocol::ContentSelector::id("run"),
        mant_protocol::ContentSelector::path("root/e1"),
    ] {
        let excerpt = mant_query::select_excerpt(&query, std::slice::from_ref(&selector)).unwrap();
        let [ExcerptSelection::DocumentEntry { entry, .. }] = &excerpt.selections[..] else {
            panic!("single entry")
        };
        let Block::List {
            kind,
            compact,
            items,
            layout,
            ..
        } = entry
        else {
            panic!("ordinary list must not become a definition")
        };
        assert_eq!(
            (*kind, *compact, layout.indent_columns),
            (ListKind::Ordered { start: Some(8) }, false, 2)
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].entry.as_ref().unwrap().names, ["run"]);
        assert_eq!(entry.entry_owner().unwrap().facts().unwrap().names, ["run"]);
        let text = render_excerpt_text(&excerpt);
        assert!(
            text.contains("8. run — SECOND: punctuation | stays."),
            "{text}"
        );
        assert!(!text.contains("FIRST"));
        let markdown = render_excerpt_markdown(&excerpt);
        assert!(
            markdown.contains(r"8. `run` — SECOND\: punctuation \| stays."),
            "{markdown}"
        );
        assert_eq!(
            mant_query::select_excerpt(&query, &[selector]).unwrap(),
            excerpt
        );
        let roundtrip = serde_json::from_str::<mant_protocol::QueryExcerpt>(
            &serde_json::to_string(&excerpt).unwrap(),
        )
        .unwrap();
        assert_eq!(roundtrip, excerpt);
    }
    assert_eq!(query.document.as_ref().unwrap(), &original);
}

#[test]
fn excerpt_ordinals_preserve_unknown_zero_and_saturated_source_starts() {
    for start in [None, Some(0), Some(7), Some(u64::MAX)] {
        let mut query = query(true);
        let document = query.document.as_mut().unwrap();
        let last = item(&mut document.content_store, "last", "THIRD", true);
        let Block::List { kind, items, .. } = &mut document.blocks[0] else {
            unreachable!()
        };
        *kind = ListKind::Ordered { start };
        items.push(last);
        let original = query.document.as_ref().unwrap().clone();
        let excerpt = semantic_read::semantic_excerpt(&query, &["run", "last"]).unwrap();
        for (index, selection) in excerpt.selections.iter().enumerate() {
            let expected = start.unwrap_or(1).saturating_add(index as u64 + 1);
            let ExcerptSelection::DocumentEntry {
                entry: Block::List { kind, .. },
                ..
            } = selection
            else {
                panic!("original ordered owner");
            };
            assert_eq!(
                *kind,
                ListKind::Ordered {
                    start: Some(expected)
                }
            );
        }
        let expected = start.unwrap_or(1).saturating_add(1);
        assert!(render_excerpt_text(&excerpt).contains(&format!("{expected}. run")));
        assert!(render_excerpt_markdown(&excerpt).contains(&format!("{expected}. `run`")));
        assert_eq!(query.document.as_ref().unwrap(), &original);
    }
}

#[test]
fn nested_ordinary_owners_share_semantic_paths_without_losing_parent_content() {
    let mut query = query(true);
    let document = query.document.as_mut().unwrap();
    let child = item(&mut document.content_store, "child", "CHILD", true);
    let Block::List { items, .. } = &mut document.blocks[0] else {
        unreachable!()
    };
    items[1].blocks.push(Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![child],
        layout: LayoutHint::default(),
        source: None,
    });
    let outline = build_outline_projection(
        &query,
        EntryProjection::All,
        Some(mant_protocol::ContentSelector::id("run")),
    )
    .unwrap();
    let [OutlineNode::DocumentEntry { children, .. }] = &outline.nodes[..] else {
        panic!("parent")
    };
    assert!(
        matches!(&children[..], [OutlineNode::DocumentEntry { path, .. }] if path.as_ref() == "root/e1/e1")
    );
    let child = mant_query::select_excerpt(
        &query,
        &[mant_protocol::ContentSelector::path("root/e1/e1")],
    )
    .unwrap();
    assert!(render_excerpt_text(&child).contains("child — CHILD: punctuation | stays."));
    assert!(!render_excerpt_text(&child).contains("SECOND"));
    let parent = semantic_read::semantic_excerpt(&query, &["run"]).unwrap();
    assert!(render_excerpt_text(&parent).contains("CHILD"));
}

#[test]
fn search_maps_ordinary_list_content_to_the_innermost_entry() {
    let query = query(true);
    let search = mant_query::search_query(
        &query,
        &mant_protocol::SearchQuery {
            pattern: "SECOND".into(),
            syntax: mant_protocol::SearchSyntax::default(),
            case: mant_protocol::SearchCase::default(),
            scope: mant_protocol::SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(search.matches.len(), 1);
    assert_eq!(search.matches[0].outline.path(), "root/e1");
}

#[test]
fn transparent_definition_and_table_preserve_entry_paths_and_nearest_owner() {
    for table in [false, true] {
        let mut query = query(true);
        let document = query.document.as_mut().unwrap();
        let ordinary = document.blocks.remove(0);
        let content = if table {
            serde_json::json!({"type": "table", "rows": [{"cells": [{"blocks": [ordinary]}]}]})
        } else {
            serde_json::to_value(ordinary).unwrap()
        };
        let transparent: Block = serde_json::from_value(serde_json::json!({
            "type": "definition-list",
            "items": [{"terms": [], "description": [content]}]
        }))
        .unwrap();
        document.blocks.push(transparent);
        let outline = build_outline_projection(
            &query,
            EntryProjection::All,
            Some(mant_protocol::ContentSelector::id("run")),
        )
        .unwrap();
        assert!(
            matches!(&outline.nodes[..], [OutlineNode::DocumentEntry {path, ..}] if path.as_ref() == "root/e1")
        );
        let excerpt =
            mant_query::select_excerpt(&query, &[mant_protocol::ContentSelector::path("root/e1")])
                .unwrap();
        assert!(render_excerpt_text(&excerpt).contains("SECOND"));
        assert!(!render_excerpt_text(&excerpt).contains("FIRST"));
        let explained = mant_query::explain_query(
            &query,
            &mant_protocol::ExplanationQuery {
                entry: "SECOND".into(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(explained.total, 1);
        assert_eq!(explained.evidence[0].outline.path(), "root/e1");
        let search = mant_query::search_query(
            &query,
            &mant_protocol::SearchQuery {
                pattern: "SECOND".into(),
                syntax: mant_protocol::SearchSyntax::default(),
                case: mant_protocol::SearchCase::default(),
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(search.matches.len(), 1);
        assert_eq!(search.matches[0].outline.path(), "root/e1");
    }
}

#[test]
fn table_search_tracks_independent_and_nested_owners_without_changing_text() {
    use mant_ir::visit::{VisitMut, walk_definition_item_mut, walk_list_item_mut};
    struct StripFacts;
    impl VisitMut for StripFacts {
        fn visit_list_item_mut(&mut self, item: &mut ListItem) {
            item.entry = None;
            walk_list_item_mut(self, item);
        }
        fn visit_definition_item_mut(&mut self, item: &mut mant_ir::DefinitionItem) {
            item.entry = None;
            walk_definition_item_mut(self, item);
        }
    }
    for wrapped in [false, true] {
        let mut query = query(false);
        let document = query.document.as_mut().unwrap();
        let store = &mut document.content_store;
        let mut parent = item(store, "parent", "BEFORE", true);
        parent.blocks.push(Block::List {
            kind: ListKind::Bullet,
            compact: false,
            items: vec![item(store, "child", "日本PAYLOAD", true)],
            layout: LayoutHint::default(),
            source: None,
        });
        parent
            .blocks
            .extend(item(store, "tail", "AFTER", false).blocks);
        let ordinary = Block::List {
            kind: ListKind::Plain,
            compact: false,
            items: vec![parent],
            layout: LayoutHint::default(),
            source: None,
        };
        let sibling_item = item(store, "sibling", "unused", false);
        let Block::Paragraph { children, .. } = &sibling_item.blocks[0] else {
            unreachable!()
        };
        let sibling_term = children[0].clone();
        let neighbor = item(store, "text", "NEIGHBOR", false);
        let definition = Block::DefinitionList {
            items: vec![mant_ir::DefinitionItem {
                terms: vec![vec![sibling_term]],
                description: neighbor.blocks,
                entry: Some(
                    serde_json::from_value(serde_json::json!({
                        "id": "sibling", "kind": {"kind": "term"},
                        "case": "sensitive", "names": []
                    }))
                    .unwrap(),
                ),
                layout: mant_ir::DefinitionLayout::default(),
                source: None,
            }],
            declaration_groups: Vec::new(),
            compact: false,
            layout: LayoutHint::default(),
            source: None,
        };
        let cell = |blocks| mant_ir::TableCell {
            kind: mant_ir::TableCellKind::Text,
            blocks,
            column_span: 1,
            row_span: 1,
            alignment: None,
        };
        let table = Block::Table {
            fixed_view: None,
            rows: vec![mant_ir::TableRow {
                kind: mant_ir::TableRowKind::Data,
                cells: vec![cell(vec![ordinary]), cell(vec![definition])],
            }],
            layout: LayoutHint::default(),
            source: None,
        };
        document.blocks = if wrapped {
            vec![Block::List {
                kind: ListKind::Bullet,
                compact: false,
                items: vec![ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                    blocks: vec![table],
                }],
                layout: LayoutHint::default(),
                source: None,
            }]
        } else {
            vec![table]
        };
        let text = render_query_text(&query);
        let markdown = render_markdown(&query);
        assert_table_search_owners(&query);
        StripFacts.visit_document_mut(query.document.as_mut().unwrap());
        assert_eq!(text, render_query_text(&query));
        assert_eq!(markdown, render_markdown(&query));
    }
}

fn assert_table_search_owners(query: &ResolvedContent) {
    for (pattern, path) in [
        ("BEFORE", "root/e1"),
        ("日本PAYLOAD", "root/e1/e1"),
        ("AFTER", "root/e1"),
        ("NEIGHBOR", "root/e2"),
    ] {
        let result = mant_query::search_query(
            query,
            &mant_protocol::SearchQuery {
                pattern: pattern.into(),
                syntax: mant_protocol::SearchSyntax::default(),
                case: mant_protocol::SearchCase::default(),
                scope: mant_protocol::SearchScope::Visible,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(result.matches.len(), 1, "{pattern}");
        assert_eq!(result.matches[0].outline.path(), path, "{pattern}");
    }
}
