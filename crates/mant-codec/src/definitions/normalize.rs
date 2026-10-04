//! Definition normalize policy; coordinated by the parent discovery passes.
use super::{context::DefinitionContext, syntax::is_inferred_head};
use mant_ir::geometry::{block_layout, block_layout_mut};
use mant_ir::{Block, DefinitionItem, HeadBodyRelation, LayoutHint};
use std::{collections::VecDeque, mem};

/// Recover only executed HP → headless IP pairs. Their two original blocks
/// remain authoritative: a marker-free plain item supplies semantic ownership
/// without losing the HP's hanging continuation or moving the IP's body origin.
pub(super) fn normalize_native_hanging_owners(
    blocks: &mut Vec<Block>,
    context: DefinitionContext,
    evidence: &super::hanging_owner::HangingOwnerEvidence,
) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());
    while let Some(mut head) = pending.pop_front() {
        let entry = pending.front().and_then(|next| {
            let Block::DefinitionList { items, .. } = next else {
                return None;
            };
            let body = items.first()?;
            evidence.matches(&head, body).then_some(())?;
            let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = &head
            else {
                return None;
            };
            super::syntax::recognize_inferred_head_with_operands(
                children,
                context,
                evidence.operands(&head),
            )
        });
        let Some(entry) = entry else {
            normalized.push(head);
            continue;
        };
        let Block::DefinitionList {
            declaration_groups,
            mut items,
            compact,
            layout,
            source,
        } = pending.pop_front().expect("matched following block")
        else {
            unreachable!("the witness only accepts a definition body");
        };
        let mut body = items.remove(0);
        super::hanging_owner::HangingOwnerEvidence::consume(&mut head, &mut body);
        let facts = hanging_owner_facts(&head, entry);
        let owner_source = mant_ir::geometry::block_source(&head);
        normalized.push(Block::List {
            kind: mant_ir::ListKind::Plain,
            compact: true,
            layout: LayoutHint::default(),
            source: owner_source,
            items: vec![mant_ir::ListItem {
                layout: mant_ir::ListItemLayout {
                    spacing_before_lines: Some(0),
                },
                source: owner_source,
                entry: Some(facts),
                blocks: vec![
                    head,
                    Block::DefinitionList {
                        declaration_groups: Vec::new(),
                        items: vec![body],
                        compact,
                        layout,
                        source,
                    },
                ],
            }],
        });
        if !items.is_empty() {
            // The original list's leading gap belongs to its first IP. Later
            // items retain their own explicit gaps; the split adds none.
            normalized.push(Block::DefinitionList {
                declaration_groups,
                items,
                compact,
                layout: LayoutHint {
                    spacing_before_lines: 0,
                    ..layout
                },
                source,
            });
        }
    }
    *blocks = normalized;
}

fn hanging_owner_facts(
    head: &Block,
    identity: super::syntax::InferredIdentity,
) -> mant_ir::EntryFacts {
    let (Block::Paragraph { children, .. } | Block::Preformatted { children, .. }) = head else {
        unreachable!("a recovered hanging head is an inline block");
    };
    let mut bindings = super::binding::native_name_bindings_for_head(
        children,
        &identity.names,
        &identity.occurrences,
    );
    for part in bindings
        .iter_mut()
        .flat_map(|binding| &mut binding.occurrences)
        .flat_map(|form| &mut form.parts)
    {
        part.root = mant_ir::EntryInlineRoot::Block { index: 0 };
    }
    mant_ir::EntryFacts {
        // Allocation uses the same collision policy as existing list owners.
        id: "pending-native-owner".into(),
        kind: identity.kind,
        case: identity.case,
        names: identity.names,
        forms: vec![mant_ir::EntryForm {
            parts: vec![mant_ir::EntryContentSlice {
                root: mant_ir::EntryInlineRoot::Block { index: 0 },
                path: Vec::new(),
                bytes: None,
            }],
        }],
        name_bindings: bindings,
        alias_groups: Vec::new(),
        alias_of: None,
        value_domain: None,
    }
}

/// Reattach source-neutral indented continuations to their owning definition.
///
/// libmandoc can retain man(7) `.RS` continuations as later sibling blocks
/// whose absolute indentation is greater than the preceding definition list.
/// They remain visually correct in that flat form, but the topology loses the
/// command → parameter → value relationship needed by semantic navigation.
/// Move the run under the last definition and translate its layout to the
/// description's relative coordinate system so rendering is unchanged.
#[cfg(any(feature = "roff", test))]
pub(crate) fn normalize_definition_nesting(blocks: &mut Vec<Block>) {
    normalize_definition_nesting_with_boundaries(blocks, &std::collections::HashSet::new());
}

pub(super) fn normalize_definition_nesting_with_boundaries(
    blocks: &mut Vec<Block>,
    boundaries: &std::collections::HashSet<(u32, u32)>,
) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(mut block) = pending.pop_front() {
        let Some(base_indent) = block_definition_indent(&block) else {
            normalized.push(block);
            continue;
        };
        let Some(last_item) = last_definition_mut(&mut block) else {
            normalized.push(block);
            continue;
        };
        let description_origin = base_indent.saturating_add(last_item.layout.body_indent_columns);
        while let Some(length) = indented_continuation_len(&pending, base_indent) {
            if pending.iter().take(length).any(|block| {
                mant_ir::geometry::block_source(block)
                    .is_some_and(|s| boundaries.contains(&(s.line, s.column)))
            }) {
                break;
            }
            for mut nested in pending.drain(..length) {
                shift_block_indent(&mut nested, description_origin);
                last_item.description.push(nested);
            }
        }
        normalized.push(block);
    }

    *blocks = normalized;
}

/// Whitespace has no indentation and cannot independently establish or end
/// ownership. Carry a run of it only when the next substantive block proves
/// an indented continuation. Do not cross other layout-less blocks, equal or
/// shallower content, or the end of this container. Consume a whole run at once
/// so even long sequences of explicit spacing are examined only once.
fn indented_continuation_len(pending: &VecDeque<Block>, base_indent: i32) -> Option<usize> {
    let (index, block) = pending
        .iter()
        .enumerate()
        .find(|(_, block)| !matches!(block, Block::VerticalSpace { .. }))?;
    block_layout(block)
        .is_some_and(|layout| layout.indent_columns > base_indent)
        .then_some(index + 1)
}

fn block_definition_indent(block: &Block) -> Option<i32> {
    match block {
        Block::DefinitionList { layout, .. } => Some(layout.indent_columns),
        _ => None,
    }
}

fn last_definition_mut(block: &mut Block) -> Option<&mut DefinitionItem> {
    match block {
        Block::DefinitionList { items, .. } => items.last_mut(),
        _ => None,
    }
}

/// Turn renderer-neutral hanging-indent runs into semantic definitions.
///
/// Some man(7) generators use `.PP` followed by `.RS` instead of `.TP` for
/// options and environment variables. Native parsers correctly retain that
/// layout, but neither representation is a definition list on its own.
/// Recognising the shared visible shape here keeps identity independent of
/// the source macro set or source parser used by the query pipeline.
pub(super) fn normalize_hanging_definitions(blocks: &mut Vec<Block>, context: DefinitionContext) {
    let mut pending: VecDeque<Block> = mem::take(blocks).into();
    let mut normalized = Vec::with_capacity(pending.len());

    while let Some(block) = pending.pop_front() {
        let Some((term_indent, first_length)) = hanging_definition_start(&block, &pending, context)
        else {
            normalized.push(block);
            continue;
        };

        let mut description = pending.drain(..first_length).collect::<Vec<_>>();
        while let Some(length) = hanging_description_len(&pending, term_indent) {
            description.extend(pending.drain(..length));
        }

        let Block::Paragraph {
            children,
            inline_layout,
            layout,
            source,
        } = block
        else {
            unreachable!("hanging_definition_start only accepts paragraphs");
        };
        let description_origin = description
            .iter()
            .find_map(block_layout)
            .map_or(term_indent, |layout| layout.indent_columns);
        for child in &mut description {
            shift_block_indent(child, description_origin);
        }
        let terms = vec![mant_ir::DefinitionTerm {
            content: children,
            inline_layout,
        }];
        normalized.push(Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                source,
                entry: None,
                layout: mant_ir::DefinitionLayout {
                    // This is an ownership change, not a request to join two
                    // originally distinct source paragraphs into one line.
                    head_body_relation: HeadBodyRelation::Separate,
                    body_indent_columns: mant_ir::geometry::rebase_origin(
                        description_origin,
                        0,
                        term_indent,
                    ),
                    spacing_before_lines: Some(layout.spacing_before_lines),
                    ..Default::default()
                },
                terms,
                description,
            }],
            compact: true,
            layout: LayoutHint {
                indent_columns: term_indent,
                spacing_before_lines: 0,
                ..Default::default()
            },
            source,
        });
    }

    *blocks = normalized;
}

fn hanging_definition_start(
    block: &Block,
    pending: &VecDeque<Block>,
    context: DefinitionContext,
) -> Option<(i32, usize)> {
    let Block::Paragraph {
        children, layout, ..
    } = block
    else {
        return None;
    };
    // A semantic head can be transferred only when there is a deeper layout
    // successor. Check this necessary condition before running the complete
    // head grammar; ordinary prose with no description needs no syntax scan.
    let length = hanging_description_len(pending, layout.indent_columns)?;
    #[cfg(test)]
    eligibility_tests::record_recognition();
    is_inferred_head(children, context).then_some((layout.indent_columns, length))
}

fn hanging_description_len(pending: &VecDeque<Block>, base_indent: i32) -> Option<usize> {
    let (index, block) = pending
        .iter()
        .enumerate()
        .find(|(_, block)| !matches!(block, Block::VerticalSpace { .. }))?;
    // Tables are an explicit ownership boundary even when indented. A
    // same-level paragraph also fails the geometry check, independently of
    // whether its text could be recognized as another declaration head.
    (!matches!(block, Block::Table { .. })
        && block_layout(block).is_some_and(|layout| layout.indent_columns > base_indent))
    .then_some(index + 1)
}

fn shift_block_indent(block: &mut Block, origin: i32) {
    if let Some(layout) = block_layout_mut(block) {
        layout.indent_columns = mant_ir::geometry::rebase_origin(layout.indent_columns, 0, origin);
    }
}

#[cfg(test)]
#[path = "normalize/eligibility_tests.rs"]
mod eligibility_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use mant_ir::Inline;

    fn paragraph(text: &str, indent_columns: i32) -> Block {
        Block::Paragraph {
            inline_layout: mant_ir::InlineLayout::default(),
            children: vec![Inline::Text { value: text.into() }],
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines: 0,
                ..Default::default()
            },
            source: None,
        }
    }

    fn definition(indent_columns: i32) -> Block {
        Block::DefinitionList {
            declaration_groups: Vec::new(),
            items: vec![DefinitionItem {
                source: None,
                entry: None,
                terms: (vec![vec![Inline::Text {
                    value: "--owner".into(),
                }]])
                .into_iter()
                .map(Into::into)
                .collect(),
                description: vec![paragraph("Initial description.", 4)],
                layout: mant_ir::DefinitionLayout {
                    spacing_before_lines: None,
                    ..Default::default()
                },
            }],
            compact: false,
            layout: LayoutHint {
                indent_columns,
                spacing_before_lines: 0,
                ..Default::default()
            },
            source: None,
        }
    }

    fn space(lines: u16) -> Block {
        Block::VerticalSpace {
            lines,
            source: None,
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum GeometryAtom {
        Content {
            children: Vec<Inline>,
            absolute_indent: i32,
            spacing_before_lines: u16,
        },
        Space(u16),
    }

    // Inspect only stored IR fields. This does not run normalization, reuse its
    // rebase helper, or model rendered rows/run-in placement. Public rendering
    // is checked separately in definition_normalization_layout integration tests.
    fn absolute_geometry(blocks: &[Block]) -> Vec<GeometryAtom> {
        fn collect(blocks: &[Block], origin: i32, output: &mut Vec<GeometryAtom>) {
            for block in blocks {
                match block {
                    Block::Paragraph {
                        children, layout, ..
                    } => output.push(GeometryAtom::Content {
                        children: children.clone(),
                        absolute_indent: origin + layout.indent_columns,
                        spacing_before_lines: layout.spacing_before_lines,
                    }),
                    Block::VerticalSpace { lines, .. } => output.push(GeometryAtom::Space(*lines)),
                    Block::DefinitionList { items, layout, .. } => {
                        let term_origin = origin + layout.indent_columns;
                        for item in items {
                            for term in &item.terms {
                                output.push(GeometryAtom::Content {
                                    children: term.content.clone(),
                                    absolute_indent: term_origin,
                                    spacing_before_lines: item
                                        .layout
                                        .spacing_before_lines
                                        .unwrap_or(layout.spacing_before_lines),
                                });
                            }
                            collect(
                                &item.description,
                                term_origin + item.layout.body_indent_columns,
                                output,
                            );
                        }
                    }
                    _ => panic!("unexpected fixture block: {block:?}"),
                }
            }
        }
        let mut result = Vec::new();
        collect(blocks, 0, &mut result);
        result
    }

    #[test]
    fn moving_spaced_continuations_preserves_text_geometry_and_blank_lines() {
        for base in [0, 7] {
            for inline_term in [false, true] {
                for label in ["-a", "--long-option", "界", "e\u{301}"] {
                    let mut owner = definition(base);
                    let Block::DefinitionList { items, .. } = &mut owner else {
                        unreachable!()
                    };
                    items[0].layout.head_body_relation = if inline_term {
                        mant_ir::HeadBodyRelation::from(true)
                    } else {
                        mant_ir::HeadBodyRelation::Separate
                    };
                    items[0].terms[0].content = vec![Inline::Text {
                        value: label.into(),
                    }];
                    let mut blocks = vec![
                        owner,
                        space(1),
                        space(3),
                        paragraph("First continuation.", base + 4),
                        space(2),
                        definition(base + 8),
                        space(1),
                        paragraph("Last continuation.", base + 4),
                        space(2),
                        paragraph("Outside.", base),
                    ];
                    let before = absolute_geometry(&blocks);
                    let outside = blocks[8..].to_vec();
                    normalize_definition_nesting(&mut blocks);
                    assert_eq!(absolute_geometry(&blocks), before, "base indent {base}");
                    assert_eq!(&blocks[1..], outside);
                    let Block::DefinitionList { items, .. } = &blocks[0] else {
                        unreachable!()
                    };
                    assert_eq!(items[0].description.len(), 8);
                    assert_eq!(items[0].layout.inline_term(), inline_term);
                    assert_eq!(
                        items[0].terms,
                        [mant_ir::DefinitionTerm::from(vec![Inline::Text {
                            value: label.into()
                        }])]
                    );
                    let once = blocks.clone();
                    normalize_definition_nesting(&mut blocks);
                    assert_eq!(blocks, once);
                }
            }
        }
    }

    #[test]
    fn whitespace_does_not_cross_an_outer_or_nonlayout_boundary() {
        for boundary in [
            vec![],
            vec![paragraph("Same level.", 4)],
            vec![paragraph("Outer level.", 0)],
            vec![
                Block::ThematicBreak { source: None },
                paragraph("Deep but unrelated.", 8),
            ],
        ] {
            let mut blocks = vec![definition(4), space(1), space(2)];
            blocks.extend(boundary);
            let before = blocks.clone();
            normalize_definition_nesting(&mut blocks);
            assert_eq!(blocks, before);
        }
    }

    #[test]
    fn hanging_definitions_share_the_boundary_without_stealing_trailing_spacing() {
        let mut blocks = vec![
            paragraph("--owner", 0),
            paragraph("Description.", 4),
            space(2),
            paragraph("Continuation.", 4),
            space(3),
            paragraph("--next", 0),
            paragraph("Next description.", 4),
            space(5),
        ];
        normalize_hanging_definitions(&mut blocks, DefinitionContext::Generic);
        assert_eq!(blocks.len(), 4);
        assert_eq!(blocks[1], space(3));
        assert_eq!(blocks[3], space(5));
        let Block::DefinitionList { items, .. } = &blocks[0] else {
            unreachable!()
        };
        assert_eq!(items[0].description.len(), 3);
        assert_eq!(items[0].description[1], space(2));
    }

    #[test]
    fn explicit_head_spacing_preserves_original_paragraph_geometry() {
        for spacing in [0, 1, 3] {
            let mut blocks = vec![paragraph("--option", 0), space(spacing)];
            blocks.push(paragraph("Description.", 4));
            let before = absolute_geometry(&blocks);
            normalize_hanging_definitions(&mut blocks, DefinitionContext::Parameters);
            assert_eq!(absolute_geometry(&blocks), before, "spacing={spacing}");
            let Block::DefinitionList { items, .. } = &blocks[0] else {
                panic!("inferred definition")
            };
            assert!(!items[0].layout.inline_term());
            assert_eq!(items[0].layout.spacing_before_lines, Some(0));
            assert_eq!(items[0].description[0], space(spacing));
        }
    }

    #[test]
    fn inferred_ownership_preserves_distinct_unspaced_paragraphs() {
        for origin in [0, 2, 5, -2] {
            for offset in [2, 4, 12] {
                for head in ["--x", "--long-option-name"] {
                    let mut blocks = vec![
                        paragraph(head, origin),
                        paragraph("Description.", origin + offset),
                    ];
                    let before = absolute_geometry(&blocks);
                    normalize_hanging_definitions(&mut blocks, DefinitionContext::Parameters);
                    let Block::DefinitionList { items, .. } = &blocks[0] else {
                        panic!("inferred definition")
                    };
                    assert!(!items[0].layout.inline_term());
                    assert_eq!(items[0].layout.body_indent_columns, offset);
                    assert_eq!(
                        absolute_geometry(&blocks),
                        before,
                        "origin={origin} offset={offset}"
                    );
                }
            }
        }
    }
}
