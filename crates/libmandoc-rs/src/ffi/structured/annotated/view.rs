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

struct CheckedDisplay<'h> {
    rows: &'h [RowView],
    runs: &'h [RunView],
    text_bytes: &'h [u8],
    text_view: &'h str,
}

impl<'h> CheckedAnnotatedView<'h> {
    /// Validate the required source/display arenas and, unless degrading,
    /// every optional annotation arena before owned transfer begins.
    pub(super) fn bind(
        handle: &'h Handle,
        view: &ResultView,
        limits: &super::super::raw::Limits,
        surface_only: bool,
    ) -> Result<Self, AnnotatedError> {
        let source_views = checked_slice::<SourceView>(handle, view.sources, limits.max_sources)?;
        let span_views = checked_slice::<SpanView>(handle, view.spans, limits.max_content_points)?;
        let provenance_views =
            checked_slice::<ProvenanceView>(handle, view.provenances, limits.max_content_points)?;
        let diagnostic_views =
            checked_slice::<DiagnosticView>(handle, view.diagnostics, limits.max_diagnostics)?;
        // A body-only transfer must not even borrow rejected mark/selection
        // descriptors. Source identities and every display byte remain hard
        // checks below, while annotation pointers are completely isolated.
        let (
            mark_views,
            selection_part_views,
            join_text_view,
            coverage_check_views,
            coverage_issue_views,
        ) = if surface_only {
            (&[][..], &[][..], &[][..], &[][..], &[][..])
        } else {
            for (count, maximum, kind) in [
                (view.marks.count, limits.max_transfer_objects, 32),
                (view.selection_parts.count, limits.max_transfer_edges, 33),
                (view.join_text.count, limits.max_content_bytes, 10),
                (view.coverage_checks.count, 24, 32),
                (view.coverage_issues.count, limits.max_transfer_objects, 32),
            ] {
                if u64::from(count) > maximum {
                    return Err(transfer_budget(kind, u64::from(count), maximum));
                }
            }
            let marks = checked_slice::<MarkView>(handle, view.marks, limits.max_transfer_objects)?;
            (
                marks,
                checked_slice::<SelectionPartView>(
                    handle,
                    view.selection_parts,
                    limits.max_transfer_edges,
                )?,
                checked_slice::<u8>(handle, view.join_text, limits.max_content_bytes)?,
                checked_slice::<CoverageCheckView>(handle, view.coverage_checks, 24)?,
                checked_slice::<CoverageIssueView>(
                    handle,
                    view.coverage_issues,
                    limits.max_transfer_objects,
                )?,
            )
        };
        let display = checked_display(handle, view, limits)?;
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
            rows: display.rows,
            runs: display.runs,
            text_bytes: display.text_bytes,
            text_view: display.text_view,
        })
    }
}

/// Display arenas are mandatory even when optional annotations are rejected.
/// Keep their aligned typed borrows tied to the still-live native handle.
fn checked_display<'h>(
    handle: &'h Handle,
    view: &ResultView,
    limits: &super::super::raw::Limits,
) -> Result<CheckedDisplay<'h>, AnnotatedError> {
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
    // bounds the arenas, and alignment is checked before these typed borrows.
    let rows = if view.display.row_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(view.display.rows, view.display.row_count as usize) }
    };
    let runs = if view.display.run_count == 0 {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(view.display.runs, view.display.run_count as usize) }
    };
    let text_bytes = checked_bytes(
        handle,
        view.display.bytes,
        view.display.byte_count,
        limits.max_content_bytes,
    )?;
    let text_view = std::str::from_utf8(text_bytes).map_err(|_| invalid_result())?;
    Ok(CheckedDisplay {
        rows,
        runs,
        text_bytes,
        text_view,
    })
}
