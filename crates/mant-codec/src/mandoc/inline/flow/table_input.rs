//! Table input observations borrow the shared word and source registers.
use super::{InlineBuilder, InlineExecutionState};
use crate::{definitions::NativeOperandRole, mandoc::blocks::InlineTablePrefix};
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
            // The inferred vertical edge spacing runs before any box rule
            // or cell writes, and consumes the same skipvsp as roff .sp.
            // A box rule closes an occupied line too, but its decoration
            // contributes no independent row to this phrasing projection.
            if node.flags.table_leading_space {
                self.native_vertical_space(1);
            }
            if node.table_frame.is_some() && self.execution.has_open_native_device_row() {
                self.commit_table_row_projection(true, false);
            }
        }
        let open = self.execution.has_open_native_device_row();
        let prefix = self.table_prefix(open);
        let mut formatter = crate::mandoc::formatter::FormatterState::default();
        formatter.execution = std::mem::take(&mut self.execution);
        let row = table.lower_row(node, index, &mut formatter, prefix);
        self.execution = formatter.execution;
        if let Some(row) = row {
            self.project_table_row(row, open);
        }
        self.finish_executed_node(node);
        self.restore_definition_geometry(geometry);
    }

    fn table_prefix(&self, open: bool) -> Option<InlineTablePrefix> {
        let field = self.execution.definition.as_ref()?;
        if open
            && self
                .execution
                .last_visible_character
                .is_some_and(|c| !c.is_whitespace())
        {
            let presentation = self.execution.row_presentation;
            Some(InlineTablePrefix {
                offset: self.execution.table_source_bounds(0).0,
                printed: usize::try_from(
                    presentation
                        .device_column(field.hang_row.viscol)
                        .saturating_add(i64::from(presentation.parent)),
                )
                .unwrap_or_default(),
                min_padding: field.hang_row.minbl.saturating_mul(24),
            })
        } else {
            None
        }
    }

    fn project_table_row(&mut self, row: crate::mandoc::blocks::InlineTableRow, open: bool) {
        let content = row.content;
        if row.separates_prefix {
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
    /// The first accepted `term_fill()` width (`vbr`) excludes a trailing
    /// terminal blank run but includes fixed blanks. tblcalc's whole-cell
    /// measurement is a distinct operation and still uses the full input.
    pub(in crate::mandoc) fn table_first_pass_width(&self, text: &str, target: usize) -> usize {
        let mut font = super::FontState::new();
        let mut zero_advance = crate::mandoc::inline::ZeroAdvanceState::default();
        // tbl_term.c clears both BACK registers before filling each column.
        let word = crate::mandoc::inline::parse_roff_text_with_zero_advance(
            text,
            crate::mandoc::inline::TextExecutionContext {
                font: &mut font,
                zero_advance: &mut zero_advance,
                defer_leading_blanks: false,
                policy: crate::mandoc::inline::TextExecutionPolicy {
                    recognize_generated_references: false,
                    record_native_cells: true,
                    field_authoritative: true,
                    word_space: crate::mandoc::roff_escape::WordSpace::Breakable,
                },
            },
        );
        let mut field = super::field_buffer::FieldBuffer::default();
        field.configure_tabs(&self.tab_stops);
        field.apply_writes(&word.native_writes);
        field.fill_pass_units(target).map_or(0, |pass| pass.units)
    }

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
            // pre_br clears BRIND and restores maxrmargin. The saved
            // field margin still carries its BODY origin, not this bound.
            .filter(|field| field.margin_override.is_none())
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
