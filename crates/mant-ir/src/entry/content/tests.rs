use super::*;
use crate::{
    Document, DocumentIndex, DocumentMeta, DocumentSource, EntryKind, LayoutHint, ListKind,
    NameCase, SemanticIndex, SourceFormat, ValueDomain,
};

fn item(id: &str, kind: EntryKind, name: &str) -> ListItem {
    ListItem {
        layout: crate::ListItemLayout::default(),
        source: None,
        entry: Some(EntryFacts {
            name_bindings: vec![EntryNameBinding {
                name: 0,
                evidence: EntryNameEvidence::Declared,
                occurrences: vec![EntryForm {
                    parts: vec![EntryContentSlice {
                        root: EntryInlineRoot::Block { index: 0 },
                        path: vec![0],
                        bytes: None,
                    }],
                }],
            }],
            alias_groups: Vec::new(),
            alias_of: None,
            id: id.into(),
            kind,
            case: NameCase::Sensitive,
            names: vec![name.into()],
            value_domain: None,
            forms: vec![EntryForm {
                parts: vec![EntryContentSlice {
                    root: EntryInlineRoot::Block { index: 0 },
                    path: vec![0],
                    bytes: None,
                }],
            }],
        }),
        blocks: vec![Block::Paragraph {
            inline_layout: crate::InlineLayout::default(),
            children: vec![
                Inline::Code { value: name.into() },
                Inline::Text {
                    value: ": Body — unchanged.".into(),
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }],
    }
}

fn list(items: Vec<ListItem>) -> Block {
    Block::List {
        kind: ListKind::Ordered { start: Some(7) },
        items,
        compact: false,
        layout: LayoutHint::default(),
        source: None,
    }
}

fn document(blocks: Vec<Block>) -> Document {
    Document {
        heading: None,
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Markdown,
            path: None,
        },
        meta: DocumentMeta::default(),
        fragment_aliases: Vec::new(),
        diagnostics: Vec::new(),
        blocks,
        sections: Vec::new(),
    }
}

#[test]
fn ordinary_owners_rebuild_indexes_and_choices_without_rewriting_content() {
    let mut parent = item(
        "option-color",
        EntryKind::Parameter {
            parameter_kind: crate::ParameterKind::Option,
        },
        "--color",
    );
    let original = parent.blocks.clone();
    parent
        .blocks
        .push(list(vec![item("value-auto", EntryKind::Value, "auto")]));
    parent.entry.as_mut().unwrap().value_domain = Some(ValueDomain::Choices { exhaustive: true });
    assert!(parent.has_value_choices());
    let doc = document(vec![list(vec![parent])]);
    assert_eq!(crate::validate_document(&doc).len(), 0);
    let rebuilt: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(rebuilt, doc);
    let index = SemanticIndex::build(&rebuilt);
    assert_eq!(index.root().len(), 1);
    assert_eq!(index.root()[0].forms, ["--color"]);
    assert_eq!(index.root()[0].children[0].names, ["auto"]);
    assert!(DocumentIndex::build(&rebuilt).contains("value-auto"));
    let Block::List {
        items,
        kind,
        compact,
        ..
    } = &rebuilt.blocks[0]
    else {
        panic!("ordinary list");
    };
    assert_eq!(
        (*kind, *compact),
        (ListKind::Ordered { start: Some(7) }, false)
    );
    assert_eq!(&items[0].blocks[..1], original);
    let mut unannotated = items[0].clone();
    unannotated.entry = None;
    assert_eq!(unannotated.blocks, items[0].blocks);
}

#[test]
fn borrowed_form_count_agrees_with_atomic_projection_for_valid_and_invalid_bindings() {
    let original = item("term-name", EntryKind::Term, "é名");
    for parts in [
        vec![],
        vec![EntryContentSlice {
            root: EntryInlineRoot::Block { index: 0 },
            path: vec![0],
            bytes: None,
        }],
        vec![EntryContentSlice {
            root: EntryInlineRoot::Block { index: 0 },
            path: vec![0],
            bytes: Some(0..2),
        }],
        vec![EntryContentSlice {
            root: EntryInlineRoot::Block { index: 0 },
            path: vec![0],
            bytes: Some(1..2),
        }],
        vec![EntryContentSlice {
            root: EntryInlineRoot::Block { index: 99 },
            path: vec![],
            bytes: None,
        }],
        vec![EntryContentSlice {
            root: EntryInlineRoot::Block { index: 0 },
            path: vec![],
            bytes: Some(0..2),
        }],
    ] {
        let mut item = original.clone();
        item.entry.as_mut().unwrap().forms = vec![EntryForm { parts }];
        let owner = EntryOwner::List(&item);
        assert_eq!(
            owner.validated_form_count(),
            owner.forms().map(|forms| forms.iter().count())
        );
    }
    let mut item = original;
    item.entry.as_mut().unwrap().forms.clear();
    assert_eq!(EntryOwner::List(&item).validated_form_count(), Some(0));
}

#[test]
fn form_slices_preserve_link_style_and_reject_invalid_utf8_and_owner_paths() {
    let mut item = item("term-name", EntryKind::Term, "é名");
    let Block::Paragraph { children, .. } = &mut item.blocks[0] else {
        panic!("paragraph");
    };
    children[0] = Inline::Link {
        target: crate::LinkTarget::Document {
            name: "other".into(),
            fragment: None,
        },
        title: None,
        children: vec![Inline::Strong {
            children: vec![Inline::Code {
                value: "é名".into(),
            }],
        }],
    };
    let slice = EntryContentSlice {
        root: EntryInlineRoot::Block { index: 0 },
        path: vec![0, 0, 0],
        bytes: Some(0..2),
    };
    let owner = EntryOwner::List(&item);
    assert!(
        matches!(owner.content_slice(&slice).unwrap().as_slice(), [Inline::Link { children, .. }]
            if matches!(children.as_slice(), [Inline::Strong { children }] if matches!(children.as_slice(), [Inline::Code { value } | Inline::Equation { value, .. }] if value == "é")))
    );
    for invalid in [
        EntryContentSlice {
            bytes: Some(1..2),
            ..slice.clone()
        },
        EntryContentSlice {
            bytes: Some(0..999),
            ..slice.clone()
        },
        EntryContentSlice {
            path: vec![0],
            ..slice.clone()
        },
        EntryContentSlice {
            root: EntryInlineRoot::Term { index: 0 },
            ..slice.clone()
        },
        EntryContentSlice {
            root: EntryInlineRoot::Block { index: 1 },
            ..slice.clone()
        },
    ] {
        assert!(owner.content_slice(&invalid).is_none());
    }
    assert_eq!(owner.facts().unwrap().names, ["é名"]);
}

#[test]
fn styled_link_slices_validate_scalar_name_bindings() {
    let mut item = item("styled-name", EntryKind::Term, "é");
    let Block::Paragraph { children, .. } = &mut item.blocks[0] else {
        panic!("paragraph");
    };
    children[0] = Inline::Link {
        target: crate::LinkTarget::Document {
            name: "other".into(),
            fragment: None,
        },
        title: None,
        children: vec![Inline::Strong {
            children: vec![Inline::Code {
                value: "é名".into(),
            }],
        }],
    };
    let slice = EntryContentSlice {
        root: EntryInlineRoot::Block { index: 0 },
        path: vec![0, 0, 0],
        bytes: Some(0..2),
    };
    let form = EntryForm {
        parts: vec![slice.clone()],
    };
    let facts = item.entry.as_mut().unwrap();
    facts.forms = vec![form.clone()];
    facts.name_bindings[0].occurrences = vec![form.clone()];
    let json = serde_json::to_string(&item).unwrap();
    let decoded = serde_json::from_str::<ListItem>(&json).unwrap();
    let owner = EntryOwner::List(&decoded);
    assert_eq!(owner.validated_form_count(), Some(1));
    assert!(owner.form_text_equals(&form, "é"));
    assert!(!owner.form_text_equals(&form, "é名"));
    let selected = owner.content_slice(&slice).unwrap();
    assert_eq!(crate::inline_plain_text(&selected), "é");
    assert_eq!(
        crate::project_content_slice(owner, &slice).unwrap().chars,
        0..1
    );
    let complete = owner
        .content_slice(&EntryContentSlice {
            root: slice.root.clone(),
            path: vec![0],
            bytes: None,
        })
        .unwrap();
    assert!(matches!(complete.as_slice(), [Inline::Link { .. }]));
    assert!(
        matches!(selected.as_slice(), [Inline::Link { children, .. }]
            if matches!(children.as_slice(), [Inline::Strong { children }]
                if matches!(children.as_slice(), [Inline::Code { value }] if value == "é")))
    );
    assert!(
        owner
            .content_slice(&EntryContentSlice {
                bytes: Some(1..2),
                ..slice
            })
            .is_none()
    );
    assert_eq!(
        crate::validate_document(&document(vec![list(vec![decoded])])),
        []
    );
}

#[test]
fn invalid_form_bindings_are_diagnostics_not_partial_index_facts() {
    for parts in [vec![], vec![0, 0], vec![1, 0], vec![0, 9]] {
        let mut entry = item(
            "option-probe",
            EntryKind::Parameter {
                parameter_kind: crate::ParameterKind::Option,
            },
            "--probe",
        );
        entry.entry.as_mut().unwrap().forms[0].parts = parts
            .into_iter()
            .map(|index| EntryContentSlice {
                root: EntryInlineRoot::Block { index: 0 },
                path: vec![index],
                bytes: None,
            })
            .collect();
        let doc = document(vec![list(vec![entry])]);
        let diagnostics = crate::validate_document(&doc);
        assert!(diagnostics.iter().any(|d| {
            d.code.as_deref() == Some("ir.invalid-entry-content")
                && d.impact == crate::DiagnosticImpact::SemanticCoverage
        }));
        assert!(!crate::semantics_complete(&diagnostics));
        let index = SemanticIndex::build(&doc);
        assert_eq!(index.root().len(), 1);
        assert_eq!(index.root()[0].forms.len(), 0);
        assert_eq!(index.root()[0].names.len(), 0);
    }
}

#[test]
fn explicit_terms_borrow_and_unrecorded_forms_do_not_remove_owners() {
    let list_item = item("parent", EntryKind::Term, "one");
    let mut native = DefinitionItem {
        head_body_relation: crate::HeadBodyRelation::from(false),
        source: None,
        entry: list_item.entry,
        terms: (vec![
            vec![Inline::Code {
                value: "one".into(),
            }],
            vec![Inline::Code {
                value: "two".into(),
            }],
        ])
        .into_iter()
        .map(Into::into)
        .collect(),
        description: vec![list(vec![item("child", EntryKind::Value, "auto")])],
        layout: crate::DefinitionLayout {
            body_alignment: crate::DefinitionBodyAlignment::Indented,
            spacing_before_lines: None,
            ..Default::default()
        },
    };
    let facts = native.entry.as_mut().unwrap();
    facts.names.clear();
    facts.name_bindings.clear();
    facts.forms = vec![EntryForm::term(0), EntryForm::term(1)];
    let owner = EntryOwner::Definition(&native);
    let Some(EntryForms::Borrowed(terms)) = owner.forms() else {
        panic!("complete terms must borrow")
    };
    assert!(std::ptr::eq(terms, native.terms.as_slice()));
    assert!(
        matches!(owner.form(&EntryForm::term(1)), Some(Cow::Borrowed(term)) if std::ptr::eq(term, native.terms[1].as_slice()))
    );
    native.entry.as_mut().unwrap().forms = vec![EntryForm::term(1), EntryForm::term(0)];
    let Some(EntryForms::Projected(forms)) = EntryOwner::Definition(&native).forms() else {
        panic!("separate borrowed forms")
    };
    assert!(forms.iter().all(|form| matches!(form, Cow::Borrowed(_))));
    native.entry.as_mut().unwrap().forms.clear();
    assert!(matches!(
        EntryOwner::Definition(&native).forms(),
        Some(EntryForms::Unrecorded)
    ));
    let doc = document(vec![Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![native],
        compact: true,
        layout: LayoutHint::default(),
        source: None,
    }]);
    let index = SemanticIndex::build(&doc);
    assert_eq!(index.root()[0].forms.len(), 0);
    assert_eq!(index.root()[0].children[0].id, "child");
    assert_eq!(crate::validate_document(&doc).len(), 0);
}

#[test]
fn invalid_names_leave_valid_forms_and_children_intact() {
    let mut parent = item("parent", EntryKind::Command, "run");
    parent.entry.as_mut().unwrap().name_bindings.clear();
    parent
        .blocks
        .push(list(vec![item("child", EntryKind::Value, "auto")]));
    let doc = document(vec![list(vec![parent])]);
    let index = SemanticIndex::build(&doc);
    assert_eq!(index.root()[0].names.len(), 0);
    assert_eq!(index.root()[0].forms, ["run"]);
    assert_eq!(index.root()[0].children.len(), 1);
    assert!(
        crate::validate_document(&doc)
            .iter()
            .any(|d| d.code.as_deref() == Some("ir.invalid-entry-name-binding"))
    );
}

#[test]
fn empty_group_projection_never_bypasses_name_or_nonempty_group_validation() {
    for valid_names in [false, true] {
        for groups in [Vec::new(), vec![vec!["run".into(), "unbound".into()]]] {
            let mut owner = item("run", EntryKind::Command, "run");
            let facts = owner.entry.as_mut().unwrap();
            facts.alias_groups = groups;
            if !valid_names {
                facts.name_bindings.clear();
            }
            let doc = document(vec![list(vec![owner])]);
            let projected = SemanticIndex::build(&doc);
            assert_eq!(projected.root()[0].id, "run");
            assert_eq!(projected.root()[0].forms, ["run"]);
            assert_eq!(projected.root()[0].names.is_empty(), !valid_names);
            assert_eq!(projected.root()[0].alias_groups.len(), 0);
        }
    }
}
