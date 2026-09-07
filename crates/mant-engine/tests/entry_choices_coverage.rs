//! A producer's rejected child cannot leave a false local exhaustive claim.
use mant_engine::{build_outline_projection, query_markdown_text, select_explanation};
use mant_ir::{Block, Document, SemanticIndex, ValueDomain};
use mant_protocol::{EntryProjection, ExcerptSelection, OutlineNode};
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
        let query = query_markdown_text(
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
        let outline =
            build_outline_projection(&query, EntryProjection::All, Some("--mode".into())).unwrap();
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
        let excerpt = select_explanation(&query, "--mode").unwrap();
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
            mant_engine::render_query_text(&query).contains(["auto", "manual", "other"][rejected])
        );
    }
}

#[test]
fn independent_lists_and_nested_owners_do_not_share_rejection_state() {
    let query = query_markdown_text(&source("  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n  <!-- mant:entries role=value case=sensitive -->\n  - `manual`\n"), None).unwrap();
    let index = SemanticIndex::build(query.document.as_ref().unwrap());
    assert!(!matches!(
        index.root()[0].value_domain,
        Some(ValueDomain::Choices { exhaustive: true })
    ));

    let independent = format!(
        "{}\n<!-- mant:entries role=value case=sensitive -->\n- `unrelated`\n",
        source("  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.")
    );
    let query = query_markdown_text(&independent, None).unwrap();
    let index = SemanticIndex::build(query.document.as_ref().unwrap());
    assert_eq!(
        index.root()[0].value_domain,
        Some(ValueDomain::Choices { exhaustive: true })
    );

    let nested = source(
        "  <!-- mant:entries role=value case=sensitive -->\n  - `auto`: Automatic.\n\n    <!-- mant:entries role=value case=sensitive -->\n    - `deeper`\n",
    );
    let query = query_markdown_text(&nested, None).unwrap();
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
        let query = query_markdown_text(&source, None).unwrap();
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
    let query = query_markdown_text(&source, None).unwrap();
    let index = SemanticIndex::build(query.document.as_ref().unwrap());
    assert_eq!(
        index.root()[0].value_domain,
        Some(ValueDomain::Choices { exhaustive: false })
    );
}
