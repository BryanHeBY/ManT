//! A producer's rejected child cannot leave a false local exhaustive claim.
#[path = "../src/semantic_test_read.rs"]
mod semantic_read;
use mant_ir::{Block, Document, SemanticIndex, ValueDomain};
use mant_loader::load_markdown_text;
use mant_protocol::{EntryProjection, ExcerptSelection, OutlineNode};
use mant_query::build_outline_projection;
use std::fmt::Write;

fn source(children: &str) -> String {
    format!(
        "# Probe\n\n<!-- mant:entries role=option case=sensitive -->\n- `--mode MODE`: Choose.\n\n  <!-- mant:domain choices=exhaustive -->\n\n{children}\n"
    )
}

#[test]
fn every_rejected_position_invalidates_only_the_local_exhaustive_claim() {
    for rejected in 0..3 {
        let mut children = String::new();
        for (index, name) in ["auto", "manual", "other"].iter().enumerate() {
            writeln!(
                children,
                "  - `{name}`{}",
                if index == rejected {
                    ""
                } else {
                    ": Description."
                }
            )
            .unwrap();
        }
        let query = load_markdown_text(
            &source(&format!(
                "  <!-- mant:entries role=value case=sensitive -->\n{children}"
            )),
            None,
        )
        .unwrap();
        let doc = query.document.as_ref().unwrap();
        let Block::List { items, .. } = &doc.blocks[0] else {
            panic!("list")
        };
        assert!(items[0].entry.as_ref().unwrap().value_domain.is_none());
        assert!(doc.diagnostics.iter().any(|d| d.code.as_deref()
            == Some("markdown.semantic-value-domain")
            && d.source.is_some()));
        let copied: Document = serde_json::from_str(&serde_json::to_string(doc).unwrap()).unwrap();
        assert_eq!(&copied, doc);
        let index = SemanticIndex::build(&copied);
        assert_eq!(index.root()[0].children.len(), 2);
        assert!(!matches!(
            index.root()[0].value_domain,
            Some(ValueDomain::Choices { exhaustive: true })
        ));
        let outline = build_outline_projection(
            &query,
            EntryProjection::All,
            Some(mant_protocol::ContentSelector::path("root/e1")),
        )
        .unwrap();
        assert!(!outline.semantics_complete);
        let OutlineNode::DocumentEntry {
            value_domain,
            children,
            ..
        } = &outline.nodes[0]
        else {
            panic!("entry")
        };
        assert_eq!(children.len(), 2);
        assert!(
            !serde_json::to_string(value_domain)
                .unwrap()
                .contains("\"exhaustive\":true")
        );
        let excerpt = semantic_read::semantic_excerpt(&query, &["--mode"]).unwrap();
        let [ExcerptSelection::DocumentEntry { entry, .. }] = &excerpt.selections[..] else {
            panic!("entry")
        };
        assert!(
            entry
                .entry_owner()
                .unwrap()
                .facts()
                .unwrap()
                .value_domain
                .is_none()
        );
        // Rejected head and all original list items remain visible.
        assert!(
            mant_render::render_query_text(&query).contains(["auto", "manual", "other"][rejected])
        );
    }
}

#[test]
fn independent_lists_and_nested_owners_do_not_share_rejection_state() {
    let query = load_markdown_text(&source("  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `manual`\n"), None).unwrap();
    let index = SemanticIndex::build(query.document.as_ref().unwrap());
    assert!(!matches!(
        index.root()[0].value_domain,
        Some(ValueDomain::Choices { exhaustive: true })
    ));

    let independent = format!(
        "{}\n<!-- mant:entries role=value case=sensitive -->\n- `unrelated`\n",
        source("  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.")
    );
    let query = load_markdown_text(&independent, None).unwrap();
    let index = SemanticIndex::build(query.document.as_ref().unwrap());
    assert_eq!(
        index.root()[0].value_domain,
        Some(ValueDomain::Choices { exhaustive: true })
    );

    let nested = source(
        "  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n    <!-- mant:entries role=value case=sensitive -->\n    - `deeper`\n",
    );
    let query = load_markdown_text(&nested, None).unwrap();
    assert_eq!(
        SemanticIndex::build(query.document.as_ref().unwrap()).root()[0].value_domain,
        Some(ValueDomain::Choices { exhaustive: true })
    );
}

#[test]
fn empty_and_rejected_declarations_cannot_complete_a_partial_enumeration() {
    for suffix in [
        "  <!-- mant:entries role=value case=sensitive -->\n",
        "  <!-- mant:entries role=invalid -->\n  - `lost`: Visible.\n",
        "  <!-- mant:entries role=command case=sensitive -->\n  - `run`: Not a value.\n",
        "  <!-- mant:entries role=value case=sensitive -->\n  -\n",
    ] {
        let source = source(&format!(
            "  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n{suffix}"
        ));
        let query = load_markdown_text(&source, None).unwrap();
        let index = SemanticIndex::build(query.document.as_ref().unwrap());
        assert!(
            !matches!(
                index.root()[0].value_domain,
                Some(ValueDomain::Choices { exhaustive: true })
            ),
            "{source}"
        );
        assert!(!index.root()[0].children.is_empty());
    }
}

#[test]
fn explicitly_open_choices_can_retain_successful_partial_evidence() {
    let source = source(
        "  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n  - `manual`\n",
    )
    .replace("choices=exhaustive", "choices=open");
    let query = load_markdown_text(&source, None).unwrap();
    let index = SemanticIndex::build(query.document.as_ref().unwrap());
    assert_eq!(
        index.root()[0].value_domain,
        Some(ValueDomain::Choices { exhaustive: false })
    );
}

fn indent(source: &str, columns: usize) -> String {
    let prefix = " ".repeat(columns);
    source.lines().fold(String::new(), |mut output, line| {
        writeln!(output, "{prefix}{line}").unwrap();
        output
    })
}

fn ordinary_containers(source: &str, marker: &str, depth: usize) -> String {
    (0..depth).fold(source.to_owned(), |body, _| {
        format!(
            "{marker} Ordinary container.\n\n{}",
            indent(&body, marker.len() + 1)
        )
    })
}

#[test]
fn rejected_declarations_flow_through_ordinary_containers_to_the_semantic_owner() {
    let valid = "<!-- mant:entries role=value case=sensitive -->\n- `manual`: Manual mode.\n";
    for marker in ["-", "1."] {
        for depth in [1, 3] {
            for newline in ["\n", "\r\n", "\r"] {
                let make_source = |declaration| {
                    source(&format!(
                        "  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n  Additional documented modes:\n\n{}",
                        indent(&ordinary_containers(declaration, marker, depth), 2)
                    ))
                    .replace('\n', newline)
                };
                // The control proves that the nested manual value belongs to
                // this parent's enumeration, not to the ordinary containers.
                let control = load_markdown_text(&make_source(valid), None).unwrap();
                let index = SemanticIndex::build(control.document.as_ref().unwrap());
                assert!(
                    control.document.as_ref().unwrap().diagnostics.is_empty(),
                    "{:?}: {:?}",
                    make_source(valid),
                    control.document.as_ref().unwrap().diagnostics
                );
                assert_eq!(index.root()[0].children.len(), 2);
                assert_eq!(index.root()[0].children[0].names, ["auto"]);
                assert_eq!(index.root()[0].children[1].names, ["manual"]);
                assert_eq!(
                    index.root()[0].value_domain,
                    Some(ValueDomain::Choices { exhaustive: true })
                );

                for rejected in [
                    "<!-- mant:entries role=invalid case=sensitive -->\n- `manual`: Manual mode.\n",
                    "<!-- mant:entries role=value case=invalid -->\n- `manual`: Manual mode.\n",
                    "<!-- mant:entries role=value case=sensitive -->\n\nInterrupting paragraph.\n\n- `manual`: Manual mode.\n",
                ] {
                    let input = make_source(rejected);
                    let query = load_markdown_text(&input, None).unwrap();
                    let doc = query.document.as_ref().unwrap();
                    let Block::List { items, .. } = &doc.blocks[0] else {
                        panic!("parent list")
                    };
                    assert!(
                        items[0].entry.as_ref().unwrap().value_domain.is_none(),
                        "{input:?}"
                    );
                    assert!(doc.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code.as_deref() == Some("markdown.semantic-value-domain")
                            && diagnostic.source.is_some()
                    }));
                    let copied: Document =
                        serde_json::from_str(&serde_json::to_string(doc).unwrap()).unwrap();
                    assert_eq!(&copied, doc);
                    let index = SemanticIndex::build(&copied);
                    assert_eq!(index.root()[0].children.len(), 1);
                    assert_eq!(index.root()[0].children[0].names, ["auto"]);
                    assert_eq!(
                        index.root()[0].value_domain,
                        Some(ValueDomain::Choices { exhaustive: false })
                    );
                    let outline = build_outline_projection(
                        &query,
                        EntryProjection::All,
                        Some(mant_protocol::ContentSelector::path("root/e1")),
                    )
                    .unwrap();
                    assert!(!outline.semantics_complete);
                    assert!(
                        !serde_json::to_string(&outline)
                            .unwrap()
                            .contains("\"exhaustive\":true")
                    );
                    let excerpt = semantic_read::semantic_excerpt(&query, &["--mode"]).unwrap();
                    let [ExcerptSelection::DocumentEntry { entry, .. }] = &excerpt.selections[..]
                    else {
                        panic!("parent entry")
                    };
                    assert!(
                        entry
                            .entry_owner()
                            .unwrap()
                            .facts()
                            .unwrap()
                            .value_domain
                            .is_none()
                    );
                    let visible = mant_render::render_query_text(&query);
                    assert!(visible.contains("Ordinary container."));
                    assert!(visible.contains("Manual mode."));
                }
            }
        }
    }
}

#[test]
fn recognized_children_stop_rejections_from_their_ordinary_descendants() {
    for marker in ["-", "1."] {
        let rejected = ordinary_containers(
            "<!-- mant:entries role=invalid case=sensitive -->\n- `manual`: Manual mode.\n",
            marker,
            2,
        );
        let child = format!(
            "<!-- mant:entries role=value case=sensitive -->\n- `auto`: Automatic.\n\n  <!-- mant:domain choices=exhaustive -->\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `fast`: Fast.\n\n  Additional documented modes:\n\n{}",
            indent(&rejected, 2)
        );
        let input = source(&indent(&ordinary_containers(&child, marker, 2), 2));
        let query = load_markdown_text(&input, None).unwrap();
        let index = SemanticIndex::build(query.document.as_ref().unwrap());
        let parent = &index.root()[0];
        assert_eq!(parent.children.len(), 1);
        assert_eq!(
            parent.value_domain,
            Some(ValueDomain::Choices { exhaustive: true })
        );
        let child = &parent.children[0];
        assert_eq!(child.children.len(), 1);
        assert_eq!(
            child.value_domain,
            Some(ValueDomain::Choices { exhaustive: false })
        );
    }
}

#[test]
fn leading_removed_comments_do_not_change_item_ownership() {
    for marker in ["-", "1."] {
        for newline in ["\n", "\r\n", "\r"] {
            for role in ["value", "invalid"] {
                let input = source(&format!(
                    "  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n  Other values:\n\n  {marker}\n{}",
                    indent(&format!("<!-- mant:entries role={role} case=sensitive -->\n- `manual`: Manual mode.\n"), marker.len() + 3)
                )).replace('\n', newline);
                let query = load_markdown_text(&input, None).unwrap();
                let doc = query.document.as_ref().unwrap();
                let index = SemanticIndex::build(doc);
                let parent = &index.root()[0];
                assert_eq!(
                    parent.children.len(),
                    if role == "value" { 2 } else { 1 },
                    "{input:?}"
                );
                assert_eq!(
                    parent.value_domain,
                    Some(ValueDomain::Choices {
                        exhaustive: role == "value"
                    }),
                    "{input:?}"
                );
                assert!(mant_render::render_query_text(&query).contains("Manual mode."));
                if role == "invalid" {
                    assert!(
                        doc.diagnostics
                            .iter()
                            .any(|d| d.code.as_deref() == Some("markdown.semantic-value-domain"))
                    );
                }
            }
        }
    }
}
