//! Check the shipped link graph, not filenames mentioned in prose or examples.
use mant_engine::{project_references, query_markdown_text};
use mant_ir::{
    DocumentAddress, DocumentIndex, DocumentReference, LinkTarget, MarkdownOrigin, ReferenceScope,
    ReferenceTargetType,
};
use mant_protocol::{ReferenceCoverageStatus, ReferenceProjection, ReferenceProjectionMode};
use std::{collections::BTreeMap, fs, path::Path};

#[test]
fn manifest_manual_links_close_inside_the_installed_namespace() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/manuals");
    let manifest = include_str!("../../../docs/manuals/manifest.txt");
    let documents: BTreeMap<_, _> = manifest
        .lines()
        .filter(|line| !line.is_empty())
        .map(|file| {
            let name = file
                .strip_suffix(".md")
                .expect("Markdown manifest member")
                .to_owned();
            let query =
                query_markdown_text(&fs::read_to_string(root.join(file)).unwrap(), None).unwrap();
            (name, query.document.unwrap())
        })
        .collect();
    let mut incoming = BTreeMap::<String, usize>::new();
    for (name, document) in &documents {
        let address = DocumentAddress::Markdown {
            path: name.clone(),
            origin: MarkdownOrigin::Documents,
        };
        let inventory = project_references(
            document,
            Some(&address),
            ReferenceScope::Document,
            &ReferenceProjection {
                mode: ReferenceProjectionMode::All,
                target_types: vec![ReferenceTargetType::Document, ReferenceTargetType::Local],
                limit: 1000,
                ..Default::default()
            },
        );
        assert!(
            matches!(
                inventory.coverage.status,
                ReferenceCoverageStatus::Complete {}
            ),
            "{name}"
        );
        assert!(
            inventory.page.next_offset.is_none() && inventory.page.limited.is_none(),
            "{name}"
        );
        for record in inventory.records {
            assert!(
                record.origin.resolve_link(document).is_some(),
                "actual IR occurrence"
            );
            match record.target {
                LinkTarget::Document {
                    name: target,
                    fragment,
                } => {
                    let reference = DocumentReference::Document {
                        name: target.clone(),
                        fragment: fragment.clone(),
                    };
                    let resolved = reference
                        .resolve_from(&address)
                        .unwrap_or_else(|| panic!("{name}: out-of-namespace link {target}"));
                    let DocumentAddress::Markdown {
                        path,
                        origin: MarkdownOrigin::Documents,
                    } = resolved
                    else {
                        panic!("namespace changed")
                    };
                    let destination = documents
                        .get(&path)
                        .unwrap_or_else(|| panic!("{name}: {path} is not shipped"));
                    if let Some(fragment) = fragment {
                        assert!(
                            DocumentIndex::build(destination)
                                .fragment_target(&fragment)
                                .is_some(),
                            "{name}: {path}#{fragment}"
                        );
                    }
                    *incoming.entry(path).or_default() += 1;
                }
                LinkTarget::Section { id } => assert!(
                    DocumentIndex::build(document)
                        .fragment_target(id.as_str())
                        .is_some(),
                    "{name}: {id}"
                ),
                _ => unreachable!(),
            }
        }
    }
    for name in documents.keys() {
        assert!(
            incoming.get(name).copied().unwrap_or_default() >= 2,
            "{name} needs real inbound links, not code examples"
        );
    }
}

#[test]
fn fenced_link_examples_are_not_link_occurrences() {
    let document = query_markdown_text(
        "```markdown\n[not shipped](missing.md)\n```\n\n[real](other.md#part)",
        None,
    )
    .unwrap()
    .document
    .unwrap();
    let inventory = project_references(
        &document,
        None,
        ReferenceScope::Document,
        &ReferenceProjection {
            mode: ReferenceProjectionMode::All,
            ..Default::default()
        },
    );
    assert_eq!(inventory.records.len(), 1);
    assert!(
        matches!(&inventory.records[0].target, LinkTarget::Document { name, fragment: Some(fragment) } if name == "other" && fragment == "part")
    );
}
