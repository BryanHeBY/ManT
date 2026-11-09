//! Table input observations borrow the shared word and source registers.
use super::InlineExecutionState;

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
