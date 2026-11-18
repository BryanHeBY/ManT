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
        // print_mdoc_node() invokes term_newln() at table entry. HANG and
        // fitting NOBREAK fields can print their prefix without ending the
        // device row. The table's endline is a separate operation.
        if node.flags.table_start {
            self.execute_native_newline_with_tail();
        }
        let open = self.execution.has_open_native_device_row();
        let has_gap = self
            .execution
            .definition
            .as_ref()
            .is_some_and(|field| field.hang_row.padding_units(field.field_offset_units) >= 24);
        let needs_gap = open
            && has_gap
            && self
                .execution
                .last_visible_character
                .is_some_and(|c| !c.is_whitespace());
        let mut formatter = crate::mandoc::formatter::FormatterState::default();
        formatter.execution = std::mem::take(&mut self.execution);
        let row = table.lower_row(node, index, &mut formatter);
        self.execution = formatter.execution;
        if let Some(row) = row {
            self.project_table_row(row, open, needs_gap);
        }
        self.finish_executed_node(node);
        self.restore_definition_geometry(geometry);
    }

    fn project_table_row(
        &mut self,
        row: crate::mandoc::blocks::InlineTableRow,
        open: bool,
        needs_gap: bool,
    ) {
        let content = row.content;
        if needs_gap && !content.is_empty() {
            self.append_projected(vec![Inline::Text { value: " ".into() }]);
        }
        if !content.is_empty() && self.head_operand_capture.is_some() {
            // The table cells belong to the term's readable content only.
            self.with_native_operand_role(NativeOperandRole::DisplayOnly, |builder| {
                builder.begin_native_word_owner();
            });
            if let Some(marker) = self.execution.native_word_owner.take() {
                self.nodes.push(Inline::anchor(marker));
            }
        }
        self.append_projected(content);
        // The phrasing projection omits grid geometry. A real data span
        // still owns its endline even when its accepted cells are empty.
        self.commit_table_row_projection(row.data || open, row.data);
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
