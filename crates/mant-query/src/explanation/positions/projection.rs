//! Coordinate translation from source slices into an excerpted owner.
use super::Domain;
use mant_ir::{Block, EntryForm, EntryInlineRoot, EntryOwner, project_content_slice};
use mant_protocol::{
    ExplanationBlockStep as Step, ExplanationContentRange as ContentRange, ExplanationFormRange,
    ExplanationOccurrence, MAX_EXPLANATION_FRAGMENTS,
};

pub(super) fn occurrence(
    owner: EntryOwner<'_>,
    form: &EntryForm,
    domain: Domain,
) -> Option<ExplanationOccurrence> {
    if form.parts.len() > MAX_EXPLANATION_FRAGMENTS {
        return None;
    }
    let mut result = ExplanationOccurrence::default();
    for part in &form.parts {
        let range = project_content_slice(owner, part)?;
        if range.chars.is_empty() {
            continue;
        }
        match domain {
            Domain::Content => {
                let start_char = u32::try_from(range.chars.start).ok()?;
                let end_char = u32::try_from(range.chars.end).ok()?;
                let mapped = match range.root {
                    EntryInlineRoot::Term { index } => ContentRange::DefinitionTerm {
                        path: Vec::new(),
                        item_index: 0,
                        term_index: u32::try_from(index).ok()?,
                        start_char,
                        end_char,
                    },
                    EntryInlineRoot::Block { index } => ContentRange::BlockText {
                        path: vec![
                            owner_step(owner),
                            Step::Block {
                                index: u32::try_from(index).ok()?,
                            },
                        ],
                        start_char,
                        end_char,
                    },
                };
                result.content.push(mapped);
            }
            Domain::Forms => {
                let mut covered = false;
                for (index, source_form) in owner.facts()?.forms.iter().enumerate() {
                    let mut offset = 0;
                    for source_part in &source_form.parts {
                        let source = project_content_slice(owner, source_part)?;
                        if source.root == range.root
                            && source.chars.start <= range.chars.start
                            && range.chars.end <= source.chars.end
                        {
                            if result.forms.len() == MAX_EXPLANATION_FRAGMENTS {
                                return None;
                            }
                            result.forms.push(ExplanationFormRange {
                                form_index: u32::try_from(index).ok()?,
                                start_char: u32::try_from(
                                    offset + range.chars.start - source.chars.start,
                                )
                                .ok()?,
                                end_char: u32::try_from(
                                    offset + range.chars.end - source.chars.start,
                                )
                                .ok()?,
                            });
                            covered = true;
                        }
                        offset += source.chars.len();
                    }
                }
                if !covered {
                    return None;
                }
            }
        }
    }
    Some(result)
}

fn owner_step(owner: EntryOwner<'_>) -> Step {
    match owner {
        EntryOwner::Definition(_) => Step::DefinitionItem { index: 0 },
        EntryOwner::List(_) => Step::ListItem { index: 0 },
    }
}

pub(in crate::explanation) fn preview_range(
    owner: Option<EntryOwner<'_>>,
    hit: &crate::explanation::preview::LiteralHit<'_>,
) -> Option<ContentRange> {
    let path = if let Some(owner) = owner {
        let mut path = vec![owner_step(owner)];
        if !find_block(owner.blocks(), hit.block, &mut path) {
            return None;
        }
        path
    } else {
        Vec::new()
    };
    let text = crate::explanation::literal::block_text(hit.block)?;
    Some(ContentRange::BlockText {
        path,
        start_char: u32::try_from(text.get(..hit.range.start)?.chars().count()).ok()?,
        end_char: u32::try_from(text.get(..hit.range.end)?.chars().count()).ok()?,
    })
}

fn find_block(blocks: &[Block], target: &Block, path: &mut Vec<Step>) -> bool {
    // Source traversal is already bounded; reject malformed external producer
    // depth rather than manufacturing a location outside the response contract.
    if path.len() > 512 {
        return false;
    }
    for (index, block) in blocks.iter().enumerate() {
        let Ok(index) = u32::try_from(index) else {
            return false;
        };
        path.push(Step::Block { index });
        if std::ptr::eq(block, target) {
            return true;
        }
        match block {
            Block::List { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    let Ok(index) = u32::try_from(index) else {
                        return false;
                    };
                    path.push(Step::ListItem { index });
                    if find_block(&item.blocks, target, path) {
                        return true;
                    }
                    path.pop();
                }
            }
            Block::DefinitionList { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    let Ok(index) = u32::try_from(index) else {
                        return false;
                    };
                    path.push(Step::DefinitionItem { index });
                    if find_block(&item.description, target, path) {
                        return true;
                    }
                    path.pop();
                }
            }
            Block::Table { rows, .. } => {
                for (row, cells) in rows.iter().enumerate() {
                    for (column, cell) in cells.cells.iter().enumerate() {
                        let (Ok(row), Ok(column)) = (u32::try_from(row), u32::try_from(column))
                        else {
                            return false;
                        };
                        path.push(Step::TableCell { row, column });
                        if find_block(&cell.blocks, target, path) {
                            return true;
                        }
                        path.pop();
                    }
                }
            }
            _ => {}
        }
        path.pop();
    }
    false
}
