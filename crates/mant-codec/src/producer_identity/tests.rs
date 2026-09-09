use super::*;
use mant_ir::{
    DefinitionItem, DefinitionLayout, EntryFacts, EntryKind, Heading, Inline, LayoutHint, NameCase,
};

fn span(line: u32) -> SourceSpan {
    SourceSpan {
        byte_range: None,
        line,
        column: 1,
        end_line: None,
        end_column: None,
    }
}

fn definition(id: &str, line: u32, children: Vec<Block>) -> DefinitionItem {
    DefinitionItem {
        terms: vec![vec![Inline::Code {
            value: "same-name".into(),
        }]],
        description: children,
        entry: Some(EntryFacts {
            id: id.into(),
            kind: EntryKind::Term,
            case: NameCase::Sensitive,
            // Intentionally invalid bindings: identity diagnostics must not
            // validate/filter semantic names before locating their owners.
            names: vec!["same-name".into()],
            name_bindings: Vec::new(),
            forms: Vec::new(),
            alias_groups: Vec::new(),
            alias_of: None,
            value_domain: None,
        }),
        layout: DefinitionLayout::default(),
        source: Some(span(line)),
    }
}

fn definitions(items: Vec<DefinitionItem>) -> Block {
    Block::DefinitionList {
        declaration_groups: Vec::new(),
        items,
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn section(id: &str, line: u32, blocks: Vec<Block>, children: Vec<Section>) -> Section {
    Section {
        id: id.into(),
        heading: Heading {
            content: vec![Inline::Text {
                value: "heading".into(),
            }],
            source: None,
        },
        fragment_aliases: Vec::new(),
        spacing_before_lines: 0,
        blocks,
        children,
        source: Some(span(line)),
    }
}

#[test]
fn duplicate_identity_diagnostics_preserve_source_order_paths_and_first_span() {
    let blocks = vec![definitions(vec![definition(
        "same",
        1,
        vec![definitions(vec![definition("same", 2, Vec::new())])],
    )])];
    let sections = vec![section(
        "same",
        3,
        vec![definitions(vec![definition("same", 4, Vec::new())])],
        vec![section("same", 5, Vec::new(), Vec::new())],
    )];
    let diagnostics = outline_identity_diagnostics(&blocks, &sections, "markdown");
    assert_eq!(diagnostics.len(), 1);
    let diagnostic = &diagnostics[0];
    assert_eq!(diagnostic.level, DiagnosticLevel::Warning);
    assert_eq!(diagnostic.impact, mant_ir::DiagnosticImpact::None);
    assert_eq!(
        diagnostic.code.as_deref(),
        Some("markdown.outline.duplicate-id")
    );
    assert_eq!(diagnostic.source, Some(span(1)));
    assert_eq!(
        diagnostic.message,
        "outline ID 'same' belongs to multiple nodes: root/e1 (same), root/e1/e1 (same), 1 (same), 1/e1 (same), 1.1 (same); select by path"
    );
}

#[test]
fn diagnostics_sort_by_identity_and_do_not_confuse_names_or_transparent_containers() {
    let mut transparent = definition(
        "ignored",
        1,
        vec![definitions(vec![
            definition("zeta", 2, Vec::new()),
            definition("alpha", 3, Vec::new()),
        ])],
    );
    transparent.entry = None;
    let blocks = vec![definitions(vec![
        transparent,
        definition("unique", 4, Vec::new()),
        definition("zeta", 5, Vec::new()),
        definition("alpha", 6, Vec::new()),
    ])];
    let diagnostics = outline_identity_diagnostics(&blocks, &[], "manual");
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(
        diagnostics[0].message,
        "outline ID 'alpha' belongs to multiple nodes: root/e2 (alpha), root/e5 (alpha); select by path"
    );
    assert_eq!(
        diagnostics[1].message,
        "outline ID 'zeta' belongs to multiple nodes: root/e1 (zeta), root/e4 (zeta); select by path"
    );
    assert_eq!(diagnostics[0].source, Some(span(3)));
    assert!(
        diagnostics
            .iter()
            .all(|item| item.code.as_deref() == Some("manual.outline.duplicate-id"))
    );
}

#[test]
fn authored_identity_reservations_keep_structural_grammar_and_generated_prefixes() {
    for value in [
        "root",
        "tldr",
        "0",
        "1",
        "2.1",
        "root/e1",
        "2.1/e3",
        "option-help",
        "marker-end",
        "operand-file",
        "command-test",
        "configuration-key",
        "environment-path",
        "variable-home",
        "value-auto",
        "term-name",
    ] {
        assert!(is_reserved_selector(value), "{value}");
    }
    for value in [
        "ordinary-heading",
        "ROOT",
        "2.0",
        "0.1",
        "1/e0",
        "1/e",
        "root/e",
        "option",
        "unrelated",
    ] {
        assert!(!is_reserved_selector(value), "{value}");
    }
}

fn declared_definition(
    id: &str,
    role: EntryKind,
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
                .map(|(name, spelling)| mant_ir::EntryNameBinding {
                    name,
                    evidence: mant_ir::EntryNameEvidence::Declared,
                    occurrences: forms
                        .iter()
                        .enumerate()
                        .filter_map(|(index, form)| {
                            form.find(spelling).map(|start| mant_ir::EntryForm {
                                parts: vec![mant_ir::EntryContentSlice {
                                    root: mant_ir::EntryInlineRoot::Term { index },
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
            forms: (0..forms.len()).map(mant_ir::EntryForm::term).collect(),
            id: id.into(),
            kind: role,
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
        layout: mant_ir::DefinitionLayout {
            inline_term: false,
            spacing_before_lines: None,
            ..Default::default()
        },
    }
}

#[test]
fn repeated_names_do_not_create_content_identity_diagnostics() {
    let sensitive = declared_definition(
        "command-sensitive-mode",
        EntryKind::Command,
        &["Mode"],
        &["Mode"],
        Vec::new(),
    );
    let mut insensitive = declared_definition(
        "command-insensitive-mode",
        EntryKind::Command,
        &["MODE", "mode"],
        &["MODE", "mode"],
        Vec::new(),
    );
    insensitive.entry.as_mut().expect("identity").case = NameCase::Insensitive;
    let blocks = vec![Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![sensitive, insensitive],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }];
    let sections = vec![Section {
        id: "mode".into(),
        fragment_aliases: Vec::new(),
        heading: "Mode".into(),
        spacing_before_lines: 0,
        blocks: Vec::new(),
        children: Vec::new(),
        source: None,
    }];

    let diagnostics = outline_identity_diagnostics(&blocks, &sections, "manual");
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}
