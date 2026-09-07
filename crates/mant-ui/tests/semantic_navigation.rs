use mant_ir::{
    Block, DefinitionItem, Document, DocumentMeta, DocumentSource, EntryFacts, EntryKind, Inline,
    LayoutHint, NameCase, ParameterKind, ResolvedContent, Section, SourceFormat,
};
use mant_ui::{DocumentView, NavKind};

fn assert_compact_and_complete_entry_labels(view: &DocumentView) {
    assert_eq!(view.navigation()[1].title, "ENTRIES · 4");
    assert_eq!(
        view.navigation()[1].full_title.as_deref(),
        Some("ENTRIES (4 direct · 1 nested · 6 forms)")
    );
    assert_eq!(view.navigation()[2].title, "--help");
    assert_eq!(
        view.navigation()[2].full_title.as_deref(),
        Some("--help MODE | -h")
    );
}

#[test]
#[allow(clippy::too_many_lines)] // Keep the cross-role navigation fixture and expectations together.
fn sidebar_exposes_every_semantic_role_supported_by_the_document_contract() {
    let mut entries = [
        (
            EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            "--help",
        ),
        (EntryKind::Command, "build"),
        (EntryKind::EnvironmentVariable, "MANT_HOME"),
        (EntryKind::Variable, "$LASTEXITCODE"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, name))| DefinitionItem {
        source: None,
        entry: Some(EntryFacts {
            name_bindings: vec![mant_ir::EntryNameBinding {
                name: 0,
                evidence: mant_ir::EntryNameEvidence::Declared,
                occurrences: vec![mant_ir::EntryForm::term(0)],
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            forms: vec![mant_ir::EntryForm::term(0)],
            id: format!("entry-{index}").into(),
            kind: role,
            case: NameCase::Sensitive,
            names: vec![name.to_owned()],
            value_domain: None,
        }),
        terms: vec![vec![Inline::Code {
            value: name.to_owned(),
        }]],
        description: Vec::new(),
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines: None,
        },
    })
    .collect::<Vec<_>>();
    entries[0].terms = vec![
        vec![Inline::Code {
            value: "--help MODE".to_owned(),
        }],
        vec![Inline::Code {
            value: "-h".to_owned(),
        }],
    ];
    let facts = entries[0].entry.as_mut().unwrap();
    facts.forms = vec![mant_ir::EntryForm::term(0), mant_ir::EntryForm::term(1)];
    facts.name_bindings[0].occurrences = vec![mant_ir::EntryForm {
        parts: vec![mant_ir::EntryContentSlice {
            root: mant_ir::EntryInlineRoot::Term { index: 0 },
            path: vec![0],
            bytes: Some(0..6),
        }],
    }];
    entries[0].description = vec![Block::DefinitionList {
        items: vec![DefinitionItem {
            source: None,
            entry: Some(EntryFacts {
                name_bindings: vec![mant_ir::EntryNameBinding {
                    name: 0,
                    evidence: mant_ir::EntryNameEvidence::Declared,
                    occurrences: vec![mant_ir::EntryForm::term(0)],
                }],
                alias_groups: Vec::new(),
                alias_of: None,
                forms: vec![mant_ir::EntryForm::term(0)],
                id: "entry-help-value".into(),
                kind: EntryKind::Value,
                case: NameCase::Sensitive,
                names: vec!["brief".to_owned()],
                value_domain: None,
            }),
            terms: vec![vec![Inline::Code {
                value: "brief".to_owned(),
            }]],
            description: Vec::new(),
            layout: mant_ir::DefinitionLayout {
                inline_term: false,
                spacing_before_lines: None,
            },
        }],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }];
    let bundle = ResolvedContent {
        address: None,
        label: "tool".to_owned(),
        tldr: None,
        document: Some(Document {
            parser: None,
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            fragment_aliases: Vec::new(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "reference".to_owned().into(),
                fragment_aliases: Vec::new(),
                title: "REFERENCE".to_owned(),
                spacing_before_lines: 0,
                blocks: vec![Block::DefinitionList {
                    items: entries,
                    compact: true,
                    layout: LayoutHint::default(),
                    source: None,
                }],
                children: Vec::new(),
                source: None,
            }],
        }),
    };

    let view = DocumentView::new(&bundle);
    assert_compact_and_complete_entry_labels(&view);
    assert_eq!(view.navigation()[3].parent_id.as_deref(), Some("entry-0"));
    assert_eq!(view.navigation()[3].depth, view.navigation()[2].depth + 1);
    assert_eq!(
        view.navigation()[2..]
            .iter()
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        vec![
            NavKind::Entry(EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            }),
            NavKind::Entry(EntryKind::Value),
            NavKind::Entry(EntryKind::Command),
            NavKind::Entry(EntryKind::EnvironmentVariable),
            NavKind::Entry(EntryKind::Variable),
        ]
    );
}
