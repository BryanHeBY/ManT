//! The documented roff reading exception for one empty initial code row.
//!
//! CVS `man_term.c`/`mdoc_term.c` execute empty TEXT via `term_vspace()`, so this
//! is deliberately a reading policy, not a correction to native execution.
//! Run once, after complete literal owners are assembled and before semantic
//! identification captures public text positions. Renderers never trim rows.

use mant_ir::{
    Block, DefinitionItem, Inline, Section,
    visit::{self, VisitMut},
};

use crate::definitions::NativeHeadEvidence;

pub(super) fn normalize(
    blocks: &mut [Block],
    sections: &mut [Section],
    evidence: &mut NativeHeadEvidence,
) {
    struct Normalize<'a>(&'a mut NativeHeadEvidence);

    impl VisitMut for Normalize<'_> {
        fn visit_block_mut(&mut self, block: &mut Block) {
            if matches!(block, Block::Preformatted { .. }) {
                self.0
                    .hanging
                    .transform_literal_head(block, omit_initial_row);
            } else {
                visit::walk_block_mut(self, block);
            }
        }

        fn visit_definition_item_mut(&mut self, item: &mut DefinitionItem) {
            // A shared literal prefix continues a HEAD row. Its first break
            // belongs to that relationship, not an independent empty code row.
            // Use the borrowed selection so transparent prefixes keep identity.
            let shared = item.shared_description().map(|body| body.block_index);
            for (index, block) in item.description.iter_mut().enumerate() {
                if Some(index) != shared || !matches!(block, Block::Preformatted { .. }) {
                    self.visit_block_mut(block);
                }
            }
        }
    }

    let mut visitor = Normalize(evidence);
    for block in blocks {
        visitor.visit_block_mut(block);
    }
    for section in sections {
        visitor.visit_section_mut(section);
    }
}

/// Return removed UTF-8 prefix bytes for a matching native-head proof.
fn omit_initial_row(block: &mut Block) -> usize {
    let Block::Preformatted {
        children,
        inline_layout,
        layout,
        ..
    } = block
    else {
        return 0;
    };
    if !eligible(children) {
        return 0;
    }
    // Old hard rows >= 1 all use indent + continuation. They become the new
    // first and continued rows; preserve both origins and every row exception.
    let Some(indent) = layout
        .indent_columns
        .checked_add(layout.continuation_indent_columns)
    else {
        return 0;
    };
    if !remove_first_break(children) {
        return 0;
    }
    layout.indent_columns = indent;
    layout.continuation_indent_columns = 0;
    inline_layout.row_hints.retain_mut(|hint| {
        if hint.row == 0 {
            return false;
        }
        hint.row -= 1;
        true
    });
    1
}

fn eligible(children: &[Inline]) -> bool {
    struct Prefix {
        scalars: u8,
        body: bool,
        rejected: bool,
    }

    impl Prefix {
        fn text(&mut self, text: &str) {
            for character in text.chars() {
                if self.rejected || self.body {
                    return;
                }
                match self.scalars {
                    0 if character != '\n' => self.rejected = true,
                    1 if character == '\n' => self.rejected = true,
                    0 | 1 => self.scalars += 1,
                    _ => {}
                }
                self.body |= self.scalars >= 2 && !character.is_whitespace();
            }
        }

        fn scan(&mut self, children: &[Inline]) {
            for child in children {
                if self.rejected || self.body {
                    return;
                }
                match child {
                    Inline::Text { value } | Inline::Code { value } => self.text(value),
                    Inline::LineBreak {} => self.text("\n"),
                    Inline::Strong { children }
                    | Inline::Emphasis { children }
                    | Inline::Link { children, .. } => self.scan(children),
                    // Math is an atomic projection: never trim its value
                    // independently from the authoritative expression.
                    Inline::Equation { value, .. } => {
                        if self.scalars == 0 && !value.is_empty() {
                            self.rejected = true;
                        } else {
                            self.text(value);
                        }
                    }
                    Inline::Anchor { .. } => {}
                }
            }
        }
    }

    let mut prefix = Prefix {
        scalars: 0,
        body: false,
        rejected: false,
    };
    prefix.scan(children);
    prefix.body && !prefix.rejected
}

/// Remove one scalar without flattening wrappers or dropping zero-width IDs.
fn remove_first_break(children: &mut Vec<Inline>) -> bool {
    for index in 0..children.len() {
        match &mut children[index] {
            Inline::LineBreak {} => {
                children.remove(index);
                return true;
            }
            Inline::Text { value } | Inline::Code { value } if value.starts_with('\n') => {
                value.remove(0);
                return true;
            }
            Inline::Strong { children }
            | Inline::Emphasis { children }
            | Inline::Link { children, .. } => {
                if remove_first_break(children) {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests;
