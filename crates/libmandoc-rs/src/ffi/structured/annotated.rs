//! Handle-bound transfer of the post-device annotated surface.

use super::super::guard::NativeSessionGuard;
use super::raw::{DiagnosticView, MetadataView, ProvenanceView, SourceView, SpanView};
use super::{
    FailureView, InputStorage, STATUS_OK, STATUS_REENTRANT, STATUS_RELATION, SliceView, raw_limits,
};
use crate::annotated::{
    AnnotatedDiagnostic, AnnotatedDisplayPoint, AnnotatedDocument, AnnotatedError, AnnotatedLabel,
    AnnotatedLinkTarget, AnnotatedMark, AnnotatedMetadata, AnnotatedProvenance, AnnotatedRow,
    AnnotatedRun, AnnotatedSelectionPart, AnnotatedSource, AnnotatedSpan, AnnotatedTextJoin,
    AnnotationCheckState, AnnotationCoverage, AnnotationCoverageCheck, AnnotationCoverageIssue,
    AnnotationDimension, AnnotationIssueReason, AnnotationProducer, AnnotationScope,
    AnnotationSourcePosition,
};
use crate::{InputFormat, SourceBundle};
use std::ptr::NonNull;

#[repr(C)]
struct ResultHandleRaw {
    _private: [u8; 0],
}

struct Handle(NonNull<ResultHandleRaw>);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { mant_annotated_result_free(self.0.as_ptr()) };
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LabelView {
    owner: u32,
    link: u32,
    source: u32,
    style: u32,
    role: u32,
    glyph_origin: u64,
    flags: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RowView {
    key: u32,
    first_run: u32,
    run_count: u32,
    column_count: u32,
    break_after: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RunView {
    key: u32,
    column: u32,
    width: u32,
    reserved: u32,
    byte_start: u64,
    byte_count: u64,
    label: LabelView,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MarkView {
    key: u32,
    kind: u32,
    parent: u32,
    owner: u32,
    source: u32,
    line: u32,
    column: u32,
    token: u32,
    region_kind: u32,
    title_region: u32,
    body_region: u32,
    flags: u32,
    reserved: u32,
    table_column: u32,
    table_position_present: u32,
    table_offset: u64,
    name: *const u8,
    name_length: u64,
    target_kind: u32,
    target_b_present: u32,
    target_a: super::BytesView,
    target_b: super::BytesView,
    selection_first: u32,
    selection_count: u32,
    point_kind: u32,
    point_row: u32,
    point_column: u32,
    point_reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SelectionPartView {
    run: u32,
    join_before: u32,
    start_byte: u64,
    end_byte: u64,
    join_text_start: u64,
    join_text_len: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CoverageCheckView {
    producer: u32,
    dimension: u32,
    state: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CoverageIssueView {
    producer: u32,
    dimension: u32,
    reason: u32,
    scope: u32,
    scope_key: u32,
    source: u32,
    line: u32,
    column: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DisplayView {
    bytes: *const u8,
    byte_count: u64,
    rows: *const RowView,
    row_count: u32,
    runs: *const RunView,
    run_count: u32,
    input_bytes: u64,
    work: u64,
    peak_allocated_bytes: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ResultView {
    root_source: u32,
    profile: u32,
    width: u32,
    reserved: u32,
    metadata: MetadataView,
    sources: SliceView,
    spans: SliceView,
    provenances: SliceView,
    diagnostics: SliceView,
    marks: SliceView,
    coverage_checks: SliceView,
    coverage_issues: SliceView,
    display: DisplayView,
    selection_parts: SliceView,
    join_text: SliceView,
}

unsafe extern "C" {
    fn mant_annotated_abi_version() -> u32;
    fn mant_annotated_render(
        input: *const super::raw::InputView,
        limits: *const super::raw::Limits,
        result: *mut *mut ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    fn mant_annotated_result_check(
        result: *const ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    fn mant_annotated_result_view(result: *const ResultHandleRaw, view: *mut ResultView) -> u32;
    fn mant_annotated_result_free(result: *mut ResultHandleRaw);
    fn mant_annotated_sizeof_result_view() -> usize;
    fn mant_annotated_alignof_result_view() -> usize;
    fn mant_annotated_offsetof_result_view_display() -> usize;
    fn mant_annotated_sizeof_display_row() -> usize;
    fn mant_annotated_alignof_display_row() -> usize;
    fn mant_annotated_offsetof_display_row_break_after() -> usize;
    fn mant_annotated_sizeof_display_run() -> usize;
    fn mant_annotated_alignof_display_run() -> usize;
    fn mant_annotated_offsetof_display_run_label() -> usize;
    fn mant_annotated_sizeof_display_label() -> usize;
    fn mant_annotated_alignof_display_label() -> usize;
    fn mant_annotated_offsetof_display_label_glyph_origin() -> usize;
    fn mant_annotated_sizeof_mark() -> usize;
    fn mant_annotated_alignof_mark() -> usize;
    fn mant_annotated_offsetof_mark_name() -> usize;
    fn mant_annotated_offsetof_mark_table_offset() -> usize;
    fn mant_annotated_offsetof_mark_target_a() -> usize;
    fn mant_annotated_offsetof_mark_selection_first() -> usize;
    fn mant_annotated_offsetof_mark_point_kind() -> usize;
    fn mant_annotated_sizeof_selection_part() -> usize;
    fn mant_annotated_alignof_selection_part() -> usize;
    fn mant_annotated_offsetof_selection_part_end_byte() -> usize;
    fn mant_annotated_offsetof_selection_part_join_text_start() -> usize;
    fn mant_annotated_sizeof_coverage_check() -> usize;
    fn mant_annotated_alignof_coverage_check() -> usize;
    fn mant_annotated_sizeof_coverage_issue() -> usize;
    fn mant_annotated_alignof_coverage_issue() -> usize;
    fn mant_annotated_offsetof_result_view_coverage_checks() -> usize;
    fn mant_annotated_offsetof_result_view_coverage_issues() -> usize;
    fn mant_annotated_offsetof_result_view_selection_parts() -> usize;
    fn mant_annotated_offsetof_result_view_join_text() -> usize;
}

fn invalid_result() -> AnnotatedError {
    AnnotatedError {
        status: STATUS_RELATION,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    }
}

fn transfer_alloc() -> AnnotatedError {
    AnnotatedError {
        status: super::STATUS_BUILDER_ALLOC,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    }
}

fn transfer_budget(kind: u32, observed: u64, allowed: u64) -> AnnotatedError {
    AnnotatedError {
        status: super::STATUS_BUDGET,
        stage: 6,
        limit_kind: kind,
        observed,
        allowed,
    }
}

fn from_native(error: &super::NativeStructuredError) -> AnnotatedError {
    AnnotatedError {
        status: error.status,
        stage: error.stage,
        limit_kind: error.limit_kind,
        observed: error.observed,
        allowed: error.allowed,
    }
}

fn checked_failure(status: u32, failure: FailureView) -> AnnotatedError {
    super::session::error_from_failure(status, failure)
        .map_or_else(|_| invalid_result(), |error| from_native(&error))
}

fn check_abi() -> bool {
    unsafe {
        mant_annotated_abi_version() == 7
            && mant_annotated_sizeof_result_view() == std::mem::size_of::<ResultView>()
            && mant_annotated_alignof_result_view() == std::mem::align_of::<ResultView>()
            && mant_annotated_offsetof_result_view_display()
                == std::mem::offset_of!(ResultView, display)
            && mant_annotated_sizeof_display_row() == std::mem::size_of::<RowView>()
            && mant_annotated_alignof_display_row() == std::mem::align_of::<RowView>()
            && mant_annotated_offsetof_display_row_break_after()
                == std::mem::offset_of!(RowView, break_after)
            && mant_annotated_sizeof_display_run() == std::mem::size_of::<RunView>()
            && mant_annotated_alignof_display_run() == std::mem::align_of::<RunView>()
            && mant_annotated_offsetof_display_run_label() == std::mem::offset_of!(RunView, label)
            && mant_annotated_sizeof_display_label() == std::mem::size_of::<LabelView>()
            && mant_annotated_alignof_display_label() == std::mem::align_of::<LabelView>()
            && mant_annotated_offsetof_display_label_glyph_origin()
                == std::mem::offset_of!(LabelView, glyph_origin)
            && mant_annotated_sizeof_mark() == std::mem::size_of::<MarkView>()
            && mant_annotated_alignof_mark() == std::mem::align_of::<MarkView>()
            && mant_annotated_offsetof_mark_name() == std::mem::offset_of!(MarkView, name)
            && mant_annotated_offsetof_mark_table_offset()
                == std::mem::offset_of!(MarkView, table_offset)
            && mant_annotated_offsetof_mark_target_a() == std::mem::offset_of!(MarkView, target_a)
            && mant_annotated_offsetof_mark_selection_first()
                == std::mem::offset_of!(MarkView, selection_first)
            && mant_annotated_offsetof_mark_point_kind()
                == std::mem::offset_of!(MarkView, point_kind)
            && mant_annotated_sizeof_selection_part() == std::mem::size_of::<SelectionPartView>()
            && mant_annotated_alignof_selection_part() == std::mem::align_of::<SelectionPartView>()
            && mant_annotated_offsetof_selection_part_end_byte()
                == std::mem::offset_of!(SelectionPartView, end_byte)
            && mant_annotated_offsetof_selection_part_join_text_start()
                == std::mem::offset_of!(SelectionPartView, join_text_start)
            && mant_annotated_sizeof_coverage_check() == std::mem::size_of::<CoverageCheckView>()
            && mant_annotated_alignof_coverage_check() == std::mem::align_of::<CoverageCheckView>()
            && mant_annotated_sizeof_coverage_issue() == std::mem::size_of::<CoverageIssueView>()
            && mant_annotated_alignof_coverage_issue() == std::mem::align_of::<CoverageIssueView>()
            && mant_annotated_offsetof_result_view_coverage_checks()
                == std::mem::offset_of!(ResultView, coverage_checks)
            && mant_annotated_offsetof_result_view_coverage_issues()
                == std::mem::offset_of!(ResultView, coverage_issues)
            && mant_annotated_offsetof_result_view_selection_parts()
                == std::mem::offset_of!(ResultView, selection_parts)
            && mant_annotated_offsetof_result_view_join_text()
                == std::mem::offset_of!(ResultView, join_text)
    }
}

fn checked_slice<T>(
    _handle: &Handle,
    slice: SliceView,
    maximum: u64,
) -> Result<&[T], AnnotatedError> {
    if slice.stride as usize != std::mem::size_of::<T>()
        || u64::from(slice.count) > maximum
        || (slice.count == 0) != slice.ptr.is_null()
    {
        return Err(invalid_result());
    }
    if slice.count == 0 {
        return Ok(&[]);
    }
    let pointer = slice.ptr.cast::<T>();
    let bytes = (slice.count as usize)
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(invalid_result)?;
    if !(pointer as usize).is_multiple_of(std::mem::align_of::<T>()) || bytes > isize::MAX as usize
    {
        return Err(invalid_result());
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, slice.count as usize) })
}

fn checked_bytes(
    _handle: &Handle,
    pointer: *const u8,
    length: u64,
    maximum: u64,
) -> Result<&[u8], AnnotatedError> {
    if length > maximum || (length == 0) != pointer.is_null() {
        return Err(invalid_result());
    }
    if length == 0 {
        return Ok(&[]);
    }
    let length = usize::try_from(length).map_err(|_| invalid_result())?;
    if length > isize::MAX as usize {
        return Err(invalid_result());
    }
    Ok(unsafe { std::slice::from_raw_parts(pointer, length) })
}

fn copy_string(
    handle: &Handle,
    view: super::BytesView,
    max: u64,
) -> Result<String, AnnotatedError> {
    let bytes = checked_bytes(handle, view.ptr, view.len, max)?;
    let text = std::str::from_utf8(bytes).map_err(|_| invalid_result())?;
    let mut owned = String::new();
    owned
        .try_reserve(text.len())
        .map_err(|_| transfer_alloc())?;
    owned.push_str(text);
    Ok(owned)
}

fn copy_optional(
    handle: &Handle,
    metadata: MetadataView,
    bit: u32,
    view: super::BytesView,
    max: u64,
) -> Result<Option<String>, AnnotatedError> {
    if metadata.presence_flags & bit == 0 {
        if !view.ptr.is_null() || view.len != 0 {
            return Err(invalid_result());
        }
        return Ok(None);
    }
    Ok(Some(copy_string(handle, view, max)?))
}

fn reserve<T>(count: usize) -> Result<Vec<T>, AnnotatedError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| transfer_alloc())?;
    Ok(result)
}

fn coverage_producer(value: u32) -> Result<AnnotationProducer, AnnotatedError> {
    match value {
        1 => Ok(AnnotationProducer::Native),
        2 => Ok(AnnotationProducer::Codec),
        3 => Ok(AnnotationProducer::Validator),
        _ => Err(invalid_result()),
    }
}

fn coverage_dimension(value: u32) -> Result<AnnotationDimension, AnnotatedError> {
    match value {
        1 => Ok(AnnotationDimension::Section),
        2 => Ok(AnnotationDimension::OwnerBoundary),
        3 => Ok(AnnotationDimension::Declaration),
        4 => Ok(AnnotationDimension::Link),
        5 => Ok(AnnotationDimension::Anchor),
        6 => Ok(AnnotationDimension::Relation),
        7 => Ok(AnnotationDimension::Source),
        8 => Ok(AnnotationDimension::Join),
        _ => Err(invalid_result()),
    }
}

fn coverage_state(value: u32) -> Result<AnnotationCheckState, AnnotatedError> {
    match value {
        1 => Ok(AnnotationCheckState::Checked),
        2 => Ok(AnnotationCheckState::NotApplicable),
        3 => Ok(AnnotationCheckState::Unverified),
        4 => Ok(AnnotationCheckState::Pending),
        _ => Err(invalid_result()),
    }
}

fn coverage_reason(value: u32) -> Result<AnnotationIssueReason, AnnotatedError> {
    match value {
        1 => Ok(AnnotationIssueReason::NotObserved),
        2 => Ok(AnnotationIssueReason::Unverified),
        3 => Ok(AnnotationIssueReason::Rejected),
        4 => Ok(AnnotationIssueReason::AmbiguousSurvival),
        _ => Err(invalid_result()),
    }
}

fn coverage_issue_location(
    issue: &CoverageIssueView,
    marks: &[AnnotatedMark],
    sources: &[AnnotatedSource],
) -> Result<(AnnotationScope, Option<AnnotationSourcePosition>), AnnotatedError> {
    let scope = match issue.scope {
        1 if issue.scope_key == 0 => AnnotationScope::Document,
        2..=4 => {
            let mark = issue
                .scope_key
                .checked_sub(1)
                .and_then(|index| marks.get(index as usize))
                .ok_or_else(invalid_result)?;
            // Scope tags are 2/3/4; native heading/owner/region kinds are 1/2/5.
            let expected = match issue.scope {
                2 => 1,
                3 => 2,
                _ => 5,
            };
            if mark.key != issue.scope_key || mark.kind != expected {
                return Err(invalid_result());
            }
            match issue.scope {
                2 => AnnotationScope::Section(issue.scope_key),
                3 => AnnotationScope::Owner(issue.scope_key),
                _ => AnnotationScope::Region(issue.scope_key),
            }
        }
        5 if issue.scope_key != 0 && (issue.scope_key as usize) <= sources.len() => {
            AnnotationScope::Source(issue.scope_key)
        }
        _ => return Err(invalid_result()),
    };
    let source = if issue.source == 0 && issue.line == 0 && issue.column == 0 {
        None
    } else if issue.source != 0
        && (issue.source as usize) <= sources.len()
        && issue.line != 0
        && issue.column != 0
    {
        Some(AnnotationSourcePosition {
            source: issue.source,
            line: issue.line,
            column: issue.column,
        })
    } else {
        return Err(invalid_result());
    };
    if let (AnnotationScope::Source(scope_key), Some(position)) = (scope, source)
        && scope_key != position.source
    {
        return Err(invalid_result());
    }
    Ok((scope, source))
}

fn transfer_coverage(
    checks: &[CoverageCheckView],
    issues: &[CoverageIssueView],
    marks: &[AnnotatedMark],
    sources: &[AnnotatedSource],
) -> Result<AnnotationCoverage, AnnotatedError> {
    if checks.len() != 24 {
        return Err(invalid_result());
    }
    let mut states = [[None; 8]; 3];
    let mut owned_checks = reserve(checks.len())?;
    for check in checks {
        let producer = coverage_producer(check.producer)?;
        let dimension = coverage_dimension(check.dimension)?;
        let state = coverage_state(check.state)?;
        if check.reserved != 0
            || (producer == AnnotationProducer::Native && state == AnnotationCheckState::Pending)
        {
            return Err(invalid_result());
        }
        let slot = &mut states[check.producer as usize - 1][check.dimension as usize - 1];
        if slot.replace(state).is_some() {
            return Err(invalid_result());
        }
        owned_checks.push(AnnotationCoverageCheck {
            producer,
            dimension,
            state,
        });
    }
    let mut owned_issues = reserve(issues.len())?;
    let mut seen = [[false; 8]; 3];
    for issue in issues {
        let producer = coverage_producer(issue.producer)?;
        let dimension = coverage_dimension(issue.dimension)?;
        let reason = coverage_reason(issue.reason)?;
        if states[issue.producer as usize - 1][issue.dimension as usize - 1]
            != Some(AnnotationCheckState::Unverified)
        {
            return Err(invalid_result());
        }
        let (scope, source) = coverage_issue_location(issue, marks, sources)?;
        seen[issue.producer as usize - 1][issue.dimension as usize - 1] = true;
        owned_issues.push(AnnotationCoverageIssue {
            producer,
            dimension,
            reason,
            scope,
            source,
        });
    }
    if states.iter().zip(seen).any(|(row, seen_row)| {
        row.iter().zip(seen_row).any(|(state, seen_issue)| {
            state.is_none() || (*state == Some(AnnotationCheckState::Unverified)) != seen_issue
        })
    }) {
        return Err(invalid_result());
    }
    Ok(AnnotationCoverage {
        checks: owned_checks,
        issues: owned_issues,
    })
}

#[cfg(test)]
mod coverage_tests;

pub(crate) fn render_annotated(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    width: u32,
    options: &crate::structured::StructuredLimits,
) -> Result<AnnotatedDocument, AnnotatedError> {
    let _guard = NativeSessionGuard::enter().map_err(|_| AnnotatedError {
        status: STATUS_REENTRANT,
        stage: 1,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?;
    if !check_abi() {
        return Err(invalid_result());
    }
    let limits = raw_limits(options);
    let storage =
        InputStorage::new(root, bundle, format, &limits).map_err(|error| from_native(&error))?;
    let input = storage.view(width, super::PROFILE_UTF8);
    let mut pointer = std::ptr::null_mut();
    let mut failure = FailureView::default();
    let status = unsafe {
        mant_annotated_render(
            &raw const input,
            &raw const limits,
            &raw mut pointer,
            &raw mut failure,
        )
    };
    if status != STATUS_OK {
        if !pointer.is_null() {
            unsafe { mant_annotated_result_free(pointer) };
        }
        return Err(checked_failure(status, failure));
    }
    let handle = Handle(NonNull::new(pointer).ok_or_else(invalid_result)?);
    let mut failure = FailureView::default();
    let status = unsafe { mant_annotated_result_check(handle.0.as_ptr(), &raw mut failure) };
    if status != STATUS_OK {
        return Err(checked_failure(status, failure));
    }
    let mut view = ResultView::default();
    if unsafe { mant_annotated_result_view(handle.0.as_ptr(), &raw mut view) } != STATUS_OK
        || view.reserved != 0
        || view.root_source != 1
        || view.width != width
        || view.profile != super::PROFILE_UTF8
    {
        return Err(invalid_result());
    }
    transfer(&handle, &view, &limits)
}

#[allow(clippy::too_many_lines)] // Mirrors the checked one-copy wire transfer.
fn transfer(
    handle: &Handle,
    view: &ResultView,
    limits: &super::raw::Limits,
) -> Result<AnnotatedDocument, AnnotatedError> {
    let source_views = checked_slice::<SourceView>(handle, view.sources, limits.max_sources)?;
    let span_views = checked_slice::<SpanView>(handle, view.spans, limits.max_content_points)?;
    let provenance_views =
        checked_slice::<ProvenanceView>(handle, view.provenances, limits.max_content_points)?;
    let diagnostic_views =
        checked_slice::<DiagnosticView>(handle, view.diagnostics, limits.max_diagnostics)?;
    let mark_views = checked_slice::<MarkView>(handle, view.marks, limits.max_transfer_objects)?;
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
    let object_count = [
        source_views.len(),
        span_views.len(),
        provenance_views.len(),
        diagnostic_views.len(),
        mark_views.len(),
        selection_part_views.len(),
        coverage_check_views.len(),
        coverage_issue_views.len(),
        rows.len(),
        runs.len(),
    ]
    .into_iter()
    .try_fold(2_u64, |sum, count| {
        sum.checked_add(count as u64).ok_or_else(invalid_result)
    })?;
    if object_count > limits.max_transfer_objects {
        return Err(transfer_budget(
            32,
            object_count,
            limits.max_transfer_objects,
        ));
    }
    let mut transfer_bytes = 0_u64;
    let mut charge = |amount: u64| -> Result<(), AnnotatedError> {
        transfer_bytes = transfer_bytes
            .checked_add(amount)
            .ok_or_else(invalid_result)?;
        if transfer_bytes > limits.max_transfer_bytes {
            return Err(transfer_budget(
                34,
                transfer_bytes,
                limits.max_transfer_bytes,
            ));
        }
        Ok(())
    };
    for (count, item_size) in [
        (source_views.len(), std::mem::size_of::<AnnotatedSource>()),
        (span_views.len(), std::mem::size_of::<AnnotatedSpan>()),
        (
            provenance_views.len(),
            std::mem::size_of::<AnnotatedProvenance>(),
        ),
        (
            diagnostic_views.len(),
            std::mem::size_of::<AnnotatedDiagnostic>(),
        ),
        (mark_views.len(), std::mem::size_of::<AnnotatedMark>()),
        (
            selection_part_views.len(),
            std::mem::size_of::<AnnotatedSelectionPart>(),
        ),
        (
            coverage_check_views.len(),
            std::mem::size_of::<AnnotationCoverageCheck>(),
        ),
        (
            coverage_issue_views.len(),
            std::mem::size_of::<AnnotationCoverageIssue>(),
        ),
        (rows.len(), std::mem::size_of::<AnnotatedRow>()),
        (runs.len(), std::mem::size_of::<AnnotatedRun>()),
    ] {
        charge(
            (count as u64)
                .checked_mul(item_size as u64)
                .ok_or_else(invalid_result)?,
        )?;
    }
    charge(std::mem::size_of::<AnnotatedDocument>() as u64)?;
    charge(std::mem::size_of::<AnnotatedMetadata>() as u64)?;
    charge(text_bytes.len() as u64)?;
    charge(join_text_view.len() as u64)?;
    // Temporary one-byte-per-run census prevents missing or duplicate
    // direct owner/link selections from masquerading as a complete transfer.
    charge(runs.len() as u64)?;
    for field in [
        view.metadata.title,
        view.metadata.section,
        view.metadata.volume,
        view.metadata.operating_system,
        view.metadata.architecture,
        view.metadata.name,
        view.metadata.date,
        view.metadata.alias_target,
    ] {
        charge(field.len)?;
    }
    for source in source_views {
        charge(source.logical_name.len)?;
    }
    for diagnostic in diagnostic_views {
        charge(diagnostic.message.len)?;
    }
    for mark in mark_views {
        charge(mark.name_length)?;
        charge(mark.target_a.len)?;
        charge(mark.target_b.len)?;
    }
    let metadata = view.metadata;
    if metadata.reserved != 0 || metadata.reserved_bytes != [0; 3] || metadata.has_body > 1 {
        return Err(invalid_result());
    }
    let metadata = AnnotatedMetadata {
        macroset: metadata.macroset,
        title: copy_optional(
            handle,
            metadata,
            1,
            metadata.title,
            limits.max_content_bytes,
        )?,
        section: copy_optional(
            handle,
            metadata,
            2,
            metadata.section,
            limits.max_content_bytes,
        )?,
        volume: copy_optional(
            handle,
            metadata,
            4,
            metadata.volume,
            limits.max_content_bytes,
        )?,
        operating_system: copy_optional(
            handle,
            metadata,
            8,
            metadata.operating_system,
            limits.max_content_bytes,
        )?,
        architecture: copy_optional(
            handle,
            metadata,
            16,
            metadata.architecture,
            limits.max_content_bytes,
        )?,
        name: copy_optional(
            handle,
            metadata,
            32,
            metadata.name,
            limits.max_content_bytes,
        )?,
        date: copy_optional(
            handle,
            metadata,
            64,
            metadata.date,
            limits.max_content_bytes,
        )?,
        alias_target: copy_optional(
            handle,
            metadata,
            128,
            metadata.alias_target,
            limits.max_content_bytes,
        )?,
        has_body: metadata.has_body != 0,
    };
    let mut sources = reserve(source_views.len())?;
    for (index, source) in source_views.iter().enumerate() {
        if source.key != u32::try_from(index + 1).map_err(|_| invalid_result())?
            || source.reserved != 0
            || source.reserved_bytes != [0; 7]
            || source.hash_present > 1
        {
            return Err(invalid_result());
        }
        sources.push(AnnotatedSource {
            key: source.key,
            identity_kind: source.identity_kind,
            format: source.format,
            coordinate_kind: source.coordinate_kind,
            logical_name: copy_string(handle, source.logical_name, limits.max_source_path_bytes)?,
            decoded_length: source.decoded_length,
            hash: (source.hash_present != 0).then_some(source.hash),
        });
    }
    let mut spans = reserve(span_views.len())?;
    for span in span_views {
        if span.reserved != 0
            || span.reserved_bytes != [0; 2]
            || span.source == 0
            || span.source as usize > sources.len()
            || span.byte_range_present != 0
        {
            return Err(invalid_result());
        }
        spans.push(AnnotatedSpan {
            source: span.source,
            line_column: (span.line_column_present != 0).then_some((
                span.line_start,
                span.column_start,
                span.line_end,
                span.column_end,
            )),
        });
    }
    let mut provenances = reserve(provenance_views.len())?;
    for provenance in provenance_views {
        if provenance.reserved != 0
            || !(1..=3).contains(&provenance.kind)
            || provenance.authored_span as usize > spans.len()
            || provenance.generated_trigger_span as usize > spans.len()
        {
            return Err(invalid_result());
        }
        provenances.push(AnnotatedProvenance {
            kind: provenance.kind,
            authored_span: provenance.authored_span,
            generated_trigger_span: provenance.generated_trigger_span,
        });
    }
    let mut diagnostics = reserve(diagnostic_views.len())?;
    for diagnostic in diagnostic_views {
        if diagnostic.reserved != 0
            || diagnostic.owner != 0
            || diagnostic.span as usize > spans.len()
        {
            return Err(invalid_result());
        }
        diagnostics.push(AnnotatedDiagnostic {
            level: diagnostic.level,
            code: diagnostic.code,
            message: copy_string(handle, diagnostic.message, limits.max_content_bytes)?,
            span: diagnostic.span,
        });
    }
    let mut marks = reserve(mark_views.len())?;
    for (index, mark) in mark_views.iter().enumerate() {
        if mark.key != u32::try_from(index + 1).map_err(|_| invalid_result())?
            || mark.parent >= mark.key
            || mark.owner >= mark.key
            || mark.source as usize > sources.len()
            || mark.reserved != 0
            || mark.point_reserved != 0
            || mark.flags & !1 != 0
        {
            return Err(invalid_result());
        }
        let point = match mark.point_kind {
            0 if mark.point_row == 0 && mark.point_column == 0 && mark.kind != 4 => None,
            1 if (mark.kind == 2 || mark.kind == 4) && mark.point_row != 0 => {
                let row = rows
                    .get(usize::try_from(mark.point_row - 1).map_err(|_| invalid_result())?)
                    .ok_or_else(invalid_result)?;
                if mark.point_column > row.column_count {
                    return Err(invalid_result());
                }
                Some(AnnotatedDisplayPoint::RowColumn {
                    row: mark.point_row,
                    column: mark.point_column,
                })
            }
            2 if (mark.kind == 2 || mark.kind == 4)
                && mark.point_row == view.display.row_count
                && mark.point_column == 0 =>
            {
                Some(AnnotatedDisplayPoint::DocumentEnd {
                    row_count: mark.point_row,
                })
            }
            _ => return Err(invalid_result()),
        };
        let native_table_position = if mark.kind == 5 && mark.region_kind == 9 {
            if mark.table_position_present != 1
                || mark.parent == 0
                || mark.owner != mark.parent
                || mark.line != 0
                || mark.column != 0
                || mark.flags & 1 != 0
                || marks
                    .get(usize::try_from(mark.parent - 1).map_err(|_| invalid_result())?)
                    .is_none_or(|parent: &AnnotatedMark| {
                        parent.kind != 5 || parent.region_kind != 7
                    })
            {
                return Err(invalid_result());
            }
            Some((mark.table_column, mark.table_offset))
        } else {
            if mark.table_column != 0 || mark.table_position_present != 0 || mark.table_offset != 0
            {
                return Err(invalid_result());
            }
            None
        };
        marks.push(AnnotatedMark {
            key: mark.key,
            kind: mark.kind,
            parent: mark.parent,
            owner: mark.owner,
            source: mark.source,
            line: mark.line,
            column: mark.column,
            token: mark.token,
            region_kind: mark.region_kind,
            title_region: mark.title_region,
            body_region: mark.body_region,
            flags: mark.flags,
            selection_first: mark.selection_first,
            selection_count: mark.selection_count,
            point,
            native_table_position,
            name: if mark.kind == 4 {
                Some(copy_string(
                    handle,
                    super::BytesView {
                        ptr: mark.name,
                        len: mark.name_length,
                    },
                    limits.max_content_bytes,
                )?)
            } else {
                if !mark.name.is_null() || mark.name_length != 0 {
                    return Err(invalid_result());
                }
                None
            },
            link_target: if mark.kind == 3 {
                if mark.target_kind > 5
                    || mark.target_b_present > 1
                    || (mark.target_b_present == 1) != (mark.target_kind == 4)
                {
                    return Err(invalid_result());
                }
                if mark.target_kind == 0 {
                    if !mark.target_a.ptr.is_null()
                        || mark.target_a.len != 0
                        || !mark.target_b.ptr.is_null()
                        || mark.target_b.len != 0
                    {
                        return Err(invalid_result());
                    }
                    None
                } else {
                    let primary = copy_string(handle, mark.target_a, limits.max_content_bytes)?;
                    if primary.is_empty() && mark.target_kind >= 3 {
                        return Err(invalid_result());
                    }
                    let secondary = if mark.target_b_present == 1 {
                        let value = copy_string(handle, mark.target_b, limits.max_content_bytes)?;
                        if value.is_empty() {
                            return Err(invalid_result());
                        }
                        Some(value)
                    } else {
                        if !mark.target_b.ptr.is_null() || mark.target_b.len != 0 {
                            return Err(invalid_result());
                        }
                        None
                    };
                    Some(AnnotatedLinkTarget {
                        kind: mark.target_kind,
                        primary,
                        secondary,
                    })
                }
            } else {
                if mark.target_kind != 0
                    || mark.target_b_present != 0
                    || !mark.target_a.ptr.is_null()
                    || mark.target_a.len != 0
                    || !mark.target_b.ptr.is_null()
                    || mark.target_b.len != 0
                {
                    return Err(invalid_result());
                }
                None
            },
        });
    }
    let coverage = transfer_coverage(coverage_check_views, coverage_issue_views, &marks, &sources)?;
    let mut owned_rows = reserve(rows.len())?;
    for (index, row) in rows.iter().enumerate() {
        if row.key != u32::try_from(index + 1).map_err(|_| invalid_result())? || row.break_after > 1
        {
            return Err(invalid_result());
        }
        owned_rows.push(AnnotatedRow {
            key: row.key,
            first_run: row.first_run,
            run_count: row.run_count,
            column_count: row.column_count,
            break_after: row.break_after != 0,
        });
    }
    let mut owned_runs = reserve(runs.len())?;
    for (index, run) in runs.iter().enumerate() {
        let end = run
            .byte_start
            .checked_add(run.byte_count)
            .ok_or_else(invalid_result)?;
        let start_usize = usize::try_from(run.byte_start).map_err(|_| invalid_result())?;
        let end_usize = usize::try_from(end).map_err(|_| invalid_result())?;
        if run.key != u32::try_from(index + 1).map_err(|_| invalid_result())?
            || run.reserved != 0
            || run.label.reserved != 0
            || run.label.glyph_origin != 0
            || run.label.flags != 0
            || end > view.display.byte_count
            || run.byte_count == 0
            || run.label.source as usize > sources.len()
            || run.label.owner as usize > marks.len()
            || run.label.link as usize > marks.len()
            || (run.label.role != 1 && run.label.role != 4 && run.label.role != 5)
            || !text_view.is_char_boundary(start_usize)
            || !text_view.is_char_boundary(end_usize)
            || (run.label.role == 5
                && (run.label.owner != 0
                    || run.label.link != 0
                    || run.label.source != 0
                    || run.label.style != 0
                    || !text_view[start_usize..end_usize]
                        .bytes()
                        .all(|byte| byte == b' ')))
        {
            return Err(invalid_result());
        }
        owned_runs.push(AnnotatedRun {
            key: run.key,
            column: run.column,
            width: run.width,
            byte_start: run.byte_start,
            byte_count: run.byte_count,
            label: AnnotatedLabel {
                owner: run.label.owner,
                link: run.label.link,
                source: run.label.source,
                style: run.label.style,
                role: run.label.role,
            },
        });
    }
    let mut selection_parts = reserve(selection_part_views.len())?;
    let mut selected = reserve::<u8>(runs.len())?;
    selected.resize(runs.len(), 0);
    let mut next_part = 0_usize;
    let mut next_join_text = 0_usize;
    for mark in &marks {
        let first = usize::try_from(mark.selection_first).map_err(|_| invalid_result())?;
        let count = usize::try_from(mark.selection_count).map_err(|_| invalid_result())?;
        let end = first.checked_add(count).ok_or_else(invalid_result)?;
        if first != next_part || end > selection_part_views.len() || (mark.kind == 4 && count != 0)
        {
            return Err(invalid_result());
        }
        let mut previous_run = 0_u32;
        for (index, part) in selection_part_views[first..end].iter().enumerate() {
            if part.run == 0 || part.run <= previous_run {
                return Err(invalid_result());
            }
            let run_index = usize::try_from(part.run - 1).map_err(|_| invalid_result())?;
            let run = owned_runs.get(run_index).ok_or_else(invalid_result)?;
            let (bit, direct_key) = if mark.kind == 3 {
                (2_u8, run.label.link)
            } else if mark.kind == 1 || mark.kind == 2 || mark.kind == 5 {
                (1_u8, run.label.owner)
            } else {
                return Err(invalid_result());
            };
            let join_before = match (index, part.join_before) {
                (0, 0) => AnnotatedTextJoin::None,
                (1.., 1) => AnnotatedTextJoin::DirectContact,
                (1.., 2) => AnnotatedTextJoin::AuthoredSeparator,
                (1.., 3) => AnnotatedTextJoin::HardBoundary,
                (1.., 4) => AnnotatedTextJoin::Unknown,
                _ => return Err(invalid_result()),
            };
            if join_before == AnnotatedTextJoin::AuthoredSeparator {
                let join_start =
                    usize::try_from(part.join_text_start).map_err(|_| invalid_result())?;
                let join_len = usize::try_from(part.join_text_len).map_err(|_| invalid_result())?;
                let join_end = join_start
                    .checked_add(join_len)
                    .ok_or_else(invalid_result)?;
                if join_len == 0
                    || join_start != next_join_text
                    || join_text_view
                        .get(join_start..join_end)
                        .is_none_or(|bytes| bytes.iter().any(|byte| *byte != b' '))
                {
                    return Err(invalid_result());
                }
                next_join_text = join_end;
            } else if part.join_text_start != 0 || part.join_text_len != 0 {
                return Err(invalid_result());
            }
            let start = run
                .byte_start
                .checked_add(part.start_byte)
                .ok_or_else(invalid_result)?;
            let finish = run
                .byte_start
                .checked_add(part.end_byte)
                .ok_or_else(invalid_result)?;
            if direct_key != mark.key
                || part.start_byte != 0
                || part.end_byte != run.byte_count
                || start >= finish
                || finish > view.display.byte_count
                || !text_view
                    .is_char_boundary(usize::try_from(start).map_err(|_| invalid_result())?)
                || !text_view
                    .is_char_boundary(usize::try_from(finish).map_err(|_| invalid_result())?)
                || selected[run_index] & bit != 0
            {
                return Err(invalid_result());
            }
            selected[run_index] |= bit;
            selection_parts.push(AnnotatedSelectionPart {
                run: part.run,
                start_byte: part.start_byte,
                end_byte: part.end_byte,
                join_before,
                join_text_start: part.join_text_start,
                join_text_len: part.join_text_len,
            });
            previous_run = part.run;
        }
        next_part = end;
    }
    if next_part != selection_part_views.len() || next_join_text != join_text_view.len() {
        return Err(invalid_result());
    }
    for (index, run) in owned_runs.iter().enumerate() {
        let expected = u8::from(run.label.owner != 0) | (u8::from(run.label.link != 0) << 1);
        if selected[index] != expected {
            return Err(invalid_result());
        }
    }
    let mut text = String::new();
    text.try_reserve_exact(text_view.len())
        .map_err(|_| transfer_alloc())?;
    text.push_str(text_view);
    let mut join_text = String::new();
    join_text
        .try_reserve_exact(join_text_view.len())
        .map_err(|_| transfer_alloc())?;
    // Every byte was checked against ASCII space while validating the exact
    // authored-separator partition above.
    join_text.push_str(std::str::from_utf8(join_text_view).map_err(|_| invalid_result())?);
    Ok(AnnotatedDocument {
        root_source: view.root_source,
        profile: view.profile,
        width: view.width,
        metadata,
        sources,
        spans,
        provenances,
        diagnostics,
        text,
        rows: owned_rows,
        runs: owned_runs,
        marks,
        selection_parts,
        join_text,
        coverage,
    })
}
