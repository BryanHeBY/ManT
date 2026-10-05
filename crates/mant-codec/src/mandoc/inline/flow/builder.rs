//! Construct inline projection owners without changing persistent execution state.
use super::{CellProduction, Inline, InlineBuilder, InlineExecutionState, output};
use libmandoc_rs::MacroSet;

impl InlineBuilder {
    #[cfg(test)]
    pub(in crate::mandoc) fn new() -> Self {
        Self::with_spacing(true)
    }

    pub(in crate::mandoc) fn with_spacing(spacing_enabled: bool) -> Self {
        Self {
            nodes: Vec::new(),
            output_positions: None,
            head_operand_capture: None,
            direct_word_operands: false,
            asserted_vertical_row: false,
            produced_formatter_cell: CellProduction::None,
            pending_output_scope_prefixes: Vec::new(),
            execution: InlineExecutionState::with_spacing(spacing_enabled),
            external_head_row_pending: false,
        }
    }

    pub(in crate::mandoc) fn from_parts(
        nodes: Vec<Inline>,
        execution: InlineExecutionState,
    ) -> Self {
        Self {
            nodes,
            output_positions: None,
            head_operand_capture: None,
            direct_word_operands: false,
            asserted_vertical_row: false,
            produced_formatter_cell: CellProduction::None,
            pending_output_scope_prefixes: Vec::new(),
            execution,
            external_head_row_pending: false,
        }
    }

    pub(in crate::mandoc) fn inherit_external_head_row(&mut self, pending: bool) {
        self.external_head_row_pending = pending;
    }

    pub(in crate::mandoc) fn with_direct_word_operands<R>(
        &mut self,
        append: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(&mut self.direct_word_operands, true);
        let result = append(self);
        self.direct_word_operands = previous;
        result
    }

    pub(in crate::mandoc) const fn asserted_vertical_row(&self) -> bool {
        self.asserted_vertical_row
    }

    pub(in crate::mandoc) const fn produced_formatter_cell(&self) -> bool {
        matches!(self.produced_formatter_cell, CellProduction::Produced)
    }

    pub(super) fn note_produced_formatter_cell(&mut self, produced: bool) {
        if produced {
            self.produced_formatter_cell = CellProduction::Produced;
        }
    }

    pub(in crate::mandoc) fn visits_empty_text_as_space(&self, node: &libmandoc_rs::Node) -> bool {
        if self.direct_word_operands {
            return false;
        }
        match self.execution.macro_set {
            MacroSet::Man => true,
            MacroSet::Mdoc => node.flags.line_start,
            MacroSet::None => false,
        }
    }

    pub(in crate::mandoc) fn mark_definition_term_break(&mut self) {
        if let Some(index) = self
            .nodes
            .iter()
            .rposition(|node| !output::is_private_output_marker(node))
            && matches!(self.nodes[index], Inline::LineBreak {})
            && !self.nodes[index + 1..]
                .iter()
                .any(output::is_term_alternative)
        {
            self.nodes
                .push(Inline::anchor(output::INTERNAL_TERM_ALTERNATIVE));
        }
    }

    pub(in crate::mandoc) fn into_parts(self) -> (Vec<Inline>, InlineExecutionState) {
        (self.nodes, self.execution)
    }
}
