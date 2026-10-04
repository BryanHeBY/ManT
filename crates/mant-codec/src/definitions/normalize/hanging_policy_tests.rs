//! Pure IR ownership changes retain the complete original paragraph policy.
use super::*;
use crate::definitions::{NativeHeadEvidence, NativeHeadRole, identify_definitions_with_evidence};
use mant_ir::{
    Document, DocumentSource, EntryInlineRoot, EntryKind, EntryOwner, Inline, InlineLayout,
    LinkTarget, RowLayoutHint, Section, SourceFormat, SourceSpan,
};
use std::collections::HashSet;

fn text(value: &str) -> Inline {
    Inline::Text {
        value: value.into(),
    }
}

fn source(line: u32) -> SourceSpan {
    SourceSpan {
        line,
        column: 2,
        end_line: Some(line + 1),
        end_column: Some(9),
        byte_range: None,
    }
}

fn head(children: Vec<Inline>, continuation: i32) -> Block {
    Block::Paragraph {
        children,
        inline_layout: InlineLayout {
            row_hints: vec![RowLayoutHint {
                row: 0,
                indent_columns: -2,
            }],
        },
        layout: LayoutHint {
            indent_columns: 4,
            continuation_indent_columns: continuation,
            spacing_before_lines: 2,
        },
        source: Some(source(4)),
    }
}

fn description() -> Vec<Block> {
    vec![
        Block::Preformatted {
            language: None,
            children: vec![text("BodyWord"), Inline::line_break(), text("SecondRow")],
            inline_layout: InlineLayout {
                row_hints: vec![RowLayoutHint {
                    row: 1,
                    indent_columns: -1,
                }],
            },
            layout: LayoutHint {
                indent_columns: 9,
                continuation_indent_columns: 3,
                spacing_before_lines: 1,
            },
            source: Some(source(8)),
        },
        Block::VerticalSpace {
            lines: 2,
            source: Some(source(10)),
        },
        Block::Paragraph {
            children: vec![text("Original body continuation.")],
            inline_layout: InlineLayout::default(),
            layout: LayoutHint {
                indent_columns: 11,
                spacing_before_lines: 0,
                ..LayoutHint::default()
            },
            source: Some(source(11)),
        },
    ]
}

fn sections(head: Block, heading: &str) -> Vec<Section> {
    let mut blocks = vec![head];
    blocks.extend(description());
    vec![Section {
        id: "owners".into(),
        fragment_aliases: Vec::new(),
        heading: heading.into(),
        spacing_before_lines: 0,
        blocks,
        children: Vec::new(),
        source: Some(source(2)),
    }]
}

fn identify(sections: Vec<Section>, evidence: &NativeHeadEvidence) -> Document {
    identify_reserved(sections, evidence, &HashSet::new())
}

fn identify_reserved(
    mut sections: Vec<Section>,
    evidence: &NativeHeadEvidence,
    reserved: &HashSet<String>,
) -> Document {
    let mut blocks = Vec::new();
    let result =
        identify_definitions_with_evidence(&mut blocks, &mut sections, reserved, None, evidence);
    let diagnostics = {
        #[cfg(feature = "roff")]
        {
            let mut diagnostics = result.diagnostics;
            diagnostics.extend(crate::definitions::manual_discovery_diagnostics(&sections));
            diagnostics
        }
        #[cfg(not(feature = "roff"))]
        {
            result.diagnostics
        }
    };
    Document {
        parser: None,
        source: DocumentSource {
            format: SourceFormat::Man,
            path: Some("pure-ir-hanging-owner".into()),
        },
        meta: mant_ir::DocumentMeta::default(),
        heading: None,
        fragment_aliases: Vec::new(),
        diagnostics,
        blocks,
        sections,
    }
}

fn roundtrip(document: &Document) -> Document {
    let json = serde_json::to_string(document).unwrap();
    let restored: Document = serde_json::from_str(&json).unwrap();
    assert_eq!(&restored, document);
    restored
}

fn document_owner(document: &Document) -> EntryOwner<'_> {
    document.sections[0].blocks[0].entry_owner().unwrap()
}

fn witness(head: &Block) -> DefinitionItem {
    let Block::Paragraph {
        children,
        inline_layout,
        source,
        ..
    } = head
    else {
        unreachable!()
    };
    DefinitionItem {
        terms: vec![mant_ir::DefinitionTerm {
            content: children.clone(),
            inline_layout: inline_layout.clone(),
        }],
        description: Vec::new(),
        entry: None,
        head_body_relation: HeadBodyRelation::Separate,
        layout: mant_ir::DefinitionLayout::default(),
        source: *source,
    }
}

fn multipart_head(continuation: i32) -> Block {
    let mut head = head(
        vec![
            Inline::Strong {
                children: vec![text("-")],
            },
            Inline::Link {
                children: vec![Inline::Strong {
                    children: vec![text("alpha")],
                }],
                target: LinkTarget::External {
                    uri: "https://example.org/alpha".into(),
                },
                title: None,
            },
            text(" ARG"),
            Inline::line_break(),
            text("VALUE"),
            Inline::line_break(),
            text("MORE"),
        ],
        continuation,
    );
    let Block::Paragraph { inline_layout, .. } = &mut head else {
        unreachable!()
    };
    inline_layout.row_hints.push(RowLayoutHint {
        row: 2,
        indent_columns: 3,
    });
    head
}

#[test]
fn nonzero_h_keeps_original_blocks_sparse_hints_source_and_geometry() {
    for continuation in [-3, 6] {
        let head = multipart_head(continuation);
        let expected_head = head.clone();
        let original_sections = sections(head, "OPTIONS");
        let expected_description = original_sections[0].blocks[1..].to_vec();
        let Block::Preformatted { children, .. } = &original_sections[0].blocks[1] else {
            unreachable!()
        };
        let Inline::Text { value } = &children[0] else {
            unreachable!()
        };
        let original_body_allocation = value.as_ptr();
        let document = identify(original_sections, &NativeHeadEvidence::default());
        let Block::List {
            kind,
            layout,
            items,
            ..
        } = &document.sections[0].blocks[0]
        else {
            panic!("accepted H head retains a plain block owner")
        };
        assert_eq!(*kind, mant_ir::ListKind::Plain);
        assert_eq!(*layout, LayoutHint::default());
        assert_eq!(items[0].blocks[0], expected_head);
        assert_eq!(&items[0].blocks[1..], expected_description);
        let Block::Preformatted { children, .. } = &items[0].blocks[1] else {
            unreachable!()
        };
        let Inline::Text { value } = &children[0] else {
            unreachable!()
        };
        assert_eq!(value.as_ptr(), original_body_allocation, "move the body");
        assert_eq!(document.diagnostics, Vec::<mant_ir::Diagnostic>::new());
        let restored = roundtrip(&document);
        assert_sparse_owner_geometry(document_owner(&restored), continuation);
        let mut repeated = restored.sections.clone();
        identify_definitions_with_evidence(
            &mut Vec::new(),
            &mut repeated,
            &HashSet::new(),
            None,
            &NativeHeadEvidence::default(),
        );
        assert_eq!(
            repeated, restored.sections,
            "the existing Block0 owner stays owned"
        );
    }
}

fn assert_sparse_owner_geometry(owner: EntryOwner<'_>, continuation: i32) {
    let content = owner
        .inline_content_root(&EntryInlineRoot::Block { index: 0 })
        .unwrap();
    assert_eq!(content.layout.row_hints.len(), 2);
    assert_eq!(content.layout.row_indent(1), 0, "do not repeat H in hints");
    let expected = match continuation {
        6 => [(2, 8), (10, 10), (13, 13)],
        -3 => [(2, -1), (1, 1), (4, 4)],
        _ => unreachable!("the fixture covers both signs"),
    };
    for (row, (first_visual_origin, continuation_origin)) in expected.into_iter().enumerate() {
        let first = if row == 0 { 4 } else { 4 + continuation };
        assert_eq!(
            mant_ir::resolve_row_origins(first, 4 + continuation, content.layout.row_indent(row)),
            mant_ir::RowOrigins {
                first_visual_origin,
                continuation_origin,
            }
        );
    }
    assert!(content.sliced_layout(11..16).unwrap().is_empty());
    assert_eq!(
        content.sliced_layout(17..21).unwrap().row_hints,
        [RowLayoutHint {
            row: 0,
            indent_columns: 3,
        }]
    );
    assert_eq!(
        owner.form_inline_layout(&owner.facts().unwrap().forms[0]),
        Some(content.layout.clone())
    );
}

#[test]
fn named_h_owner_uses_complete_block0_form_and_contiguous_multipart_binding() {
    let baseline = identify(
        sections(multipart_head(0), "OPTIONS"),
        &NativeHeadEvidence::default(),
    );
    let document = identify(
        sections(multipart_head(6), "OPTIONS"),
        &NativeHeadEvidence::default(),
    );
    let restored = roundtrip(&document);
    let owner = document_owner(&restored);
    let facts = owner.facts().unwrap();
    let baseline_facts = document_owner(&baseline).facts().unwrap();
    assert_eq!(facts.id, baseline_facts.id);
    assert_eq!(facts.id.as_str(), "option-alpha");
    assert_eq!(facts.names, ["-alpha"]);
    assert_eq!(facts.kind, baseline_facts.kind);
    assert_eq!(facts.case, baseline_facts.case);
    assert_eq!(facts.forms.len(), 1);
    assert_eq!(facts.forms[0].parts.len(), 1);
    assert_eq!(
        facts.forms[0].parts[0].root,
        EntryInlineRoot::Block { index: 0 }
    );
    assert_eq!(facts.forms[0].parts[0].path, Vec::<usize>::new());
    assert_eq!(facts.forms[0].parts[0].bytes, None);
    assert_eq!(
        mant_ir::inline_plain_text(&owner.form(&facts.forms[0]).unwrap()),
        "-alpha ARG\nVALUE\nMORE"
    );
    let occurrence = &facts.name_bindings[0].occurrences[0];
    let ranges = occurrence
        .parts
        .iter()
        .map(|part| mant_ir::project_content_slice(owner, part).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].chars, 0..1);
    assert_eq!(ranges[1].chars, 1..6);
    assert!(
        ranges
            .iter()
            .all(|range| range.root == (EntryInlineRoot::Block { index: 0 }))
    );
    assert_eq!(
        mant_ir::inline_plain_text(&owner.form(occurrence).unwrap()),
        "-alpha"
    );
    assert_eq!(owner.validated_names().unwrap(), ["-alpha"]);
}

#[test]
fn aliasless_templates_keep_admission_preferred_id_and_unclassified_warning() {
    for (value, heading) in [("-<name>", "OPTIONS"), ("LC_<category>", "ENVIRONMENT")] {
        let baseline = identify(
            sections(head(vec![text(value)], 0), heading),
            &NativeHeadEvidence::default(),
        );
        let document = identify(
            sections(head(vec![text(value)], 6), heading),
            &NativeHeadEvidence::default(),
        );
        let restored = roundtrip(&document);
        let facts = document_owner(&restored).facts().unwrap();
        let baseline_facts = document_owner(&baseline).facts().unwrap();
        assert_eq!(facts.id, baseline_facts.id, "{value}");
        assert_ne!(facts.id.as_str(), "term-entry");
        assert_eq!(facts.kind, EntryKind::Term);
        assert_eq!(facts.names, Vec::<String>::new());
        assert_eq!(facts.name_bindings, Vec::<mant_ir::EntryNameBinding>::new());
        assert_eq!(document.diagnostics, baseline.diagnostics);
        assert_eq!(
            mant_ir::inline_plain_text(&document_owner(&restored).form(&facts.forms[0]).unwrap()),
            value
        );
        #[cfg(feature = "roff")]
        {
            assert_eq!(document.diagnostics.len(), 1, "{value}");
            assert_eq!(
                document.diagnostics[0].code.as_deref(),
                Some("manual.semantic-entry.unclassified-definition")
            );
            assert_eq!(document.diagnostics[0].source, Some(source(4)));
        }
    }
}

#[test]
fn presentation_h_retains_the_unowned_original_run_and_zero_h_keeps_definitions() {
    for continuation in [0, 6] {
        let head = head(vec![text("--presentation")], continuation);
        let mut evidence = NativeHeadEvidence::default();
        evidence.record(&witness(&head), NativeHeadRole::Presentation);
        let original = sections(head, "OPTIONS");
        let expected = original[0].blocks.clone();
        let restored = roundtrip(&identify(original, &evidence));
        assert_eq!(restored.diagnostics, Vec::<mant_ir::Diagnostic>::new());
        if continuation == 0 {
            let Block::DefinitionList { items, .. } = &restored.sections[0].blocks[0] else {
                panic!("H=0 keeps the existing definition topology")
            };
            assert!(items[0].entry.is_none());
            assert_eq!(items[0].head_body_relation, HeadBodyRelation::Separate);
        } else {
            assert_eq!(restored.sections[0].blocks, expected);
        }
    }
}

#[test]
fn repeated_preferred_names_and_reserved_anchors_keep_unique_stable_owner_ids() {
    let mut input = sections(head(vec![text("-<name>")], 6), "OPTIONS");
    input[0].id = "left".into();
    let mut right = sections(head(vec![text("-<name>")], 6), "OPTIONS");
    right[0].id = "right".into();
    let Block::Paragraph { children, .. } = &mut right[0].blocks[3] else {
        unreachable!()
    };
    *children = vec![text("Distinct original description.")];
    input.extend(right);
    let mut reserved_owner = sections(
        head(
            vec![Inline::anchor("option-reserved"), text("--reserved")],
            6,
        ),
        "OPTIONS",
    );
    reserved_owner[0].id = "reserved".into();
    input.extend(reserved_owner);
    let reserved = HashSet::from(["option-reserved".to_owned()]);
    let document = roundtrip(&identify_reserved(
        input,
        &NativeHeadEvidence::default(),
        &reserved,
    ));
    let ids = document
        .sections
        .iter()
        .map(|section| {
            section.blocks[0]
                .entry_owner()
                .unwrap()
                .facts()
                .unwrap()
                .id
                .to_string()
        })
        .collect::<Vec<_>>();
    assert_ne!(ids[0], ids[1]);
    assert!(ids[..2].iter().all(|id| id.starts_with("term-name-")));
    assert!(ids[2].starts_with("option-reserved-"));
    assert_ne!(ids[2], "option-reserved");
    let mut repeated = document.sections.clone();
    identify_definitions_with_evidence(
        &mut Vec::new(),
        &mut repeated,
        &reserved,
        None,
        &NativeHeadEvidence::default(),
    );
    assert_eq!(repeated, document.sections);
    let EntryOwner::List(item) = repeated[2].blocks[0].entry_owner().unwrap() else {
        unreachable!()
    };
    let Block::Paragraph { children, .. } = &item.blocks[0] else {
        unreachable!()
    };
    assert!(matches!(&children[0], Inline::Anchor { id, .. } if id.as_str() == "option-reserved"));
}

#[cfg(feature = "roff")]
#[test]
fn private_head_marker_is_removed_before_original_block_bindings_are_captured() {
    let mut head = multipart_head(6);
    let Block::Paragraph { children, .. } = &mut head else {
        unreachable!()
    };
    crate::definitions::groups::mark_native_inline_owner(children, 10);
    let mut evidence = NativeHeadEvidence::default();
    evidence.record(&witness(&head), NativeHeadRole::Option);
    let restored = roundtrip(&identify(sections(head, "OPTIONS"), &evidence));
    let owner = document_owner(&restored);
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, ["-alpha"]);
    let parts = &facts.name_bindings[0].occurrences[0].parts;
    assert_eq!(parts[0].path, [0, 0]);
    assert_eq!(parts[1].path, [1, 0, 0]);
    let content = owner
        .inline_root(&EntryInlineRoot::Block { index: 0 })
        .unwrap();
    assert_eq!(
        crate::definitions::groups::native_inline_owner(content),
        None
    );
    assert_eq!(
        mant_ir::inline_plain_text(&owner.form(&facts.name_bindings[0].occurrences[0]).unwrap()),
        "-alpha"
    );
}

#[cfg(feature = "roff")]
#[test]
fn accepted_native_name_limit_survives_the_h_owner_change_without_partial_names() {
    for count in [256, 257] {
        let value = (0..count)
            .map(|index| format!("--flag{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut documents = Vec::new();
        for continuation in [0, 6] {
            let mut head = head(
                vec![Inline::Strong {
                    children: vec![text(&value)],
                }],
                continuation,
            );
            let Block::Paragraph { children, .. } = &mut head else {
                unreachable!()
            };
            crate::definitions::groups::mark_native_inline_owner(children, 20);
            let witness = witness(&head);
            let mut evidence = NativeHeadEvidence::default();
            evidence.record(&witness, NativeHeadRole::Option);
            let mut start = 0;
            let operands = value
                .split(", ")
                .map(|name| {
                    let operand = crate::definitions::NativeOperand {
                        bytes: start..start + name.len(),
                        role: crate::definitions::NativeOperandRole::ExplicitOption,
                    };
                    start += name.len() + 2;
                    operand
                })
                .collect();
            evidence.capture_operands(
                20,
                crate::definitions::CapturedHeadOperands {
                    text: value.clone(),
                    operands,
                },
            );
            evidence.record_operands(&witness, 20);
            documents.push(roundtrip(&identify(sections(head, "OPTIONS"), &evidence)));
        }
        let baseline = document_owner(&documents[0]);
        let preserved = document_owner(&documents[1]);
        let facts = preserved.facts().unwrap();
        assert_eq!(facts.id, baseline.facts().unwrap().id);
        assert_eq!(facts.names, baseline.facts().unwrap().names);
        assert_eq!(documents[0].diagnostics, documents[1].diagnostics);
        assert_eq!(
            mant_ir::inline_plain_text(&preserved.form(&facts.forms[0]).unwrap()),
            value
        );
        if count == 256 {
            assert_eq!(facts.names.len(), 256);
            assert_eq!(documents[1].diagnostics, Vec::<mant_ir::Diagnostic>::new());
        } else {
            assert_eq!(facts.names, Vec::<String>::new());
            assert_eq!(facts.name_bindings, Vec::<mant_ir::EntryNameBinding>::new());
            assert_eq!(documents[1].diagnostics.len(), 1);
            assert_eq!(
                documents[1].diagnostics[0].code.as_deref(),
                Some("manual.semantic-entry.name-limit")
            );
            assert_eq!(documents[1].diagnostics[0].source, Some(source(4)));
        }
    }
}

#[cfg(feature = "roff")]
#[test]
fn constructed_option_role_without_operand_receipt_does_not_report_a_name_limit() {
    let value = (0..257)
        .map(|index| format!("--flag{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    for continuation in [0, 6] {
        let head = head(
            vec![Inline::Strong {
                children: vec![text(&value)],
            }],
            continuation,
        );
        let mut evidence = NativeHeadEvidence::default();
        evidence.record(&witness(&head), NativeHeadRole::Option);
        let document = roundtrip(&identify(sections(head, "OPTIONS"), &evidence));
        let owner = document_owner(&document);
        let facts = owner.facts().unwrap();
        assert_eq!(
            facts.kind,
            EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            }
        );
        assert_eq!(facts.names, Vec::<String>::new());
        assert_eq!(facts.name_bindings, Vec::<mant_ir::EntryNameBinding>::new());
        assert_eq!(document.diagnostics, Vec::<mant_ir::Diagnostic>::new());
        assert_eq!(
            mant_ir::inline_plain_text(&owner.form(&facts.forms[0]).unwrap()),
            value
        );
    }
}
