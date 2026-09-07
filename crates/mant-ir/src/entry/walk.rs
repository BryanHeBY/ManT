//! Visit semantic children through transparent structural containers.
use crate::{Block, DefinitionItem, DefinitionRole};

/// Visit direct semantic children through transparent structural containers.
/// An entry's description belongs to that child, not to the current parent.
pub(super) fn visit_child_definitions(blocks: &[Block], visit: &mut impl FnMut(&DefinitionItem)) {
    for block in blocks {
        match block {
            Block::DefinitionList { items, .. } => {
                for item in items.iter().filter(|item| item.identity.is_some()) {
                    visit(item);
                }
            }
            Block::List { items, .. } => {
                for item in items {
                    visit_child_definitions(&item.blocks, visit);
                }
            }
            Block::Table { rows, .. } => {
                for cell in rows.iter().flat_map(|row| &row.cells) {
                    visit_child_definitions(&cell.blocks, visit);
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
        let mut found = false;
        let mut only_values = true;
        visit_child_definitions(&self.description, &mut |item| {
            found = true;
            only_values &= item
                .identity
                .as_ref()
                .is_some_and(|identity| identity.role == DefinitionRole::Value);
        });
        found && only_values
    }
}
