//! Stable positions in an output owner, independent of formatter registers.

use super::{
    Inline, InlineBuilder, OutputCheckpoint, has_rendered_formatter_glyph,
    retain_inline_identities, row_origins,
};

impl InlineBuilder {
    /// Apply one projection's original-root boundary receipts to live scopes
    /// and native owner indices. Each address is looked up once.
    pub(in crate::mandoc::inline::flow) fn remap_output_projection(
        &mut self,
        edits: &row_origins::OutputNodeEdits,
    ) {
        self.remap_output_projection_checkpoints(edits);
        self.execution.remap_output_projection(edits);
    }

    pub(in crate::mandoc::inline::flow) fn remap_output_projection_checkpoints(
        &mut self,
        edits: &row_origins::OutputNodeEdits,
    ) {
        if let Some(registry) = &self.output_positions {
            for position in registry.borrow().iter().filter_map(std::rc::Weak::upgrade) {
                let mut index = position.get();
                edits.remap(&mut index);
                position.set(index);
            }
        }
    }

    /// Rehome an already represented HEAD row without moving the remaining
    /// native unit's output cursors away from their source owners.
    pub(in crate::mandoc) fn consume_invisible_head_row(&mut self) -> bool {
        let Some(end) = self
            .nodes
            .iter()
            .position(|node| matches!(node, Inline::LineBreak {}))
        else {
            return false;
        };
        if has_rendered_formatter_glyph(&self.nodes[..end]) {
            return false;
        }
        let removed = end + 1;
        let mut identities: Vec<_> = self.nodes.drain(..removed).collect();
        retain_inline_identities(&mut identities);
        let inserted = identities.len();
        self.nodes.splice(0..0, identities);
        self.remap_output_positions(row_origins::OutputNodeEdit {
            start: 0,
            removed,
            inserted,
        });
        true
    }

    /// Row-layout carriers can be inserted into an already accepted prefix.
    /// Keep every live root-vector address on its original content boundary.
    /// `NativeWordAnchor` and field owner ranges are `FieldBuffer` cell/scalar
    /// positions; stable word/annotation markers and committed IR need no map.
    pub(in crate::mandoc::inline::flow) fn remap_output_positions(
        &mut self,
        edit: row_origins::OutputNodeEdit,
    ) {
        if let Some(registry) = &self.output_positions {
            for position in registry.borrow().iter().filter_map(std::rc::Weak::upgrade) {
                let mut index = position.get();
                edit.remap(&mut index);
                position.set(index);
            }
        }
        edit.remap(&mut self.execution.flush_unit_output_start);
        if let Some(author) = &mut self.execution.author_execution {
            edit.remap(&mut author.field_output_start);
        }
        if let Some(definition) = &mut self.execution.definition {
            if let Some(field) = &mut definition.no_break {
                field.remap_output_positions(edit);
            }
            if let Some(index) = &mut definition.hang_row.provisional_trailing_break {
                edit.remap(index);
            }
        }
    }

    pub(in crate::mandoc) fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Record an output slice before its source executes. The checkpoint
    /// owns no formatter registers and cannot roll execution back.
    pub(in crate::mandoc) fn begin_output_checkpoint(&mut self) -> OutputCheckpoint {
        let position = std::rc::Rc::new(std::cell::Cell::new(self.nodes.len()));
        let registry = self
            .output_positions
            .get_or_insert_with(std::rc::Rc::default);
        registry
            .borrow_mut()
            .push(std::rc::Rc::downgrade(&position));
        OutputCheckpoint {
            position,
            registry: std::rc::Rc::downgrade(registry),
        }
    }

    pub(super) fn output_checkpoint_index(&self, checkpoint: &OutputCheckpoint) -> usize {
        checkpoint.position.get().min(self.nodes.len())
    }

    /// Whether the accepted output slice contains a readable semantic glyph.
    /// Syntactic label eligibility and pending-glyph consumption remain with
    /// the macro's execution stages, independent of this output observation.
    pub(in crate::mandoc) fn output_since_has_non_whitespace_glyph(
        &self,
        checkpoint: OutputCheckpoint,
    ) -> bool {
        fn contains_glyph(nodes: &[Inline]) -> bool {
            nodes.iter().any(|node| match node {
                Inline::Text { value }
                | Inline::Code { value }
                | Inline::Equation { value, .. } => {
                    value.chars().any(|character| !character.is_whitespace())
                }
                Inline::Strong { children }
                | Inline::Emphasis { children }
                | Inline::Link { children, .. } => contains_glyph(children),
                Inline::Anchor { .. } | Inline::LineBreak { .. } => false,
            })
        }
        let present = contains_glyph(&self.nodes[self.output_checkpoint_index(&checkpoint)..]);
        drop(checkpoint);
        present
    }

    /// Wrap the output emitted since `checkpoint` without replaying its
    /// formatter execution. Links and other semantic wrappers are IR
    /// annotations over an already-executed source stream.
    pub(in crate::mandoc) fn wrap_output_since(
        &mut self,
        checkpoint: OutputCheckpoint,
        wrap: impl FnMut(Vec<Inline>) -> Vec<Inline>,
    ) {
        self.wrap_output_from(self.output_checkpoint_index(&checkpoint), wrap);
        drop(checkpoint);
    }
}

impl super::super::InlineExecutionState {
    pub(in crate::mandoc::inline::flow) fn remap_output_projection(
        &mut self,
        edits: &row_origins::OutputNodeEdits,
    ) {
        edits.remap(&mut self.flush_unit_output_start);
        if let Some(author) = &mut self.author_execution {
            edits.remap(&mut author.field_output_start);
        }
        if let Some(definition) = &mut self.definition {
            if let Some(field) = &mut definition.no_break {
                field.remap_output_projection(edits);
            }
            if let Some(index) = &mut definition.hang_row.provisional_trailing_break {
                edits.remap(index);
            }
        }
    }
}
