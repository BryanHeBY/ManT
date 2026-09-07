//! Entry projections must borrow ordinary content, not reconstruct definitions.
use mant_engine::{
    build_outline_projection, query_markdown_text, render_excerpt_markdown, render_excerpt_text,
    render_markdown, render_query_text, select_excerpt,
};
use mant_ir::{
    Block, DefinitionCase, DefinitionRole, EntryContentSlice, EntryFacts, EntryForm,
    EntryInlineRoot, Inline, LayoutHint, ListItem, ListKind, ResolvedContent,
};
use mant_protocol::{EntryProjection, ExcerptSelection, OutlineNode};

fn item(name: &str, payload: &str, entry: bool) -> ListItem {
    ListItem {
        entry: entry.then(|| EntryFacts {
            id: name.into(),
            role: DefinitionRole::Command,
            case: DefinitionCase::Sensitive,
            names: vec![name.into()],
            value_domain: None,
            alias_groups: Vec::new(),
            alias_of: None,
            name_bindings: Vec::new(),
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
        kind: ListKind::Ordered,
        start: Some(7),
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
            start,
            compact,
            items,
            layout,
            ..
        } = entry
        else {
            panic!("ordinary list must not become a definition")
        };
        assert_eq!(
            (*kind, *start, *compact, layout.indent_columns),
            (ListKind::Ordered, Some(8), false, 2)
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
fn nested_ordinary_owners_share_semantic_paths_without_losing_parent_content() {
    let mut query = query(true);
    let Block::List { items, .. } = &mut query.document.as_mut().unwrap().blocks[0] else {
        unreachable!()
    };
    items[1].blocks.push(Block::List {
        kind: ListKind::Bullet,
        start: None,
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
