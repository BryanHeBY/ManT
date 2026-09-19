//! Raw C ABI declarations. Snapshots are borrowed only for immediate copying.
use std::os::raw::{c_char, c_void};
#[cfg(any(feature = "execute", feature = "render"))]
use unicode_width::UnicodeWidthChar;
#[repr(C)]
pub(super) struct CDocument {
    pub(super) _private: [u8; 0],
}

#[cfg(feature = "execute")]
#[repr(C)]
pub(super) struct CExecutionReport {
    pub(super) _private: [u8; 0],
}

#[cfg(feature = "execute")]
#[repr(C)]
#[allow(clippy::struct_field_names)]
pub(super) struct CExecutionLimits {
    pub(super) abi_version: u32,
    pub(super) abi_size: u32,
    pub(super) max_nodes: u64,
    pub(super) max_depth: u64,
    pub(super) max_work: u64,
    pub(super) max_records: u64,
    pub(super) max_pool_bytes: u64,
    pub(super) max_buffer_cells: u64,
    pub(super) max_report_bytes: u64,
}

#[repr(C)]
pub(super) struct CNode {
    pub(super) _private: [u8; 0],
}

#[repr(C)]
pub(super) struct CEquationBox {
    pub(super) _private: [u8; 0],
}

#[repr(C)]
pub(super) struct CTableCell {
    pub(super) _private: [u8; 0],
}

#[repr(C)]
pub(super) struct CTableRuleCell {
    pub(super) _private: [u8; 0],
}

#[repr(C)]
pub(super) struct CNodeView {
    pub(super) kind: i32,
    pub(super) execution_node_key: u32,
    pub(super) macro_name: *const c_char,
    pub(super) text: *const c_char,
    pub(super) tag: *const c_char,
    pub(super) line: i32,
    pub(super) column: i32,
    pub(super) flow_epoch: u64,
    pub(super) table_escape: i32,
    pub(super) table_source_recovery_safe: i32,
    pub(super) table_row_kind: i32,
    pub(super) flags: u32,
    pub(super) list_kind: i32,
    pub(super) definition_list_style: i32,
    pub(super) display_kind: i32,
    pub(super) font_kind: i32,
    pub(super) author_mode: i32,
    pub(super) compact: i32,
    pub(super) offset: *const c_char,
    pub(super) width: *const c_char,
    pub(super) enclosure_open: *const c_char,
    pub(super) enclosure_close: *const c_char,
    pub(super) equation: *const CEquationBox,
    pub(super) table_cells: *const CTableCell,
    pub(super) table_rule_cells: *const CTableRuleCell,
    pub(super) child: *const CNode,
    pub(super) next: *const CNode,
}

#[repr(C)]
pub(super) struct CTableCellView {
    pub(super) text: *const c_char,
    pub(super) kind: i32,
    pub(super) layout_kind: i32,
    pub(super) data_kind: i32,
    pub(super) text_block: i32,
    pub(super) source_recovery_safe: i32,
    pub(super) vertical_continuation: i32,
    pub(super) column_span: u32,
    pub(super) row_span: u32,
    pub(super) alignment: i32,
    pub(super) next: *const CTableCell,
}

#[repr(C)]
pub(super) struct CTableRuleCellView {
    pub(super) kind: i32,
    pub(super) next: *const CTableRuleCell,
}

#[repr(C)]
pub(super) struct CEquationBoxView {
    pub(super) kind: i32,
    pub(super) font: i32,
    pub(super) position: i32,
    pub(super) size: i32,
    pub(super) expected_args: u64,
    pub(super) actual_args: u64,
    pub(super) text: *const c_char,
    pub(super) left: *const c_char,
    pub(super) right: *const c_char,
    pub(super) top: *const c_char,
    pub(super) bottom: *const c_char,
    pub(super) first: *const CEquationBox,
    pub(super) next: *const CEquationBox,
}

#[repr(C)]
pub(super) struct CSource {
    pub(super) path: *const c_char,
    pub(super) data: *const u8,
    pub(super) length: usize,
}

#[repr(C)]
pub(super) struct CResolvedSource {
    pub(super) path: *const c_char,
    pub(super) data: *const u8,
    pub(super) length: usize,
}

pub(super) type CSourceResolver =
    extern "C" fn(*mut c_void, *const c_char, *const c_char, *mut CResolvedSource) -> i32;

#[cfg(feature = "execute")]
pub(super) type CExecutionCancellation = extern "C" fn(*mut c_void) -> i32;

#[cfg(any(feature = "execute", feature = "render"))]
#[unsafe(no_mangle)]
extern "C" fn mant_mandoc_utf8_width(codepoint: i32) -> usize {
    u32::try_from(codepoint)
        .ok()
        .and_then(char::from_u32)
        .and_then(UnicodeWidthChar::width)
        .unwrap_or(0)
}

unsafe extern "C" {
    #[cfg(feature = "execute")]
    pub(super) fn mant_mandoc_execution_limits_size() -> usize;
    #[cfg(feature = "execute")]
    pub(super) fn mant_mandoc_execution_limits_align() -> usize;
    #[cfg(feature = "execute")]
    pub(super) fn mant_mandoc_execution_limits_field_count() -> u32;
    #[cfg(feature = "execute")]
    pub(super) fn mant_mandoc_execution_limits_offset(field: u32) -> usize;
    #[cfg(unix)]
    pub(super) fn mant_mandoc_parse_file(
        path: *const c_char,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
        operating_system: *const c_char,
    ) -> *mut CDocument;
    pub(super) fn mant_mandoc_parse_buffer(
        path: *const c_char,
        buffer: *const u8,
        length: usize,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
        operating_system: *const c_char,
        resolver: Option<CSourceResolver>,
        resolver_context: *mut c_void,
    ) -> *mut CDocument;
    pub(super) fn mant_mandoc_parse_bundle(
        root: *const c_char,
        sources: *const CSource,
        source_count: usize,
        input_format: i32,
        operating_system: *const c_char,
    ) -> *mut CDocument;
    #[cfg(feature = "execute")]
    pub(super) fn mant_mandoc_execute_buffer(
        path: *const c_char,
        buffer: *const u8,
        length: usize,
        input_format: i32,
        operating_system: *const c_char,
        limits: *const CExecutionLimits,
        cancelled: Option<CExecutionCancellation>,
        cancellation_context: *mut c_void,
    ) -> *mut CDocument;
    #[cfg(feature = "execute")]
    pub(super) fn mant_mandoc_document_execution(
        document: *const CDocument,
    ) -> *const CExecutionReport;
    #[cfg(feature = "execute")]
    pub(super) fn mant_mandoc_execution_buffer_cell_count(report: *const CExecutionReport) -> u64;
    #[cfg(all(feature = "render", unix))]
    pub(super) fn mant_mandoc_render_file(
        path: *const c_char,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
        operating_system: *const c_char,
        render_format: i32,
        render_width: usize,
        html_fragment: i32,
        output_limit: usize,
    ) -> *mut CDocument;
    #[cfg(feature = "render")]
    pub(super) fn mant_mandoc_render_buffer(
        path: *const c_char,
        buffer: *const u8,
        length: usize,
        include_root: *const c_char,
        allow_include: i32,
        input_format: i32,
        operating_system: *const c_char,
        render_format: i32,
        render_width: usize,
        html_fragment: i32,
        output_limit: usize,
        resolver: Option<CSourceResolver>,
        resolver_context: *mut c_void,
    ) -> *mut CDocument;
    #[cfg(feature = "render")]
    pub(super) fn mant_mandoc_render_bundle(
        root: *const c_char,
        sources: *const CSource,
        source_count: usize,
        input_format: i32,
        operating_system: *const c_char,
        render_format: i32,
        render_width: usize,
        html_fragment: i32,
        output_limit: usize,
    ) -> *mut CDocument;
    pub(super) fn mant_mandoc_document_free(document: *mut CDocument);
    pub(super) fn mant_mandoc_document_ok(document: *const CDocument) -> i32;
    pub(super) fn mant_mandoc_document_error(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_diagnostics(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_macroset(document: *const CDocument) -> i32;
    pub(super) fn mant_mandoc_document_title(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_section(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_volume(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_os(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_arch(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_name(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_date(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_alias_target(document: *const CDocument) -> *const c_char;
    pub(super) fn mant_mandoc_document_has_body(document: *const CDocument) -> i32;
    pub(super) fn mant_mandoc_is_native_roff_request(name: *const c_char, length: usize) -> i32;
    pub(super) fn mant_mandoc_node_view_size() -> usize;
    pub(super) fn mant_mandoc_node_view_align() -> usize;
    pub(super) fn mant_mandoc_node_view_field_count() -> u32;
    pub(super) fn mant_mandoc_node_view_offset(field: u32) -> usize;
    pub(super) fn mant_mandoc_table_cell_view_size() -> usize;
    pub(super) fn mant_mandoc_table_cell_view_align() -> usize;
    pub(super) fn mant_mandoc_table_cell_view_field_count() -> u32;
    pub(super) fn mant_mandoc_table_cell_view_offset(field: u32) -> usize;
    pub(super) fn mant_mandoc_table_rule_cell_view_size() -> usize;
    pub(super) fn mant_mandoc_table_rule_cell_view_align() -> usize;
    pub(super) fn mant_mandoc_table_rule_cell_view_field_count() -> u32;
    pub(super) fn mant_mandoc_table_rule_cell_view_offset(field: u32) -> usize;
    pub(super) fn mant_mandoc_equation_box_view_size() -> usize;
    pub(super) fn mant_mandoc_equation_box_view_align() -> usize;
    pub(super) fn mant_mandoc_equation_box_view_field_count() -> u32;
    pub(super) fn mant_mandoc_equation_box_view_offset(field: u32) -> usize;
    pub(super) fn mant_mandoc_document_root(document: *const CDocument) -> *const CNode;
    pub(super) fn mant_mandoc_node_snapshot(
        document: *mut CDocument,
        node: *const CNode,
        view: *mut CNodeView,
        view_size: usize,
    ) -> i32;
    pub(super) fn mant_mandoc_table_cell_snapshot(
        document: *const CDocument,
        cell: *const CTableCell,
        view: *mut CTableCellView,
        view_size: usize,
    ) -> i32;
    pub(super) fn mant_mandoc_table_rule_cell_snapshot(
        document: *const CDocument,
        cell: *const CTableRuleCell,
        view: *mut CTableRuleCellView,
        view_size: usize,
    ) -> i32;
    pub(super) fn mant_mandoc_equation_box_snapshot(
        document: *const CDocument,
        box_pointer: *const CEquationBox,
        view: *mut CEquationBoxView,
        view_size: usize,
    ) -> i32;
    #[cfg(feature = "render")]
    pub(super) fn mant_mandoc_document_output(document: *const CDocument) -> *const u8;
    #[cfg(feature = "render")]
    pub(super) fn mant_mandoc_document_output_length(document: *const CDocument) -> usize;
    #[cfg(feature = "render")]
    pub(super) fn mant_mandoc_document_render_status(document: *const CDocument) -> i32;
    #[cfg(all(feature = "render", test))]
    pub(super) fn mant_mandoc_ctype_locale() -> *const c_char;
}
