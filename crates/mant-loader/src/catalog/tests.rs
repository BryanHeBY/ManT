use std::path::PathBuf;

use mant_sources::{RegisteredDocument, RegisteredDocumentOrigin};

use crate::ManualPage;

use mant_protocol::{
    CatalogDocumentKind, CatalogQuery, DocumentAddress, MAX_CATALOG_PATTERN_CHARS, SearchCase,
    SearchSyntax,
};

use super::{
    AvailableDocument, AvailableDocumentKind, AvailableDocumentOrigin,
    list_available_documents_from, query_available_documents,
};

#[test]
fn invalid_catalog_requests_never_start_inventory_discovery() {
    let queries = [
        CatalogQuery {
            limit: 0,
            ..CatalogQuery::default()
        },
        CatalogQuery {
            pattern: Some("[".into()),
            syntax: SearchSyntax::Regex,
            ..CatalogQuery::default()
        },
        CatalogQuery {
            pattern: Some("x".repeat(MAX_CATALOG_PATTERN_CHARS + 1)),
            ..CatalogQuery::default()
        },
        CatalogQuery {
            source: Some("local".into()),
            kind: Some(CatalogDocumentKind::Manual),
            ..CatalogQuery::default()
        },
    ];
    for query in queries {
        let calls = std::cell::Cell::new(0);
        assert!(
            super::discover_with(&query, || {
                calls.set(calls.get() + 1);
                panic!("invalid catalog request must not discover source roots")
            })
            .is_err()
        );
        assert_eq!(calls.get(), 0);
    }
    let calls = std::cell::Cell::new(0);
    let catalog = super::discover_with(&CatalogQuery::default(), || {
        calls.set(calls.get() + 1);
        Ok(Vec::new())
    })
    .expect("valid request executes its inventory once");
    assert_eq!(calls.get(), 1);
    assert_eq!((catalog.total, catalog.returned), (0, 0));
}

#[test]
fn catalog_pages_an_immutable_inventory_without_losing_scope_coverage() {
    let documents = ["zeta", "alpha", "beta"]
        .into_iter()
        .map(|name| AvailableDocument {
            name: name.to_owned(),
            logical_path: name.to_owned(),
            kind: AvailableDocumentKind::Markdown,
            manual_section: None,
            path: PathBuf::from(format!("/not-opened/{name}.md")),
            origin: AvailableDocumentOrigin::Documents,
            source_priority: None,
        })
        .collect::<Vec<_>>();
    let snapshot = documents.clone();
    let query = CatalogQuery {
        offset: 1,
        limit: 1,
        ..CatalogQuery::default()
    };
    let page = query_available_documents(&documents, &query).expect("middle page");
    assert_eq!(page.documents[0].address.name(), "beta");
    assert_eq!((page.total, page.returned, page.offset), (3, 1, 1));
    assert_eq!(page.next_offset, Some(2));
    assert!(page.truncated);
    assert_eq!(page.coverage.scope_total, 3);

    let end = query_available_documents(
        &documents,
        &CatalogQuery {
            offset: u32::MAX,
            ..query
        },
    )
    .expect("past end page");
    assert!(end.documents.is_empty());
    assert_eq!((end.total, end.returned, end.offset), (3, 0, 3));
    assert_eq!(end.next_offset, None);
    assert!(!end.truncated);
    assert_eq!(end.coverage, page.coverage);
    assert_eq!(documents, snapshot);
}

#[test]
fn prepared_catalog_query_reuses_filters_without_loading_or_recompiling() {
    let documents = ["beta", "ALPHA", "other"]
        .into_iter()
        .map(|name| AvailableDocument {
            name: name.to_owned(),
            logical_path: name.to_owned(),
            kind: AvailableDocumentKind::Markdown,
            manual_section: None,
            path: PathBuf::from(format!("/never-opened/{name}.md")),
            origin: AvailableDocumentOrigin::Documents,
            source_priority: None,
        })
        .collect::<Vec<_>>();
    let query = CatalogQuery {
        pattern: Some("^(alpha|beta)$".to_owned()),
        syntax: SearchSyntax::Regex,
        case: SearchCase::Insensitive,
        limit: 1,
        ..CatalogQuery::default()
    };
    let prepared = crate::PreparedCatalogQuery::new(&query).expect("prepare without IO");
    let first = prepared.apply(&documents);
    let second = prepared.apply(&documents);
    assert_eq!(first, second);
    assert_eq!(
        first,
        query_available_documents(&documents, &query).unwrap()
    );
    assert_eq!(first.documents[0].address.name(), "ALPHA");
    assert_eq!(
        (first.total, first.returned, first.next_offset),
        (2, 1, Some(1))
    );
    assert_eq!(first.coverage.scope_total, 3);
    assert_eq!(prepared.apply(&[]).total, 0);
}

#[test]
fn merges_both_namespaces_without_hiding_manual_sections() {
    let documents = list_available_documents_from(
        vec![RegisteredDocument {
            logical_path: "printf".to_owned(),
            path: PathBuf::from("/home/demo/.local/share/mant/documents/printf.md"),
            origin: RegisteredDocumentOrigin::Documents,
            source_priority: None,
        }],
        &[
            ManualPage {
                name: "printf".to_owned(),
                section: "1".to_owned(),
                path: PathBuf::from("/usr/share/man/man1/printf.1.gz"),
                manual_root: PathBuf::from("/usr/share/man"),
            },
            ManualPage {
                name: "printf".to_owned(),
                section: "3".to_owned(),
                path: PathBuf::from("/usr/share/man/man3/printf.3.gz"),
                manual_root: PathBuf::from("/usr/share/man"),
            },
        ],
    );

    assert_eq!(documents.len(), 3);
    assert_eq!(documents[0].kind, AvailableDocumentKind::Markdown);
    assert_eq!(documents[0].origin, AvailableDocumentOrigin::Documents);
    assert_eq!(documents[1].manual_section.as_deref(), Some("1"));
    assert_eq!(documents[2].manual_section.as_deref(), Some("3"));
}

#[test]
fn catalog_pattern_limit_counts_unicode_scalars() {
    let accepted = CatalogQuery {
        pattern: Some("界".repeat(MAX_CATALOG_PATTERN_CHARS)),
        ..CatalogQuery::default()
    };
    assert!(query_available_documents(&[], &accepted).is_ok());

    let rejected = CatalogQuery {
        pattern: Some("界".repeat(MAX_CATALOG_PATTERN_CHARS + 1)),
        ..CatalogQuery::default()
    };
    assert!(matches!(
        query_available_documents(&[], &rejected),
        Err(super::CatalogError::PatternTooLong)
    ));
}

#[test]
fn keeps_shadowed_markdown_candidates_in_fallback_order() {
    let documents = list_available_documents_from(
        vec![
            RegisteredDocument {
                logical_path: "tool".to_owned(),
                path: PathBuf::from("/data/mant/documents/tool.md"),
                origin: RegisteredDocumentOrigin::Documents,
                source_priority: None,
            },
            RegisteredDocument {
                logical_path: "tool".to_owned(),
                path: PathBuf::from("/data/mant/sources/alpha/tool.md"),
                origin: RegisteredDocumentOrigin::Source("alpha".to_owned()),
                source_priority: Some(1),
            },
        ],
        &[],
    );
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].origin, AvailableDocumentOrigin::Documents);
    assert_eq!(
        documents[1].origin,
        AvailableDocumentOrigin::Source("alpha".to_owned())
    );
}

#[test]
fn catalog_orders_sources_around_the_native_manual_zero_baseline() {
    let documents = list_available_documents_from(
        vec![
            RegisteredDocument {
                logical_path: "tool".to_owned(),
                path: PathBuf::from("/sources/low/tool.md"),
                origin: RegisteredDocumentOrigin::Source("low".to_owned()),
                source_priority: Some(-1),
            },
            RegisteredDocument {
                logical_path: "tool".to_owned(),
                path: PathBuf::from("/sources/high/tool.md"),
                origin: RegisteredDocumentOrigin::Source("high".to_owned()),
                source_priority: Some(1),
            },
            RegisteredDocument {
                logical_path: "tool".to_owned(),
                path: PathBuf::from("/sources/tie/tool.md"),
                origin: RegisteredDocumentOrigin::Source("tie".to_owned()),
                source_priority: Some(0),
            },
        ],
        &[ManualPage {
            name: "tool".to_owned(),
            section: "1".to_owned(),
            path: PathBuf::from("/man/tool.1"),
            manual_root: PathBuf::from("/man"),
        }],
    );

    assert_eq!(
        documents
            .iter()
            .map(|document| match &document.origin {
                AvailableDocumentOrigin::Source(name) => format!("source:{name}"),
                AvailableDocumentOrigin::ManualPath => "manual".to_owned(),
                AvailableDocumentOrigin::Documents => "documents".to_owned(),
            })
            .collect::<Vec<_>>(),
        ["source:high", "manual", "source:tie", "source:low"]
    );
}

#[test]
fn catalog_search_ranks_exact_prefix_and_substring_matches() {
    let documents = ["process", "Start-Process", "process-tree"]
        .into_iter()
        .map(|name| AvailableDocument {
            name: name.to_owned(),
            logical_path: name.to_owned(),
            kind: AvailableDocumentKind::Markdown,
            manual_section: None,
            path: PathBuf::from(format!("/data/{name}.md")),
            origin: AvailableDocumentOrigin::Source("pwsh7".to_owned()),
            source_priority: Some(1),
        })
        .collect::<Vec<_>>();
    let catalog = query_available_documents(
        &documents,
        &CatalogQuery {
            pattern: Some("process".to_owned()),
            limit: 10,
            ..CatalogQuery::default()
        },
    )
    .expect("catalog");

    assert_eq!(catalog.total, 3);
    assert_eq!(catalog.documents[0].address.name(), "process");
    assert_eq!(catalog.documents[1].address.name(), "process-tree");
    assert_eq!(catalog.documents[2].address.name(), "Start-Process");
}

#[test]
fn catalog_prefers_case_faithful_prefixes_before_folded_prefixes() {
    let documents = ["exec", "execlp", "EXECUTE", "execv", "execve"]
        .into_iter()
        .map(|name| AvailableDocument {
            name: name.to_owned(),
            logical_path: name.to_owned(),
            kind: AvailableDocumentKind::Manual,
            manual_section: Some("1".to_owned()),
            path: PathBuf::from(format!("/man/{name}.1")),
            origin: AvailableDocumentOrigin::ManualPath,
            source_priority: None,
        })
        .collect::<Vec<_>>();
    let catalog = query_available_documents(
        &documents,
        &CatalogQuery {
            pattern: Some("exec".to_owned()),
            ..CatalogQuery::default()
        },
    )
    .expect("catalog");

    assert_eq!(
        catalog
            .documents
            .iter()
            .map(|document| document.address.name())
            .collect::<Vec<_>>(),
        ["exec", "execlp", "execv", "execve", "EXECUTE"]
    );
}

#[test]
fn catalog_distinguishes_unindexed_scopes_from_empty_name_matches() {
    let documents = ["execve", "EPIOCGPARAMS"]
        .into_iter()
        .zip(["2", "2const"])
        .map(|(name, section)| AvailableDocument {
            name: name.to_owned(),
            logical_path: name.to_owned(),
            kind: AvailableDocumentKind::Manual,
            manual_section: Some(section.to_owned()),
            path: PathBuf::from(format!("/man/{name}.{section}")),
            origin: AvailableDocumentOrigin::ManualPath,
            source_priority: None,
        })
        .collect::<Vec<_>>();

    let unindexed = query_available_documents(
        &documents,
        &CatalogQuery {
            pattern: Some("exec".to_owned()),
            kind: Some(CatalogDocumentKind::Manual),
            manual_section: Some("42".to_owned()),
            ..CatalogQuery::default()
        },
    )
    .expect("unindexed scope remains a valid query");
    assert_eq!(unindexed.total, 0);
    assert_eq!(unindexed.coverage.scope_total, 0);
    assert_eq!(unindexed.coverage.manual_sections, ["2", "2const"]);

    let covered = query_available_documents(
        &documents,
        &CatalogQuery {
            pattern: Some("not-present".to_owned()),
            kind: Some(CatalogDocumentKind::Manual),
            manual_section: Some("2".to_owned()),
            ..CatalogQuery::default()
        },
    )
    .expect("covered scope");
    assert_eq!(covered.total, 0);
    assert_eq!(covered.coverage.scope_total, 1);
}

#[test]
fn catalog_puts_an_exact_manual_before_every_prefix_and_substring() {
    let documents = ["woman", "manpath", "man", "man.conf", "printf"]
        .into_iter()
        .map(|name| AvailableDocument {
            name: name.to_owned(),
            logical_path: name.to_owned(),
            kind: AvailableDocumentKind::Manual,
            manual_section: Some("1".to_owned()),
            path: PathBuf::from(format!("/man/{name}.1")),
            origin: AvailableDocumentOrigin::ManualPath,
            source_priority: None,
        })
        .collect::<Vec<_>>();
    let catalog = query_available_documents(
        &documents,
        &CatalogQuery {
            pattern: Some("man".to_owned()),
            limit: 10,
            ..CatalogQuery::default()
        },
    )
    .expect("catalog");
    let names = catalog
        .documents
        .iter()
        .map(|document| document.address.name())
        .collect::<Vec<_>>();

    assert_eq!(names, ["man", "man.conf", "manpath", "woman"]);
}

#[test]
fn catalog_ranks_hierarchical_exact_suffix_prefix_and_substring_matches() {
    let documents = ["tool", "languages/en/tool", "toolbox", "guides/mytool"]
        .into_iter()
        .map(|logical_path| AvailableDocument {
            name: logical_path.rsplit('/').next().expect("leaf").to_owned(),
            logical_path: logical_path.to_owned(),
            kind: AvailableDocumentKind::Markdown,
            manual_section: None,
            path: PathBuf::from(format!("/documents/{logical_path}.md")),
            origin: AvailableDocumentOrigin::Documents,
            source_priority: None,
        })
        .collect::<Vec<_>>();
    let catalog = query_available_documents(
        &documents,
        &CatalogQuery {
            pattern: Some("tool".to_owned()),
            limit: 10,
            ..CatalogQuery::default()
        },
    )
    .expect("hierarchical catalog");
    assert_eq!(
        catalog
            .documents
            .iter()
            .map(mant_protocol::DocumentSummary::catalog_path)
            .collect::<Vec<_>>(),
        [
            "documents/languages/en/tool".to_owned(),
            "documents/tool".to_owned(),
            "documents/toolbox".to_owned(),
            "documents/guides/mytool".to_owned(),
        ]
    );

    let exact = AvailableDocument {
        name: "tool".to_owned(),
        logical_path: "en/tool".to_owned(),
        kind: AvailableDocumentKind::Markdown,
        manual_section: None,
        path: PathBuf::from("/documents/en/tool.md"),
        origin: AvailableDocumentOrigin::Documents,
        source_priority: None,
    };
    let catalog = query_available_documents(
        &[exact, documents[1].clone()],
        &CatalogQuery {
            pattern: Some("en/tool".to_owned()),
            limit: 10,
            ..CatalogQuery::default()
        },
    )
    .expect("component suffix catalog");
    assert_eq!(
        catalog
            .documents
            .iter()
            .map(mant_protocol::DocumentSummary::catalog_path)
            .collect::<Vec<_>>(),
        [
            "documents/en/tool".to_owned(),
            "documents/languages/en/tool".to_owned()
        ]
    );
}

#[test]
fn catalog_filters_keep_manual_sections_and_exact_addresses() {
    let documents = vec![
        AvailableDocument {
            name: "printf".to_owned(),
            logical_path: "printf".to_owned(),
            kind: AvailableDocumentKind::Manual,
            manual_section: Some("1".to_owned()),
            path: PathBuf::from("/man/printf.1"),
            origin: AvailableDocumentOrigin::ManualPath,
            source_priority: None,
        },
        AvailableDocument {
            name: "printf".to_owned(),
            logical_path: "printf".to_owned(),
            kind: AvailableDocumentKind::Manual,
            manual_section: Some("3".to_owned()),
            path: PathBuf::from("/man/printf.3"),
            origin: AvailableDocumentOrigin::ManualPath,
            source_priority: None,
        },
    ];
    let catalog = query_available_documents(
        &documents,
        &CatalogQuery {
            pattern: Some("^PRINT".to_owned()),
            syntax: SearchSyntax::Regex,
            case: SearchCase::Insensitive,
            kind: Some(CatalogDocumentKind::Manual),
            manual_section: Some("3".to_owned()),
            limit: 10,
            ..CatalogQuery::default()
        },
    )
    .expect("catalog");

    assert_eq!(catalog.documents.len(), 1);
    assert_eq!(
        catalog.documents[0].address,
        DocumentAddress::Manual {
            name: "printf".to_owned(),
            manual_section: "3".to_owned(),
        }
    );
}
