//! Annotates semantic entries without changing ordinary Markdown content.

mod bindings;
mod diagnostics;
mod names;
mod signature;
/// Prove that one list-wide declaration reconstructs the final visible bindings.
/// This is producer grammar reuse, not a hidden spelling or parser-state export.
pub(crate) fn export_attached_policy(
    content: mant_ir::ContentContext<'_>,
    items: &[ListItem],
) -> Option<&'static str> {
    [AttachedValuePolicy::Infer, AttachedValuePolicy::Fixed]
        .into_iter()
        .find(|policy| {
            items.iter().all(|item| {
                let Some(facts) = &item.entry else {
                    return false;
                };
                let Ok(signature) = entry_signature(content, item, facts.kind, true, *policy)
                else {
                    return false;
                };
                let rebuilt =
                    bindings::entry_facts(item, &signature, facts.kind, facts.case, *policy, true);
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
use names::is_option_code;
use signature::{EntrySignature, entry_signature};

use super::directives::{
    AttachedValuePolicy, DomainDeclaration, DomainDeclarationState, SemanticDeclarations,
    domain_diagnostic, semantic_diagnostic,
};
use mant_ir::{Block, Diagnostic, EntryKind, ListItem, ListKind, SourceSpan, ValueDomain};

/// Attach facts to each declared owner without consuming its head or delimiter.
pub(super) fn normalize_entry_lists(
    content: &super::content::MarkdownContent,
    blocks: &mut [Block],
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) {
    entry_coverage(content, blocks, declarations, diagnostics);
}

/// Coverage of direct semantic children through transparent containers. A
/// successfully annotated owner is a boundary: failures inside its own body
/// cannot invalidate a sibling's or parent's enumeration of that owner.
#[derive(Clone, Copy, Default)]
struct EntryCoverage {
    rejected: bool,
}

#[allow(clippy::too_many_lines)] // This pass retains list ownership and coverage in one traversal.
fn entry_coverage(
    content: &super::content::MarkdownContent,
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
            coverage.rejected |=
                normalize_nested_blocks(content, block, declarations, diagnostics).rejected;
            continue;
        };
        let owner_offsets = declarations.bindings.items(*source).to_vec();
        let child_coverage = items
            .iter_mut()
            .enumerate()
            .map(|(index, item)| {
                item_child_coverage(
                    content,
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
        let role = declaration.map_or(
            EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            |value| value.role,
        );
        let case = declaration.map_or(mant_ir::NameCase::Sensitive, |value| value.case);
        let attached = declaration.map_or(AttachedValuePolicy::Infer, |value| value.attached);
        let signatures = items
            .iter()
            .map(|item| {
                entry_signature(
                    content.content(),
                    item,
                    role,
                    declaration.is_some(),
                    attached,
                )
            })
            .collect::<Vec<_>>();
        // Undeclared option recognition retains its conservative whole-list
        // admission rule. An explicit declaration instead owns each item's
        // validation independently; one rejection cannot erase valid siblings.
        if declaration.is_none() && signatures.iter().any(Result::is_err) {
            coverage.rejected |= child_coverage.iter().any(|value| value.rejected);
            warn_incomplete_option_list(content, items, *source, diagnostics);
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

fn warn_incomplete_option_list(
    content: &super::content::MarkdownContent,
    items: &[ListItem],
    source: Option<SourceSpan>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if resembles_rejected_option_list(content, items) {
        semantic_diagnostic(
            diagnostics,
            source.unwrap_or(SourceSpan {
                source: mant_ir::SourceKey::FIRST,
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
    content: &super::content::MarkdownContent,
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
    let mut coverage = entry_coverage(content, &mut item.blocks, declarations, diagnostics);
    coverage.rejected |= rejected_declaration;
    coverage
}

fn normalize_nested_blocks(
    content: &super::content::MarkdownContent,
    block: &mut Block,
    declarations: &mut SemanticDeclarations,
    diagnostics: &mut Vec<Diagnostic>,
) -> EntryCoverage {
    let mut coverage = EntryCoverage::default();
    match block {
        Block::List { .. } => {
            return entry_coverage(
                content,
                std::slice::from_mut(block),
                declarations,
                diagnostics,
            );
        }
        Block::DefinitionList { items, .. } => {
            for item in items {
                normalize_entry_lists(content, &mut item.description, declarations, diagnostics);
            }
        }
        Block::Table { rows, .. } => {
            for cell in rows.iter_mut().flat_map(|row| &mut row.cells) {
                coverage.rejected |=
                    entry_coverage(content, &mut cell.blocks, declarations, diagnostics).rejected;
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

fn resembles_rejected_option_list(
    content: &super::content::MarkdownContent,
    items: &[ListItem],
) -> bool {
    let option_like = items
        .iter()
        .filter(|item| {
            matches!(
                item.blocks.first(),
                Some(Block::Paragraph { children, .. })
                    if children.iter().any(|inline| {
                        matches!(content.content().inline(inline), Ok(mant_ir::InlineView::Code(value)) if is_option_code(value))
                    })
            )
        })
        .count();
    option_like > 0 && option_like.saturating_add(1) >= items.len()
}

#[cfg(test)]
fn normalize_option_lists(content: &super::content::MarkdownContent, blocks: &mut [Block]) {
    normalize_entry_lists(
        content,
        blocks,
        &mut SemanticDeclarations::default(),
        &mut Vec::new(),
    );
}

#[cfg(test)]
mod tests {
    use mant_ir::{
        Block, ContentRootKind, EntryKind, Inline, InlineView, LayoutHint, ListItem, ListKind,
        NameCase,
    };

    use super::normalize_option_lists;
    use crate::markdown::content::MarkdownContent;

    fn option_text(content: &mut MarkdownContent, name: &str, description: &str) -> Vec<Inline> {
        let mut root = content.root(ContentRootKind::Body, None);
        vec![
            root.code(name.to_owned(), None),
            root.text(format!(": {description}"), None),
        ]
    }

    fn paragraph(children: Vec<Inline>) -> Block {
        Block::Paragraph {
            children,
            layout: LayoutHint::default(),
            source: None,
        }
    }

    #[test]
    fn annotates_only_complete_undeclared_option_lists() {
        let mut content = MarkdownContent::new();
        let mut option = |name: &str, description: &str| ListItem {
            layout: mant_ir::ListItemLayout::default(),
            source: None,
            entry: None,
            blocks: vec![paragraph(option_text(&mut content, name, description))],
        };
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            compact: true,
            items: vec![
                option("-h, --help", "Show help."),
                option("--version", "Print version."),
            ],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&content, &mut blocks);

        let Block::List { items, .. } = &blocks[0] else {
            panic!("ordinary list must remain intact");
        };
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| {
            item.entry.as_ref().is_some_and(|identity| {
                identity.kind
                    == EntryKind::Parameter {
                        parameter_kind: mant_ir::ParameterKind::Option,
                    }
                    && identity.case == NameCase::Sensitive
            })
        }));
        assert!(matches!(
            &items[0].blocks[0],
            Block::Paragraph { children, .. }
                if matches!(content.content().inline(&children[1]), Ok(InlineView::Text(": Show help.")))
        ));
    }

    #[test]
    fn keeps_trailing_content_blocks_in_the_original_item() {
        let mut content = MarkdownContent::new();
        let paragraph_content = option_text(&mut content, "--config", "Read configuration.");
        let preformatted = content
            .root(ContentRootKind::FixedBody, None)
            .text("tool --config path".to_owned(), None);
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            compact: false,
            items: vec![ListItem {
                layout: mant_ir::ListItemLayout::default(),
                source: None,
                entry: None,
                blocks: vec![
                    paragraph(paragraph_content),
                    Block::Preformatted {
                        children: vec![preformatted],
                        language: None,
                        layout: LayoutHint::default(),
                        source: None,
                    },
                ],
            }],
            layout: LayoutHint::default(),
            source: None,
        }];

        normalize_option_lists(&content, &mut blocks);

        let Block::List { items, .. } = &blocks[0] else {
            panic!("ordinary list must remain intact");
        };
        assert!(matches!(
            items[0].blocks.as_slice(),
            [Block::Paragraph { .. }, Block::Preformatted { children, .. }]
                if matches!(content.content().inline(&children[0]), Ok(InlineView::Text("tool --config path")))
        ));
    }

    #[test]
    fn leaves_mixed_lists_unchanged() {
        let mut content = MarkdownContent::new();
        let option = option_text(&mut content, "--color", "Control colour.");
        let prose = content
            .root(ContentRootKind::Body, None)
            .text("ordinary prose".to_owned(), None);
        let mut blocks = vec![Block::List {
            kind: ListKind::Bullet,
            compact: true,
            items: vec![
                ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                    blocks: vec![paragraph(option)],
                },
                ListItem {
                    layout: mant_ir::ListItemLayout::default(),
                    source: None,
                    entry: None,
                    blocks: vec![paragraph(vec![prose])],
                },
            ],
            layout: LayoutHint::default(),
            source: None,
        }];
        let original = blocks.clone();

        normalize_option_lists(&content, &mut blocks);

        assert_eq!(blocks, original, "a rejected mixed list remains untouched");
    }
}
