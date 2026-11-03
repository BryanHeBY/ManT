//! Bibliography fields retain their posts in every native output owner.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{
    Block, Inline, LinkTarget, ResolvedContent, inline_plain_text,
    visit::{self, Visit},
};
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
    heading: String,
    context: String,
    expected: String,
}

fn find<'a>(node: &'a Node, name: &str, kind: NodeKind) -> Option<&'a Node> {
    if node.kind == kind && node.macro_token.as_deref() == Some(name) {
        Some(node)
    } else {
        node.children.iter().find_map(|node| find(node, name, kind))
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

fn rendered_description(content: &ResolvedContent, heading: &str) -> String {
    let rendered = mant_render::render_query_man(content);
    let (_, rest) = rendered.split_once(&format!("{heading}\n")).unwrap();
    let (body, _) = rest.split_once("NEXT\n").unwrap();
    body.trim_end_matches('\n')
        .split('\n')
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn bibliography_field_posts_and_sentence_cells_keep_the_caller_output_owner() {
    // Every complete source ran pristine ASCII/UTF-8(width120)/HTML/tree
    // first. termp_rs_pre() has only SEE ALSO spacing, no return post;
    // termp____post() emits comma/final period and explicitly arms
    // SENTENCE (mdoc_term.c:1103-1111,1844-1870). Empty/control words
    // consume that register at their real term_word entry (term.c:573-589).
    let fixture: Fixture =
        serde_json::from_str(include_str!("reference_scope_children/cases.json")).unwrap();
    assert_eq!(fixture.count, 10);
    assert_eq!(fixture.count, fixture.cases.len());
    for case in fixture.cases {
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
            .find(|section| section.heading.plain_text() == case.heading)
            .unwrap();
        let payload = match (case.context.as_str(), section.blocks.as_slice()) {
            ("prose", [Block::Paragraph { children, .. }])
            | ("literal", [Block::Preformatted { children, .. }]) => children,
            ("head", [Block::DefinitionList { items, .. }]) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0].terms.len(), 1);
                let parsed = Parser::default()
                    .parse_bytes(&case.id, case.source.as_bytes())
                    .unwrap();
                let item = find(&parsed.document.root, "It", NodeKind::Block).unwrap();
                let head = find(item, "It", NodeKind::Head).unwrap();
                let bibliography = find(head, "Rs", NodeKind::Block).unwrap();
                if case.heading == "SEE ALSO" {
                    assert_eq!(
                        bibliography.section,
                        libmandoc_rs::NormalizedSection::SeeAlso
                    );
                }
                &items[0].terms[0].content
            }
            (_, other) => panic!("{}: changed bibliography owner: {other:#?}", case.id),
        };
        assert_eq!(inline_plain_text(payload), case.expected, "{}", case.id);
        if case.context != "head" {
            assert_eq!(
                rendered_description(&content, &case.heading),
                case.expected,
                "{}",
                case.id
            );
            let markdown = mant_codec::encode::render_markdown(&content);
            let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
            assert_eq!(
                rendered_description(&read, &case.heading),
                case.expected,
                "{}: {markdown}",
                case.id
            );
        }
        if case.id == "rs-url-target" {
            let mut links = Links::default();
            links.visit_document(document);
            assert_eq!(
                links.0,
                [(
                    "https://example.test".into(),
                    LinkTarget::External {
                        uri: "https://example.test".into()
                    }
                )]
            );
        }
    }
}

#[derive(Default)]
struct Links(Vec<(String, LinkTarget)>);

impl<'ir> Visit<'ir> for Links {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        if let Inline::Link {
            children, target, ..
        } = inline
        {
            self.0.push((inline_plain_text(children), target.clone()));
        }
        visit::walk_inline(self, inline);
    }
}
