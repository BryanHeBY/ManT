//! Direct It siblings execute before HEAD pre without losing their owner.

use libmandoc_rs::{MacroToken, MdocMacro, Node, NodeKind};
use mant_ir::{Block, Inline, LinkTarget, ResolvedContent, inline_plain_text};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    count: usize,
    oracle_sha256: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    source: String,
    term: String,
    direct_prefix: String,
    column: bool,
    manual_links: usize,
}

fn first_item(node: &Node) -> Option<&Node> {
    if node.kind == NodeKind::Block && node.macro_token == Some(MacroToken::Mdoc(MdocMacro::It)) {
        Some(node)
    } else {
        node.children.iter().find_map(first_item)
    }
}

fn roundtrip(source: &str) -> ResolvedContent {
    let query = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let json = mant_render::render_query_json(&query, false).unwrap();
    let restored: mant_protocol::QueryBundle = serde_json::from_str(&json).unwrap();
    let restored: ResolvedContent = restored.into();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&json).unwrap(),
        serde_json::to_value(mant_protocol::QueryBundle::from(&restored)).unwrap()
    );
    restored
}

fn reference_labels(nodes: &[Inline], labels: &mut Vec<String>) {
    for node in nodes {
        match node {
            Inline::Link {
                target, children, ..
            } => {
                if matches!(target, LinkTarget::Manual { .. }) {
                    labels.push(inline_plain_text(children));
                }
                reference_labels(children, labels);
            }
            Inline::Strong { children } | Inline::Emphasis { children } => {
                reference_labels(children, labels);
            }
            _ => {}
        }
    }
}

fn assert_bound_owner(case: &Case, item: &mant_ir::DefinitionItem, content: &ResolvedContent) {
    let owner = mant_ir::EntryOwner::Definition(item);
    if matches!(
        case.id.as_str(),
        "tag-role-fl" | "tag-role-cm" | "tag-role-ev"
    ) {
        assert!(owner.facts().is_some(), "{}: semantic owner lost", case.id);
    }
    let Some(facts) = owner.facts() else { return };
    if case.id == "tag-role-fl" {
        assert_eq!(facts.names, ["-a"]);
    } else if case.id == "tag-role-cm" {
        // Literal Cm heads keep their complete Term identity; only an Fl
        // receipt establishes an option selector inside outside punctuation.
        assert_eq!(facts.names, ["(action)"]);
    } else if case.id == "tag-role-ev" {
        // An enclosing '(' is not an environment-variable declaration prefix.
        assert_eq!(facts.names, [] as [String; 0]);
    }
    for form in owner.forms().unwrap().iter() {
        assert_eq!(inline_plain_text(form), case.term, "{}", case.id);
    }
    for binding in &facts.name_bindings {
        let name = &facts.names[binding.name];
        for occurrence in &binding.occurrences {
            assert_eq!(inline_plain_text(&owner.form(occurrence).unwrap()), *name);
        }
        let explanation = mant_query::explain_query(
            content,
            &mant_protocol::ExplanationQuery {
                entry: name.clone(),
                options: mant_protocol::ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(
            explanation
                .evidence
                .iter()
                .filter(|evidence| evidence
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, mant_protocol::EvidenceBasis::Name { .. })))
                .count(),
            1,
            "{}: original semantic owner",
            case.id
        );
    }
}

fn strengths(nodes: &[Inline], strong: bool, output: &mut Vec<(char, bool)>) {
    for node in nodes {
        match node {
            Inline::Strong { children } => strengths(children, true, output),
            Inline::Emphasis { children } | Inline::Link { children, .. } => {
                strengths(children, strong, output);
            }
            Inline::Text { value } | Inline::Code { value } | Inline::Equation { value, .. } => {
                output.extend(value.chars().map(|character| (character, strong)));
            }
            _ => {}
        }
    }
}

fn assert_prefix_font(case: &Case, item: &mant_ir::DefinitionItem) {
    if !matches!(case.id.as_str(), "diag-ar" | "tag-role-fl") {
        return;
    }
    // Exact UTF-8 receipts: the direct '(' is never overstruck. Diagnostic
    // HEAD pre makes its own ')' bold; Fl returns before the ordinary ')'.
    let mut characters = Vec::new();
    strengths(&item.terms[0], false, &mut characters);
    assert_eq!(characters[0], ('(', false));
    if case.id == "diag-ar" {
        assert_eq!(characters.last(), Some(&(')', true)));
        assert!(
            characters[1..]
                .iter()
                .all(|(ch, strong)| ch.is_whitespace() || *strong)
        );
    } else {
        assert_eq!(
            characters,
            [('(', false), ('-', true), ('a', true), (')', false)]
        );
    }
}

#[test]
fn direct_definition_prefixes_keep_native_order_and_accepted_head_ranges() {
    // Every complete source ran pinned ASCII/UTF-8/tree/lint first.
    // mdoc_macro.c::blk_full() emits DELIMO before allocating HEAD;
    // mdoc_term.c::print_mdoc_node() visits that sibling before
    // termp_it_pre(HEAD) selects NOSPACE, font, margin and field flags.
    let fixtures: Fixture =
        serde_json::from_str(include_str!("ordered_definition_heads/cases.json")).unwrap();
    assert_eq!(fixtures.count, 55);
    assert_eq!(fixtures.count, fixtures.cases.len());
    assert_eq!(
        fixtures.oracle_sha256,
        "9f7aecf5aec662d87557db9f273b2ab248f379058d8d2d5193bf1c1e7b88719e"
    );
    for case in fixtures.cases {
        let report = libmandoc_rs::Parser::default()
            .parse_bytes("definition-prefix.1", case.source.as_bytes())
            .unwrap();
        let original = first_item(&report.document.root).unwrap();
        let head_index = original
            .children
            .iter()
            .position(|part| part.kind == NodeKind::Head)
            .unwrap();
        assert_eq!(
            original.children[..head_index]
                .iter()
                .filter_map(|node| node.text.as_deref())
                .collect::<String>(),
            case.direct_prefix,
            "{}: actual native sibling ownership",
            case.id
        );
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
        if case.column {
            let Block::Table { rows, .. } = &section.blocks[0] else {
                panic!("{}: column ownership changed", case.id)
            };
            let Block::Paragraph { children, .. } = &rows[0].cells[0].blocks[0] else {
                panic!("{}: missing column paragraph", case.id)
            };
            assert_eq!(inline_plain_text(children), case.term);
            continue;
        }
        let [Block::DefinitionList { items, .. }] = section.blocks.as_slice() else {
            panic!("{}: prefix created another block: {section:#?}", case.id)
        };
        assert_eq!(items.len(), 1, "{}", case.id);
        let item = &items[0];
        assert_eq!(item.terms.len(), 1, "{}", case.id);
        assert_eq!(inline_plain_text(&item.terms[0]), case.term, "{}", case.id);
        let source = item.source.unwrap();
        assert_eq!(
            (source.line, source.column),
            (original.line, original.column)
        );
        assert_bound_owner(&case, item, &content);
        assert_prefix_font(&case, item);
        let mut labels = Vec::new();
        reference_labels(&item.terms[0], &mut labels);
        assert_eq!(labels.len(), case.manual_links, "{}", case.id);
        for label in labels {
            assert_eq!(
                label, "printf(3)",
                "{}: outside prefix entered link",
                case.id
            );
        }
        let text = mant_render::render_query_text(&content);
        if case.term != "(" {
            assert_eq!(text.matches(&case.term).count(), 1, "{}: {text}", case.id);
        }
        assert_eq!(
            text.matches("BODY.").count(),
            usize::from(case.source.contains("BODY."))
        );
        let markdown = mant_codec::encode::render_markdown(&content);
        let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
        let restored_text = mant_render::render_query_text(&readback);
        if case.term != "(" {
            assert_eq!(
                restored_text.matches(&case.term).count(),
                1,
                "{}: real Markdown readback\n{markdown}\n{restored_text}",
                case.id
            );
        }
    }
}
