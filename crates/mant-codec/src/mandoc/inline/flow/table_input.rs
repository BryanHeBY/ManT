//! Table input observations borrow the shared word and source registers.
use super::{InlineBuilder, InlineExecutionState};
use crate::definitions::NativeOperandRole;
use mant_ir::Inline;

impl InlineBuilder {
    pub(in crate::mandoc::inline) fn append_table_span(
        &mut self,
        node: &libmandoc_rs::Node,
        table: &crate::mandoc::blocks::InlineTableSequence<'_>,
        index: usize,
    ) {
        self.begin_executed_node(node);
        let geometry = self.definition_geometry_checkpoint(node);
        // mdoc_term.c::print_mdoc_node flushes the preceding word before the
        // first span. tbl_term.c accepts the cell contents and ends its device
        // row separately. Later HEAD field passes cannot reject that prefix.
        if node.flags.table_start {
            self.hard_break();
        }
        let mut formatter = crate::mandoc::formatter::FormatterState::default();
        formatter.execution = std::mem::take(&mut self.execution);
        let row = table.lower_row(node, index, &mut formatter);
        self.execution = formatter.execution;
        if !row.is_empty() && self.head_operand_capture.is_some() {
            // The table cells belong to the term's readable content only.
            self.with_native_operand_role(NativeOperandRole::Description, |builder| {
                builder.begin_native_word_owner();
            });
            if let Some(marker) = self.execution.native_word_owner.take() {
                self.nodes.push(Inline::anchor(marker));
            }
        }
        if !row.is_empty() {
            self.append_projected(row);
            self.commit_table_row_projection();
        }
        self.finish_executed_node(node);
        self.restore_definition_geometry(geometry);
    }
}

impl InlineExecutionState {
    pub(in crate::mandoc) fn observe_table_word_writes(
        &mut self,
        writes: &[super::field_buffer::FieldWrite],
        separator: bool,
    ) {
        if let Some(buffered) = &mut self.table_buffer_written {
            use super::field_buffer::FieldWrite;
            *buffered |= separator
                || writes.iter().any(|write| {
                    !matches!(
                        write,
                        FieldWrite::ArmBackafter
                            | FieldWrite::CancelBackafter
                            | FieldWrite::CancelBackbefore
                            | FieldWrite::RewindWordEntry(_)
                    )
                });
        }
    }
    /// `roff_term_pre_ll` -> `term_setwidth` -> `ascii_setwidth`. This private
    /// measurement register does not force responsive paragraphs to wrap.
    pub(super) fn set_table_line_length(&mut self, argument: Option<&str>) {
        let previous = self.table_page_margin;
        self.table_page_margin = libmandoc_rs::TableColumnMetrics::line_length(
            previous,
            self.table_previous_margin,
            argument,
        );
        self.table_previous_margin = previous;
        if let Some(field) = &mut self.definition {
            field.native_margin_units = Some(
                i64::try_from(self.table_page_margin)
                    .unwrap_or(i64::MAX)
                    .saturating_sub(i64::from(self.row_presentation.parent)),
            );
            field.margin_override = None;
        }
    }

    pub(in crate::mandoc) fn table_source_bounds(&self, fallback_offset: usize) -> (usize, usize) {
        let offset = self.row_presentation.source_offset_units(
            self.definition
                .as_ref()
                .map_or(0, |field| field.field_offset_units),
        );
        let margin = self
            .definition
            .as_ref()
            .and_then(|field| field.native_margin_units)
            .and_then(|margin| {
                usize::try_from(margin.saturating_add(i64::from(self.row_presentation.parent))).ok()
            })
            .unwrap_or(self.table_page_margin);
        let parent = i64::from(self.row_presentation.parent);
        let offset = if self.definition.is_some() {
            usize::try_from(offset.saturating_add(parent)).unwrap_or_default()
        } else {
            fallback_offset.saturating_add_signed(self.row_presentation.offset_shift as isize)
        };
        (offset, margin)
    }
}
