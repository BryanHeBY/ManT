//! Structured FFI contract, transfer, resource, and behavior tests.

use super::*;
use crate::{InputFormat, SourceBundle};
use std::{ffi::c_void, ptr::NonNull};

mod abi;
mod content;
mod list_lifecycle;
mod list_markers;
mod lists;
mod resources;
mod source;
mod transfer;
mod workset;

struct ResolverContext {
    status: u32,
    out_slot: u32,
    seen_current: u32,
}

unsafe extern "C" fn controlled_resolver(
    context: *mut c_void,
    current_input: u32,
    _requested: BytesView,
    out_input: *mut u32,
) -> u32 {
    let context = unsafe { &mut *context.cast::<ResolverContext>() };
    context.seen_current = current_input;
    unsafe { *out_input = context.out_slot };
    context.status
}

struct ReentryContext {
    observed_status: u32,
}

unsafe extern "C" fn reentering_resolver(
    context: *mut c_void,
    _current_input: u32,
    _requested: BytesView,
    out_input: *mut u32,
) -> u32 {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut nested = SourceBundle::new();
        nested
            .insert("nested.1", b".TH NESTED 1\n".to_vec())
            .unwrap();
        render_prelude(
            "nested.1",
            &nested,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect_err("native C re-entry must be rejected")
        .status
    }));
    unsafe { *out_input = 0 };
    match outcome {
        Ok(status) => {
            unsafe { &mut *context.cast::<ReentryContext>() }.observed_status = status;
            RESOLVE_NOT_FOUND
        }
        Err(_) => RESOLVE_PANIC,
    }
}

fn raw_render(input: &InputView, limits: &Limits) -> (u32, *mut ResultHandleRaw, FailureView) {
    let mut result = std::ptr::null_mut();
    let mut failure = FailureView::default();
    let status =
        unsafe { mant_structured_render(input, limits, &raw mut result, &raw mut failure) };
    (status, result, failure)
}

fn diagnostic_span<'a>(
    document: &'a OwnedStructuredDocument,
    diagnostic: &OwnedDiagnostic,
) -> Option<&'a OwnedSpan> {
    diagnostic
        .span
        .and_then(|key| document.spans.get(key as usize - 1))
}

fn abi_slice<T>(values: &[T]) -> SliceView {
    SliceView {
        ptr: if values.is_empty() {
            std::ptr::null()
        } else {
            values.as_ptr().cast()
        },
        count: u32::try_from(values.len()).unwrap(),
        stride: u32::try_from(size_of::<T>()).unwrap(),
    }
}
