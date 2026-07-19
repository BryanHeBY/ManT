//! Unsafe declarations and immediate copying for the opaque C shim.

use std::{ffi::CStr, os::raw::c_char, ptr::NonNull};

use super::{
    DisplayKind, MacroSet, Metadata, Node, NodeFlags, NodeKind, NormalizedListKind, ParsedDocument,
    TableAlignment, TableCell,
};

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

unsafe extern "C" {
    fn mant_mandoc_parse_file(path: *const c_char, allow_include: i32) -> *mut CDocument;
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
    fn mant_mandoc_node_kind(node: *const CNode) -> i32;
    fn mant_mandoc_node_macro(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_text(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_line(node: *const CNode) -> i32;
    fn mant_mandoc_node_column(node: *const CNode) -> i32;
    fn mant_mandoc_node_flags(node: *const CNode) -> u32;
    fn mant_mandoc_node_list_kind(node: *const CNode) -> i32;
    fn mant_mandoc_node_display_kind(node: *const CNode) -> i32;
    fn mant_mandoc_node_compact(node: *const CNode) -> i32;
    fn mant_mandoc_node_offset(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_equation(node: *const CNode) -> *const c_char;
    fn mant_mandoc_node_table_cells(node: *const CNode) -> *const CTableCell;
    fn mant_mandoc_table_cell_text(cell: *const CTableCell) -> *const c_char;
    fn mant_mandoc_table_cell_column_span(cell: *const CTableCell) -> u32;
    fn mant_mandoc_table_cell_row_span(cell: *const CTableCell) -> u32;
    fn mant_mandoc_table_cell_alignment(cell: *const CTableCell) -> i32;
    fn mant_mandoc_table_cell_next(cell: *const CTableCell) -> *const CTableCell;
    fn mant_mandoc_node_child(node: *const CNode) -> *const CNode;
    fn mant_mandoc_node_next(node: *const CNode) -> *const CNode;
}

const NODE_GENERATED: u32 = 1 << 0;
const NODE_SENTENCE_END: u32 = 1 << 1;
const NODE_NO_PRINT: u32 = 1 << 2;
const NODE_NO_FILL: u32 = 1 << 3;

struct DocumentHandle(NonNull<CDocument>);

impl Drop for DocumentHandle {
    fn drop(&mut self) {
        unsafe { mant_mandoc_document_free(self.0.as_ptr()) };
    }
}

pub(super) fn parse_file(path: &CStr, allow_includes: bool) -> Result<ParsedDocument, String> {
    let pointer = unsafe { mant_mandoc_parse_file(path.as_ptr(), i32::from(allow_includes)) };
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

    Ok(ParsedDocument {
        macro_set: macro_set(unsafe { mant_mandoc_document_macroset(document) })?,
        metadata: Metadata {
            title: unsafe { optional_string(mant_mandoc_document_title(document)) },
            section: unsafe { optional_string(mant_mandoc_document_section(document)) },
            volume: unsafe { optional_string(mant_mandoc_document_volume(document)) },
            os: unsafe { optional_string(mant_mandoc_document_os(document)) },
            arch: unsafe { optional_string(mant_mandoc_document_arch(document)) },
            name: unsafe { optional_string(mant_mandoc_document_name(document)) },
            date: unsafe { optional_string(mant_mandoc_document_date(document)) },
            alias_target: unsafe { optional_string(mant_mandoc_document_alias_target(document)) },
            has_body: unsafe { mant_mandoc_document_has_body(document) } != 0,
        },
        diagnostics: unsafe {
            optional_string(mant_mandoc_document_diagnostics(document)).unwrap_or_default()
        },
        root: unsafe { copy_node(root) }?,
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

unsafe fn copy_node(pointer: *const CNode) -> Result<Node, String> {
    let raw_flags = unsafe { mant_mandoc_node_flags(pointer) };
    let mut children = Vec::new();
    let mut child = unsafe { mant_mandoc_node_child(pointer) };
    while !child.is_null() {
        children.push(unsafe { copy_node(child) }?);
        child = unsafe { mant_mandoc_node_next(child) };
    }

    Ok(Node {
        kind: node_kind(unsafe { mant_mandoc_node_kind(pointer) })?,
        macro_name: unsafe { optional_string(mant_mandoc_node_macro(pointer)) },
        text: unsafe { optional_string(mant_mandoc_node_text(pointer)) },
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
        },
        list_kind: list_kind(unsafe { mant_mandoc_node_list_kind(pointer) })?,
        display_kind: display_kind(unsafe { mant_mandoc_node_display_kind(pointer) })?,
        compact: unsafe { mant_mandoc_node_compact(pointer) } != 0,
        offset: unsafe { optional_string(mant_mandoc_node_offset(pointer)) },
        table_cells: unsafe { copy_table_cells(mant_mandoc_node_table_cells(pointer)) },
        equation: unsafe { optional_string(mant_mandoc_node_equation(pointer)) },
        children,
    })
}

unsafe fn copy_table_cells(mut pointer: *const CTableCell) -> Vec<TableCell> {
    let mut cells = Vec::new();
    while !pointer.is_null() {
        cells.push(TableCell {
            text: unsafe { optional_string(mant_mandoc_table_cell_text(pointer)) },
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
