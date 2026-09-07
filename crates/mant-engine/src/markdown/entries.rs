//! Annotates semantic entries without changing ordinary Markdown content.

mod bindings;
mod diagnostics;
mod names;
mod signature;
/// Prove that one list-wide declaration reconstructs the final visible bindings.
/// This is producer grammar reuse, not a hidden spelling or parser-state export.
pub(crate) fn export_attached_policy(items: &[ListItem]) -> Option<&'static str> {
    [AttachedValuePolicy::Infer, AttachedValuePolicy::Fixed]
        .into_iter()
        .find(|policy| {
            items.iter().all(|item| {
                let Some(facts) = &item.entry else {
                    return false;
                };
                let Ok(signature) = entry_signature(item, facts.role, true, *policy) else {
                    return false;
                };
                let rebuilt =
                    bindings::entry_facts(item, &signature, facts.role, facts.case, *policy, true);
                rebuilt.names == facts.names
                    && rebuilt.forms == facts.forms
                    && rebuilt.name_bindings == facts.name_bindings
            })
        })
        .map(|policy| match policy {
            AttachedValuePolicy::Infer => "",
            AttachedValuePolicy::Fixed => " attached=fixed",
        })
}
use super::bindings::{OriginalItemId, OriginalListId};
pub(crate) use diagnostics::is_semantic_entry_rejection_code;
use names::{entry_names, is_option_code};
use signature::{EntrySignature, entry_signature, entry_term_text};

use super::directives::{
    AttachedValuePolicy, DomainDeclaration, DomainDeclarationState, SemanticDeclarations,
    domain_diagnostic, semantic_diagnostic,
};
use mant_ir::{
    Block, DefinitionRole, Diagnostic, Inline, ListItem, ListKind, SourceSpan, ValueDomain,
};

/// Attach facts to each declared owner without consuming its head or delimiter.
pub(super) fn normalize_entry_lists(
    blocks: &mut [Block],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    entry_coverage(blocks, declarations, diagnostics);
}

/// Coverage of direct semantic children through transparent containers. A
/// successfully annotated owner is a boundary: failures inside its own body
/// cannot invalidate a sibling's or parent's enumeration of that owner.
#[derive(Clone, Copy, Default)]
struct EntryCoverage {
    rejected: bool,
}

fn entry_coverage(
    blocks: &mut [Block],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> EntryCoverage {
    let mut coverage = EntryCoverage::default();
    for block in blocks {
        let Block::List {
            kind,
            items,
            source,
            ..
        } = block
        else {
            coverage.rejected |= normalize_nested_blocks(block, declarations, diagnostics).rejected;
            continue;
        };
        let owner_offsets = declarations.bindings.items(*source).to_vec();
        let child_coverage = items
            .iter_mut()
            .enumerate()
            .map(|(index, item)| {
                item_child_coverage(
                    item,
                    owner_offsets.get(index).copied(),
                    declarations,
                    diagnostics,
                )
            })
            .collect::<Vec<_>>();
        if items.is_empty() {
            continue;
        }
        let declaration =
            OriginalListId::from_source(*source).and_then(|id| declarations.entries.remove(&id));
        if declaration.is_none() && *kind != ListKind::Bullet {
            coverage.rejected |= child_coverage.iter().any(|value| value.rejected);
            continue;
        }
        let role = declaration.map_or(DefinitionRole::Option, |value| value.role);
        let case = declaration.map_or(mant_ir::DefinitionCase::Sensitive, |value| value.case);
        let attached = declaration.map_or(AttachedValuePolicy::Infer, |value| value.attached);
        let signatures = items
            .iter()
            .map(|item| entry_signature(item, role, declaration.is_some(), attached))
            .collect::<Vec<_>>();
        // Undeclared option recognition retains its conservative whole-list
        // admission rule. An explicit declaration instead owns each item's
        // validation independently; one rejection cannot erase valid siblings.
        if declaration.is_none() && signatures.iter().any(Result::is_err) {
            coverage.rejected |= child_coverage.iter().any(|value| value.rejected);
            if resembles_rejected_option_list(items) {
                semantic_diagnostic(
                    diagnostics,
                    source.unwrap_or(SourceSpan {
                        byte_range: None,
                        line: 1,
                        column: 1,
                        end_line: None,
                        end_column: None,
                    }),
                    "option-like list is not complete; every item needs code terms and a ':' or dash description delimiter"
                        .to_owned(),
                );
            }
            continue;
        }
        for (item_index, ((item, signature), children)) in items
            .iter_mut()
            .zip(signatures)
            .zip(child_coverage)
            .enumerate()
        {
            let signature = match signature {
                Ok(signature) => signature,
                Err(rejection) => {
                    coverage.rejected = true;
                    rejection.emit(
                        diagnostics,
                        source.unwrap_or(declaration.expect("declared rejection").source),
                    );
                    continue;
                }
            };
            let domain = owner_offsets
                .get(item_index)
                .and_then(|offset| declarations.bindings.domains.remove(offset))
                .and_then(DomainDeclarationState::into_unique);
            item.entry = Some(bindings::entry_facts(
                item,
                &signature,
                role,
                case,
                attached,
                declaration.is_some(),
            ));
            if declaration.is_some()
                && let Some(offset) = owner_offsets.get(item_index)
            {
                declarations.bindings.declared_items.insert(*offset);
            }
            if let Some(declaration) = domain {
                attach_domain(item, declaration, children, diagnostics);
            }
        }
    }
    coverage
}

fn attach_domain(
    item: &mut ListItem,
    declaration: DomainDeclaration,
    children: EntryCoverage,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if matches!(declaration.value, ValueDomain::Choices { exhaustive: true }) && children.rejected {
        domain_diagnostic(diagnostics, declaration.source, "exhaustive choices requires complete extraction of the direct child entries; rejected children remain visible and the declared domain was omitted".to_owned());
    } else if matches!(declaration.value, ValueDomain::Choices { .. }) && !item.has_value_choices()
    {
        domain_diagnostic(diagnostics, declaration.source, "choices requires nonempty direct semantic children of role=value; the declared domain was omitted".to_owned());
    } else {
        item.entry
            .as_mut()
            .expect("an annotated entry has facts")
            .value_domain = Some(declaration.value);
    }
}

/// Collect both kinds of child failure before deciding whether this item is a
/// semantic owner. Ordinary bullet/ordered containers must carry declaration
/// failures upward just like failures returned by their nested content.
fn item_child_coverage(
    item: &mut ListItem,
    owner_offset: Option<OriginalItemId>,
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> EntryCoverage {
    let rejected_declaration = owner_offset.is_some_and(|offset| {
        declarations
            .bindings
            .incomplete_entry_children
            .remove(&offset)
    });
    let mut coverage = entry_coverage(&mut item.blocks, declarations, diagnostics);
    coverage.rejected |= rejected_declaration;
    coverage
}

fn normalize_nested_blocks(
    block: &mut Block,
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> EntryCoverage {
    let mut coverage = EntryCoverage::default();
    match block {
        Block::List { .. } => {
            return entry_coverage(std::slice::from_mut(block), declarations, diagnostics);
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                normalize_entry_lists(&mut item.description, declarations, diagnostics);
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                coverage.rejected |=
                    entry_coverage(&mut cell.blocks, declarations, diagnostics).rejected;
            }
        }
        Block::Paragraph { .. }
        | Block::Preformatted { .. }
        | Block::Equation { .. }
        | Block::VerticalSpace { .. }
        | Block::ThematicBreak { .. }
        | Block::Unsupported { .. } => {}
    }
    coverage
}

fn resembles_rejected_option_list(items: &[ListItem]) -> bool {
    let option_like = items
        .iter()
        .filter(|item| {
            matches!(
                item.blocks.first(),
                Some(Block::Paragraph { children, .. })
                    if children.iter().any(|inline| matches!(inline, Inline::Code { value } if is_option_code(value)))
            )
        })
        .count();
    option_like > 0 && option_like.saturating_add(1) >= items.len()
}

#[cfg(test)]
fn normalize_option_lists(blocks: &mut [Block]) {
    normalize_entry_lists(
        blocks,
        &mut SemanticDeclarations::default(),
        &mut Vec::new(),
    );
}

#[cfg(test)]
mod tests {
    use mant_ir::{Block, DefinitionCase, DefinitionRole, Inline, LayoutHint, ListItem, ListKind};

    use super::normalize_option_lists;

    fn paragraph(children: Vec<Inline>) -> Block {
        Block::Paragraph {
            children,
            layout: LayoutHint::default(),
            source: None,
        }
    }

    #[test]
    fn annotates_only_complete_undeclared_option_lists() {
        let option = |name: &str, description: &str| ListItem {
            source: None,
            entry: None,
            blocks: vec![paragraph(vec![
                Inline::Code {
                    value: name.to_owned(),
                },
                Inline::Text {
                    value: format!(": {description}"),
                },
            ])],
        };
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: true,
            items: vec![
                option("-h, --help", "Show help."),
                option("--version", "Print version."),
            ],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&mut blocks);

        let Block::List { items, .. } = &blocks[0] else {
            panic!("ordinary list must remain intact");
        };
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| {
            item.entry.as_ref().is_some_and(|identity| {
                identity.role == DefinitionRole::Option
                    && identity.case == DefinitionCase::Sensitive
            })
        }));
        assert!(matches!(
            &items[0].blocks[0],
            Block::Paragraph { children, .. }
                if matches!(&children[1], Inline::Text { value } if value == ": Show help.")
        ));
    }

    #[test]
    fn keeps_trailing_content_blocks_in_the_original_item() {
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: false,
            items: vec![ListItem {
                source: None,
                entry: None,
                blocks: vec![
                    paragraph(vec![
                        Inline::Code {
                            value: "--config".to_owned(),
                        },
                        Inline::Text {
                            value: ": Read configuration.".to_owned(),
                        },
                    ]),
                    Block::Preformatted {
                        children: vec![Inline::Text {
                            value: "tool --config path".to_owned(),
                        }],
                        language: None,
                        layout: LayoutHint::default(),
                        source: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&mut blocks);

        let Block::List { items, .. } = &blocks[0] else {
            panic!("ordinary list must remain intact");
        };
        assert!(matches!(
            items[0].blocks.as_slice(),
            [Block::Paragraph { .. }, Block::Preformatted { children, .. }]
                if matches!(&children[0], Inline::Text { value } if value == "tool --config path")
        ));
    }

    #[test]
    fn leaves_mixed_lists_unchanged() {
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            start: None,
            compact: true,
            items: vec![
                ListItem {
                    source: None,
                    entry: None,
                    blocks: vec![paragraph(vec![
                        Inline::Code {
                            value: "--color".to_owned(),
                        },
                        Inline::Text {
                            value: ": Control colour.".to_owned(),
                        },
                    ])],
                },
                ListItem {
                    source: None,
                    entry: None,
                    blocks: vec![paragraph(vec![Inline::Text {
                        value: "ordinary prose".to_owned(),
                    }])],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        let original = blocks.clone();

        normalize_option_lists(&mut blocks);

        assert_eq!(blocks, original, "a rejected mixed list remains untouched");
    }
}
