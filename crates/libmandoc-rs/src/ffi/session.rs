//! Synchronous native calls and the sole native document drop guard.
#[cfg(windows)]
use super::windows_root;
use super::{owned::copy_document, raw};
#[cfg(feature = "execute")]
use crate::{ExecutionErrorKind, ExecutionLimits, NativeExecutionReport};
use crate::{InputFormat, RawDocument, SourceBundle};
#[cfg(windows)]
use std::path::Path;
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
) -> Result<(RawDocument, NativeExecutionReport), (ExecutionErrorKind, String)> {
    super::execution::validate_limits_layout()
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    let limits = super::execution::native_limits(limits);
    let pointer = unsafe {
        raw::mant_mandoc_execute_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            &raw const limits,
        )
    };
    super::execution::copy_executed_document(pointer)
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
        called: bool,
        rejected: bool,
    }

    extern "C" fn reenter_from_resolver(
        context: *mut std::ffi::c_void,
        _current_path: *const std::ffi::c_char,
        _requested_path: *const std::ffi::c_char,
        _resolved: *mut raw::CResolvedSource,
    ) -> i32 {
        let state = unsafe { &mut *context.cast::<ReentryProbe>() };
        state.called = true;
        let path = c"nested.1";
        let source = b".TH NESTED 1\n.SH NAME\nnested \\- probe\n";
        let document = unsafe {
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
        if !document.is_null() {
            let rejected = unsafe { raw::mant_mandoc_document_ok(document) } == 0;
            let error = unsafe { raw::mant_mandoc_document_error(document) };
            state.rejected = rejected
                && !error.is_null()
                && unsafe { CStr::from_ptr(error) }
                    .to_bytes()
                    .windows(b"recursive libmandoc session entry".len())
                    .any(|window| window == b"recursive libmandoc session entry");
            unsafe { raw::mant_mandoc_document_free(document) };
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
        let mut probe = ReentryProbe {
            called: false,
            rejected: false,
        };
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
        assert!(probe.called);
        assert!(probe.rejected);
    }
}
