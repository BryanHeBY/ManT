//! Entry projections must borrow ordinary content, not reconstruct definitions.
use mant_engine::{
    build_outline_projection, query_markdown_text, render_excerpt_markdown, render_excerpt_text,
    render_markdown, render_query_text, select_excerpt,
};
use mant_ir::{
    Block, EntryContentSlice, EntryFacts, EntryForm, EntryInlineRoot, EntryKind, Inline,
    LayoutHint, ListItem, ListKind, NameCase, ResolvedContent,
};
use mant_protocol::{EntryProjection, ExcerptSelection, OutlineNode};

fn item(name: &str, payload: &str, entry: bool) -> ListItem {
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
                Inline::Code { value: name.into() },
                Inline::Text {
                    value: format!(" — {payload}: punctuation | stays."),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }],
    }
}

fn query(annotated: bool) -> ResolvedContent {
    let mut query = query_markdown_text("# Example\n\nPlaceholder.\n", None).unwrap();
    query.document.as_mut().unwrap().blocks = vec![Block::List {
        kind: ListKind::Ordered { start: Some(7) },
        compact: false,
        items: vec![
            item("intro", "FIRST", false),
            item("run", "SECOND", annotated),
        ],
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
    let outline =
        build_outline_projection(&query, EntryProjection::All, Some("run".into())).unwrap();
    assert!(
        matches!(&outline.nodes[..], [OutlineNode::DocumentEntry { path, id, .. }]
        if id == "run" && path.as_ref() == "root/e1")
    );
    for selector in ["run", "root/e1"] {
        let excerpt = select_excerpt(&query, &[selector]).unwrap();
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
        assert_eq!(items, &[item("run", "SECOND", true)]);
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
        assert_eq!(select_excerpt(&query, &[selector]).unwrap(), excerpt);
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
        let Block::List { kind, items, .. } = &mut query.document.as_mut().unwrap().blocks[0]
        else {
            unreachable!()
        };
        *kind = ListKind::Ordered { start };
        items.push(item("last", "THIRD", true));
        let original = query.document.as_ref().unwrap().clone();
        let excerpt = select_excerpt(&query, &["run", "last"]).unwrap();
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
    let Block::List { items, .. } = &mut query.document.as_mut().unwrap().blocks[0] else {
        unreachable!()
    };
    items[1].blocks.push(Block::List {
        kind: ListKind::Bullet,
        compact: true,
        items: vec![item("child", "CHILD", true)],
        layout: LayoutHint::default(),
        source: None,
    });
    let outline =
        build_outline_projection(&query, EntryProjection::All, Some("run".into())).unwrap();
    let [OutlineNode::DocumentEntry { children, .. }] = &outline.nodes[..] else {
        panic!("parent")
    };
    assert!(
        matches!(&children[..], [OutlineNode::DocumentEntry { path, .. }] if path.as_ref() == "root/e1/e1")
    );
    let child = select_excerpt(&query, &["root/e1/e1"]).unwrap();
    assert!(render_excerpt_text(&child).contains("child — CHILD: punctuation | stays."));
    assert!(!render_excerpt_text(&child).contains("SECOND"));
    let parent = select_excerpt(&query, &["run"]).unwrap();
    assert!(render_excerpt_text(&parent).contains("CHILD"));
}

#[test]
fn search_maps_ordinary_list_content_to_the_innermost_entry() {
    let query = query(true);
    let search = mant_engine::search_query(
        &query,
        &mant_protocol::SearchQuery {
            pattern: "SECOND".into(),
            syntax: mant_protocol::SearchSyntax::default(),
            case: mant_protocol::SearchCase::default(),
            scope: mant_protocol::SearchScope::default(),
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
        let outline =
            build_outline_projection(&query, EntryProjection::All, Some("run".into())).unwrap();
        assert!(
            matches!(&outline.nodes[..], [OutlineNode::DocumentEntry {path, ..}] if path.as_ref() == "root/e1")
        );
        let excerpt = select_excerpt(&query, &["root/e1"]).unwrap();
        assert!(render_excerpt_text(&excerpt).contains("SECOND"));
        assert!(!render_excerpt_text(&excerpt).contains("FIRST"));
        let explained = mant_engine::explain_query(
            &query,
            &mant_protocol::ExplanationQuery {
                entry: "SECOND".into(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(explained.total, 1);
        assert_eq!(explained.evidence[0].outline.path(), "root/e1");
        let search = mant_engine::search_query(
            &query,
            &mant_protocol::SearchQuery {
                pattern: "SECOND".into(),
                syntax: mant_protocol::SearchSyntax::default(),
                case: mant_protocol::SearchCase::default(),
                scope: mant_protocol::SearchScope::default(),
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
        let mut parent = item("parent", "BEFORE", true);
        parent.blocks.push(Block::List {
            kind: ListKind::Bullet,
            compact: false,
            items: vec![item("child", "日本PAYLOAD", true)],
            layout: LayoutHint::default(),
            source: None,
        });
        parent.blocks.extend(item("tail", "AFTER", false).blocks);
        let ordinary = Block::List {
            kind: ListKind::Plain,
            compact: false,
            items: vec![parent],
            layout: LayoutHint::default(),
            source: None,
        };
        let table: Block = serde_json::from_value(serde_json::json!({
            "type": "table", "rows": [{"cells": [
                {"blocks": [ordinary]},
                {"blocks": [{"type": "definition-list", "items": [{
                    "terms": [[{"type": "code", "value": "sibling"}]],
                    "description": item("text", "NEIGHBOR", false).blocks,
                    "entry": {"id": "sibling", "kind": {"kind": "term"},
                        "case": "sensitive", "names": []}
                }]}]}
            ]}]
        }))
        .unwrap();
        let mut query = query(false);
        query.document.as_mut().unwrap().blocks = if wrapped {
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
        for (pattern, path) in [
            ("BEFORE", "root/e1"),
            ("日本PAYLOAD", "root/e1/e1"),
            ("AFTER", "root/e1"),
            ("NEIGHBOR", "root/e2"),
        ] {
            for scope in [
                mant_protocol::SearchScope::Visible,
                mant_protocol::SearchScope::Markdown,
            ] {
                let result = mant_engine::search_query(
                    &query,
                    &mant_protocol::SearchQuery {
                        pattern: pattern.into(),
                        syntax: mant_protocol::SearchSyntax::default(),
                        case: mant_protocol::SearchCase::default(),
                        scope,
                        word: false,
                        context_lines: 0,
                        limit: 10,
                        offset: 0,
                    },
                )
                .unwrap();
                assert_eq!(result.matches.len(), 1, "{pattern}: {scope:?}");
                assert_eq!(
                    result.matches[0].outline.path(),
                    path,
                    "{pattern}: {scope:?}"
                );
            }
        }
        StripFacts.visit_document_mut(query.document.as_mut().unwrap());
        assert_eq!(text, render_query_text(&query));
        assert_eq!(markdown, render_markdown(&query));
    }
}
