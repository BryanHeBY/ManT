//! Native AST owners and accepted Unicode scalar ranges stay distinct.
//!
//! Exact complete sources and five pristine profiles are committed in the
//! adjacent consumer fixture; no ownership expectation comes from product rows.

use libmandoc_rs::{Node, NodeKind, Parser};
use mant_codec::encode::{MarkdownOptions, render_addressable_markdown_with_options};
use mant_ir::{Inline, LinkTarget, visit};
use mant_protocol::SearchScope;

use super::consumer_projections::{round_trip, search};

fn find_block<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
    if node.kind == NodeKind::Block && node.macro_name.as_deref() == Some(name) {
        return Some(node);
    }
    node.children
        .iter()
        .find_map(|child| find_block(child, name))
}

fn contains_word(node: &Node, word: &str) -> bool {
    node.text.as_deref().is_some_and(|text| text.contains(word))
        || node.children.iter().any(|child| contains_word(child, word))
}

#[test]
fn selected_definition_sources_execute_the_asserted_head_and_body_owners() {
    // The exact pristine trees were read before these assertions: column It
    // has two BODYs and an empty HEAD; TP puts HeadWord in HEAD; tag/hang
    // preserve the extended HEAD, independently from their actual BODY.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("consumer_cases.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mechanism = case["mechanism"].as_str().unwrap();
        let macro_name = match mechanism {
            "column-authored-row"
            | "definition-body-event-order"
            | "vspace-ordered-execution"
            | "blank-head-final-receipt"
            | "accepted-invisible-prefix-receipt" => "It",
            "man-definition-lifecycle" => "TP",
            _ => continue,
        };
        let source = case["source"].as_str().unwrap();
        let parsed = Parser::default()
            .parse_bytes("consumer-owners.1", source.as_bytes())
            .unwrap();
        let block = find_block(&parsed.document.root, macro_name).unwrap();
        let head = block
            .children
            .iter()
            .find(|node| node.kind == NodeKind::Head)
            .unwrap();
        let bodies = block
            .children
            .iter()
            .filter(|node| node.kind == NodeKind::Body)
            .collect::<Vec<_>>();
        assert!(!bodies.is_empty(), "{}", case["name"]);
        match mechanism {
            "column-authored-row" => {
                assert_eq!(head.children.len(), 0);
                assert_eq!(bodies.len(), 2);
                assert!(contains_word(bodies[1], "RightWord"));
            }
            "man-definition-lifecycle"
            | "definition-body-event-order"
            | "accepted-invisible-prefix-receipt" => {
                assert!(contains_word(head, "HeadWord"));
                assert!(!contains_word(head, "AFTER"));
            }
            "vspace-ordered-execution" => {
                assert!(contains_word(head, "D"));
                assert!(contains_word(bodies[0], "BodyWord"));
                assert!(!contains_word(head, "BodyWord"));
            }
            "blank-head-final-receipt" => assert_eq!(bodies[0].children.len(), 0),
            _ => unreachable!(),
        }
    }
}

#[derive(Default)]
struct OwnedScalars {
    text: String,
    links: Vec<(String, usize, usize)>,
}

impl<'ir> visit::Visit<'ir> for OwnedScalars {
    fn visit_inline(&mut self, inline: &'ir Inline) {
        match inline {
            Inline::Text { value } | Inline::Code { value } => self.text.push_str(value),
            Inline::LineBreak { .. } => self.text.push('\n'),
            Inline::Link {
                target: LinkTarget::External { uri },
                ..
            } => {
                let start = self.text.chars().count();
                visit::walk_inline(self, inline);
                self.links
                    .push((uri.clone(), start, self.text.chars().count()));
            }
            _ => visit::walk_inline(self, inline),
        }
    }
}

#[test]
fn pending_and_unicode_cells_keep_exact_scalar_owner_boundaries() {
    // term.c::encode1 assigns pending P to its original accepted cell; the
    // next link owns only Y/α中. These are scalar offsets, not UTF-8 bytes
    // or display cells (中 is one scalar and two terminal columns).
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("consumer_cases.json")).unwrap();
    let unicode = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["name"] == "unicode-owned-cells")
        .unwrap();
    for (source, expected_text, range) in [
        (
            super::load_case("word_owner_pending_prefix").source,
            "PY",
            (1, 2),
        ),
        (
            unicode["source"].as_str().unwrap().to_owned(),
            "Pα中: https://ex.org AFTER",
            (1, 3),
        ),
    ] {
        let query = round_trip(&source);
        let section = query
            .document
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading.plain_text() == "DESCRIPTION")
            .unwrap();
        let mut owned = OwnedScalars::default();
        for block in &section.blocks {
            visit::Visit::visit_block(&mut owned, block);
        }
        assert_eq!(owned.text, expected_text);
        assert_eq!(owned.links, [("https://ex.org".into(), range.0, range.1)]);
        let label = owned
            .text
            .chars()
            .skip(range.0)
            .take(range.1 - range.0)
            .collect::<String>();
        let found = search(&query, SearchScope::Visible, &label);
        assert_eq!(
            found
                .matches
                .iter()
                .map(|hit| hit.occurrences.len())
                .sum::<usize>(),
            1
        );
        let artifact = render_addressable_markdown_with_options(
            &query,
            MarkdownOptions {
                native_text: true,
                ..MarkdownOptions::ADDRESSABLE
            },
        );
        let occurrence = &found.matches[0].occurrences[0];
        assert_eq!(
            &artifact.text()[usize::try_from(occurrence.markdown.start_byte).unwrap()
                ..usize::try_from(occurrence.markdown.end_byte).unwrap()],
            label
        );
    }
}
