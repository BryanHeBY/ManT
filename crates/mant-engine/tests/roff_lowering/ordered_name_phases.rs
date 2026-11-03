//! Nm parts keep original order, field posts and their semantic owners.

use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind, Parser};
use mant_ir::{Block, Inline, ResolvedContent, inline_plain_text};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    rows: Vec<String>,
}

#[derive(Deserialize)]
struct OriginFixture {
    cases: Vec<OriginCase>,
}

#[derive(Deserialize)]
struct OriginCase {
    id: String,
    source: String,
    rows: Vec<String>,
    native_columns: BTreeMap<String, i32>,
}

fn first_macro(node: &Node, token: MdocMacro) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token == Some(MacroToken::Mdoc(token)) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| first_macro(child, token))
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

fn rows(content: &ResolvedContent) -> Vec<String> {
    let text = mant_render::render_query_man(content);
    let region = text
        .split_once("SYNOPSIS\n")
        .unwrap()
        .1
        .split_once("NEXT\n")
        .unwrap()
        .0;
    // Only the responsive horizontal field placement is normalized. Native
    // physical rows, including the leading no-fill row, remain independent.
    region
        .trim_end_matches('\n')
        .split('\n')
        .map(|row| row.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

fn strengths(nodes: &[Inline], strong: bool, output: &mut Vec<(char, bool)>) {
    for node in nodes {
        match node {
            Inline::Strong { children } => strengths(children, true, output),
            Inline::Emphasis { children } | Inline::Link { children, .. } => {
                strengths(children, strong, output);
            }
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                output.extend(value.chars().map(|ch| (ch, strong)));
            }
            _ => {}
        }
    }
}

fn readback_rows(content: &ResolvedContent, case: &Case) -> Vec<String> {
    if !case.source.contains(".Bl -tag") {
        return rows(content);
    }
    let section = content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap();
    // Markdown canonically exports this original definition as one list
    // item. Inspect that real owner rather than erasing rendered bullets.
    // A no-fill prefix stays a separate exact physical-row owner.
    let (before, list) = if case.id == "nm-head-args-font" {
        let [list, Block::Paragraph { children, .. }] = section.blocks.as_slice() else {
            panic!("{}: list followed by original font-scope text", case.id)
        };
        assert_eq!(inline_plain_text(children), "OUTER PREVIOUS");
        // The outer paragraph is a separate semantic owner. Canonical
        // Markdown requires its blank separator from the list; its exact
        // payload is checked above, independently of the item hard rows.
        (&[][..], list)
    } else {
        let Some((list, before)) = section.blocks.split_last() else {
            panic!("{}: exported list", case.id)
        };
        (before, list)
    };
    let Block::List {
        kind: mant_ir::ListKind::Bullet,
        items,
        ..
    } = list
    else {
        panic!("{}: exported definition list owner", case.id)
    };
    if case.source.contains("\n.nf\n") {
        let [Block::Paragraph { children, .. }] = before else {
            panic!("{}: leading row owner", case.id)
        };
        assert_eq!(inline_plain_text(children), "\n");
    } else {
        assert_eq!(before, []);
    }
    assert_eq!(items.len(), 1);
    let mut blocks = before.to_vec();
    blocks.extend(items[0].blocks.clone());
    let mut owner = content.clone();
    owner
        .document
        .as_mut()
        .unwrap()
        .sections
        .iter_mut()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap()
        .blocks = blocks;
    rows(&owner)
}

fn assert_name_owner(case: &Case, root: &Node, content: &ResolvedContent) {
    let original = first_macro(root, MdocMacro::Nm).unwrap();
    let head = original
        .children
        .iter()
        .position(|part| part.kind == NodeKind::Head)
        .unwrap();
    assert_eq!(
        original.children[..head]
            .iter()
            .filter_map(|node| node.text.as_deref())
            .collect::<String>(),
        "(",
        "{}",
        case.id
    );
    let document = content.document.as_ref().unwrap();
    assert!(
        mant_ir::validate_document(document).is_empty(),
        "{}",
        case.id
    );
    let section = document
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")
        .unwrap();
    if let Some(outer) = first_macro(root, MdocMacro::It) {
        let (list, after) = section.blocks.split_first().unwrap();
        let Block::DefinitionList { items, .. } = list else {
            panic!("{}: original definition owner", case.id)
        };
        if case.id == "nm-head-args-font" {
            let [
                Block::Paragraph {
                    children, source, ..
                },
            ] = after
            else {
                panic!("original OUTER/PREVIOUS owner")
            };
            assert_eq!(inline_plain_text(children), "OUTER PREVIOUS");
            assert_eq!((source.unwrap().line, source.unwrap().column), (14, 1));
        } else {
            assert_eq!(after, []);
        }
        assert_eq!(items.len(), 1);
        let source = items[0].source.unwrap();
        assert_eq!((source.line, source.column), (outer.line, outer.column));
        assert_eq!(
            inline_plain_text(&items[0].terms[0]).matches('(').count(),
            1
        );
    } else if case.id == "nm-synopsis" {
        let Block::Paragraph { children, .. } = &section.blocks[0] else {
            panic!("original name paragraph")
        };
        let mut actual = Vec::new();
        strengths(children, false, &mut actual);
        // The exact UTF-8 oracle bolds HEAD only: the direct '(' is outside
        // HEAD pre, while AFTER executes after its font/pop and field post.
        let expected: Vec<_> = "(probe) AFTER"
            .chars()
            .enumerate()
            .map(|(index, ch)| (ch, (1..7).contains(&index)))
            .collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn ordered_name_parts_share_native_head_body_posts_and_original_owners() {
    // Eight complete sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // mdoc_term.c:966-1045: BLOCK PREKEEP precedes direct TEXT; HEAD alone
    // selects bold, and its post flush clears NOBREAK/BRIND/HANG, not BRTRSP.
    // BODY selects its source-derived offset and flushes before geometry
    // returns. A present empty HEAD still enters/exits but pre returns zero.
    let fixture: Fixture =
        serde_json::from_str(include_str!("ordered_name_phases/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 8);
    for case in fixture.cases {
        let parsed = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        let content = roundtrip(&case.source);
        assert_name_owner(&case, &parsed.document.root, &content);
        assert_eq!(rows(&content), case.rows, "{}", case.id);
        let markdown = mant_codec::encode::render_markdown(&content);
        let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let expected = if case.id == "nm-head-args-font" {
            let (tail, item) = case.rows.split_last().unwrap();
            assert_eq!(tail, "OUTER PREVIOUS");
            item
        } else {
            case.rows.as_slice()
        };
        assert_eq!(
            readback_rows(&restored, &case),
            expected,
            "{}: Markdown readback\n{markdown}",
            case.id
        );
    }
}

fn word_column(content: &ResolvedContent, word: &str) -> Option<i32> {
    let section = content
        .document
        .as_ref()?
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "SYNOPSIS")?;
    section.blocks.iter().find_map(|block| {
        let (Block::Paragraph {
            children,
            inline_layout,
            layout,
            ..
        }
        | Block::Preformatted {
            children,
            inline_layout,
            layout,
            ..
        }) = block
        else {
            return None;
        };
        let text = inline_plain_text(children);
        let parent = 5 + layout.indent_columns;
        let continuation = parent + layout.continuation_indent_columns;
        text.split('\n').enumerate().find_map(|(row, text)| {
            let index = text.find(word)?;
            let first = if row == 0 { parent } else { continuation };
            let origin = inline_layout
                .row_layout(row)
                .resolve(first, continuation)
                .first_visual_origin;
            Some(origin + i32::try_from(mant_ir::geometry::text_width(&text[..index])).unwrap())
        })
    })
}

#[test]
fn name_head_margins_keep_signed_source_capacity_and_exact_word_origins() {
    // All eight complete sources ran five pristine profiles first. Native
    // size_t offset/rmargin are absolute; IR layout stores their signed
    // difference from its five-column manual parent. Nm HEAD sets rmargin
    // from the actual current offset (mdoc_term.c:1006-1016), including ti.
    // Capacity subtracts signed target_consumption before clamping at zero
    // (term.c:113-137). A short name at .ti 0 has negative relative margin;
    // clamping that margin early would falsely accept its direct '(' tail.
    let fixture: OriginFixture =
        serde_json::from_str(include_str!("ordered_name_phases/source_origins.json")).unwrap();
    assert_eq!(fixture.cases.len(), 8);
    for case in fixture.cases {
        let content = roundtrip(&case.source);
        assert_eq!(
            mant_ir::validate_document(content.document.as_ref().unwrap()),
            []
        );
        assert_eq!(
            rows(&content),
            case.rows,
            "{}: complete physical rows",
            case.id
        );
        for (word, column) in &case.native_columns {
            assert_eq!(
                word_column(&content, word),
                Some(*column),
                "{} / {word}",
                case.id
            );
        }
    }
}
