//! Complete container children retain source order and their original owners.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_ir::{
    Block, Inline, LinkTarget, ResolvedContent, inline_plain_text,
    visit::{self, Visit},
};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    oracle_sha256: String,
    count: usize,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    r#macro: String,
    context: String,
    direct_text: Vec<String>,
    expected: String,
}

fn cases() -> Vec<Case> {
    let fixture: Fixture =
        serde_json::from_str(include_str!("ordered_container_children/cases.json")).unwrap();
    assert_eq!(
        fixture.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    assert_eq!(fixture.count, 285);
    assert_eq!(fixture.count, fixture.cases.len());
    fixture.cases
}

fn original_container<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if node.kind == NodeKind::Block && node.macro_token.as_deref() == Some(name) {
        Some(node)
    } else {
        node.children
            .iter()
            .find_map(|child| original_container(child, name))
    }
}

fn roundtrip(source: &str) -> ResolvedContent {
    let content = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let original = mant_protocol::QueryBundle::from(&content);
    let json = serde_json::to_string(&original).unwrap();
    let decoded: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    assert_eq!(
        serde_json::to_value(original).unwrap(),
        serde_json::to_value(&decoded).unwrap()
    );
    decoded.into()
}

fn description(content: &ResolvedContent) -> &[Block] {
    &content
        .document
        .as_ref()
        .unwrap()
        .sections
        .iter()
        .find(|section| section.heading.plain_text() == "DESCRIPTION")
        .unwrap()
        .blocks
}

fn one_inline_block<'a>(blocks: &'a [Block], case: &Case) -> &'a [Inline] {
    match blocks {
        [Block::Paragraph { children, .. }] if case.context != "literal" => children,
        [Block::Preformatted { children, .. }] if case.context == "literal" => children,
        other => panic!("{}: split or changed output owner: {other:#?}", case.id),
    }
}

fn owned_payload<'a>(content: &'a ResolvedContent, case: &Case) -> &'a [Inline] {
    let blocks = description(content);
    match case.context.as_str() {
        "tag" => {
            let [Block::DefinitionList { items, .. }] = blocks else {
                panic!("{}: lost definition owner", case.id)
            };
            assert_eq!(items.len(), 1, "{}", case.id);
            assert_eq!(items[0].terms.len(), 1, "{}", case.id);
            assert_eq!(inline_plain_text(&items[0].terms[0]), "LABEL");
            one_inline_block(&items[0].description, case)
        }
        "column" => {
            let [Block::Table { rows, .. }] = blocks else {
                panic!("{}: lost table owner", case.id)
            };
            assert_eq!(rows.len(), 1, "{}", case.id);
            assert_eq!(rows[0].cells.len(), 2, "{}", case.id);
            assert_eq!(
                inline_plain_text(one_inline_block(&rows[0].cells[1].blocks, case)),
                "RIGHT",
                "{}: following field",
                case.id
            );
            one_inline_block(&rows[0].cells[0].blocks, case)
        }
        _ => one_inline_block(blocks, case),
    }
}

#[test]
fn container_children_match_native_payloads_and_preserve_block_ownership_after_json() {
    // Every complete source ran pristine ASCII/UTF-8(width120)/HTML/tree
    // before these expectations were recorded. print_mdoc_node() visits
    // all children (mdoc_term.c:406-407); Xo has no pre/post. Quote pre/post
    // belong only to BODY/ELEM (1594-1703), so direct BLOCK punctuation runs
    // outside the quotes even without DELIMO/DELIMC. The 252 quote cases
    // cover nine macros x seven payloads x prose/tag/column/literal owners.
    // Device origin and inter-field padding are excluded; accepted spaces
    // inside each payload, including EOS double spacing, are exact.
    for case in cases() {
        let parsed = Parser::default()
            .parse_bytes(&case.id, case.source.as_bytes())
            .unwrap();
        let owner = original_container(&parsed.document.root, &case.r#macro).unwrap();
        let direct: Vec<_> = owner
            .children
            .iter()
            .filter(|node| node.kind == NodeKind::Text)
            .map(|node| node.text.clone().unwrap())
            .collect();
        assert_eq!(direct, case.direct_text, "{}: original AST", case.id);
        if case.source.contains("Li \"!\" ,\n") {
            let comma = owner
                .children
                .iter()
                .find(|node| node.text.as_deref() == Some(","))
                .unwrap();
            assert!(
                !comma.flags.delimiter_close,
                "{}: accepted non-DELIMC tail must execute too",
                case.id
            );
        }
        let content = roundtrip(&case.source);
        assert!(
            mant_ir::validate_document(content.document.as_ref().unwrap()).is_empty(),
            "{}",
            case.id
        );
        assert_eq!(
            inline_plain_text(owned_payload(&content, &case)),
            case.expected,
            "{}: complete accepted payload",
            case.id
        );
    }
}

fn rendered_description(content: &ResolvedContent) -> String {
    let rendered = mant_render::render_query_text(content);
    let (_, rest) = rendered.split_once("DESCRIPTION\n").unwrap();
    let (body, _) = rest.split_once("NEXT\n").unwrap();
    body.trim_end_matches('\n')
        .lines()
        .map(str::trim_start)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn container_payloads_reach_native_reading_and_the_actual_markdown_reader() {
    // Same exact oracle sources as the ownership matrix above. This pass
    // checks consumer-visible rows separately from the producer's IR owner.
    for case in cases()
        .into_iter()
        .filter(|case| matches!(case.context.as_str(), "prose" | "literal"))
    {
        let content = roundtrip(&case.source);
        assert_eq!(rendered_description(&content), case.expected, "{}", case.id);
        let markdown = mant_codec::encode::render_markdown(&content);
        let read = mant_loader::load_markdown_text(&markdown, None).unwrap();
        assert_eq!(
            rendered_description(&read),
            case.expected,
            "{}: Markdown readback\n{markdown}",
            case.id
        );
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

#[test]
fn container_delimiters_remain_outside_the_single_manual_link_occurrence() {
    for case in cases()
        .into_iter()
        .filter(|case| matches!(case.id.as_str(), "xo-link" | "quote-link" | "quote-target"))
    {
        let content = roundtrip(&case.source);
        let mut links = Links::default();
        links.visit_document(content.document.as_ref().unwrap());
        assert_eq!(
            links.0,
            [(
                "printf(3)".into(),
                LinkTarget::Manual {
                    name: "printf".into(),
                    manual_section: Some("3".into()),
                }
            )],
            "{}: wrappers did not capture punctuation or duplicate execution",
            case.id
        );
    }
}

#[test]
fn enclosure_font_pop_precedes_closing_punctuation_and_retains_previous_font() {
    // Exact quote-font oracle: BODY's Ar is underlined; both generated
    // quotes and authored parentheses, OUTER and fP PREVIOUS are bold.
    // CVS print_mdoc_node():410 restores BODY font before quote post.
    let case = cases()
        .into_iter()
        .find(|case| case.id == "quote-font")
        .unwrap();
    let content = roundtrip(&case.source);
    super::font_boundaries::assert_style(&content, "VALUE", 2);
    for text in ["(", "“", "”", ")", "OUTER", "PREVIOUS"] {
        super::font_boundaries::assert_style(&content, text, 1);
    }
}

#[test]
fn direct_font_and_function_prefixes_precede_their_head_or_body_font_scope() {
    // Exact bf-scoped-font and fo-font UTF-8 probes underline the direct
    // '(' in the enclosing italic font. termp_bf_pre selects bold only at
    // BODY; termp_fo_pre selects it only at HEAD (mdoc_term.c:1750-1817).
    for case in cases()
        .into_iter()
        .filter(|case| matches!(case.id.as_str(), "bf-scoped-font" | "fo-font"))
    {
        let content = roundtrip(&case.source);
        for text in ["(", "OUTER", "PREVIOUS"] {
            super::font_boundaries::assert_style(&content, text, 2);
        }
        let bold = if case.id == "fo-font" {
            "function"
        } else {
            "BODY"
        };
        super::font_boundaries::assert_style(&content, bold, 1);
    }
}

#[test]
fn enclosure_children_keep_nested_structure_between_their_original_paragraphs() {
    // This exact source ran pristine ASCII/UTF-8/HTML/tree. BODY's list
    // executes before the explicit quote post, which marks the original
    // BODY ended (print_mdoc_node, mdoc_term.c:406-431). Generated marks do
    // not flatten the list or manufacture another paragraph owner.
    let source = include_str!("ordered_container_children/structural.1");
    let content = roundtrip(source);
    let [
        Block::Paragraph {
            children: before,
            source: before_source,
            ..
        },
        Block::DefinitionList { items, .. },
        Block::Paragraph {
            children: after,
            source: after_source,
            ..
        },
    ] = description(&content)
    else {
        panic!("changed structural enclosure ownership")
    };
    assert_eq!(inline_plain_text(before), "BEFORE (“");
    assert_eq!(inline_plain_text(after), "”) AFTER");
    assert_eq!(before_source.unwrap().line, 5);
    assert_eq!(after_source.unwrap().line, 6);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].source.unwrap().line, 8);
    assert_eq!(inline_plain_text(&items[0].terms[0]), "LABEL");
    let [Block::Paragraph { children, .. }] = items[0].description.as_slice() else {
        panic!("lost nested definition body")
    };
    assert_eq!(inline_plain_text(children), "BODY");
}
