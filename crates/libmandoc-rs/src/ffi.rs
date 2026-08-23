//! Unsafe declarations and immediate ownership transfer from the opaque C shim.

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
struct CNodeView {
    kind: i32,
    macro_name: *const c_char,
    text: *const c_char,
    tag: *const c_char,
    line: i32,
    column: i32,
    flags: u32,
    list_kind: i32,
    display_kind: i32,
    font_kind: i32,
    author_mode: i32,
    compact: i32,
    offset: *const c_char,
    width: *const c_char,
    enclosure_open: *const c_char,
    enclosure_close: *const c_char,
    equation: *const c_char,
    table_cells: *const CTableCell,
    child: *const CNode,
    next: *const CNode,
}

#[repr(C)]
struct CTableCellView {
    text: *const c_char,
    text_block: i32,
    vertical_continuation: i32,
    column_span: u32,
    row_span: u32,
    alignment: i32,
    next: *const CTableCell,
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
    fn mant_mandoc_node_snapshot(
        document: *mut CDocument,
        node: *const CNode,
        view: *mut CNodeView,
    ) -> i32;
    fn mant_mandoc_table_cell_snapshot(
        document: *const CDocument,
        cell: *const CTableCell,
        view: *mut CTableCellView,
    ) -> i32;
    #[cfg(feature = "render")]
    fn mant_mandoc_document_output(document: *const CDocument) -> *const u8;
    #[cfg(feature = "render")]
    fn mant_mandoc_document_output_length(document: *const CDocument) -> usize;
    #[cfg(feature = "render")]
    fn mant_mandoc_document_render_status(document: *const CDocument) -> i32;
    #[cfg(all(feature = "render", test))]
    fn mant_mandoc_ctype_locale() -> *const c_char;
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
    let mut resolver = include_root.map(|root| windows_root::RootResolver::new(root, path));
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
const MAX_OWNED_NODE_DEPTH: usize = 256;

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
    let mut resolver = include_root.map(|root| windows_root::RootResolver::new(root, path));
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
            root: unsafe { copy_node(document, root, 0) }?.0,
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

unsafe fn copy_node(
    document: *mut CDocument,
    pointer: *const CNode,
    depth: usize,
) -> Result<(Node, *const CNode), String> {
    let mut view = std::mem::MaybeUninit::<CNodeView>::uninit();
    if unsafe { mant_mandoc_node_snapshot(document, pointer, view.as_mut_ptr()) } == 0 {
        return Err("libmandoc returned an invalid borrowed syntax node".to_owned());
    }
    let view = unsafe { view.assume_init() };
    let text = unsafe { optional_string(view.text) };
    let line_continuation = text.as_deref().is_some_and(ends_with_no_space_escape);
    let enclosure_open = unsafe { optional_string(view.enclosure_open) };
    let enclosure_close = unsafe { optional_string(view.enclosure_close) };
    let mut node = Node {
        kind: node_kind(view.kind)?,
        macro_name: unsafe { optional_string(view.macro_name) },
        text,
        tag: unsafe { optional_string(view.tag) },
        line: view.line.try_into().unwrap_or_default(),
        column: view.column.try_into().unwrap_or_default(),
        flags: NodeFlags {
            generated: view.flags & NODE_GENERATED != 0,
            sentence_end: view.flags & NODE_SENTENCE_END != 0,
            no_print: view.flags & NODE_NO_PRINT != 0,
            no_fill: view.flags & NODE_NO_FILL != 0,
            deep_link_target: view.flags & NODE_DEEP_LINK_TARGET != 0,
            permalink: view.flags & NODE_PERMALINK != 0,
            line_start: view.flags & NODE_LINE_START != 0,
            delimiter_open: view.flags & NODE_DELIMITER_OPEN != 0,
            delimiter_close: view.flags & NODE_DELIMITER_CLOSE != 0,
            synopsis_pretty: view.flags & NODE_SYNOPSIS_PRETTY != 0,
            line_continuation,
        },
        list_kind: list_kind(view.list_kind)?,
        display_kind: display_kind(view.display_kind)?,
        font: font_kind(view.font_kind)?,
        author_mode: author_mode(view.author_mode)?,
        enclosure: enclosure_open.map(|opening| NormalizedEnclosure {
            opening,
            closing: enclosure_close,
        }),
        compact: view.compact != 0,
        offset: unsafe { optional_string(view.offset) },
        width: unsafe { optional_string(view.width) },
        table_cells: unsafe { copy_table_cells(document, view.table_cells) }?,
        equation: unsafe { optional_string(view.equation) },
        children: Vec::new(),
    };

    if depth + 1 < MAX_OWNED_NODE_DEPTH {
        let mut child = view.child;
        while !child.is_null() {
            let (owned, next) = unsafe { copy_node(document, child, depth + 1) }?;
            node.children.push(owned);
            child = next;
        }
    }
    Ok((node, view.next))
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

unsafe fn copy_table_cells(
    document: *const CDocument,
    mut pointer: *const CTableCell,
) -> Result<Vec<TableCell>, String> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        let mut view = std::mem::MaybeUninit::<CTableCellView>::uninit();
        if unsafe { mant_mandoc_table_cell_snapshot(document, pointer, view.as_mut_ptr()) } == 0 {
            return Err("libmandoc returned an invalid borrowed table cell".to_owned());
        }
        let view = unsafe { view.assume_init() };
        cells.push(TableCell {
            text: unsafe { optional_string(view.text) },
            text_block: view.text_block != 0,
            vertical_continuation: view.vertical_continuation != 0,
            column_span: view.column_span.try_into().unwrap_or(u16::MAX),
            row_span: view.row_span.try_into().unwrap_or(u16::MAX),
            alignment: match view.alignment {
                1 => TableAlignment::Center,
                2 => TableAlignment::Right,
                _ => TableAlignment::Left,
            },
        });
        pointer = view.next;
    }
    Ok(cells)
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::CString,
        fs,
        io::Read,
        path::{Path, PathBuf},
    };

    use flate2::read::MultiGzDecoder;

    use super::{InputFormat, Node, parse_buffer};

    #[test]
    fn owned_transfer_preserves_semantic_edges_after_parser_release() {
        assert_owned_transfer(
            "semantic-man.1",
            br".TH TRANSFER 1
.SH NAME
transfer \- ownership boundary
.TS
tab(|);
l l.
left|right
.TE
.EQ
x sup 2
.EN
",
        );
        assert_owned_transfer(
            "semantic-mdoc.1",
            br".Dd August 23, 2026
.Dt TRANSFER 1
.Os
.Sh NAME
.Nm transfer
.Nd ownership boundary
.Bl -bullet -compact -offset indent -width Ds
.It item
.El
.Bd -literal -offset indent
literal display
.Ed
.Bf -emphasis
emphasis
.Ef
.Es ( )
.En wrapped
.An -split
",
        );

        let mut nested = String::from(".TH DEEP 1\n");
        for _ in 0..300 {
            nested.push_str(".RS\n");
        }
        nested.push_str("bounded\n");
        for _ in 0..300 {
            nested.push_str(".RE\n");
        }
        assert_owned_transfer("deep.1", nested.as_bytes());
    }

    #[test]
    fn owned_transfer_survives_parser_release_for_real_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/fixtures/roff/real");
        if !root.is_dir() {
            return;
        }
        let mut fixtures = Vec::new();
        collect_manuals(&root, &mut fixtures);
        fixtures.sort();
        assert!(
            fixtures.len() >= 20,
            "the repository fixture corpus unexpectedly contains only {} manuals",
            fixtures.len()
        );
        for fixture in fixtures {
            let source = read_fixture(&fixture);
            assert_owned_transfer(&fixture.to_string_lossy(), &source);
        }
    }

    fn assert_owned_transfer(label: &str, source: &[u8]) {
        let path = CString::new(label).expect("fixture labels contain no NUL bytes");
        // `parse_buffer` destroys its private native parser handle before it
        // returns. Traversing every owned string and cell afterwards catches
        // any borrowed pointer that accidentally escaped the FFI boundary.
        let parsed = parse_buffer(&path, source, None, false, InputFormat::Auto)
            .unwrap_or_else(|error| panic!("owned transfer failed for {label}: {error}"));
        let (nodes, bytes) = touch_owned_node(&parsed.document.root);
        assert!(
            nodes > 1,
            "owned syntax tree is unexpectedly empty for {label}"
        );
        assert!(
            bytes > 0,
            "owned syntax tree has no string data for {label}"
        );
        let _ = parsed.diagnostics.len();
    }

    fn touch_owned_node(node: &Node) -> (usize, usize) {
        let mut bytes = node.macro_name.as_ref().map_or(0, String::len)
            + node.text.as_ref().map_or(0, String::len)
            + node.tag.as_ref().map_or(0, String::len)
            + node.offset.as_ref().map_or(0, String::len)
            + node.width.as_ref().map_or(0, String::len)
            + node.equation.as_ref().map_or(0, String::len)
            + node.enclosure.as_ref().map_or(0, |enclosure| {
                enclosure.opening.len() + enclosure.closing.as_ref().map_or(0, String::len)
            })
            + node
                .table_cells
                .iter()
                .map(|cell| cell.text.as_ref().map_or(0, String::len))
                .sum::<usize>();
        let mut nodes = 1;
        for child in &node.children {
            let (child_nodes, child_bytes) = touch_owned_node(child);
            nodes += child_nodes;
            bytes += child_bytes;
        }
        (nodes, bytes)
    }

    fn collect_manuals(directory: &Path, output: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read real fixture directory") {
            let path = entry.expect("read fixture entry").path();
            if path.is_dir() {
                collect_manuals(&path, output);
            } else if is_manual(&path) {
                output.push(path);
            }
        }
    }

    fn is_manual(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(extension, "gz" | "zst")
                    || extension.as_bytes().first().is_some_and(u8::is_ascii_digit)
            })
    }

    fn read_fixture(path: &Path) -> Vec<u8> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("gz") => {
                let mut decoded = Vec::new();
                MultiGzDecoder::new(fs::File::open(path).expect("open gzip fixture"))
                    .read_to_end(&mut decoded)
                    .expect("decode gzip fixture");
                decoded
            }
            Some("zst") => {
                zstd::stream::decode_all(fs::File::open(path).expect("open zstd fixture"))
                    .expect("decode zstd fixture")
            }
            _ => fs::read(path).expect("read plain fixture"),
        }
    }
}
