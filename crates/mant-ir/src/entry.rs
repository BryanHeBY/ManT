//! Source-neutral semantic entry model and rebuildable indexes.
mod content;
mod facts;
mod index;
mod model;
mod relations;
mod walk;
#[cfg(test)]
mod wire;
#[cfg(test)]
use crate::{Block, DefinitionItem, Document, Inline, LinkTarget};
pub use content::*;
pub use facts::*;
pub use index::SemanticIndex;
#[cfg(test)]
use index::entry_from_definition;
pub use model::*;
pub(crate) use relations::relation_issues;
pub use relations::{EntryRelationIssue, EntryRelationIssueKind, entry_relation_issues};
pub use walk::visit_child_entries;

#[cfg(test)]
mod tests {
    use crate::{
        DocumentMeta, DocumentSource, EntryFacts, LayoutHint, NameCase, Section, SourceFormat,
    };

    use super::*;

    fn definition(
        id: &str,
        kind: EntryKind,
        names: &[&str],
        forms: &[&str],
        description: Vec<Block>,
    ) -> DefinitionItem {
        DefinitionItem {
            source: None,
            entry: Some(EntryFacts {
                name_bindings: names
                    .iter()
                    .enumerate()
                    .map(|(name, spelling)| crate::EntryNameBinding {
                        name,
                        evidence: crate::EntryNameEvidence::Declared,
                        occurrences: forms
                            .iter()
                            .enumerate()
                            .filter_map(|(index, form)| {
                                form.find(spelling).map(|start| crate::EntryForm {
                                    parts: vec![crate::EntryContentSlice {
                                        root: crate::EntryInlineRoot::Term { index },
                                        path: vec![0],
                                        bytes: Some(start..start + spelling.len()),
                                    }],
                                })
                            })
                            .collect(),
                    })
                    .collect(),
                alias_groups: Vec::new(),
                alias_of: None,
                forms: (0..forms.len()).map(crate::EntryForm::term).collect(),
                id: id.into(),
                kind,
                case: NameCase::Sensitive,
                names: names.iter().map(|alias| (*alias).to_owned()).collect(),
                value_domain: None,
            }),
            terms: forms
                .iter()
                .map(|form| {
                    vec![Inline::Code {
                        value: (*form).to_owned(),
                    }]
                })
                .collect(),
            description,
            layout: crate::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
            },
        }
    }

    #[test]
    fn choice_validation_uses_direct_entry_ownership_through_containers() {
        let list = |item| Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![item],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        };
        let grandchild = definition(
            "command-child",
            EntryKind::Command,
            &["child"],
            &["child"],
            Vec::new(),
        );
        let value = definition(
            "value-auto",
            EntryKind::Value,
            &["auto"],
            &["auto"],
            vec![list(grandchild)],
        );
        assert!(!value.has_value_choices());
        let mut transparent = definition("unused", EntryKind::Term, &[], &[], vec![list(value)]);
        transparent.entry = None;
        let parent = definition(
            "option-color",
            EntryKind::Parameter {
                parameter_kind: crate::ParameterKind::Option,
            },
            &["--color"],
            &["--color WHEN"],
            vec![Block::List {
                kind: crate::ListKind::Bullet,
                compact: true,
                items: vec![crate::ListItem {
                    source: None,
                    entry: None,
                    blocks: vec![list(transparent)],
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
        );
        assert!(parent.has_value_choices());
        let entry = entry_from_definition(&parent).unwrap();
        assert_eq!(
            entry.value_domain,
            Some(ValueDomain::Choices { exhaustive: false })
        );
        assert_eq!(entry.children[0].children[0].kind, EntryKind::Command);
    }

    #[test]
    fn preserves_definition_nesting_and_counts_forms_separately() {
        let option = definition(
            "option-local-forward",
            EntryKind::Parameter {
                parameter_kind: crate::ParameterKind::Option,
            },
            &["-L"],
            &["-L port:host:hostport", "-L socket:remote_socket"],
            Vec::new(),
        );
        let command = definition(
            "command-ssh",
            EntryKind::Command,
            &["ssh"],
            &["ssh destination"],
            vec![Block::DefinitionList {
                declaration_groups: Vec::new(),
                items: vec![option],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            }],
        );
        let document = Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Mdoc,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "synopsis".into(),
                fragment_aliases: Vec::new(),
                title: "SYNOPSIS".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
                    declaration_groups: Vec::new(),
                    items: vec![command],
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: Vec::new(),
                source: None,
            }],
        };

        let index = SemanticIndex::build(&document);
        let entries = index.section("synopsis");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].kind, EntryKind::Command);
        assert_eq!(entries[0].children.len(), 1);
        assert_eq!(entries[0].children[0].names, ["-L"]);
        assert_eq!(entries[0].children[0].forms.len(), 2);
        assert_eq!(entries[0].subtree_len(), 2);

        assert_eq!(
            index.section_summary("synopsis"),
            EntrySummary {
                direct: 1,
                descendants: 1,
                forms: 3,
                by_kind: vec![
                    EntryKindCount {
                        kind: EntryKind::Command,
                        count: 1,
                    },
                    EntryKindCount {
                        kind: EntryKind::Parameter {
                            parameter_kind: ParameterKind::Option,
                        },
                        count: 1,
                    },
                ],
            }
        );
    }

    #[test]
    fn derives_document_targets_only_from_linked_terms() {
        let mut item = definition(
            "command-winget",
            EntryKind::Command,
            &["winget.exe"],
            &[],
            vec![Block::Paragraph {
                children: vec![Inline::Link {
                    target: LinkTarget::Document {
                        name: "description-only".to_owned(),
                        fragment: None,
                    },
                    title: None,
                    children: vec![Inline::Text {
                        value: "details".to_owned(),
                    }],
                }],
                layout: LayoutHint::default(),
                source: None,
            }],
        );
        item.terms = vec![vec![Inline::Link {
            target: LinkTarget::Document {
                name: "winget.exe".to_owned(),
                fragment: None,
            },
            title: None,
            children: vec![Inline::Code {
                value: "winget.exe".to_owned(),
            }],
        }]];

        assert!(
            entry_from_definition(&item)
                .unwrap()
                .document_targets
                .is_empty()
        );
        item.entry.as_mut().unwrap().forms = vec![crate::EntryForm::term(0)];

        let entry = entry_from_definition(&item).expect("entry");
        assert_eq!(
            entry.document_targets,
            [SemanticDocumentTarget {
                label: "winget.exe".to_owned(),
                reference: SemanticDocumentReference::Document {
                    name: "winget.exe".to_owned(),
                    fragment: None,
                },
            }]
        );
    }

    #[test]
    fn explicit_cross_document_domain_survives_index_derivation() {
        let mut item = definition(
            "option-config",
            EntryKind::Parameter {
                parameter_kind: crate::ParameterKind::Option,
            },
            &["-o"],
            &["-o option"],
            Vec::new(),
        );
        item.entry.as_mut().expect("identity").value_domain = Some(ValueDomain::EntrySet {
            reference: SemanticDocumentReference::Manual {
                name: "ssh_config".to_owned(),
                manual_section: Some("5".to_owned()),
            },
            entry_kinds: vec![EntryKind::ConfigurationKey],
            source: None,
        });

        let entry = entry_from_definition(&item).expect("entry");
        assert!(matches!(
            entry.value_domain,
            Some(ValueDomain::EntrySet {
                reference: SemanticDocumentReference::Manual {
                    ref name,
                manual_section: Some(ref section),
                },
                ref entry_kinds,
                source: None,
            }) if name == "ssh_config"
                && section == "5"
                && entry_kinds == &[EntryKind::ConfigurationKey]
        ));
    }

    #[test]
    fn semantic_document_references_share_one_strict_grammar() {
        for reference in [
            SemanticDocumentReference::Document {
                name: "../reference/options".to_owned(),
                fragment: Some("output".to_owned()),
            },
            SemanticDocumentReference::Manual {
                name: "ssh_config".to_owned(),
                manual_section: Some("5".to_owned()),
            },
        ] {
            assert!(reference.is_well_formed(), "{reference:?}");
        }
        for reference in [
            SemanticDocumentReference::Document {
                name: "broken//path".to_owned(),
                fragment: None,
            },
            SemanticDocumentReference::Manual {
                name: "ssh_config".to_owned(),
                manual_section: Some("qgroup".to_owned()),
            },
        ] {
            assert!(!reference.is_well_formed(), "{reference:?}");
        }
    }
}
