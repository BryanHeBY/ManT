use super::{
    FilledBoundary, Font, FormatterColumn, Inline, InlineBuilder, OutputCheckpoint,
    PendingBoundary, TrailingOutput, WordEndBreak, has_printable_character,
};

// A private boundary carried only while one authored Link spans two native
// term_flushln() fields. It is removed before any IR owner is returned.
pub(in crate::mandoc::inline) const INTERNAL_LINK_SPLIT: &str = "\0mant:field-link-split";
const INTERNAL_COMPLETED_ROW: &str = "\0mant:output-scope:completed-row";
const INTERNAL_LITERAL_ROW: &str = "\0mant:output-scope:literal-row";
const INTERNAL_ROW_ORIGIN: &str = "\0mant:output-scope:row-origin:";
pub(in crate::mandoc::inline::flow) const INTERNAL_FIELD_PREFIX: &str =
    "\0mant:output-scope:field-prefix:";

#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::mandoc) enum CompletedRowOrigin {
    Layout,
    LiteralText,
}

/// Proof attached to the latest accepted physical row, before private markers
/// leave its owner. A generated close can replace its projected delimiter;
/// completed literal content must keep its delimiter and empty-row witness.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(in crate::mandoc) enum OutputRowEnd {
    Open,
    GeneratedClose,
    LiteralClose,
    // The gap owner carries the completed row, without an inline delimiter
    // to retire. This still proves that the latest physical row was closed.
    CompletedClose,
}

impl OutputRowEnd {
    pub(in crate::mandoc) const fn is_closed(self) -> bool {
        !matches!(self, Self::Open)
    }
}
pub(super) const INTERNAL_TERM_ALTERNATIVE: &str = "\0mant:output-scope:term-alternative";
pub(super) fn is_term_alternative(node: &Inline) -> bool {
    matches!(node, Inline::Anchor { id, .. } if id.as_str() == INTERNAL_TERM_ALTERNATIVE)
}

const INTERNAL_DEVICE_ROW_END: &str = "\0mant:output-scope:device-row-end";
pub(in crate::mandoc::inline) const INTERNAL_FIELD_WORD: &str = "\0mant:field-word:";
pub(in crate::mandoc::inline) const INTERNAL_OUTPUT_SCOPE: &str = "\0mant:output-scope:";

/// The public drain and local row-index consumers share this classification.
/// Metadata can sit after a real row end without becoming that row's payload.
pub(in crate::mandoc::inline::flow) fn is_private_output_marker(node: &Inline) -> bool {
    row_origins::is_layout_carrier(node)
        || matches!(node, Inline::Anchor { id, .. }
        if id.as_str().starts_with(INTERNAL_FIELD_WORD)
            || id.as_str().starts_with(INTERNAL_OUTPUT_SCOPE)
            || id.as_str() == INTERNAL_LINK_SPLIT)
}

mod boundary;
mod checkpoints;
mod drain;
mod hard_breaks;
mod projection;
mod row_layout;
mod scopes;
mod words;

pub(in crate::mandoc) use projection::consume_one_row_ending;
pub(in crate::mandoc) use projection::ends_with_executed_line_break;
pub(in crate::mandoc) use projection::has_rendered_formatter_glyph;
use projection::line_break_count;
pub(in crate::mandoc) use projection::retain_inline_identities;
pub(in crate::mandoc::inline) use projection::trailing_ascii_spaces;
pub(in crate::mandoc) use projection::trailing_completed_row_origins;
pub(in crate::mandoc) use projection::trim_trailing_breakable_spaces;
pub(in crate::mandoc) use projection::{
    native_row_origin, prepare_inline_output, split_output_scope_prefix,
    strip_native_projection_markers,
};
pub(in crate::mandoc::inline::flow) mod native_passes;
mod record;
pub(in crate::mandoc::inline::flow) mod row_origins;
pub(in crate::mandoc::inline::flow) mod tab_projection;
pub(in crate::mandoc) use row_origins::{
    push_row_break, split_row_origin, take_definition_term_breaks, take_inline_layout,
};
pub(in crate::mandoc::inline::flow) mod split;

impl InlineBuilder {
    /// Already executed empty endlines in the accepted output tail.
    pub(in crate::mandoc) fn completed_empty_rows(&self) -> u16 {
        u16::try_from(projection::trailing_completed_row_origins(&self.nodes).len())
            .unwrap_or(u16::MAX)
    }
    /// Retain one actual native graph-row end in its current output owner.
    /// This is a boundary receipt, not an additional completed empty row.
    pub(in crate::mandoc) fn record_device_row_end(&mut self) {
        self.nodes.push(Inline::anchor(INTERNAL_DEVICE_ROW_END));
    }

    /// Read the accepted tail before its private execution proof is stripped.
    pub(in crate::mandoc) fn closed_graph_tail(&self) -> OutputRowEnd {
        Self::output_has_closed_graph_tail(&self.nodes)
    }

    pub(in crate::mandoc) fn output_has_closed_graph_tail(nodes: &[Inline]) -> OutputRowEnd {
        projection::trailing_device_row_end_receipt(nodes)
    }

    /// Record an executed empty row in its active output owner. A later
    /// provisional glyph cannot revoke it when native acceptance rejects
    /// that glyph; actual accepted content determines its final placement.
    pub(in crate::mandoc) fn record_completed_vertical_rows(&mut self, rows: u16) {
        self.record_completed_rows(rows, CompletedRowOrigin::Layout);
    }

    pub(in crate::mandoc) fn record_completed_rows(
        &mut self,
        rows: u16,
        origin: CompletedRowOrigin,
    ) {
        Self::record_completed_rows_at(&mut self.execution, &mut self.nodes, rows, origin);
    }

    pub(in crate::mandoc::inline::flow) fn record_completed_rows_at(
        execution: &mut super::InlineExecutionState,
        nodes: &mut Vec<Inline>,
        rows: u16,
        origin: CompletedRowOrigin,
    ) {
        execution.completed_vertical_rows = execution.completed_vertical_rows.saturating_add(rows);
        let marker = match origin {
            CompletedRowOrigin::Layout => INTERNAL_COMPLETED_ROW,
            CompletedRowOrigin::LiteralText => INTERNAL_LITERAL_ROW,
        };
        nodes.extend(std::iter::repeat_n(
            Inline::anchor(marker),
            usize::from(rows),
        ));
    }

    pub(super) fn retain_line_breaks(&mut self, count: usize) {
        if count == 0 {
            return;
        }
        self.nodes
            .extend(std::iter::repeat_n(Inline::line_break(), count));
        self.note_definition_output_row();
        self.execution.last_visible_character = Some('\n');
        self.execution.formatter_column = FormatterColumn::Origin;
        self.execution.empty_word = false;
        self.execution.trailing_output = TrailingOutput::None;
        self.execution.pending_breakable_spaces = 0;
        self.execution.pending_field_spaces = 0;
    }
}
