//! Closed wire locations resolve only inside their declared returned domain.
use mant_ir::{Block, DefinitionItem, DefinitionLayout, Inline, LayoutHint};
use mant_protocol::{
    ExplanationBlockStep as Step, ExplanationContentRange as Range, ExplanationFormRange,
};

#[test]
#[allow(clippy::too_many_lines)]
fn typed_term_roots_validate_indices_ranges_and_canonical_unicode() {
    let mut store = mant_ir::ContentStoreBuilder::new();
    let owner = store.push_owner(
        mant_ir::ContentOwnerKind::DefinitionItem,
        mant_ir::Provenance::Unknown,
    );
    let root = store.push_root(
        owner,
        mant_ir::ContentRootKind::Term,
        mant_ir::Provenance::Unknown,
    );
    let term = store.push_text(
        root,
        "é\u{1b}名\n".into(),
        None,
        mant_ir::ContentStyle::default(),
        None,
        None,
        mant_ir::Provenance::Unknown,
    );
    let form = store.push_text(
        root,
        "é名".into(),
        None,
        mant_ir::ContentStyle::default(),
        None,
        None,
        mant_ir::Provenance::Unknown,
    );
    let projection = mant_ir::ContentProjection {
        content_store: store.finish(),
    };
    let body = Block::DefinitionList {
        declaration_groups: Vec::new(),
        items: vec![DefinitionItem {
            terms: vec![vec![Inline::Code { content: term }]],
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
    assert_eq!(
        valid
            .resolve(projection.content(), &body)
            .unwrap()
            .safe_text(projection.content())
            .as_deref(),
        Some("é�名\n")
    );
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
                .resolve(projection.content(), &body)
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
            .resolve(projection.content(), &body)
            .is_none()
        );
    }
    let mut unknown = encoded;
    unknown["byteStart"] = 0.into();
    assert!(serde_json::from_value::<Range>(unknown).is_err());
    let forms = vec![vec![Inline::Code { content: form }]];
    assert!(
        ExplanationFormRange {
            form_index: 0,
            start_char: 0,
            end_char: 2
        }
        .resolve(projection.content(), &forms)
        .is_some()
    );
    assert!(
        ExplanationFormRange {
            form_index: 1,
            start_char: 0,
            end_char: 2
        }
        .resolve(projection.content(), &forms)
        .is_none()
    );
    assert!(
        ExplanationFormRange {
            form_index: 0,
            start_char: 1,
            end_char: 3
        }
        .resolve(projection.content(), &forms)
        .is_none()
    );
}
