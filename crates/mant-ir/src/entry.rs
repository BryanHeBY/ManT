//! Source-neutral semantic entry model and rebuildable indexes.
mod index;
mod model;
mod walk;
#[cfg(test)]
use crate::{Block, DefinitionItem, DefinitionRole, Document, Inline, LinkTarget};
pub use index::SemanticIndex;
#[cfg(test)]
use index::entry_from_definition;
pub use model::*;

#[cfg(test)]
mod tests {
    use crate::{
        DefinitionCase, DefinitionIdentity, DocumentMeta, DocumentSource, LayoutHint, Section,
        SourceFormat,
    };

    use super::*;

    fn definition(
        id: &str,
        role: DefinitionRole,
        aliases: &[&str],
        forms: &[&str],
        description: Vec<Block>,
    ) -> DefinitionItem {
        DefinitionItem {
            identity: Some(DefinitionIdentity {
                id: id.into(),
                role,
                case: DefinitionCase::Sensitive,
                names: aliases.iter().map(|alias| (*alias).to_owned()).collect(),
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
            inline_term: false,
            spacing_before_lines: None,
        }
    }

    #[test]
    fn choice_validation_uses_direct_entry_ownership_through_containers() {
        let list = |item| Block::DefinitionList {
            items: vec![item],
            compact: true,
            layout: LayoutHint::default(),
            source: None,
        };
        let grandchild = definition(
            "command-child",
            DefinitionRole::Command,
            &["child"],
            &["child"],
            Vec::new(),
        );
        let value = definition(
            "value-auto",
            DefinitionRole::Value,
            &["auto"],
            &["auto"],
            vec![list(grandchild)],
        );
        assert!(!value.has_value_choices());
        let parent = definition(
            "option-color",
            DefinitionRole::Option,
            &["--color"],
            &["--color WHEN"],
            vec![Block::List {
                kind: crate::ListKind::Bullet,
                start: None,
                compact: true,
                items: vec![crate::ListItem {
                    blocks: vec![list(value)],
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
            DefinitionRole::Option,
            &["-L"],
            &["-L port:host:hostport", "-L socket:remote_socket"],
            Vec::new(),
        );
        let command = definition(
            "command-ssh",
            DefinitionRole::Command,
            &["ssh"],
            &["ssh destination"],
            vec![Block::DefinitionList {
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
        assert_eq!(entries[0].children[0].aliases, ["-L"]);
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
            DefinitionRole::Command,
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
            DefinitionRole::Option,
            &["-o"],
            &["-o option"],
            Vec::new(),
        );
        item.identity.as_mut().expect("identity").value_domain = Some(ValueDomain::EntrySet {
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
