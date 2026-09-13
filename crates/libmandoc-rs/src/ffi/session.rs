//! Synchronous native calls and the sole native document drop guard.
#[cfg(windows)]
use super::windows_root;
use super::{owned::copy_document, raw};
#[cfg(feature = "execute")]
use crate::{ExecutionCancellation, ExecutionErrorKind, ExecutionLimits, NativeExecutionReport};
use crate::{InputFormat, RawDocument, SourceBundle};
#[cfg(windows)]
use std::path::Path;
#[cfg(feature = "execute")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    ffi::{CStr, CString},
    ptr::NonNull,
};
pub(super) struct DocumentHandle(pub(super) NonNull<raw::CDocument>);

impl Drop for DocumentHandle {
    fn drop(&mut self) {
        unsafe { raw::mant_mandoc_document_free(self.0.as_ptr()) };
    }
}

#[cfg(feature = "execute")]
pub(crate) fn execute_buffer(
    path: &CStr,
    buffer: &[u8],
    input_format: InputFormat,
    operating_system: Option<&CStr>,
    limits: ExecutionLimits,
    cancellation: &ExecutionCancellation,
) -> Result<(RawDocument, NativeExecutionReport), (ExecutionErrorKind, String)> {
    super::execution::validate_limits_layout()
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    let native_limits = super::execution::native_limits(limits);
    let pointer = unsafe {
        raw::mant_mandoc_execute_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            &raw const native_limits,
            Some(execution_cancelled),
            std::ptr::from_ref(cancellation.atomic()).cast_mut().cast(),
        )
    };
    super::execution::copy_executed_document(pointer, limits)
}

#[cfg(feature = "execute")]
extern "C" fn execution_cancelled(context: *mut std::ffi::c_void) -> i32 {
    if context.is_null() {
        return 0;
    }
    i32::from(unsafe { &*context.cast::<AtomicBool>() }.load(Ordering::Acquire))
}

#[cfg(unix)]
pub(crate) fn parse_file(
    path: &CStr,
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
) -> Result<RawDocument, String> {
    let pointer = unsafe {
        raw::mant_mandoc_parse_file(
            path.as_ptr(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
        )
    };
    copy_document(pointer)
}

#[cfg(unix)]
pub(crate) fn parse_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
) -> Result<RawDocument, String> {
    let pointer = unsafe {
        raw::mant_mandoc_parse_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            None,
            std::ptr::null_mut(),
        )
    };
    copy_document(pointer)
}

#[cfg(windows)]
pub(crate) fn parse_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&Path>,
    allow_includes: bool,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
) -> Result<RawDocument, String> {
    let mut resolver = include_root.map(|root| windows_root::RootResolver::new(root, path));
    let (callback, context) = windows_root::callback_parts(resolver.as_mut());
    let pointer = unsafe {
        raw::mant_mandoc_parse_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            std::ptr::null(),
            i32::from(allow_includes),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            callback,
            context,
        )
    };
    copy_document(pointer)
}

pub(crate) fn parse_bundle(
    root: &CStr,
    bundle: &SourceBundle,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
) -> Result<RawDocument, String> {
    let storage = BundleSources::new(bundle);
    let sources = storage.as_slice();
    let pointer = unsafe {
        raw::mant_mandoc_parse_bundle(
            root.as_ptr(),
            sources.as_ptr(),
            sources.len(),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
        )
    };
    copy_document(pointer)
}

/// Keeps both path allocations and caller-owned source bytes alive until the
/// synchronous native call and owned transfer have completed.
pub(super) struct BundleSources<'a> {
    _paths: Vec<CString>,
    _bundle: &'a SourceBundle,
    sources: Vec<raw::CSource>,
}
impl<'a> BundleSources<'a> {
    pub(super) fn new(bundle: &'a SourceBundle) -> Self {
        let paths = bundle
            .sources()
            .map(|(path, _)| CString::new(path).expect("source bundle paths reject NUL bytes"))
            .collect::<Vec<_>>();
        let sources = bundle
            .sources()
            .zip(&paths)
            .map(|((_, data), path)| raw::CSource {
                path: path.as_ptr(),
                data: data.as_ptr(),
                length: data.len(),
            })
            .collect();
        Self {
            _paths: paths,
            _bundle: bundle,
            sources,
        }
    }
    pub(super) fn as_slice(&self) -> &[raw::CSource] {
        &self.sources
    }
}

pub(super) const fn input_format_code(input_format: InputFormat) -> i32 {
    match input_format {
        InputFormat::Auto => 0,
        InputFormat::Man => 1,
        InputFormat::Mdoc => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReentryProbe {
        outcomes: u8,
    }

    const RESOLVER_CALLED: u8 = 1 << 0;
    const PARSE_REJECTED: u8 = 1 << 1;
    #[cfg(feature = "execute")]
    const EXECUTE_REJECTED: u8 = 1 << 2;
    #[cfg(feature = "render")]
    const RENDER_REJECTED: u8 = 1 << 3;

    fn recursive_session_rejected(document: *mut raw::CDocument) -> bool {
        if document.is_null() {
            return false;
        }
        let rejected = unsafe { raw::mant_mandoc_document_ok(document) } == 0;
        let error = unsafe { raw::mant_mandoc_document_error(document) };
        let matches = rejected
            && !error.is_null()
            && unsafe { CStr::from_ptr(error) }
                .to_bytes()
                .windows(b"recursive libmandoc session entry".len())
                .any(|window| window == b"recursive libmandoc session entry");
        unsafe { raw::mant_mandoc_document_free(document) };
        matches
    }

    extern "C" fn reenter_from_resolver(
        context: *mut std::ffi::c_void,
        _current_path: *const std::ffi::c_char,
        _requested_path: *const std::ffi::c_char,
        _resolved: *mut raw::CResolvedSource,
    ) -> i32 {
        let state = unsafe { &mut *context.cast::<ReentryProbe>() };
        state.outcomes |= RESOLVER_CALLED;
        let path = c"nested.1";
        let source = b".TH NESTED 1\n.SH NAME\nnested \\- probe\n";
        let parse_document = unsafe {
            raw::mant_mandoc_parse_buffer(
                path.as_ptr(),
                source.as_ptr(),
                source.len(),
                std::ptr::null(),
                0,
                input_format_code(InputFormat::Man),
                std::ptr::null(),
                None,
                std::ptr::null_mut(),
            )
        };
        if recursive_session_rejected(parse_document) {
            state.outcomes |= PARSE_REJECTED;
        }
        #[cfg(feature = "execute")]
        {
            let limits = super::super::execution::native_limits(ExecutionLimits::default());
            let execute_document = unsafe {
                raw::mant_mandoc_execute_buffer(
                    path.as_ptr(),
                    source.as_ptr(),
                    source.len(),
                    input_format_code(InputFormat::Man),
                    std::ptr::null(),
                    &raw const limits,
                    None,
                    std::ptr::null_mut(),
                )
            };
            if recursive_session_rejected(execute_document) {
                state.outcomes |= EXECUTE_REJECTED;
            }
        }
        #[cfg(feature = "render")]
        {
            let render_document = unsafe {
                raw::mant_mandoc_render_buffer(
                    path.as_ptr(),
                    source.as_ptr(),
                    source.len(),
                    std::ptr::null(),
                    0,
                    input_format_code(InputFormat::Man),
                    std::ptr::null(),
                    1,
                    78,
                    0,
                    1024 * 1024,
                    None,
                    std::ptr::null_mut(),
                )
            };
            if recursive_session_rejected(render_document) {
                state.outcomes |= RENDER_REJECTED;
            }
        }
        0
    }

    #[test]
    fn bundle_arguments_keep_paths_and_source_bytes_paired_after_move() {
        let mut bundle = SourceBundle::new();
        bundle.insert("man1/alpha.1", b"first".to_vec()).unwrap();
        bundle.insert("man1/beta.1", b"second".to_vec()).unwrap();
        let storage = BundleSources::new(&bundle);
        let moved = Box::new(storage);
        assert_eq!(moved.as_slice().len(), 2);
        for (argument, (path, bytes)) in moved.as_slice().iter().zip(bundle.sources()) {
            assert_eq!(
                unsafe { CStr::from_ptr(argument.path) }.to_bytes(),
                path.as_bytes()
            );
            assert_eq!(
                unsafe { std::slice::from_raw_parts(argument.data, argument.length) },
                bytes
            );
        }
    }

    #[test]
    fn source_callbacks_cannot_reenter_the_same_native_session() {
        let path = c"outer.1";
        let source = b".TH OUTER 1\n.SH NAME\nouter \\- probe\n.so nested.1\n";
        let mut probe = ReentryProbe { outcomes: 0 };
        let document = unsafe {
            raw::mant_mandoc_parse_buffer(
                path.as_ptr(),
                source.as_ptr(),
                source.len(),
                std::ptr::null(),
                1,
                input_format_code(InputFormat::Man),
                std::ptr::null(),
                Some(reenter_from_resolver),
                std::ptr::from_mut(&mut probe).cast(),
            )
        };
        assert!(!document.is_null());
        unsafe { raw::mant_mandoc_document_free(document) };
        assert_ne!(probe.outcomes & RESOLVER_CALLED, 0);
        assert_ne!(probe.outcomes & PARSE_REJECTED, 0);
        #[cfg(feature = "execute")]
        assert_ne!(probe.outcomes & EXECUTE_REJECTED, 0);
        #[cfg(feature = "render")]
        assert_ne!(probe.outcomes & RENDER_REJECTED, 0);
    }

    #[cfg(feature = "execute")]
    struct CountdownCancellation {
        remaining: usize,
        calls: usize,
    }

    #[cfg(feature = "execute")]
    extern "C" fn cancel_after_work(context: *mut std::ffi::c_void) -> i32 {
        let state = unsafe { &mut *context.cast::<CountdownCancellation>() };
        state.calls += 1;
        if state.remaining == 0 {
            1
        } else {
            state.remaining -= 1;
            0
        }
    }

    #[cfg(feature = "execute")]
    #[test]
    fn mid_execution_cancellation_releases_native_state_before_the_next_call() {
        let path = c"cancel-table.1";
        let source = b".TH CANCEL 1\n.SH BODY\n.TS\nl l.\nleft\tright\n.TE\n";
        let native_limits = super::super::execution::native_limits(ExecutionLimits::default());
        let mut cancellation = CountdownCancellation {
            remaining: 8,
            calls: 0,
        };
        let pointer = unsafe {
            raw::mant_mandoc_execute_buffer(
                path.as_ptr(),
                source.as_ptr(),
                source.len(),
                input_format_code(InputFormat::Man),
                std::ptr::null(),
                &raw const native_limits,
                Some(cancel_after_work),
                std::ptr::from_mut(&mut cancellation).cast(),
            )
        };
        let Err(error) =
            super::super::execution::copy_executed_document(pointer, ExecutionLimits::default())
        else {
            panic!("mid-execution cancellation must fail");
        };
        assert_eq!(error.0, ExecutionErrorKind::Cancelled);
        assert!(cancellation.calls > 8);

        execute_buffer(
            path,
            source,
            InputFormat::Man,
            None,
            ExecutionLimits::default(),
            &ExecutionCancellation::new(),
        )
        .expect("a cancelled table session must not poison its successor");
    }

    #[cfg(feature = "execute")]
    #[test]
    fn transfer_rejection_drops_the_native_document_before_the_next_call() {
        let path = c"transfer.1";
        let source = b".TH TRANSFER 1\n.SH BODY\ncontent\n";
        let native_limits = super::super::execution::native_limits(ExecutionLimits::default());
        let cancellation = ExecutionCancellation::new();
        let pointer = unsafe {
            raw::mant_mandoc_execute_buffer(
                path.as_ptr(),
                source.as_ptr(),
                source.len(),
                input_format_code(InputFormat::Man),
                std::ptr::null(),
                &raw const native_limits,
                Some(execution_cancelled),
                std::ptr::from_ref(cancellation.atomic()).cast_mut().cast(),
            )
        };
        let Err(error) = super::super::execution::copy_executed_document(
            pointer,
            ExecutionLimits {
                max_nodes: 1,
                ..ExecutionLimits::default()
            },
        ) else {
            panic!("restricted owned transfer must fail");
        };
        assert_eq!(error.0, ExecutionErrorKind::Transfer);

        execute_buffer(
            path,
            source,
            InputFormat::Man,
            None,
            ExecutionLimits::default(),
            &ExecutionCancellation::new(),
        )
        .expect("a rejected owned transfer must release its native document");
    }
}
