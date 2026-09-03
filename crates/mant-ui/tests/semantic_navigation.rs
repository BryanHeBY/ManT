use mant_ir::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, Document,
    DocumentMeta, DocumentSource, EntryKind, Inline, LayoutHint, ParameterKind, ResolvedContent,
    Section, SourceFormat,
};
use mant_ui::{DocumentView, NavKind};

fn assert_compact_and_complete_entry_labels(view: &DocumentView) {
    assert_eq!(view.navigation()[1].title, "ENTRIES · 4");
    assert_eq!(
        view.navigation()[1].full_title.as_deref(),
        Some("ENTRIES (4 direct · 1 nested · 3 forms)")
    );
    assert_eq!(view.navigation()[2].title, "--help");
    assert_eq!(
        view.navigation()[2].full_title.as_deref(),
        Some("--help MODE | -h")
    );
}

#[test]
fn sidebar_exposes_every_semantic_role_supported_by_the_document_contract() {
    let mut entries = [
        (DefinitionRole::Option, "--help"),
        (DefinitionRole::Command, "build"),
        (DefinitionRole::EnvironmentVariable, "MANT_HOME"),
        (DefinitionRole::Variable, "$LASTEXITCODE"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, name))| DefinitionItem {
        identity: Some(DefinitionIdentity {
            id: format!("entry-{index}").into(),
            role,
            case: DefinitionCase::Sensitive,
            names: vec![name.to_owned()],
        }),
        terms: Vec::new(),
        description: Vec::new(),
        inline_term: false,
        spacing_before_lines: None,
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
    entries[0].description = vec![Block::DefinitionList {
        items: vec![DefinitionItem {
            identity: Some(DefinitionIdentity {
                id: "entry-help-value".into(),
                role: DefinitionRole::Value,
                case: DefinitionCase::Sensitive,
                names: vec!["brief".to_owned()],
            }),
            terms: vec![vec![Inline::Code {
                value: "brief".to_owned(),
            }]],
            description: Vec::new(),
            inline_term: false,
            spacing_before_lines: None,
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
