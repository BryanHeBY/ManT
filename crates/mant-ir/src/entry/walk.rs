//! Visit semantic children through transparent structural containers.
use crate::{Block, DefinitionItem, DefinitionRole, EntryOwner, ListItem};

/// Visit direct semantic children through transparent structural containers.
/// An entry's description belongs to that child, not to the current parent.
pub fn visit_child_entries<'a>(blocks: &'a [Block], visit: &mut impl FnMut(EntryOwner<'a>)) {
    for block in blocks {
        match block {
            Block::DefinitionList { items, .. } => {
                for item in items.iter().filter(|item| item.identity.is_some()) {
                    visit(EntryOwner::Definition(item));
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    if item.entry.is_some() {
                        visit(EntryOwner::List(item));
                    } else {
                        visit_child_entries(&item.blocks, visit);
                    }
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    visit_child_entries(&cell.blocks, visit);
                }
            }
            Block::Paragraph { .. }
            | Block::Preformatted { .. }
            | Block::Equation { .. }
            | Block::VerticalSpace { .. }
            | Block::ThematicBreak { .. }
            | Block::Unsupported { .. } => {}
        }
    }
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
            .is_some_and(|identity| identity.role == DefinitionRole::Value);
    });
    found && only_values
}
