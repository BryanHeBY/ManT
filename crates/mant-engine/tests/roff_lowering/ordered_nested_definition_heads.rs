//! Nested item prefixes execute inside their original outer HEAD owner.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{Block, ResolvedContent, inline_plain_text};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    rows: Vec<String>,
}

fn first_item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some("It") {
        Some(node)
    } else {
        node.children.iter().find_map(first_item)
    }
}

fn roundtrip(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let bundle = mant_protocol::QueryBundle::from(&content);
    let json = serde_json::to_string(&bundle).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(bundle).unwrap(),
        serde_json::to_value(&decoded).unwrap()
    );
    decoded.into()
}

fn rendered_rows(content: &ResolvedContent) -> Vec<String> {
    let rendered = mant_render::render_query_man(content);
    let (_, rest) = rendered.split_once("DESCRIPTION\n").unwrap();
    let (body, _) = rest.split_once("NEXT\n").unwrap();
    body.trim_end_matches('\n')
        .split('\n')
        // These sources contain no authored whitespace run. Only native
        // horizontal field geometry is responsive; every physical row,
        // empty row, glyph and joined/separated word seam remains checked.
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn readback_item_rows(content: &ResolvedContent, case: &Case) -> Vec<String> {
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap();
    // Canonical Markdown exports this definition as one semantic list item.
    // Its marker belongs to that owner, outside the accepted roff payload.
    // Keep the no-fill leading empty row outside that list owner. Re-render
    // its original prefix and the actual item's blocks; a nested marker,
    // extra item/block or altered physical row remains observable.
    let Some((
        Block::List {
            kind: mant_ir::ListKind::Bullet,
            items,
            ..
        },
        before,
    )) = section.blocks.split_last()
    else {
        panic!("{}: expected the exported definition's list owner", case.id)
    };
    if case.source.contains("\n.nf\n") {
        let [Block::Paragraph { children, .. }] = before else {
            panic!("{}: expected the no-fill leading row owner", case.id)
        };
        assert_eq!(inline_plain_text(children), "\n", "{}", case.id);
    } else {
        assert!(before.is_empty(), "{}", case.id);
    }
    assert_eq!(items.len(), 1, "{}", case.id);
    let mut blocks = before.to_vec();
    blocks.extend(items[0].blocks.clone());
    let mut owner_content = content.clone();
    owner_content
        .document
        .as_mut()
        .unwrap()
        .sections
        .iter_mut()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
        .blocks
        .clone_from(&blocks);
    rendered_rows(&owner_content)
}

fn assert_native_ownership(case: &Case) -> (u32, u32) {
    let parsed = Parser::default()
        .parse_bytes(&case.id, case.source.as_bytes())
        .unwrap();
    let outer = first_item(&parsed.document.root).unwrap();
    let outer_head = outer
        .children
        .iter()
        .find(|part| part.kind == NodeKind::Head)
        .unwrap();
    let inner = first_item(outer_head).expect("inner It must remain inside the outer HEAD");
    let head_index = inner
        .children
        .iter()
        .position(|part| part.kind == NodeKind::Head)
        .unwrap();
    assert_eq!(
        inner.children[..head_index]
            .iter()
            .filter_map(|node| node.text.as_deref())
            .collect::<String>(),
        "(",
        "{}: original direct prefix",
        case.id
    );
    (outer.line, outer.column)
}

#[test]
fn nested_definition_prefixes_keep_original_head_ownership_and_native_rows() {
    // All ten exact sources ran pristine ASCII/UTF-8(width120)/HTML/tree
    // before assertions: five definition styles in fill and no-fill.
    // blk_full() emits DELIMO before allocating HEAD (mdoc_macro.c:1192);
    // print_mdoc_node() visits that direct sibling before It HEAD pre
    // changes NOSPACE, field geometry or diagnostic font (mdoc_term.c:406,
    // 755-855). The nested list uses the caller's existing field consumer.
    let fixture: Fixture =
        serde_json::from_str(include_str!("ordered_nested_definition_heads/cases.json")).unwrap();
    assert_eq!(fixture.count, 10);
    assert_eq!(fixture.count, fixture.cases.len());
    for case in fixture.cases {
        let original_source = assert_native_ownership(&case);
        let content = roundtrip(&case.source);
        let document = content.document.as_ref().unwrap();
        assert!(
            mant_ir::validate_document(document).is_empty(),
            "{}",
            case.id
        );
        let section = document
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap();
        let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
            panic!("{}: prefix escaped the original definition owner", case.id)
        };
        assert_eq!(items.len(), 1, "{}", case.id);
        let owner = &items[0];
        let source = owner.source.unwrap();
        assert_eq!((source.line, source.column), original_source);
        assert_eq!(owner.terms.len(), 1, "{}", case.id);
        let term = inline_plain_text(&owner.terms[0]);
        assert_eq!(term.matches('(').count(), 1, "{}: {term}", case.id);
        assert_eq!(term.matches(')').count(), 1, "{}: {term}", case.id);
        assert_eq!(term.matches("PREFIX").count(), 1, "{}", case.id);
        assert_eq!(term.matches("SUFFIX").count(), 1, "{}", case.id);
        assert_eq!(rendered_rows(&content), case.rows, "{}", case.id);
        let markdown = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(
            readback_item_rows(&read, &case),
            case.rows,
            "{}: real Markdown readback\n{markdown}",
            case.id
        );
    }
}
