//! Handle-bound transfer of the post-device annotated surface.

use super::super::guard::NativeSessionGuard;
use super::raw::{DiagnosticView, MetadataView, ProvenanceView, SourceView, SpanView};
use super::{
    FORMAT_MAN, FORMAT_MDOC, FailureView, InputStorage, STATUS_OK, STATUS_REENTRANT,
    STATUS_RELATION, SliceView, raw_limits,
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

mod raw;
use raw::{
    CoverageCheckView, CoverageIssueView, LabelView, MarkView, ResultHandleRaw, ResultView,
    RowView, RunView, SelectionPartView, mant_annotated_abi_version,
    mant_annotated_alignof_coverage_check, mant_annotated_alignof_coverage_issue,
    mant_annotated_alignof_display_label, mant_annotated_alignof_display_row,
    mant_annotated_alignof_display_run, mant_annotated_alignof_mark,
    mant_annotated_alignof_result_view, mant_annotated_alignof_selection_part,
    mant_annotated_offsetof_display_label_glyph_origin,
    mant_annotated_offsetof_display_label_head_component,
    mant_annotated_offsetof_display_row_break_after, mant_annotated_offsetof_display_run_label,
    mant_annotated_offsetof_mark_name, mant_annotated_offsetof_mark_point_kind,
    mant_annotated_offsetof_mark_selection_first, mant_annotated_offsetof_mark_table_offset,
    mant_annotated_offsetof_mark_target_a, mant_annotated_offsetof_result_view_coverage_checks,
    mant_annotated_offsetof_result_view_coverage_issues,
    mant_annotated_offsetof_result_view_display, mant_annotated_offsetof_result_view_join_text,
    mant_annotated_offsetof_result_view_selection_parts,
    mant_annotated_offsetof_selection_part_end_byte,
    mant_annotated_offsetof_selection_part_join_text_start, mant_annotated_render,
    mant_annotated_result_free, mant_annotated_result_view_sealed,
    mant_annotated_sizeof_coverage_check, mant_annotated_sizeof_coverage_issue,
    mant_annotated_sizeof_display_label, mant_annotated_sizeof_display_row,
    mant_annotated_sizeof_display_run, mant_annotated_sizeof_mark,
    mant_annotated_sizeof_result_view, mant_annotated_sizeof_selection_part,
};
#[cfg(test)]
use raw::{mant_annotated_result_check, mant_annotated_result_view};

struct Handle(NonNull<ResultHandleRaw>);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { mant_annotated_result_free(self.0.as_ptr()) };
    }
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
        mant_annotated_abi_version() == 12
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
            && mant_annotated_offsetof_display_label_head_component()
                == std::mem::offset_of!(LabelView, head_component)
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

mod coverage;
use coverage::transfer_coverage;

#[cfg(test)]
mod coverage_tests;
#[cfg(test)]
mod isolation_tests;

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
    let storage = InputStorage::new_annotated(root, bundle, format, &limits)
        .map_err(|error| from_native(&error))?;
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
    let mut view = ResultView::default();
    // render_session() made its only full final validation before sealing and
    // transferring this private handle. The C borrow path rejects an unsealed
    // or unfinished display; explicit check/view remain deep for C callers.
    if unsafe { mant_annotated_result_view_sealed(handle.0.as_ptr(), &raw mut view) } != STATUS_OK
        || view.annotation_degraded > 1
        || view.root_source != 1
        || view.width != width
        || view.profile != super::PROFILE_UTF8
    {
        return Err(invalid_result());
    }
    match transfer(&handle, &view, &limits, false) {
        Ok(page) => Ok(page),
        Err(error) if error.status == STATUS_RELATION => {
            // The native handle already passed its independent complete-body
            // check. Recheck hard source/surface relations while discarding
            // only the optional native marks and their actionable labels.
            transfer(&handle, &view, &limits, true)
        }
        Err(error) => Err(error),
    }
}

mod transfer;
use transfer::transfer;
mod view;
