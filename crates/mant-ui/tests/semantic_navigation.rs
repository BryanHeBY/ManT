use mant_ast::{
    Block, DefinitionCase, DefinitionIdentity, DefinitionItem, DefinitionRole, DocumentMeta,
    DocumentSchema, DocumentSource, LayoutHint, MantDocument, Producer, QueryBundle, QuerySchema,
    Section, SourceFormat,
};
use mant_ui::{DocumentView, NavKind};

#[test]
fn sidebar_exposes_every_semantic_role_supported_by_the_document_contract() {
    let entries = [
        (DefinitionRole::Option, "--help"),
        (DefinitionRole::Command, "build"),
        (DefinitionRole::EnvironmentVariable, "MANT_HOME"),
        (DefinitionRole::Variable, "$LASTEXITCODE"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, name))| DefinitionItem {
        identity: Some(DefinitionIdentity {
            id: format!("entry-{index}"),
            role,
            case: DefinitionCase::Sensitive,
            names: vec![name.to_owned()],
        }),
        terms: Vec::new(),
        description: Vec::new(),
        inline_term: false,
        spacing_before_lines: None,
    })
    .collect();
    let bundle = QueryBundle {
        schema: QuerySchema::V6,
        label: "tool".to_owned(),
        tldr: None,
        document: Some(MantDocument {
            schema: DocumentSchema::V6,
            producer: Producer {
                name: "test".to_owned(),
                version: "1".to_owned(),
                engine: None,
            },
            source: DocumentSource {
                format: SourceFormat::Markdown,
                path: None,
            },
            meta: DocumentMeta::default(),
            diagnostics: Vec::new(),
            blocks: Vec::new(),
            sections: vec![Section {
                id: "reference".to_owned(),
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
    assert_eq!(view.navigation()[1].title, "ENTRIES (4)");
    assert_eq!(
        view.navigation()[2..]
            .iter()
            .map(|item| item.kind)
            .collect::<Vec<_>>(),
        vec![
            NavKind::Entry(DefinitionRole::Option),
            NavKind::Entry(DefinitionRole::Command),
            NavKind::Entry(DefinitionRole::EnvironmentVariable),
            NavKind::Entry(DefinitionRole::Variable),
        ]
    );
}
