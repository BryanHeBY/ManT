use super::references::{ScopeReference, document_references};
use super::*;
use mant_ir::{Block, Inline, MarkdownOrigin};

pub(super) fn markdown_content(
    source: &str,
    source_path: Option<String>,
) -> Result<ResolvedContent, mant_codec::MarkdownParseError> {
    let parsed = mant_codec::parse_markdown(source, source_path)?;
    Ok(ResolvedContent {
        label: "fixture".to_owned(),
        address: None,
        document: Some(parsed.document),
        tldr: parsed.tldr,
    })
}

#[test]
fn entry_domains_participate_in_typed_document_traversal() {
    let parsed = mant_codec::parse_markdown(
            "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->\n",
            None,
        )
        .expect("semantic domain document");
    let references = document_references(&ResolvedContent {
        label: "ssh".to_owned(),
        address: Some(DocumentAddress::Manual {
            name: "ssh".to_owned(),
            manual_section: "1".to_owned(),
        }),
        document: Some(parsed.document),
        tldr: None,
    })
    .references;

    assert!(matches!(
        references.as_slice(),
        [ScopeReference {
            target: DocumentReference::Manual {
                name,
                manual_section: Some(section),
            },
            kind: DocumentEdgeKind::Manual,
            ..
        }] if name == "ssh_config" && section == "5"
    ));
}

#[test]
fn ambiguous_entry_domains_do_not_participate_in_traversal() {
    let parsed = mant_codec::parse_markdown(
            "# SSH\n\n<!-- mant:entries role=option case=sensitive -->\n- `-o OPTION`: Set a key.\n\n  <!-- mant:domain entries=first.md roles=configuration-key -->\n  <!-- mant:domain entries=second.md roles=configuration-key -->\n",
            None,
        )
        .expect("ambiguous semantic domain document");
    let references = document_references(&ResolvedContent {
        label: "ssh".to_owned(),
        address: Some(DocumentAddress::Markdown {
            path: "ssh".to_owned(),
            origin: MarkdownOrigin::Documents,
        }),
        document: Some(parsed.document),
        tldr: None,
    })
    .references;

    assert!(references.is_empty());
}

#[test]
fn semantic_relationships_follow_authored_source_order() {
    let parsed = mant_codec::parse_markdown(
            "# Tools\n\n<!-- mant:entries role=command case=sensitive -->\n- [`target`](target.md): See [description](description.md).\n\n  <!-- mant:domain entries=domain.md roles=command -->\n",
            None,
        )
        .expect("semantic relationships");
    let references = document_references(&ResolvedContent {
        label: "tools".to_owned(),
        address: Some(DocumentAddress::Markdown {
            path: "indexes/tools".to_owned(),
            origin: mant_ir::MarkdownOrigin::Documents,
        }),
        document: Some(parsed.document),
        tldr: None,
    })
    .references;

    assert_eq!(
        references
            .iter()
            .map(|reference| match &reference.target {
                DocumentReference::Document { name, .. }
                | DocumentReference::Manual { name, .. } => name.as_str(),
            })
            .collect::<Vec<_>>(),
        ["target", "description", "domain"]
    );
}

#[test]
fn scope_bounds_include_every_root() {
    let scope = DocumentScope {
        documents: vec![
            DocumentSelector {
                selector: "a".to_owned(),
                source: None,
                manual_section: None,
            },
            DocumentSelector {
                selector: "b".to_owned(),
                source: None,
                manual_section: None,
            },
        ],
        traversal: mant_protocol::DocumentTraversal {
            follow_links: true,
            max_documents: Some(1),
            ..mant_protocol::DocumentTraversal::default()
        },
    };
    assert_eq!(
        validate_document_scope(&scope),
        Err(ScopeLoadError::DocumentLimit)
    );
}

#[test]
fn explicit_traversal_limits_require_link_following() {
    let scope = DocumentScope {
        documents: vec![DocumentSelector {
            selector: "a".to_owned(),
            source: None,
            manual_section: None,
        }],
        traversal: mant_protocol::DocumentTraversal {
            follow_links: false,
            max_depth: Some(0),
            max_documents: None,
        },
    };
    assert_eq!(
        validate_document_scope(&scope),
        Err(ScopeLoadError::TraversalLimitsRequireLinks)
    );
}

#[test]
fn native_scope_request_enforces_document_selector_contract() {
    let mut scope = DocumentScope {
        documents: vec![DocumentSelector {
            selector: "a".repeat(MAX_DOCUMENT_SELECTOR_CHARS + 1),
            source: None,
            manual_section: None,
        }],
        traversal: mant_protocol::DocumentTraversal::default(),
    };
    assert_eq!(
        validate_document_scope(&scope),
        Err(ScopeLoadError::DocumentSelector(ScopeTextError::TooLong {
            maximum: MAX_DOCUMENT_SELECTOR_CHARS,
        }))
    );

    scope.documents[0].selector = "界".repeat(MAX_DOCUMENT_SELECTOR_CHARS);
    assert_eq!(validate_document_scope(&scope), Ok(()));

    scope.documents[0].selector = "root\nother".to_owned();
    assert_eq!(
        validate_document_scope(&scope),
        Err(ScopeLoadError::DocumentSelector(
            ScopeTextError::ControlCharacter
        ))
    );
}

#[test]
fn decoded_document_paths_still_respect_the_registered_namespace() {
    let from = DocumentAddress::Markdown {
        path: "guide/start".to_owned(),
        origin: MarkdownOrigin::Documents,
    };
    for (uri, expected) in [
        ("../space%20name.md", Some("documents/space name")),
        ("%2E%2E/%2E%2E/outside.md", None),
        ("literal%2520.md", Some("documents/guide/literal%20")),
    ] {
        let query = markdown_content(&format!("[link]({uri})"), None).unwrap();
        let document = query.document.unwrap();
        let Block::Paragraph { children, .. } = &document.blocks[0] else {
            panic!("paragraph")
        };
        let Inline::Link { target, .. } = &children[0] else {
            panic!("link")
        };
        let reference = ScopeReference {
            target: DocumentReference::from_link_target(target).unwrap(),
            kind: DocumentEdgeKind::Document,
            source_offset: None,
            sequence: 0,
        };
        assert_eq!(
            reference.selector(&from).map(|s| s.selector),
            expected.map(str::to_owned),
            "{uri}"
        );
    }
}

#[test]
fn relative_links_use_the_current_markdown_namespace() {
    let reference = ScopeReference {
        target: DocumentReference::Document {
            name: "../other".to_owned(),
            fragment: None,
        },
        kind: DocumentEdgeKind::Document,
        source_offset: None,
        sequence: 0,
    };
    let from = DocumentAddress::Markdown {
        path: "guide/start".to_owned(),
        origin: MarkdownOrigin::Documents,
    };
    assert_eq!(
        reference.selector(&from).map(|selector| selector.selector),
        Some("documents/other".to_owned())
    );
}
