//! Closed wire locations resolve only inside their declared returned domain.
use mant_ir::{Block, DefinitionItem, DefinitionLayout, Inline, LayoutHint};
use mant_protocol::{
    ExplanationBlockStep as Step, ExplanationContentRange as Range, ExplanationFormRange,
};

#[test]
fn typed_term_roots_validate_indices_ranges_and_canonical_unicode() {
    let body = Block::DefinitionList {
        items: vec![DefinitionItem {
            terms: vec![vec![Inline::Code {
                value: "é\u{1b}名\n".into(),
            }]],
            description: vec![],
            source: None,
            layout: DefinitionLayout::default(),
            entry: None,
        }],
        compact: false,
        layout: LayoutHint::default(),
        source: None,
    };
    let valid = Range::DefinitionTerm {
        path: vec![],
        item_index: 0,
        term_index: 0,
        start_char: 0,
        end_char: 4,
    };
    assert_eq!(valid.resolve(&body).unwrap().safe_text(), "é�名\n");
    let encoded = serde_json::to_value(&valid).unwrap();
    assert_eq!(
        serde_json::from_value::<Range>(encoded.clone()).unwrap(),
        valid
    );
    for (field, value) in [
        ("itemIndex", 1),
        ("termIndex", 1),
        ("endChar", 5),
        ("endChar", 0),
        ("startChar", 4),
    ] {
        let mut bad = encoded.clone();
        bad[field] = value.into();
        assert!(
            serde_json::from_value::<Range>(bad)
                .unwrap()
                .resolve(&body)
                .is_none()
        );
    }
    for path in [
        vec![],
        vec![Step::Block { index: 0 }],
        vec![Step::ListItem { index: 0 }, Step::Block { index: 0 }],
    ] {
        assert!(
            Range::BlockText {
                path,
                start_char: 0,
                end_char: 1
            }
            .resolve(&body)
            .is_none()
        );
    }
    let mut unknown = encoded;
    unknown["byteStart"] = 0.into();
    assert!(serde_json::from_value::<Range>(unknown).is_err());
    let forms = vec![vec![Inline::Code {
        value: "é名".into(),
    }]];
    assert!(
        ExplanationFormRange {
            form_index: 0,
            start_char: 0,
            end_char: 2
        }
        .resolve(&forms)
        .is_some()
    );
    assert!(
        ExplanationFormRange {
            form_index: 1,
            start_char: 0,
            end_char: 2
        }
        .resolve(&forms)
        .is_none()
    );
    assert!(
        ExplanationFormRange {
            form_index: 0,
            start_char: 1,
            end_char: 3
        }
        .resolve(&forms)
        .is_none()
    );
}
