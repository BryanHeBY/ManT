use super::*;

fn head() -> Block {
    Block::Paragraph {
        inline_layout: mant_ir::InlineLayout::default(),
        children: vec![Inline::Code {
            value: "--one".into(),
        }],
        layout: LayoutHint {
            continuation_indent_columns: 7,
            ..LayoutHint::default()
        },
        source: Some(source(4)),
    }
}

fn body() -> DefinitionItem {
    DefinitionItem {
        head_body_relation: mant_ir::HeadBodyRelation::Separate,
        terms: Vec::new(),
        description: vec![Block::Paragraph {
            inline_layout: mant_ir::InlineLayout::default(),
            children: vec![Inline::Text {
                value: "Description".into(),
            }],
            layout: LayoutHint::default(),
            source: None,
        }],
        entry: None,
        layout: mant_ir::DefinitionLayout::default(),
        source: Some(source(5)),
    }
}

fn source(line: u32) -> SourceSpan {
    SourceSpan {
        line,
        column: 2,
        byte_range: None,
        end_line: None,
        end_column: None,
    }
}

#[test]
fn same_coordinates_cannot_substitute_a_different_executed_pair() {
    let mut proof = HangingOwnerEvidence::default();
    let mut left = head();
    let mut left_body = body();
    let mut right = head();
    let mut right_body = body();
    proof.head(
        &mut left,
        10,
        11,
        super::super::CapturedHeadOperands::default(),
    );
    proof.head(
        &mut right,
        20,
        21,
        super::super::CapturedHeadOperands::default(),
    );
    assert!(
        !proof.matches(&left, &left_body),
        "future IP has not executed"
    );
    proof.body(&mut left_body, 11);
    proof.body(&mut right_body, 21);
    assert!(proof.matches(&left, &left_body));
    assert!(proof.matches(&right, &right_body));
    assert!(!proof.matches(&left, &right_body));
    assert!(!proof.matches(&right, &left_body));
    if let Block::Paragraph { layout, .. } = &mut right {
        layout.continuation_indent_columns += 1;
    }
    assert!(
        !proof.matches(&right, &right_body),
        "complete native geometry witness"
    );
}

#[test]
fn consuming_one_pair_does_not_remove_other_native_owner_markers() {
    let mut proof = HangingOwnerEvidence::default();
    let mut head = head();
    let mut body = body();
    proof.head(
        &mut head,
        10,
        11,
        super::super::CapturedHeadOperands::default(),
    );
    proof.body(&mut body, 11);
    let carrier = body_carrier_mut(&mut body.description).unwrap();
    super::super::groups::mark_native_inline_owner(carrier, 99);
    // Another owner cannot take over the handoff's direct carrier.
    assert!(!proof.matches(&head, &body));
    carrier_remove(&mut body.description, 99);
    assert!(proof.matches(&head, &body));
    HangingOwnerEvidence::consume(&mut head, &mut body);
    assert!(
        !proof.matches(&head, &body),
        "one consumed pair cannot wrap recursively"
    );
}

fn literal_head(value: &str) -> Block {
    Block::Preformatted {
        children: vec![Inline::Code {
            value: value.into(),
        }],
        inline_layout: mant_ir::InlineLayout::default(),
        language: None,
        layout: LayoutHint {
            indent_columns: 2,
            continuation_indent_columns: 7,
            ..LayoutHint::default()
        },
        source: Some(source(4)),
    }
}

// Pure IR transformation: these tests prove evidence ownership, not a
// native formatter expectation. The actual reading policy has its own
// complete-owner tests in mandoc::literal_reading.
fn remove_literal_prefix(block: &mut Block) -> usize {
    let Block::Preformatted {
        children, layout, ..
    } = block
    else {
        return 0;
    };
    let Some(Inline::Code { value }) = children
        .iter_mut()
        .find(|inline| !matches!(inline, Inline::Anchor { .. }))
    else {
        return 0;
    };
    if !value.starts_with('\n') {
        return 0;
    }
    value.remove(0);
    layout.indent_columns += layout.continuation_indent_columns;
    layout.continuation_indent_columns = 0;
    1
}

#[test]
fn matching_literal_transform_rebases_operands_without_changing_body_identity() {
    use super::super::{CapturedHeadOperands, NativeOperand, NativeOperandRole};

    let mut proof = HangingOwnerEvidence::default();
    let mut head = literal_head("\n--one α");
    let mut body = body();
    proof.head(
        &mut head,
        10,
        11,
        CapturedHeadOperands {
            text: "\n--one α".into(),
            operands: vec![
                NativeOperand {
                    bytes: 0..1,
                    role: NativeOperandRole::Literal,
                },
                NativeOperand {
                    bytes: 1..6,
                    role: NativeOperandRole::ExplicitOption,
                },
                NativeOperand {
                    bytes: 7..9,
                    role: NativeOperandRole::Argument,
                },
            ],
        },
    );
    proof.body(&mut body, 11);
    assert!(proof.matches(&head, &body));
    let original_body = body.clone();
    proof.transform_literal_head(&mut head, remove_literal_prefix);
    assert!(proof.matches(&head, &body));
    assert_eq!(
        proof.operands(&head),
        [
            NativeOperand {
                bytes: 0..5,
                role: NativeOperandRole::ExplicitOption,
            },
            NativeOperand {
                bytes: 6..8,
                role: NativeOperandRole::Argument,
            },
        ]
    );
    assert_eq!(body, original_body, "BODY and its parse-local key stay put");
    body.source = Some(source(6));
    assert!(
        !proof.matches(&head, &body),
        "BODY provenance still matters"
    );
    body = original_body;
    super::super::groups::mark_native_inline_owner(
        body_carrier_mut(&mut body.description).unwrap(),
        99,
    );
    assert!(!proof.matches(&head, &body), "BODY identity still matters");
}

#[test]
fn normalized_mismatch_cannot_reacquire_an_older_execution_proof() {
    let mut proof = HangingOwnerEvidence::default();
    let mut head = literal_head("--one");
    if let Block::Preformatted { layout, .. } = &mut head {
        layout.continuation_indent_columns = 0;
    }
    let mut body = body();
    proof.head(
        &mut head,
        10,
        11,
        super::super::CapturedHeadOperands::default(),
    );
    proof.body(&mut body, 11);
    assert!(proof.matches(&head, &body));
    if let Block::Preformatted { children, .. } = &mut head {
        let Some(Inline::Code { value }) = children
            .iter_mut()
            .find(|inline| !matches!(inline, Inline::Anchor { .. }))
        else {
            unreachable!()
        };
        value.insert(0, '\n');
    }
    assert!(!proof.matches(&head, &body));
    proof.transform_literal_head(&mut head, remove_literal_prefix);
    assert!(matches!(
        &head,
        Block::Preformatted { children, .. }
            if mant_ir::inline_plain_text(children) == "--one"
    ));
    assert!(
        !proof.matches(&head, &body),
        "a reading transform cannot repair an invalid original witness"
    );
}

fn carrier_remove(blocks: &mut [Block], key: usize) {
    body_carrier_mut(blocks)
        .unwrap()
        .retain(|inline| native_inline_owner(std::slice::from_ref(inline)) != Some(key));
}
