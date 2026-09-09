//! Visit semantic children through transparent structural containers.
use crate::{Block, DefinitionItem, EntryKind, EntryOwner, ListItem};

/// Visit direct semantic children through transparent structural containers.
/// An entry's description belongs to that child, not to the current parent.
pub fn visit_child_entries<'a>(blocks: &'a [Block], visit: &mut impl FnMut(EntryOwner<'a>)) {
    walk::<false>(blocks, &mut Vec::new(), &mut |owner, _, _| visit(owner));
}

/// Locate direct semantic children during the same authoritative owner walk.
/// The borrowed path ends at the containing list; `item` addresses its owner.
pub(super) fn visit_child_entry_locations<'a>(
    blocks: &'a [Block],
    prefix: &[crate::ContentBlockStep],
    visit: &mut impl FnMut(EntryOwner<'a>, &[crate::ContentBlockStep], u32),
) {
    walk::<true>(blocks, &mut prefix.to_vec(), visit);
}

fn walk<'a, const LOCATE: bool>(
    blocks: &'a [Block],
    path: &mut Vec<crate::ContentBlockStep>,
    visit: &mut impl FnMut(EntryOwner<'a>, &[crate::ContentBlockStep], u32),
) {
    use crate::ContentBlockStep as Step;
    for (block_index, block) in blocks.iter().enumerate() {
        if LOCATE {
            path.push(Step::Block {
                index: coordinate(block_index),
            });
        }
        match block {
            Block::DefinitionList { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    visit_or_descend::<LOCATE>(
                        EntryOwner::Definition(item),
                        coordinate(index),
                        path,
                        visit,
                    );
                }
            }
            Block::List { items, .. } => {
                for (index, item) in items.iter().enumerate() {
                    visit_or_descend::<LOCATE>(
                        EntryOwner::List(item),
                        coordinate(index),
                        path,
                        visit,
                    );
                }
            }
            Block::Table { rows, .. } => {
                for (row, cells) in rows.iter().enumerate() {
                    for (column, cell) in cells.cells.iter().enumerate() {
                        if LOCATE {
                            path.push(Step::TableCell {
                                row: coordinate(row),
                                column: coordinate(column),
                            });
                        }
                        walk::<LOCATE>(&cell.blocks, path, visit);
                        if LOCATE {
                            path.pop();
                        }
                    }
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
        if LOCATE {
            path.pop();
        }
    }
}

fn visit_or_descend<'a, const LOCATE: bool>(
    owner: EntryOwner<'a>,
    index: u32,
    path: &mut Vec<crate::ContentBlockStep>,
    visit: &mut impl FnMut(EntryOwner<'a>, &[crate::ContentBlockStep], u32),
) {
    if owner.facts().is_some() {
        visit(owner, path, index);
    } else {
        if LOCATE {
            path.push(owner_child_step(owner, index));
        }
        walk::<LOCATE>(owner.blocks(), path, visit);
        if LOCATE {
            path.pop();
        }
    }
}

pub(super) const fn owner_child_step(owner: EntryOwner<'_>, index: u32) -> crate::ContentBlockStep {
    match owner {
        EntryOwner::Definition(_) => crate::ContentBlockStep::DefinitionItem { index },
        EntryOwner::List(_) => crate::ContentBlockStep::ListItem { index },
    }
}

fn coordinate(value: usize) -> u32 {
    // A container beyond this representation cannot arise from bounded parsers.
    // Saturation remains non-panicking for an oversized third-party IR value.
    u32::try_from(value).unwrap_or(u32::MAX)
}

impl DefinitionItem {
    /// Whether the direct semantic children form a nonempty set of values.
    ///
    /// Uses the same ownership walk as [`crate::SemanticIndex`], without building or
    /// cloning entries. Deeper descendants of a child are not sibling choices.
    #[must_use]
    pub fn has_value_choices(&self) -> bool {
        has_value_choices(&self.description)
    }
}

impl ListItem {
    /// Whether this item's direct semantic children are nonempty and all values.
    #[must_use]
    pub fn has_value_choices(&self) -> bool {
        has_value_choices(&self.blocks)
    }
}

fn has_value_choices(blocks: &[Block]) -> bool {
    let mut found = false;
    let mut only_values = true;
    visit_child_entries(blocks, &mut |item| {
        found = true;
        only_values &= item
            .facts()
            .is_some_and(|identity| identity.kind == EntryKind::Value);
    });
    found && only_values
}
