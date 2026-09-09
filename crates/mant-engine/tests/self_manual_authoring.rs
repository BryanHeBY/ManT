//! Executable native-link authoring and upstream-reference contracts.
use mant_engine::{project_references, query_markdown_text};
use mant_ir::{LinkTarget, ReferenceScope, ReferenceTargetType};
use mant_protocol::{ReferenceProjection, ReferenceProjectionMode};

#[test]
fn native_manual_links_work_in_heading_body_and_linked_code_terms() {
    let query = query_markdown_text("# [Heading](man:linkprobe(3))\n\n[Body](man:linkprobe)\n\n<!-- mant:entries role=command case=sensitive -->\n- [`linkprobe`](man:linkprobe(3)): Read its manual.\n", None).unwrap();
    let document = query.document.as_ref().unwrap();
    assert!(
        document.diagnostics.is_empty(),
        "{:?}",
        document.diagnostics
    );
    let inventory = project_references(
        document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            ..Default::default()
        },
    );
    assert_eq!(inventory.records.len(), 3);
    assert!(inventory.records.iter().all(
        |record| matches!(&record.target, LinkTarget::Manual { name, .. } if name == "linkprobe")
    ));
    assert!(matches!(
        &inventory.records[1].target,
        LinkTarget::Manual {
            manual_section: None,
            ..
        }
    ));
    let index = mant_ir::SemanticIndex::build(document);
    assert_eq!(
        index.root().len(),
        1,
        "only the annotated term creates an entry"
    );
    let exported = mant_engine::render_markdown(&query);
    assert!(
        exported.contains(r"[Heading](man:linkprobe\(3\))"),
        "{exported}"
    );
    assert!(
        !exported.contains("[Body](man:"),
        "ordinary body export deliberately omits the wrapper"
    );
}

#[test]
fn authoring_manuals_expose_real_local_and_implementation_specific_references() {
    let markdown =
        query_markdown_text(include_str!("../../../docs/manuals/mant-markdown.md"), None)
            .unwrap()
            .document
            .unwrap();
    let roff = query_markdown_text(include_str!("../../../docs/manuals/mant-roff.md"), None)
        .unwrap()
        .document
        .unwrap();
    let policy = ReferenceProjection {
        mode: ReferenceProjectionMode::All,
        target_types: vec![ReferenceTargetType::Manual, ReferenceTargetType::External],
        limit: 1000,
        ..Default::default()
    };
    let markdown = project_references(&markdown, None, ReferenceScope::Document, &policy);
    for section in [None, Some("1")] {
        assert!(markdown.records.iter().any(|record| matches!(&record.target, LinkTarget::Manual { name, manual_section } if name == "git" && manual_section.as_deref() == section)));
    }
    let roff = project_references(&roff, None, ReferenceScope::Document, &policy);
    for (page, labels) in [
        ("man", &["man(7)", "mandoc man(7)"][..]),
        ("mdoc", &["mdoc(7)", "mandoc mdoc(7)"][..]),
    ] {
        let uri = format!("https://mandoc.bsd.lv/man/{page}.7.html");
        for record in &roff.records {
            if matches!(&record.target, LinkTarget::External { uri: actual } if actual == &uri) {
                assert!(
                    labels.contains(&record.label.as_str()),
                    "prose must not become part of the linked manual name: {}",
                    record.label
                );
            }
        }
    }
    for page in ["man", "mdoc", "roff", "tbl", "eqn"] {
        let uri = format!("https://mandoc.bsd.lv/man/{page}.7.html");
        assert!(roff.records.iter().any(|record| matches!(&record.target, LinkTarget::External { uri: actual } if actual == &uri)), "real upstream reference for {page}");
    }
    assert!(roff.records.iter().any(|record| matches!(&record.target, LinkTarget::External { uri } if uri == "https://mandoc.bsd.lv/snapshots/mandoc-1.14.6.tar.gz")));
    let manuals: Vec<_> = roff
        .records
        .iter()
        .filter_map(|record| match &record.target {
            LinkTarget::Manual {
                name,
                manual_section,
            } => Some((name.as_str(), manual_section.as_deref())),
            _ => None,
        })
        .collect();
    assert_eq!(
        manuals,
        [("roff", Some("7"))],
        "syntax examples and implementation-specific HTTPS references must not masquerade as local manual links"
    );
}
