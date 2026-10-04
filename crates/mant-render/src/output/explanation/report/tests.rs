//! Forms may carry geometry only from their validated original owner slice.
use super::*;
use mant_ir::{
    Block, DefinitionItem, DefinitionTerm, EntryContentSlice, EntryFacts, EntryForm,
    EntryInlineRoot, EntryKind, Inline, InlineLayout, LayoutHint, NameCase, RowLayoutHint,
};
use mant_protocol::{ExplanationContent, ExplanationEntry, OutlineNodeReference, OutlineTrail};

fn evidence() -> ExplanationEvidence {
    let original = "BEFORE\n  é名\nBeta";
    let form = vec![Inline::Text {
        value: "  é名\nBeta".into(),
    }];
    let term = |correction| DefinitionTerm {
        content: vec![Inline::Text {
            value: original.into(),
        }],
        inline_layout: InlineLayout {
            row_hints: vec![
                RowLayoutHint {
                    row: 1,
                    indent_columns: correction,
                },
                RowLayoutHint {
                    row: 2,
                    indent_columns: -2,
                },
            ],
        },
    };
    let facts = EntryFacts {
        id: "entry".into(),
        kind: EntryKind::Term,
        case: NameCase::Sensitive,
        names: vec!["é名".into()],
        name_bindings: vec![],
        alias_groups: vec![],
        alias_of: None,
        value_domain: None,
        forms: vec![EntryForm {
            parts: vec![EntryContentSlice {
                root: EntryInlineRoot::Term { index: 1 },
                path: vec![0],
                bytes: Some(7..original.len()),
            }],
        }],
    };
    ExplanationEvidence {
        support: None,
        support_omitted: false,
        class: EvidenceClass::DirectEntry,
        ordinal: 0,
        outline: OutlineTrail {
            ancestors: vec![],
            node: OutlineNodeReference::DocumentEntry {
                path: "root/e0".into(),
                id: "entry".into(),
                title: "é名".into(),
                entry_kind: EntryKind::Term,
                case: NameCase::Sensitive,
                names: vec!["é名".into()],
            },
        },
        block_path: None,
        source: None,
        bases: vec![],
        previews: vec![],
        previews_omitted: false,
        entry: Some(ExplanationEntry {
            kind: EntryKind::Term,
            case: NameCase::Sensitive,
            names: vec!["é名".into()],
            forms: vec![form],
            name_bindings: vec![],
            alias_groups: vec![],
            alias_of: None,
            value_domain: None,
        }),
        content: Some(ExplanationContent::Entry {
            block: Block::DefinitionList {
                items: vec![DefinitionItem {
                    terms: vec![term(20), term(4)],
                    description: vec![],
                    source: None,
                    entry: Some(facts),
                    layout: mant_ir::DefinitionLayout::default(),
                }],
                declaration_groups: vec![],
                compact: true,
                layout: LayoutHint::default(),
                source: None,
            },
        }),
        details_omitted: false,
        match_details_omitted: false,
        name_bindings_omitted: false,
        // This explicitly requests separate Forms even with a retained excerpt.
        content_omitted: true,
    }
}

#[test]
fn report_forms_rebase_exact_unicode_source_slices_and_preserve_authored_spaces() {
    let evidence = evidence();
    let nodes = &evidence.entry.as_ref().unwrap().forms[0];
    let layout = form_layout(&evidence, &[], 0, nodes).unwrap();
    assert_eq!(layout.row_indent(0), 4);
    assert_eq!(layout.row_indent(1), -2);
    let styles = spans::LocatedStyles::with_pool(&evidence, &[]);
    let report = Report {
        markdown: false,
        decorate: &|_, text| text.into(),
    };
    let mut output = String::new();
    report.details(&mut output, &evidence, &styles, false, &[]);
    assert_eq!(output, "\nForms:\n      é名\nBeta");
    assert_eq!(mant_ir::inline_plain_text(nodes), "  é名\nBeta");
}

#[test]
fn report_forms_without_a_valid_source_owner_have_no_inferred_row_geometry() {
    let mut evidence = evidence();
    let returned = evidence.entry.as_ref().unwrap().forms[0].clone();
    evidence.outline.node = OutlineNodeReference::DocumentRoot {
        path: "root".into(),
        id: "other".into(),
        title: "ROOT".into(),
    };
    assert!(form_layout(&evidence, &[], 0, &returned).is_none());
    evidence.content = None;
    assert!(form_layout(&evidence, &[], 0, &returned).is_none());
    let styles = spans::LocatedStyles::with_pool(&evidence, &[]);
    let report = Report {
        markdown: false,
        decorate: &|_, text| text.into(),
    };
    let mut output = String::new();
    report.details(&mut output, &evidence, &styles, false, &[]);
    assert_eq!(output, "\nForms:\n  é名\nBeta");
}
