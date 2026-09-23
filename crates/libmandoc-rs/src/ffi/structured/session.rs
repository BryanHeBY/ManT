//! Native call sequencing, re-entry protection, and handle orchestration.

use super::super::guard::NativeSessionGuard;
use super::raw::LIMIT_KIND_LAST;
use super::{
    FailureView, InputStorage, Limits, NativeStructuredError, OwnedStructuredDocument,
    PROFILE_ASCII, PROFILE_UTF8, ResultHandle, ResultView, STATUS_BUDGET, STATUS_INVALID_INPUT,
    STATUS_OK, STATUS_REENTRANT, STATUS_RELATION, STATUS_UNSUPPORTED, copy_structured_document,
    mant_structured_abi_version, mant_structured_render, mant_structured_result_check,
    mant_structured_result_free, mant_structured_result_view, raw_limits, relation_error,
    semantic_document, semantic_error,
};
#[cfg(test)]
use super::{ProbeMetrics, mant_structured_probe};
use crate::{InputFormat, SourceBundle};
use std::ptr::NonNull;

pub(super) fn render_prelude(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    width: u32,
    limits: &Limits,
) -> Result<OwnedStructuredDocument, NativeStructuredError> {
    render_prelude_profile(root, bundle, format, PROFILE_UTF8, width, limits)
}

pub(super) fn render_prelude_profile(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    profile: u32,
    width: u32,
    limits: &Limits,
) -> Result<OwnedStructuredDocument, NativeStructuredError> {
    let _guard = NativeSessionGuard::enter().map_err(|_| NativeStructuredError {
        status: STATUS_REENTRANT,
        stage: 1,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?;
    if unsafe { mant_structured_abi_version() } != 5 {
        return Err(relation_error());
    }
    let storage = InputStorage::new(root, bundle, format, limits)?;
    let input = storage.view(width, profile);
    let mut pointer = std::ptr::null_mut();
    let mut failure = FailureView::default();
    let status = unsafe {
        mant_structured_render(&raw const input, limits, &raw mut pointer, &raw mut failure)
    };
    if status != STATUS_OK {
        if !pointer.is_null() {
            unsafe { mant_structured_result_free(pointer) };
        }
        return Err(error_from_failure(status, failure)?);
    }
    let handle = ResultHandle(NonNull::new(pointer).ok_or(NativeStructuredError {
        status: STATUS_RELATION,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?);
    let mut checked = FailureView::default();
    let status = unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut checked) };
    if status != STATUS_OK {
        return Err(error_from_failure(status, checked)?);
    }
    let mut view = ResultView::default();
    let status = unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) };
    if status != STATUS_OK || view.reserved != 0 || view.metadata.reserved != 0 {
        return Err(NativeStructuredError {
            status: STATUS_RELATION,
            stage: 6,
            limit_kind: 0,
            observed: 0,
            allowed: 0,
        });
    }
    copy_structured_document(&handle, &view, limits)
}

#[cfg(test)]
pub(super) fn probe_structured(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    width: u32,
    limits: &Limits,
) -> Result<ProbeMetrics, NativeStructuredError> {
    probe_structured_profile(root, bundle, format, width, PROFILE_UTF8, limits)
}

#[cfg(test)]
pub(super) fn probe_structured_profile(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    width: u32,
    profile: u32,
    limits: &Limits,
) -> Result<ProbeMetrics, NativeStructuredError> {
    let _guard = NativeSessionGuard::enter().map_err(|_| NativeStructuredError {
        status: STATUS_REENTRANT,
        stage: 1,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?;
    let storage = InputStorage::new(root, bundle, format, limits)?;
    let input = storage.view(width, profile);
    let mut metrics = ProbeMetrics::default();
    let mut failure = FailureView::default();
    let status = unsafe {
        mant_structured_probe(&raw const input, limits, &raw mut metrics, &raw mut failure)
    };
    if status != STATUS_UNSUPPORTED {
        return Err(error_from_failure(status, failure)?);
    }
    let error = error_from_failure(status, failure)?;
    if error.stage != 4 || error.limit_kind != 0 {
        return Err(relation_error());
    }
    Ok(metrics)
}

pub(crate) fn render_structured(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    profile: crate::structured::StructuredProfile,
    width: u32,
    limits: &crate::structured::StructuredLimits,
) -> Result<crate::structured::StructuredDocument, crate::structured::StructuredError> {
    use crate::structured::StructuredProfile;

    let native_profile = match profile {
        StructuredProfile::Utf8 => PROFILE_UTF8,
        StructuredProfile::Ascii => PROFILE_ASCII,
    };
    let raw = render_prelude_profile(
        root,
        bundle,
        format,
        native_profile,
        width,
        &raw_limits(limits),
    )
    .map_err(|error| semantic_error(&error))?;
    semantic_document(raw)
}
pub(super) fn error_from_failure(
    status: u32,
    failure: FailureView,
) -> Result<NativeStructuredError, NativeStructuredError> {
    if !(STATUS_INVALID_INPUT..=STATUS_UNSUPPORTED).contains(&status)
        || failure.status != status
        || !(1..=6).contains(&failure.stage)
        || failure.limit_kind > LIMIT_KIND_LAST
        || failure.reserved != 0
        || (status == STATUS_BUDGET
            && (failure.limit_kind == 0 || failure.observed <= failure.allowed))
        || (status != STATUS_BUDGET && failure.limit_kind != 0)
    {
        return Err(relation_error());
    }
    Ok(NativeStructuredError {
        status,
        stage: failure.stage,
        limit_kind: failure.limit_kind,
        observed: failure.observed,
        allowed: failure.allowed,
    })
}
