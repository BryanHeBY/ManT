use super::{GroupEvidence, mark_native_definition_owner};
use mant_ir::{Block, DefinitionItem, DefinitionLayout, Inline, LayoutHint, SourceSpan};

fn item(line: u32, column: u32, name: &str, description: bool) -> DefinitionItem {
    DefinitionItem {
        head_body_relation: mant_ir::HeadBodyRelation::Separate,
        source: Some(SourceSpan {
            line,
            column,
            byte_range: None,
            end_line: None,
            end_column: None,
        }),
        entry: None,
        terms: (vec![vec![Inline::Text {
            value: name.to_owned(),
        }]])
        .into_iter()
        .map(Into::into)
        .collect(),
        description: if description {
            vec![Block::Paragraph {
                inline_layout: mant_ir::InlineLayout::default(),
                children: vec![Inline::Text {
                    value: "description".to_owned(),
                }],
                source: None,
                layout: LayoutHint::default(),
            }]
        } else {
            Vec::new()
        },
        layout: DefinitionLayout::default(),
    }
}

fn plan(evidence: &GroupEvidence, items: &[DefinitionItem]) -> super::GroupMatchingPlan {
    let blocks = vec![Block::DefinitionList {
        items: items.to_vec(),
        declaration_groups: Vec::new(),
        compact: false,
        source: None,
        layout: LayoutHint::default(),
    }];
    evidence.matching_plan(&blocks, &[])
}

fn record(evidence: &mut GroupEvidence, items: &mut [DefinitionItem], keys: &[usize]) {
    for (item, key) in items.iter_mut().zip(keys) {
        mark_native_definition_owner(item, *key);
        evidence.record(item, *key);
    }
}

#[test]
fn presentation_heads_split_but_do_not_poison_a_named_suffix_group() {
    let mut evidence = GroupEvidence::default();
    let mut items = vec![
        item(1, 1, "/RE", false),
        item(2, 1, "?RE", false),
        item(3, 1, "n", true),
    ];
    record(&mut evidence, &mut items, &[10, 20, 30]);
    evidence.presentation_head(10);
    evidence.adjacent(10, 20, false);
    evidence.adjacent(20, 30, false);

    let mut plan = plan(&evidence, &items);
    assert_eq!(
        evidence.resolve(&items, &[false, true, true], &mut plan),
        vec![mant_ir::DeclarationGroup {
            start_item: 1,
            end_item: 3,
        }]
    );
}

#[test]
fn macro_expansion_keeps_same_coordinate_runs_distinct() {
    let mut evidence = GroupEvidence::default();
    let mut items = vec![
        item(12, 2, "-a", false),
        item(12, 2, "-b", true),
        item(12, 2, "-a", false),
        item(12, 2, "-b", true),
    ];
    record(&mut evidence, &mut items, &[10, 20, 30, 40]);
    evidence.adjacent(10, 20, false);
    evidence.adjacent(30, 40, false);

    let mut plan = plan(&evidence, &items);
    assert_eq!(
        evidence.resolve(&items, &[true; 4], &mut plan),
        vec![
            mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            },
            mant_ir::DeclarationGroup {
                start_item: 2,
                end_item: 4,
            },
        ]
    );
}

#[test]
fn a_retained_same_coordinate_owner_keeps_its_own_macro_expansion() {
    let mut evidence = GroupEvidence::default();
    let mut native_items = [
        item(12, 2, "-a", false),
        item(12, 2, "-b", true),
        item(12, 2, "-a", false),
        item(12, 2, "-b", true),
    ];
    record(&mut evidence, &mut native_items, &[10, 20, 30, 40]);
    evidence.adjacent(10, 20, false);
    evidence.adjacent(30, 40, false);

    // A final rewrite retained only the latter source occurrence. The
    // coordinate/head pair is indistinguishable, but its carried owner
    // marker still selects the latter native run rather than borrowing
    // the first one.
    // Keep only the latter invocation's own markers. Same source
    // coordinates and heads must never make it borrow the first run.
    let mut final_items = vec![item(12, 2, "-a", false), item(12, 2, "-b", true)];
    mark_native_definition_owner(&mut final_items[0], 30);
    mark_native_definition_owner(&mut final_items[1], 40);
    let mut plan = plan(&evidence, &final_items);
    assert_eq!(
        evidence.resolve(&final_items, &[true; 2], &mut plan),
        vec![mant_ir::DeclarationGroup {
            start_item: 0,
            end_item: 2,
        }]
    );
}

#[test]
fn duplicated_owner_marker_refuses_to_allocate_a_group() {
    let mut evidence = GroupEvidence::default();
    let mut items = vec![item(12, 2, "--alpha", false), item(12, 2, "--beta", true)];
    record(&mut evidence, &mut items, &[10, 20]);
    evidence.adjacent(10, 20, false);

    let mut duplicate = items.clone();
    // A normalization bug that cloned the same owner must remain safe:
    // no duplicated marker may claim one native declaration witness.
    let mut blocks = vec![Block::DefinitionList {
        items: duplicate.clone(),
        declaration_groups: Vec::new(),
        compact: false,
        source: None,
        layout: LayoutHint::default(),
    }];
    blocks.push(Block::DefinitionList {
        items: std::mem::take(&mut duplicate),
        declaration_groups: Vec::new(),
        compact: false,
        source: None,
        layout: LayoutHint::default(),
    });
    let mut plan = evidence.matching_plan(&blocks, &[]);
    let Block::DefinitionList { items, .. } = &blocks[0] else {
        unreachable!();
    };
    assert_eq!(evidence.resolve(items, &[true; 2], &mut plan).len(), 0);
}

#[test]
fn a_body_closed_native_owner_allows_the_next_physical_run() {
    let mut evidence = GroupEvidence::default();
    let mut items = vec![
        item(1, 1, "--first", false),
        item(2, 1, "--second", true),
        item(4, 1, "--third", false),
        item(5, 1, "--fourth", true),
    ];
    record(&mut evidence, &mut items, &[10, 20, 40, 50]);
    evidence.adjacent(10, 20, false);
    // Key 30 is a native ordinal/bullet owner converted to Block::List.
    // Its readable body closes that run before --third starts.
    evidence.adjacent(30, 40, true);
    evidence.adjacent(40, 50, false);

    let mut plan = plan(&evidence, &items);
    assert_eq!(
        evidence.resolve(&items, &[true; 4], &mut plan),
        vec![
            mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            },
            mant_ir::DeclarationGroup {
                start_item: 2,
                end_item: 4,
            },
        ]
    );
}

#[test]
fn an_unclassified_head_with_its_own_body_closes_the_previous_run() {
    let mut evidence = GroupEvidence::default();
    let mut items = vec![
        item(1, 1, "This is explanatory prose.", true),
        item(2, 1, "--alpha", false),
        item(3, 1, "--beta", true),
    ];
    record(&mut evidence, &mut items, &[10, 20, 30]);
    evidence.adjacent(10, 20, true);
    evidence.adjacent(20, 30, false);

    let mut plan = plan(&evidence, &items);
    assert_eq!(
        evidence.resolve(&items, &[false, true, true], &mut plan),
        vec![mant_ir::DeclarationGroup {
            start_item: 1,
            end_item: 3,
        }]
    );
}

#[test]
fn an_empty_unclassified_head_still_blocks_a_following_suffix() {
    let mut evidence = GroupEvidence::default();
    let mut items = vec![
        item(1, 1, "unclassified", false),
        item(2, 1, "--alpha", false),
        item(3, 1, "--beta", true),
    ];
    record(&mut evidence, &mut items, &[10, 20, 30]);
    evidence.adjacent(10, 20, false);
    evidence.adjacent(20, 30, false);

    let mut plan = plan(&evidence, &items);
    assert_eq!(
        evidence
            .resolve(&items, &[false, true, true], &mut plan)
            .len(),
        0
    );
}

#[test]
fn macro_expansion_can_span_definition_lists_without_losing_owner_identity() {
    let mut evidence = GroupEvidence::default();
    let mut first = vec![item(12, 2, "--alpha", false), item(12, 2, "--beta", true)];
    let mut second = first.clone();
    record(&mut evidence, &mut first, &[10, 20]);
    record(&mut evidence, &mut second, &[30, 40]);
    evidence.adjacent(10, 20, false);
    evidence.adjacent(30, 40, false);
    let blocks = vec![
        Block::DefinitionList {
            items: first.clone(),
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: LayoutHint::default(),
        },
        Block::Paragraph {
            inline_layout: mant_ir::InlineLayout::default(),
            children: vec![Inline::Text {
                value: "ordinary separator".to_owned(),
            }],
            source: None,
            layout: LayoutHint::default(),
        },
        Block::DefinitionList {
            items: second.clone(),
            declaration_groups: Vec::new(),
            compact: false,
            source: None,
            layout: LayoutHint::default(),
        },
    ];
    let mut plan = evidence.matching_plan(&blocks, &[]);
    let expected = vec![mant_ir::DeclarationGroup {
        start_item: 0,
        end_item: 2,
    }];
    assert_eq!(evidence.resolve(&first, &[true, true], &mut plan), expected);
    assert_eq!(
        evidence.resolve(&second, &[true, true], &mut plan),
        vec![mant_ir::DeclarationGroup {
            start_item: 0,
            end_item: 2,
        }]
    );
}

#[test]
fn nested_preparation_cannot_swap_same_source_macro_owners() {
    let mut evidence = GroupEvidence::default();
    let mut outer = vec![item(12, 2, "--alpha", false), item(13, 2, "--beta", false)];
    let mut inner = vec![item(12, 2, "--alpha", false), item(13, 2, "--beta", true)];
    record(&mut evidence, &mut outer, &[10, 20]);
    record(&mut evidence, &mut inner, &[30, 40]);
    // The outer expansion has an executed paragraph boundary between its
    // heads. Only the nested expansion owns the shared description.
    evidence.adjacent(30, 40, false);

    outer[1].description.push(Block::DefinitionList {
        items: inner.clone(),
        declaration_groups: Vec::new(),
        compact: false,
        source: None,
        layout: LayoutHint::default(),
    });
    let blocks = vec![Block::DefinitionList {
        items: outer.clone(),
        declaration_groups: Vec::new(),
        compact: false,
        source: None,
        layout: LayoutHint::default(),
    }];
    let mut plan = evidence.matching_plan(&blocks, &[]);

    // Preparation descends into a definition body before resolving its
    // containing list. A FIFO allocation would give this inner pair the
    // outer witnesses because both expansions have identical coordinates
    // and heads. Parse-local markers make traversal order irrelevant.
    assert_eq!(
        evidence.resolve(&inner, &[true, true], &mut plan),
        vec![mant_ir::DeclarationGroup {
            start_item: 0,
            end_item: 2,
        }]
    );
    assert_eq!(evidence.resolve(&outer, &[true, true], &mut plan).len(), 0);
}

#[test]
fn continued_heads_keep_each_complete_head_and_final_sibling_proof() {
    // Constructed witness versions exercise TQ ownership; no new native
    // text expectation is inferred from this source-neutral control.
    let mut evidence = GroupEvidence::default();
    let mut original = vec![item(12, 2, "--alpha", false), item(13, 2, "--next", true)];
    record(&mut evidence, &mut original, &[10, 40]);
    let mut continued = original.clone();
    continued[0].terms.push(
        vec![Inline::Text {
            value: "--alias".into(),
        }]
        .into(),
    );
    evidence.continued(&continued[0], 30);
    evidence.adjacent(30, 40, false);
    let expected = vec![mant_ir::DeclarationGroup {
        start_item: 0,
        end_item: 2,
    }];
    let mut latest = plan(&evidence, &continued);
    assert_eq!(
        evidence.resolve(&continued, &[true, true], &mut latest),
        expected
    );

    // The original version did not end at node 30. A latest-only lookup
    // would assign it a sibling edge that the native walk never recorded.
    let mut initial = plan(&evidence, &original);
    assert_eq!(
        evidence
            .resolve(&original, &[true, true], &mut initial)
            .len(),
        0
    );
    continued[0].source.as_mut().unwrap().column += 1;
    let mut changed = plan(&evidence, &continued);
    assert_eq!(
        evidence
            .resolve(&continued, &[true, true], &mut changed)
            .len(),
        0
    );
}
