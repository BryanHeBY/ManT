//! Unsafe declarations and immediate copying for the opaque C shim.

use std::{
    ffi::{CStr, CString},
    os::raw::{c_char, c_void},
    ptr::NonNull,
};

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
mod windows_root;

use super::{
    AuthorMode, DisplayKind, Document, InputFormat, MacroSet, Metadata, Node, NodeFlags, NodeKind,
    NormalizedEnclosure, NormalizedFont, NormalizedListKind, RawDocument, SourceBundle,
    TableAlignment, TableCell,
};

#[cfg(feature = "render")]
use super::RawRender;
#[cfg(feature = "render")]
use unicode_width::UnicodeWidthChar;

#[repr(C)]
struct CDocument {
    _private: [u8; 0],
}

#[repr(C)]
struct CNode {
    _private: [u8; 0],
}

#[repr(C)]
struct CTableCell {
    _private: [u8; 0],
}

#[repr(C)]
struct CSource {
    path: *const c_char,
    data: *const u8,
    length: usize,
}

#[repr(C)]
struct CResolvedSource {
    path: *const c_char,
    data: *const u8,
    length: usize,
}

type CSourceResolver =
    extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut CResolvedSource) -> i32;

#[cfg(feature = "render")]
#[unsafe(no_mangle)]
extern "C" fn mant_mandoc_utf8_width(codepoint: i32) -> usize {
    u32::try_from(codepoint)
        .ok()
        .and_then(char::from_u32)
        .and_then(UnicodeWidthChar::width)
        .unwrap_or(0)
}

unsafe extern "C" {
    #[cfg(unix)]
    fn mant_mandoc_parse_file(
        path: *const c_char,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
    ) -> *mut CDocument;
    fn mant_mandoc_parse_buffer(
        path: *const c_char,
        buffer: *const u8,
        length: usize,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
        resolver: Option<CSourceResolver>,
        resolver_context: *mut c_void,
    ) -> *mut CDocument;
    fn mant_mandoc_parse_bundle(
        root: *const c_char,
        sources: *const CSource,
        source_count: usize,
        input_format: i32,
    ) -> *mut CDocument;
    #[cfg(all(feature = "render", unix))]
    fn mant_mandoc_render_file(
        path: *const c_char,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
        render_format: i32,
        render_width: usize,
        html_fragment: i32,
        output_limit: usize,
    ) -> *mut CDocument;
    #[cfg(feature = "render")]
    fn mant_mandoc_render_buffer(
        path: *const c_char,
        buffer: *const u8,
        length: usize,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
        render_format: i32,
        render_width: usize,
        html_fragment: i32,
        output_limit: usize,
        resolver: Option<CSourceResolver>,
        resolver_context: *mut c_void,
    ) -> *mut CDocument;
    #[cfg(feature = "render")]
    fn mant_mandoc_render_bundle(
        root: *const c_char,
        sources: *const CSource,
        source_count: usize,
        input_format: i32,
        render_format: i32,
        render_width: usize,
        html_fragment: i32,
        output_limit: usize,
    ) -> *mut CDocument;
    fn mant_mandoc_document_free(document: *mut CDocument);
    fn mant_mandoc_document_ok(document: *const CDocument) -> i32;
    fn mant_mandoc_document_error(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_diagnostics(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_macroset(document: *const CDocument) -> i32;
    fn mant_mandoc_document_title(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_section(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_volume(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_os(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_arch(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_name(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_date(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_alias_target(document: *const CDocument) -> *const c_char;
    fn mant_mandoc_document_has_body(document: *const CDocument) -> i32;
    fn mant_mandoc_document_root(document: *const CDocument) -> *const CNode;
    #[cfg(feature = "render")]
    fn mant_mandoc_document_output(document: *const CDocument) -> *const u8;
    #[cfg(feature = "render")]
    fn mant_mandoc_document_output_length(document: *const CDocument) -> usize;
    #[cfg(feature = "render")]
    fn mant_mandoc_document_render_status(document: *const CDocument) -> i32;
    #[cfg(all(feature = "render", test))]
    fn mant_mandoc_ctype_locale() -> *const c_char;
    fn mant_mandoc_node_kind(node: *const CNode) -> i32;
    fn mant_mandoc_node_macro(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_text(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_tag(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_line(node: *const CNode) -> i32;
    fn mant_mandoc_node_column(node: *const CNode) -> i32;
    fn mant_mandoc_node_flags(node: *const CNode) -> u32;
    fn mant_mandoc_node_list_kind(node: *const CNode) -> i32;
    fn mant_mandoc_node_display_kind(node: *const CNode) -> i32;
    fn mant_mandoc_node_font_kind(node: *const CNode) -> i32;
    fn mant_mandoc_node_author_mode(node: *const CNode) -> i32;
    fn mant_mandoc_node_compact(node: *const CNode) -> i32;
    fn mant_mandoc_node_offset(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_width(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_enclosure_open(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_enclosure_close(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_equation(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_table_cells(node: *const CNode) -> *const CTableCell;
    fn mant_mandoc_table_cell_text(cell: *const CTableCell) -> *const c_char;
    fn mant_mandoc_table_cell_is_text_block(cell: *const CTableCell) -> i32;
    fn mant_mandoc_table_cell_is_vertical_continuation(cell: *const CTableCell) -> i32;
    fn mant_mandoc_table_cell_column_span(cell: *const CTableCell) -> u32;
    fn mant_mandoc_table_cell_row_span(cell: *const CTableCell) -> u32;
    fn mant_mandoc_table_cell_alignment(cell: *const CTableCell) -> i32;
    fn mant_mandoc_table_cell_next(cell: *const CTableCell) -> *const CTableCell;
    fn mant_mandoc_node_child(node: *const CNode) -> *const CNode;
    fn mant_mandoc_node_next(node: *const CNode) -> *const CNode;
}

#[cfg(all(feature = "render", test))]
pub(crate) fn ctype_locale() -> Option<String> {
    unsafe { optional_string(mant_mandoc_ctype_locale()) }
}

#[cfg(feature = "render")]
pub(super) struct NativeRenderError {
    pub(super) status: i32,
    pub(super) message: String,
}

#[cfg(all(feature = "render", unix))]
#[allow(clippy::too_many_arguments)]
pub(super) fn render_file(
    path: &CStr,
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let pointer = unsafe {
        mant_mandoc_render_file(
            path.as_ptr(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
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
pub(super) fn render_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let pointer = unsafe {
        mant_mandoc_render_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
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
pub(super) fn render_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&Path>,
    allow_includes: bool,
    input_format: InputFormat,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let mut resolver = include_root.map(windows_root::RootResolver::new);
    let (callback, context) = windows_root::callback_parts(resolver.as_mut());
    let pointer = unsafe {
        mant_mandoc_render_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            std::ptr::null(),
            i32::from(allow_includes),
            input_format_code(input_format),
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
pub(super) fn render_bundle(
    root: &CStr,
    bundle: &SourceBundle,
    input_format: InputFormat,
    render_format: i32,
    width: usize,
    html_fragment: bool,
    output_limit: usize,
) -> Result<RawRender, NativeRenderError> {
    let (_paths, sources) = bundle_sources(bundle);
    let pointer = unsafe {
        mant_mandoc_render_bundle(
            root.as_ptr(),
            sources.as_ptr(),
            sources.len(),
            input_format_code(input_format),
            render_format,
            width,
            i32::from(html_fragment),
            output_limit,
        )
    };
    copy_render(pointer)
}

const NODE_GENERATED: u32 = 1 << 0;
const NODE_SENTENCE_END: u32 = 1 << 1;
const NODE_NO_PRINT: u32 = 1 << 2;
const NODE_NO_FILL: u32 = 1 << 3;
const NODE_DEEP_LINK_TARGET: u32 = 1 << 4;
const NODE_PERMALINK: u32 = 1 << 5;
const NODE_LINE_START: u32 = 1 << 6;
const NODE_DELIMITER_OPEN: u32 = 1 << 7;
const NODE_DELIMITER_CLOSE: u32 = 1 << 8;
const NODE_SYNOPSIS_PRETTY: u32 = 1 << 9;

struct DocumentHandle(NonNull<CDocument>);

impl Drop for DocumentHandle {
    fn drop(&mut self) {
        unsafe { mant_mandoc_document_free(self.0.as_ptr()) };
    }
}

#[cfg(unix)]
pub(super) fn parse_file(
    path: &CStr,
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
) -> Result<RawDocument, String> {
    let pointer = unsafe {
        mant_mandoc_parse_file(
            path.as_ptr(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
        )
    };
    copy_document(pointer)
}

#[cfg(unix)]
pub(super) fn parse_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&CStr>,
    allow_includes: bool,
    input_format: InputFormat,
) -> Result<RawDocument, String> {
    let pointer = unsafe {
        mant_mandoc_parse_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            include_root.map_or(std::ptr::null(), CStr::as_ptr),
            i32::from(allow_includes),
            input_format_code(input_format),
            None,
            std::ptr::null_mut(),
        )
    };
    copy_document(pointer)
}

#[cfg(windows)]
pub(super) fn parse_buffer(
    path: &CStr,
    buffer: &[u8],
    include_root: Option<&Path>,
    allow_includes: bool,
    input_format: InputFormat,
) -> Result<RawDocument, String> {
    let mut resolver = include_root.map(windows_root::RootResolver::new);
    let (callback, context) = windows_root::callback_parts(resolver.as_mut());
    let pointer = unsafe {
        mant_mandoc_parse_buffer(
            path.as_ptr(),
            buffer.as_ptr(),
            buffer.len(),
            std::ptr::null(),
            i32::from(allow_includes),
            input_format_code(input_format),
            callback,
            context,
        )
    };
    copy_document(pointer)
}

pub(super) fn parse_bundle(
    root: &CStr,
    bundle: &SourceBundle,
    input_format: InputFormat,
) -> Result<RawDocument, String> {
    let (_paths, sources) = bundle_sources(bundle);
    let pointer = unsafe {
        mant_mandoc_parse_bundle(
            root.as_ptr(),
            sources.as_ptr(),
            sources.len(),
            input_format_code(input_format),
        )
    };
    copy_document(pointer)
}

fn bundle_sources(bundle: &SourceBundle) -> (Vec<CString>, Vec<CSource>) {
    let paths = bundle
        .sources()
        .map(|(path, _)| CString::new(path).expect("source bundle paths reject NUL bytes"))
        .collect::<Vec<_>>();
    let sources = bundle
        .sources()
        .zip(&paths)
        .map(|((_, data), path)| CSource {
            path: path.as_ptr(),
            data: data.as_ptr(),
            length: data.len(),
        })
        .collect();
    (paths, sources)
}

const fn input_format_code(input_format: InputFormat) -> i32 {
    match input_format {
        InputFormat::Auto => 0,
        InputFormat::Man => 1,
        InputFormat::Mdoc => 2,
    }
}

#[cfg(feature = "render")]
fn copy_render(pointer: *mut CDocument) -> Result<RawRender, NativeRenderError> {
    let handle = DocumentHandle(NonNull::new(pointer).ok_or_else(|| NativeRenderError {
        status: 2,
        message: "libmandoc could not allocate a render result".to_owned(),
    })?);
    let document = handle.0.as_ptr();
    if unsafe { mant_mandoc_document_ok(document) } == 0 {
        return Err(NativeRenderError {
            status: unsafe { mant_mandoc_document_render_status(document) },
            message: unsafe { optional_string(mant_mandoc_document_error(document)) }
                .unwrap_or_else(|| "libmandoc could not render the source".to_owned()),
        });
    }
    let length = unsafe { mant_mandoc_document_output_length(document) };
    let output = unsafe { mant_mandoc_document_output(document) };
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
            optional_string(mant_mandoc_document_diagnostics(document)).unwrap_or_default()
        },
    })
}

fn copy_document(pointer: *mut CDocument) -> Result<RawDocument, String> {
    let handle = DocumentHandle(
        NonNull::new(pointer)
            .ok_or_else(|| "libmandoc could not allocate a document".to_owned())?,
    );
    let document = handle.0.as_ptr();
    if unsafe { mant_mandoc_document_ok(document) } == 0 {
        return Err(
            unsafe { optional_string(mant_mandoc_document_error(document)) }
                .unwrap_or_else(|| "libmandoc could not parse the source".to_owned()),
        );
    }

    let root = unsafe { mant_mandoc_document_root(document) };
    if root.is_null() {
        return Err("libmandoc produced no syntax tree".to_owned());
    }

    Ok(RawDocument {
        document: Document {
            macro_set: macro_set(unsafe { mant_mandoc_document_macroset(document) })?,
            metadata: Metadata {
                title: unsafe { optional_string(mant_mandoc_document_title(document)) },
                section: unsafe { optional_string(mant_mandoc_document_section(document)) },
                volume: unsafe { optional_string(mant_mandoc_document_volume(document)) },
                os: unsafe { optional_string(mant_mandoc_document_os(document)) },
                arch: unsafe { optional_string(mant_mandoc_document_arch(document)) },
                name: unsafe { optional_string(mant_mandoc_document_name(document)) },
                date: unsafe { optional_string(mant_mandoc_document_date(document)) },
                alias_target: unsafe {
                    optional_string(mant_mandoc_document_alias_target(document))
                },
                has_body: unsafe { mant_mandoc_document_has_body(document) } != 0,
            },
            root: unsafe { copy_node(root) }?,
        },
        diagnostics: unsafe {
            optional_string(mant_mandoc_document_diagnostics(document)).unwrap_or_default()
        },
    })
}

unsafe fn optional_string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(pointer) }
                .to_string_lossy()
                .into_owned(),
        )
    }
}

fn macro_set(value: i32) -> Result<MacroSet, String> {
    match value {
        0 => Ok(MacroSet::None),
        1 => Ok(MacroSet::Mdoc),
        2 => Ok(MacroSet::Man),
        _ => Err("libmandoc returned an unknown macro set".to_owned()),
    }
}

fn node_kind(value: i32) -> Result<NodeKind, String> {
    match value {
        0 => Ok(NodeKind::Root),
        1 => Ok(NodeKind::Block),
        2 => Ok(NodeKind::Head),
        3 => Ok(NodeKind::Body),
        4 => Ok(NodeKind::Tail),
        5 => Ok(NodeKind::Element),
        6 => Ok(NodeKind::Text),
        7 => Ok(NodeKind::Comment),
        8 => Ok(NodeKind::Table),
        9 => Ok(NodeKind::Equation),
        _ => Err("libmandoc returned an unknown node kind".to_owned()),
    }
}

fn list_kind(value: i32) -> Result<Option<NormalizedListKind>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(NormalizedListKind::Bullet)),
        2 => Ok(Some(NormalizedListKind::Ordered)),
        3 => Ok(Some(NormalizedListKind::Definition)),
        4 => Ok(Some(NormalizedListKind::Column)),
        5 => Ok(Some(NormalizedListKind::Plain)),
        _ => Err("libmandoc returned an unknown list kind".to_owned()),
    }
}

fn display_kind(value: i32) -> Result<Option<DisplayKind>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(DisplayKind::Literal)),
        2 => Ok(Some(DisplayKind::Filled)),
        _ => Err("libmandoc returned an unknown display kind".to_owned()),
    }
}

fn font_kind(value: i32) -> Result<Option<NormalizedFont>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(NormalizedFont::Emphasis)),
        2 => Ok(Some(NormalizedFont::Literal)),
        3 => Ok(Some(NormalizedFont::Symbolic)),
        _ => Err("libmandoc returned an unknown normalized font".to_owned()),
    }
}

fn author_mode(value: i32) -> Result<Option<AuthorMode>, String> {
    match value {
        0 => Ok(None),
        1 => Ok(Some(AuthorMode::Split)),
        2 => Ok(Some(AuthorMode::NoSplit)),
        _ => Err("libmandoc returned an unknown author mode".to_owned()),
    }
}

unsafe fn copy_node(pointer: *const CNode) -> Result<Node, String> {
    let raw_flags = unsafe { mant_mandoc_node_flags(pointer) };
    let text = unsafe { optional_string(mant_mandoc_node_text(pointer)) };
    let line_continuation = text.as_deref().is_some_and(ends_with_no_space_escape);
    let mut children = Vec::new();
    let mut child = unsafe { mant_mandoc_node_child(pointer) };
    while !child.is_null() {
        children.push(unsafe { copy_node(child) }?);
        child = unsafe { mant_mandoc_node_next(child) };
    }

    let enclosure_open = unsafe { optional_string(mant_mandoc_node_enclosure_open(pointer)) };
    let enclosure_close = unsafe { optional_string(mant_mandoc_node_enclosure_close(pointer)) };

    Ok(Node {
        kind: node_kind(unsafe { mant_mandoc_node_kind(pointer) })?,
        macro_name: unsafe { optional_string(mant_mandoc_node_macro(pointer)) },
        text,
        tag: unsafe { optional_string(mant_mandoc_node_tag(pointer)) },
        line: unsafe { mant_mandoc_node_line(pointer) }
            .try_into()
            .unwrap_or_default(),
        column: unsafe { mant_mandoc_node_column(pointer) }
            .try_into()
            .unwrap_or_default(),
        flags: NodeFlags {
            generated: raw_flags & NODE_GENERATED != 0,
            sentence_end: raw_flags & NODE_SENTENCE_END != 0,
            no_print: raw_flags & NODE_NO_PRINT != 0,
            no_fill: raw_flags & NODE_NO_FILL != 0,
            deep_link_target: raw_flags & NODE_DEEP_LINK_TARGET != 0,
            permalink: raw_flags & NODE_PERMALINK != 0,
            line_start: raw_flags & NODE_LINE_START != 0,
            delimiter_open: raw_flags & NODE_DELIMITER_OPEN != 0,
            delimiter_close: raw_flags & NODE_DELIMITER_CLOSE != 0,
            synopsis_pretty: raw_flags & NODE_SYNOPSIS_PRETTY != 0,
            line_continuation,
        },
        list_kind: list_kind(unsafe { mant_mandoc_node_list_kind(pointer) })?,
        display_kind: display_kind(unsafe { mant_mandoc_node_display_kind(pointer) })?,
        font: font_kind(unsafe { mant_mandoc_node_font_kind(pointer) })?,
        author_mode: author_mode(unsafe { mant_mandoc_node_author_mode(pointer) })?,
        enclosure: enclosure_open.map(|opening| NormalizedEnclosure {
            opening,
            closing: enclosure_close,
        }),
        compact: unsafe { mant_mandoc_node_compact(pointer) } != 0,
        offset: unsafe { optional_string(mant_mandoc_node_offset(pointer)) },
        width: unsafe { optional_string(mant_mandoc_node_width(pointer)) },
        table_cells: unsafe { copy_table_cells(mant_mandoc_node_table_cells(pointer)) },
        equation: unsafe { optional_string(mant_mandoc_node_equation(pointer)) },
        children,
    })
}

/// Match libmandoc's `man_hasc`: only an unescaped final `\c` continues the
/// input line. An odd number of immediately preceding backslashes escapes the
/// candidate backslash instead.
fn ends_with_no_space_escape(text: &str) -> bool {
    let bytes = text.as_bytes();
    let Some(prefix) = bytes.strip_suffix(br"\c") else {
        return false;
    };
    prefix
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 0
}

unsafe fn copy_table_cells(mut pointer: *const CTableCell) -> Vec<TableCell> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        cells.push(TableCell {
            text: unsafe { optional_string(mant_mandoc_table_cell_text(pointer)) },
            text_block: unsafe { mant_mandoc_table_cell_is_text_block(pointer) } != 0,
            vertical_continuation: unsafe {
                mant_mandoc_table_cell_is_vertical_continuation(pointer)
            } != 0,
            column_span: unsafe { mant_mandoc_table_cell_column_span(pointer) }
                .try_into()
                .unwrap_or(u16::MAX),
            row_span: unsafe { mant_mandoc_table_cell_row_span(pointer) }
                .try_into()
                .unwrap_or(u16::MAX),
            alignment: match unsafe { mant_mandoc_table_cell_alignment(pointer) } {
                1 => TableAlignment::Center,
                2 => TableAlignment::Right,
                _ => TableAlignment::Left,
            },
        });
        pointer = unsafe { mant_mandoc_table_cell_next(pointer) };
    }
    cells
}
