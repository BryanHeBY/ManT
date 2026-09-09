use super::{RegisteredDocument, RegisteredDocumentIndex, RegisteredDocumentOrigin};
use crate::{ConfiguredSource, SourceConfig, SourceLocation};
use std::{collections::BTreeMap, path::PathBuf};

#[test]
fn source_origin_carries_the_selector_name() {
    let origin = RegisteredDocumentOrigin::Source("rust".to_owned());
    assert_eq!(origin, RegisteredDocumentOrigin::Source("rust".to_owned()));
}

#[test]
fn exact_paths_precede_unique_suffixes_and_collisions_are_explicit() {
    let index = RegisteredDocumentIndex {
        config: SourceConfig::default(),
        documents: vec![
            RegisteredDocument {
                logical_path: "languages/en/tool".to_owned(),
                path: PathBuf::from("/documents/languages/en/tool.md"),
                origin: RegisteredDocumentOrigin::Documents,
                source_priority: None,
            },
            RegisteredDocument {
                logical_path: "languages/zh/tool".to_owned(),
                path: PathBuf::from("/documents/languages/zh/tool.md"),
                origin: RegisteredDocumentOrigin::Documents,
                source_priority: None,
            },
        ],
        ready_sources: std::collections::BTreeSet::default(),
    };
    let exact = index
        .find(&["languages/en/tool".to_owned()], None)
        .expect("exact lookup")
        .expect("exact document");
    assert_eq!(exact.logical_path, "languages/en/tool");
    let error = index
        .find(&["tool".to_owned()], None)
        .expect_err("leaf selector must be ambiguous");
    assert!(error.to_string().contains("languages/en/tool"));
    assert!(error.to_string().contains("languages/zh/tool"));
}

#[test]
fn fallback_ambiguity_does_not_leak_into_the_before_manual_phase() {
    let index = RegisteredDocumentIndex {
        config: SourceConfig::default(),
        documents: ["languages/en/tool", "languages/zh/tool"]
            .into_iter()
            .map(|logical_path| RegisteredDocument {
                logical_path: logical_path.to_owned(),
                path: PathBuf::from(format!("/sources/fallback/{logical_path}.md")),
                origin: RegisteredDocumentOrigin::Source("fallback".to_owned()),
                source_priority: Some(-1),
            })
            .collect(),
        ready_sources: std::collections::BTreeSet::default(),
    };

    assert_eq!(
        index
            .find_before_builtin(&["tool".to_owned()])
            .expect("preferred phase"),
        None
    );
    assert!(
        index
            .find_after_builtin(&["tool".to_owned()])
            .expect_err("fallback remains ambiguous")
            .to_string()
            .contains("source 'fallback'")
    );
}

#[test]
fn content_aware_matches_preserve_priority_groups_and_ambiguity() {
    let document = |logical_path: &str,
                    origin: RegisteredDocumentOrigin,
                    source_priority: Option<i32>| RegisteredDocument {
        logical_path: logical_path.to_owned(),
        path: PathBuf::from(format!("/{logical_path}.md")),
        origin,
        source_priority,
    };
    let index = RegisteredDocumentIndex {
        config: SourceConfig::default(),
        documents: vec![
            document("en/tool", RegisteredDocumentOrigin::Documents, None),
            document("zh/tool", RegisteredDocumentOrigin::Documents, None),
            document(
                "tool",
                RegisteredDocumentOrigin::Source("preferred".to_owned()),
                Some(2),
            ),
            document(
                "tool",
                RegisteredDocumentOrigin::Source("tied".to_owned()),
                Some(0),
            ),
            document(
                "tool",
                RegisteredDocumentOrigin::Source("fallback".to_owned()),
                Some(-1),
            ),
        ],
        ready_sources: std::collections::BTreeSet::default(),
    };

    let before = index.matches_before_builtin(&["tool".to_owned()]);
    assert_eq!(before.len(), 2);
    assert_eq!(before[0].documents.len(), 2, "ambiguity stays inspectable");
    assert_eq!(
        before[1].origin,
        RegisteredDocumentOrigin::Source("preferred".to_owned())
    );

    let after = index.matches_after_builtin(&["tool".to_owned()]);
    assert_eq!(
        after
            .iter()
            .map(|group| group.origin.clone())
            .collect::<Vec<_>>(),
        vec![
            RegisteredDocumentOrigin::Source("tied".to_owned()),
            RegisteredDocumentOrigin::Source("fallback".to_owned()),
        ]
    );
}

#[test]
fn source_priority_is_signed() {
    let mut values = BTreeMap::new();
    values.insert(
        "docs".to_owned(),
        ConfiguredSource {
            location: SourceLocation::Git {
                repo: "repo".to_owned(),
                branch: "main".to_owned(),
            },
            path: ".".to_owned(),
            include: Vec::new(),
            exclude: Vec::new(),
            priority: -1,
        },
    );
    let _ = std::mem::size_of::<SourceConfig>();
    assert_eq!(values["docs"].priority, -1);
}

#[cfg(windows)]
#[test]
fn windows_registered_names_are_ascii_case_insensitive() {
    assert!(super::document_paths_equal("cargo.exe", "cargo.EXE"));
}

#[test]
fn personal_suffix_precedes_a_sources_exact_path_without_rescanning() {
    let index = RegisteredDocumentIndex {
        config: SourceConfig::default(),
        documents: vec![
            RegisteredDocument {
                logical_path: "personal/tool".to_owned(),
                path: PathBuf::from("unopened/personal/tool.md"),
                origin: RegisteredDocumentOrigin::Documents,
                source_priority: None,
            },
            RegisteredDocument {
                logical_path: "tool".to_owned(),
                path: PathBuf::from("unopened/source/tool.md"),
                origin: RegisteredDocumentOrigin::Source("preferred".to_owned()),
                source_priority: Some(99),
            },
        ],
        ready_sources: std::collections::BTreeSet::default(),
    };
    let candidates = ["tool".to_owned()];
    for _ in 0..2 {
        let found = index.find(&candidates, None).unwrap().unwrap();
        assert!(std::ptr::eq(found, &raw const index.documents()[0]));
        assert_eq!(found.logical_path, "personal/tool");
        assert_eq!(
            index
                .find_address("tool", &RegisteredDocumentOrigin::Documents)
                .unwrap(),
            None,
            "a complete address never falls back to a suffix"
        );
    }
}

#[test]
fn equal_priority_sources_keep_snapshot_order_and_candidate_order() {
    // Config's precedence test independently pins bytewise source-name ties.
    // Selection must preserve that order, not sort origins by matching paths.
    let index = RegisteredDocumentIndex {
        config: SourceConfig::default(),
        documents: [
            ("alpha", "z/tool"),
            ("alpha", "secondary"),
            ("zeta", "a/tool"),
        ]
        .into_iter()
        .map(|(name, logical_path)| RegisteredDocument {
            logical_path: logical_path.to_owned(),
            path: PathBuf::from("unopened.md"),
            origin: RegisteredDocumentOrigin::Source(name.to_owned()),
            source_priority: Some(1),
        })
        .collect(),
        ready_sources: std::collections::BTreeSet::default(),
    };
    let candidates = ["tool".to_owned(), "secondary".to_owned()];
    let groups = index.matches_before_builtin(&candidates);
    assert_eq!(groups.len(), 2);
    assert_eq!(
        groups[0].origin,
        RegisteredDocumentOrigin::Source("alpha".into())
    );
    assert_eq!(
        groups[1].origin,
        RegisteredDocumentOrigin::Source("zeta".into())
    );
    assert_eq!(groups[0].documents[0].logical_path, "z/tool");
    assert_eq!(groups[1].documents[0].logical_path, "a/tool");
    assert_eq!(
        index.find(&candidates, None).unwrap().unwrap().logical_path,
        "z/tool"
    );
    assert!(index.matches_after_builtin(&candidates).is_empty());
}
