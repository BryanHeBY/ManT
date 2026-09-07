//! Bounded native renderer output copied before session release.
#[cfg(windows)]
use super::windows_root;
use super::{
    owned::optional_string,
    raw::{self, CDocument},
    session::{DocumentHandle, bundle_sources, input_format_code},
};
use crate::{InputFormat, RawRender, SourceBundle};
#[cfg(windows)]
use std::path::Path;
use std::{ffi::CStr, ptr::NonNull};
#[cfg(all(feature = "render", test))]
pub(crate) fn ctype_locale() -> Option<String> {
    unsafe { optional_string(raw::mant_mandoc_ctype_locale()) }
}

#[cfg(feature = "render")]
pub(crate) struct NativeRenderError {
    pub(crate) status: i32,
    pub(crate) message: String,
}

#[cfg(all(feature = "render", unix))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_file(
    path: &CStr,
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let pointer = unsafe {
        raw::mant_mandoc_render_file(
            path.as_ptr(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            render_format,
            width,
            i32::from(html_fragment),
            output_limit,
        )
    };
    copy_render(pointer)
}

#[cfg(all(feature = "render", unix))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let pointer = unsafe {
        raw::mant_mandoc_render_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            render_format,
            width,
            i32::from(html_fragment),
            output_limit,
            None,
            std::ptr::null_mut(),
        )
    };
    copy_render(pointer)
}

#[cfg(all(feature = "render", windows))]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&Path>,
    allow_includes: bool,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let mut resolver = include_root.map(|root| windows_root::RootResolver::new(root, path));
    let (callback, context) = windows_root::callback_parts(resolver.as_mut());
    let pointer = unsafe {
        raw::mant_mandoc_render_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            std::ptr::null(),
            i32::from(allow_includes),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            render_format,
            width,
            i32::from(html_fragment),
            output_limit,
            callback,
            context,
        )
    };
    copy_render(pointer)
}

#[cfg(feature = "render")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_bundle(
    root: &CStr,
    bundle: &SourceBundle,
    input_format: InputFormat,
    operating_system: Option<&CStr>,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let (_paths, sources) = bundle_sources(bundle);
    let pointer = unsafe {
        raw::mant_mandoc_render_bundle(
            root.as_ptr(),
            sources.as_ptr(),
            sources.len(),
            input_format_code(input_format),
            operating_system.map_or(std::ptr::null(), CStr::as_ptr),
            render_format,
            width,
            i32::from(html_fragment),
            output_limit,
        )
    };
    copy_render(pointer)
}

#[cfg(feature = "render")]
fn copy_render(pointer: *mut CDocument) -> Result<RawRender, NativeRenderError> {
    let handle = DocumentHandle(NonNull::new(pointer).ok_or_else(|| NativeRenderError {
        status: 2,
        message: "libmandoc could not allocate a render result".to_owned(),
    })?);
    let document = handle.0.as_ptr();
    if unsafe { raw::mant_mandoc_document_ok(document) } == 0 {
        return Err(NativeRenderError {
            status: unsafe { raw::mant_mandoc_document_render_status(document) },
            message: unsafe { optional_string(raw::mant_mandoc_document_error(document)) }
                .unwrap_or_else(|| "libmandoc could not render the source".to_owned()),
        });
    }
    let length = unsafe { raw::mant_mandoc_document_output_length(document) };
    let output = unsafe { raw::mant_mandoc_document_output(document) };
    if output.is_null() && length != 0 {
        return Err(NativeRenderError {
            status: 2,
            message: "libmandoc returned an invalid render buffer".to_owned(),
        });
    }
    Ok(RawRender {
        output: if length == 0 {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(output, length) }.to_vec()
        },
        diagnostics: unsafe {
            optional_string(raw::mant_mandoc_document_diagnostics(document)).unwrap_or_default()
        },
    })
}
