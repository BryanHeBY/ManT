//! Handle-bound checked slices of the native annotated result.

use super::{
    AnnotatedError, CoverageCheckView, CoverageIssueView, DiagnosticView, Handle, MarkView,
    ProvenanceView, ResultView, RowView, RunView, SelectionPartView, SourceView, SpanView,
    checked_bytes, checked_slice, invalid_result, transfer_budget,
};

/// Borrowed data cannot outlive the sole native handle.
pub(super) struct CheckedAnnotatedView<'h> {
    pub(super) source_views: &'h [SourceView],
    pub(super) span_views: &'h [SpanView],
    pub(super) provenance_views: &'h [ProvenanceView],
    pub(super) diagnostic_views: &'h [DiagnosticView],
    pub(super) mark_views: &'h [MarkView],
    pub(super) selection_part_views: &'h [SelectionPartView],
    pub(super) join_text_view: &'h [u8],
    pub(super) coverage_check_views: &'h [CoverageCheckView],
    pub(super) coverage_issue_views: &'h [CoverageIssueView],
    pub(super) rows: &'h [RowView],
    pub(super) runs: &'h [RunView],
    pub(super) text_bytes: &'h [u8],
    pub(super) text_view: &'h str,
}

impl<'h> CheckedAnnotatedView<'h> {
    /// Validate every fixed-size result arena before owned transfer begins.
    pub(super) fn bind(
        handle: &'h Handle,
        view: &ResultView,
        limits: &super::super::raw::Limits,
    ) -> Result<Self, AnnotatedError> {
        let source_views = checked_slice::<SourceView>(handle, view.sources, limits.max_sources)?;
        let span_views = checked_slice::<SpanView>(handle, view.spans, limits.max_content_points)?;
        let provenance_views =
            checked_slice::<ProvenanceView>(handle, view.provenances, limits.max_content_points)?;
        let diagnostic_views =
            checked_slice::<DiagnosticView>(handle, view.diagnostics, limits.max_diagnostics)?;
        let mark_views =
            checked_slice::<MarkView>(handle, view.marks, limits.max_transfer_objects)?;
        if u64::from(view.selection_parts.count) > limits.max_transfer_edges {
            return Err(transfer_budget(
                33,
                u64::from(view.selection_parts.count),
                limits.max_transfer_edges,
            ));
        }
        let selection_part_views = checked_slice::<SelectionPartView>(
            handle,
            view.selection_parts,
            limits.max_transfer_edges,
        )?;
        let join_text_view = checked_slice::<u8>(handle, view.join_text, limits.max_content_bytes)?;
        let coverage_check_views =
            checked_slice::<CoverageCheckView>(handle, view.coverage_checks, 24)?;
        let coverage_issue_views = checked_slice::<CoverageIssueView>(
            handle,
            view.coverage_issues,
            limits.max_transfer_objects,
        )?;
        let row_views = checked_bytes(
            handle,
            view.display.rows.cast::<u8>(),
            u64::from(view.display.row_count) * std::mem::size_of::<RowView>() as u64,
            limits.max_transfer_bytes,
        )?;
        let run_views = checked_bytes(
            handle,
            view.display.runs.cast::<u8>(),
            u64::from(view.display.run_count) * std::mem::size_of::<RunView>() as u64,
            limits.max_transfer_bytes,
        )?;
        if !row_views
            .len()
            .is_multiple_of(std::mem::size_of::<RowView>())
            || !run_views
                .len()
                .is_multiple_of(std::mem::size_of::<RunView>())
            || !(view.display.rows as usize).is_multiple_of(std::mem::align_of::<RowView>())
            || !(view.display.runs as usize).is_multiple_of(std::mem::align_of::<RunView>())
            || u64::from(view.display.row_count) > limits.max_fixed_lines
            || u64::from(view.display.run_count) > limits.max_annotation_runs
        {
            return Err(invalid_result());
        }
        // The native result check establishes handle ownership; checked_bytes
        // bounds the arenas, and the alignment checks above precede these
        // typed borrows. Their lifetime is tied to the still-live handle.
        let rows = if view.display.row_count == 0 {
            &[][..]
        } else {
            unsafe {
                std::slice::from_raw_parts(view.display.rows, view.display.row_count as usize)
            }
        };
        let runs = if view.display.run_count == 0 {
            &[][..]
        } else {
            unsafe {
                std::slice::from_raw_parts(view.display.runs, view.display.run_count as usize)
            }
        };
        let text_bytes = checked_bytes(
            handle,
            view.display.bytes,
            view.display.byte_count,
            limits.max_content_bytes,
        )?;
        let text_view = std::str::from_utf8(text_bytes).map_err(|_| invalid_result())?;
        Ok(Self {
            source_views,
            span_views,
            provenance_views,
            diagnostic_views,
            mark_views,
            selection_part_views,
            join_text_view,
            coverage_check_views,
            coverage_issue_views,
            rows,
            runs,
            text_bytes,
            text_view,
        })
    }
}
