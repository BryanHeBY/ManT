//! Direct `Sh`/`Ss` children execute in their heading owner before HEAD pre.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_codec::encode::{MarkdownNode, render_addressable_markdown, render_markdown};
use mant_ir::{Block, EntryOwner, Inline, LinkTarget, ResolvedContent, Section};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    r#macro: String,
    source: String,
    line: u32,
    heading: String,
    direct_text: Vec<DirectText>,
    italic_prefix: bool,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
struct DirectText {
    text: String,
    column: u32,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("ordered_section_heads/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.cases.len(), 18);
    fixture.cases
}

fn find<'a>(node: &'a Node, case: &Case) -> Option<&'a Node> {
    if node.kind == NodeKind::Block
        && node.macro_token.as_deref() == Some(case.r#macro.as_str())
        && node.line == case.line
    {
        Some(node)
    } else {
        node.children.iter().find_map(|node| find(node, case))
    }
}

fn roundtrip(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let encoded = serde_json::to_string(&mant_protocol::QueryBundle::from(&content)).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&encoded).unwrap();
    let decoded = ResolvedContent::from(decoded);
    assert_eq!(decoded, content);
    decoded
}

fn subject<'a>(content: &'a ResolvedContent, case: &Case) -> &'a Section {
    let document = content.document.as_ref().unwrap();
    assert!(
        document.blocks.is_empty(),
        "{}: no fake root paragraph",
        case.id
    );
    let base = &document.sections[0];
    assert_eq!(base.heading.plain_text(), "BASE");
    let section = if case.r#macro == "Sh" {
        assert_eq!(document.sections.len(), 3);
        assert_eq!(base.children, [] as [Section; 0]);
        &document.sections[1]
    } else {
        assert_eq!(document.sections.len(), 2);
        assert_eq!(base.children.len(), 1);
        &base.children[0]
    };
    assert_eq!(
        document.sections.last().unwrap().heading.plain_text(),
        "NEXT"
    );
    assert_eq!(section.children, [] as [Section; 0]);
    section
}

fn glyphs(nodes: &[Inline], mask: u8) -> Vec<(char, u8)> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            Inline::Text { value } => {
                output.extend(value.chars().map(|character| (character, mask)));
            }
            Inline::Strong { children } => output.extend(glyphs(children, mask | 1)),
            Inline::Emphasis { children } => output.extend(glyphs(children, mask | 2)),
            Inline::Link { children, .. } => output.extend(glyphs(children, mask)),
            Inline::Anchor { .. } => {}
            _ => panic!("original heading phrasing: {node:?}"),
        }
    }
    output
}

fn links(nodes: &[Inline]) -> Vec<(LinkTarget, String)> {
    let mut output = Vec::new();
    for node in nodes {
        match node {
            Inline::Link {
                target, children, ..
            } => {
                output.push((target.clone(), mant_ir::inline_plain_text(children)));
                output.extend(links(children));
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                output.extend(links(children));
            }
            _ => {}
        }
    }
    output
}

#[test]
fn section_prefixes_keep_heading_body_sources_structure_and_actual_markdown_readback() {
    // Every exact source ran pristine ASCII/UTF-8(width78)/HTML/tree/lint
    // before assertions. Some prefix forms trigger the pinned HTML assert;
    // their successful terminal/tree paths supply these expectations.
    // blk_full() emits direct punctuation before HEAD; termp_sh_pre/ss_pre
    // selects bold at HEAD only (mdoc_term.c:1268-1317,1511-1545). Parent,
    // HEAD and BODY source identities survive the complete executed stream.
    for case in cases() {
        let report = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        let original = find(&report.document.root, &case).unwrap();
        let direct = original
            .children
            .iter()
            .take_while(|node| node.kind != NodeKind::Head)
            .map(|node| {
                assert_eq!(node.kind, NodeKind::Text);
                DirectText {
                    text: node.text.clone().unwrap(),
                    column: node.column,
                }
            })
            .collect::<Vec<_>>();
        assert_eq!(
            direct, case.direct_text,
            "{}: original direct children",
            case.id
        );
        let content = roundtrip(&case.source);
        assert!(
            mant_ir::validate_document(content.document.as_ref().unwrap()).is_empty(),
            "{}",
            case.id
        );
        let section = subject(&content, &case);
        assert_eq!(section.heading.plain_text(), case.heading, "{}", case.id);
        for source in [section.source, section.heading.source] {
            assert_eq!(
                source.map(|source| (source.line, source.column)),
                Some((case.line, 2))
            );
        }
        let [
            Block::Paragraph {
                children, source, ..
            },
        ] = section.blocks.as_slice()
        else {
            panic!("{}: same original body paragraph", case.id)
        };
        assert_eq!(mant_ir::inline_plain_text(children), "BODY");
        assert_eq!(
            source.map(|source| (source.line, source.column)),
            Some((case.line + 1, 2))
        );
        let fonts = glyphs(&section.heading.content, 0);
        assert_eq!(
            fonts[0],
            ('(', if case.italic_prefix { 2 } else { 0 }),
            "{}: prefix ran before structural bold",
            case.id
        );
        assert!(
            fonts[1..].iter().all(|(_, mask)| *mask == 0),
            "{}: implicit HEAD bold belongs to Heading",
            case.id
        );
        let expected_links = if case.id.ends_with("-link") {
            vec![(
                LinkTarget::Manual {
                    name: "printf".into(),
                    manual_section: Some("3".into()),
                },
                "printf(3)".into(),
            )]
        } else {
            Vec::new()
        };
        assert_eq!(links(&section.heading.content), expected_links);
        let markdown = render_markdown(&content);
        let imported = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let imported = subject(&imported, &case);
        assert_eq!(
            imported.heading.plain_text(),
            case.heading,
            "{}: actual Markdown reader\n{markdown}",
            case.id
        );
        assert_eq!(
            links(&imported.heading.content),
            expected_links,
            "{}: same precise heading link occurrence",
            case.id
        );
        let [Block::Paragraph { children, .. }] = imported.blocks.as_slice() else {
            panic!("same readback BODY owner")
        };
        assert_eq!(mant_ir::inline_plain_text(children), "BODY");
    }
}

#[test]
fn section_display_prefix_keeps_head_identity_sx_name_binding_and_artifact_owners() {
    // Fresh exact source ran all five pristine profiles first. Native tree
    // identifies Sh HEAD as HEADING_) without the direct '('; mdoc_html.c
    // passes that HEAD to html_make_id, and Sx derives the same deroff key.
    // The executed heading label is complete while its name/target stays in
    // the native HEAD namespace. Presentation never relocates BODY entries.
    let content = roundtrip(include_str!("ordered_section_heads/links-names.1"));
    let document = content.document.as_ref().unwrap();
    assert_eq!(document.sections.len(), 3);
    let section = &document.sections[1];
    assert_eq!(section.heading.plain_text(), "(HEADING)");
    assert_eq!(section.id.as_str(), "heading");
    let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
        panic!("original BODY entry")
    };
    let [item] = items.as_slice() else {
        panic!("one BODY entry")
    };
    assert_eq!(
        item.source
            .as_ref()
            .map(|source| (source.line, source.column)),
        Some((8, 2))
    );
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, ["-alpha"]);
    let name = owner.form(&facts.name_bindings[0].occurrences[0]).unwrap();
    assert_eq!(mant_ir::inline_plain_text(&name), "-alpha");
    let [Block::Paragraph { children, .. }] = document.sections[2].blocks.as_slice() else {
        panic!("original Sx owner")
    };
    assert_eq!(
        links(children),
        [(
            LinkTarget::Section {
                id: section.id.clone()
            },
            "HEADING )".into()
        )]
    );
    let artifact = render_addressable_markdown(&content);
    let entry = artifact
        .nodes()
        .iter()
        .filter_map(|mapped| match mapped.node() {
            MarkdownNode::DocumentEntry {
                owner: EntryOwner::Definition(candidate),
                names,
                section: Some(slot),
                ..
            } if std::ptr::eq(*candidate, item) => Some((mapped, *names, *slot)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(entry.len(), 1);
    assert!(std::ptr::eq(entry[0].1.as_ptr(), facts.names.as_ptr()));
    assert!(std::ptr::eq(
        artifact.section(entry[0].2).unwrap().section(),
        section
    ));
    let entry_bytes = &artifact.text()[entry[0].0.range()];
    assert!(entry_bytes.contains("Payload") && !entry_bytes.contains("HEADING"));
    let heading = artifact
        .nodes()
        .iter()
        .filter(|mapped| match mapped.node() {
            MarkdownNode::DocumentSection { section: slot, .. } => {
                std::ptr::eq(artifact.section(*slot).unwrap().section(), section)
            }
            _ => false,
        })
        .collect::<Vec<_>>();
    assert_eq!(heading.len(), 1);
    assert!(artifact.text()[heading[0].range()].contains("(HEADING)"));
    // Sx selects UNDER in mdoc_term.c; its valid Markdown label retains
    // emphasis inside the link. Verify the actual reader's identity and
    // every visible scalar instead of requiring an unstyled spelling.
    let imported = mant_loader::load_markdown_text(artifact.text(), None).unwrap();
    let imported_next = imported.document.as_ref().unwrap().sections.last().unwrap();
    let [Block::Paragraph { children, .. }] = imported_next.blocks.as_slice() else {
        panic!("same Sx paragraph after actual addressable Markdown readback")
    };
    assert_eq!(
        links(children),
        [(
            LinkTarget::Section {
                id: section.id.clone()
            },
            "HEADING )".into()
        )]
    );
    assert_eq!(
        glyphs(children, 0),
        "HEADING )"
            .chars()
            .map(|character| (character, 2))
            .collect::<Vec<_>>()
    );
}
