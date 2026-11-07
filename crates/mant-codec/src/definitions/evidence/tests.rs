use super::*;

fn item() -> DefinitionItem {
    DefinitionItem {
        head_body_relation: mant_ir::HeadBodyRelation::Separate,
        source: Some(SourceSpan {
            line: 12,
            column: 4,
            byte_range: None,
            end_line: None,
            end_column: None,
        }),
        entry: None,
        terms: (vec![vec![Inline::Strong {
            children: vec![Inline::Text {
                value: "PATH".into(),
            }],
        }]])
        .into_iter()
        .map(Into::into)
        .collect(),
        description: Vec::new(),
        layout: mant_ir::DefinitionLayout {
            body_alignment: mant_ir::DefinitionBodyAlignment::Indented,
            spacing_before_lines: None,
            ..Default::default()
        },
    }
}

#[test]
fn linked_head_navigation_does_not_invalidate_native_evidence() {
    let mut original = item();
    original.terms[0].content = vec![Inline::Link {
        target: mant_ir::LinkTarget::External {
            uri: "https://example.invalid".into(),
        },
        title: None,
        children: original.terms[0].content.clone(),
    }];
    let mut evidence = NativeHeadEvidence::default();
    evidence.record(&original, NativeHeadRole::Environment);
    let mut moved = original.clone();
    let Inline::Link { children, .. } = &mut moved.terms[0].content[0] else {
        panic!("linked head");
    };
    children.insert(0, Inline::anchor("allocated-navigation-id"));
    assert_eq!(evidence.role(&moved), Some(NativeHeadRole::Environment));
    moved.terms[0].content.push(Inline::Text {
        value: "different native text".into(),
    });
    assert_eq!(evidence.role(&moved), None);
}

#[test]
fn witnesses_require_full_source_and_styled_content_not_a_line_or_address() {
    let original = item();
    let mut evidence = NativeHeadEvidence::default();
    evidence.record(&original, NativeHeadRole::Environment);
    let mut moved = Box::new(original.clone());
    moved.terms[0]
        .content
        .insert(0, Inline::anchor("new-navigation-id"));
    assert_eq!(evidence.role(&moved), Some(NativeHeadRole::Environment));
    moved.source.as_mut().unwrap().column += 1;
    assert_eq!(evidence.role(&moved), None);
    moved.source = original.source;
    moved.terms[0].content = vec![Inline::Emphasis {
        children: vec![Inline::Text {
            value: "PATH".into(),
        }],
    }];
    assert_eq!(evidence.role(&moved), None);
    moved.terms = original.terms.clone();
    moved.terms.push(
        vec![Inline::Text {
            value: "OTHER".into(),
        }]
        .into(),
    );
    assert_eq!(evidence.role(&moved), None);
    evidence.record(&original, NativeHeadRole::Literal);
    assert_eq!(evidence.role(&original), None);
}

#[test]
fn actual_owner_identity_separates_same_coordinate_macro_expansions() {
    let mut evidence = NativeHeadEvidence::default();
    for key in 1..=256 {
        let mut native = item();
        native.terms[0].content.insert(
            0,
            Inline::anchor(format!("\0mant-native-definition-owner:{key:x}")),
        );
        evidence.record(
            &native,
            if key % 2 == 0 {
                NativeHeadRole::Environment
            } else {
                NativeHeadRole::Literal
            },
        );
    }
    assert_eq!(evidence.native_witnesses.len(), 256);
    assert_eq!(evidence.witnesses.len(), 0);
    for key in 1..=256 {
        let mut moved = item();
        moved.terms[0].content.insert(
            0,
            Inline::anchor(format!("\0mant-native-definition-owner:{key:x}")),
        );
        assert_eq!(
            evidence.role(&moved),
            Some(if key % 2 == 0 {
                NativeHeadRole::Environment
            } else {
                NativeHeadRole::Literal
            })
        );
        moved.source.as_mut().unwrap().column += 1;
        assert_eq!(evidence.role(&moved), None);
    }
}

#[cfg(feature = "roff")]
fn recorded_owner() -> (DefinitionItem, NativeHeadEvidence) {
    let mut original = item();
    super::super::groups::mark_native_definition_owner(&mut original, 10);
    let mut evidence = NativeHeadEvidence::default();
    evidence.groups.record(&original, 10);
    evidence.record(&original, NativeHeadRole::Environment);
    evidence.capture_operands(
        20,
        CapturedHeadOperands {
            text: "PATH".into(),
            operands: vec![NativeOperand {
                bytes: 0..4,
                role: NativeOperandRole::Literal,
            }],
        },
    );
    evidence.record_operands(&original, 20);
    (original, evidence)
}

#[test]
#[cfg(feature = "roff")]
fn shared_owner_proofs_still_reject_source_style_and_identity_changes() {
    // Constructed proof mutations test admission, not a new roff gold.
    // Sharing must not make a group snapshot authorize another owner.
    let (original, evidence) = recorded_owner();
    assert_eq!(evidence.role(&original), Some(NativeHeadRole::Environment));
    assert!(evidence.operands(&original).is_some());

    let mut changed = original.clone();
    changed.source.as_mut().unwrap().end_column = Some(9);
    assert_eq!(evidence.role(&changed), None);
    assert!(evidence.operands(&changed).is_none());

    changed = original.clone();
    changed.terms[0].content[1] = Inline::Emphasis {
        children: vec![Inline::Text {
            value: "PATH".into(),
        }],
    };
    assert_eq!(evidence.role(&changed), None);
    assert!(evidence.operands(&changed).is_none());

    changed = original;
    changed.terms[0].content[0] = Inline::anchor("\0mant-native-definition-owner:1e");
    assert_eq!(evidence.role(&changed), None);
    assert!(evidence.operands(&changed).is_none());
}

#[test]
#[cfg(feature = "roff")]
fn a_continued_group_cannot_rewrite_an_earlier_operand_or_role_proof() {
    // TQ creates a new merged-head witness. Earlier accepted operand
    // ranges keep their original complete HEAD/source proof immutable.
    let (original, mut evidence) = recorded_owner();
    let mut continued = original.clone();
    continued.terms.push(
        vec![Inline::Text {
            value: "OTHER".into(),
        }]
        .into(),
    );
    evidence.groups.continued(&continued, 30);
    assert!(evidence.groups.shared_head(&continued).is_some());
    assert_eq!(evidence.role(&continued), None);
    assert!(evidence.operands(&continued).is_none());
    assert_eq!(evidence.role(&original), Some(NativeHeadRole::Environment));
    assert!(evidence.operands(&original).is_some());
}

#[test]
#[cfg(feature = "roff")]
fn shared_head_proof_preserves_link_destination_and_title_identity() {
    let mut original = item();
    original.terms[0].content = vec![Inline::Link {
        target: mant_ir::LinkTarget::External {
            uri: "https://example.invalid".into(),
        },
        title: Some("original".into()),
        children: original.terms[0].content.clone(),
    }];
    super::super::groups::mark_native_definition_owner(&mut original, 10);
    let mut evidence = NativeHeadEvidence::default();
    evidence.groups.record(&original, 10);
    evidence.record(&original, NativeHeadRole::Environment);
    let mut changed = original.clone();
    let Inline::Link { title, .. } = &mut changed.terms[0].content[1] else {
        unreachable!()
    };
    *title = Some("changed".into());
    assert_eq!(evidence.role(&changed), None);
    let Inline::Link { target, title, .. } = &mut changed.terms[0].content[1] else {
        unreachable!()
    };
    *title = Some("original".into());
    *target = mant_ir::LinkTarget::External {
        uri: "https://other.invalid".into(),
    };
    assert_eq!(evidence.role(&changed), None);
    assert_eq!(evidence.role(&original), Some(NativeHeadRole::Environment));
}

#[test]
#[cfg(feature = "roff")]
fn publication_rebinds_only_the_proved_owner_once() {
    use mant_ir::{Block, LinkTarget};
    let (mut original, mut evidence) = recorded_owner();
    let source = original.source;
    let link = Inline::Link {
        target: LinkTarget::External {
            uri: "private".into(),
        },
        title: None,
        children: vec![Inline::Text {
            value: " proof".into(),
        }],
    };
    original.terms[0].content.push(link);
    evidence.groups.record(&original, 10);
    evidence.record(&original, NativeHeadRole::Environment);
    evidence.capture_operands(
        20,
        CapturedHeadOperands {
            text: "PATH proof".into(),
            operands: vec![NativeOperand {
                bytes: 0..4,
                role: NativeOperandRole::Literal,
            }],
        },
    );
    evidence.record_operands(&original, 20);

    let mut invalid = original.clone();
    super::super::remove_native_definition_owner_markers_from_items(std::slice::from_mut(
        &mut invalid,
    ));
    super::super::groups::mark_native_definition_owner(&mut invalid, 30);
    evidence.groups.record(&invalid, 30);
    evidence.record(&invalid, NativeHeadRole::Environment);
    let Inline::Link { title, .. } = &mut invalid.terms[0].content[2] else {
        unreachable!()
    };
    *title = Some("changed before publication".into());

    let mut block = Block::DefinitionList {
        items: vec![original, invalid],
        declaration_groups: Vec::new(),
        compact: false,
        source,
        layout: mant_ir::LayoutHint::default(),
    };
    let mut calls = 0;
    evidence.publish_annotations(std::slice::from_mut(&mut block), &mut [], |nodes| {
        calls += 1;
        let Some(Inline::Link { target, title, .. }) = nodes.last_mut() else {
            return false;
        };
        *target = LinkTarget::External {
            uri: "https://example.test".into(),
        };
        *title = None;
        true
    });
    assert_eq!(calls, 2);
    let Block::DefinitionList { items, .. } = &block else {
        unreachable!()
    };
    assert_eq!(evidence.role(&items[0]), Some(NativeHeadRole::Environment));
    assert_eq!(evidence.operands(&items[0]).unwrap()[0][0].bytes, 0..4);
    assert!(evidence.groups.shared_head(&items[0]).is_some());
    assert_eq!(evidence.role(&items[1]), None);
    assert!(evidence.groups.shared_head(&items[1]).is_none());
}

#[test]
#[cfg(feature = "roff")]
fn publication_refuses_duplicated_owner_markers() {
    let (original, mut evidence) = recorded_owner();
    let mut block = mant_ir::Block::DefinitionList {
        items: vec![original.clone(), original],
        declaration_groups: Vec::new(),
        compact: false,
        source: None,
        layout: mant_ir::LayoutHint::default(),
    };
    evidence.publish_annotations(std::slice::from_mut(&mut block), &mut [], |_| false);
    let mant_ir::Block::DefinitionList { items, .. } = &block else {
        unreachable!()
    };
    for item in items {
        assert_eq!(evidence.role(item), None);
        assert!(evidence.operands(item).is_none());
        assert!(evidence.groups.shared_head(item).is_none());
    }
}
