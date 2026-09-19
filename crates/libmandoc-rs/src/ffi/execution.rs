//! Strict, all-or-error transfer of the sealed native execution report.

use super::{
    owned::copy_document_from_handle,
    raw::{self, CDocument, CExecutionLimits, CExecutionReport},
    session::DocumentHandle,
};
use crate::{
    AtomDisposition, AtomKey, AtomKind, AtomRole, BoundaryEffect, BoundaryRequest,
    BufferCloseReason, ExecutionAffinity, ExecutionAnchor, ExecutionAtom, ExecutionBoundary,
    ExecutionBufferGeneration, ExecutionControl, ExecutionControlRequest, ExecutionDiagnostic,
    ExecutionErrorKind, ExecutionFlush, ExecutionFont, ExecutionFragment, ExecutionGeometry,
    ExecutionHeadingKind, ExecutionLimits, ExecutionMdocListKind, ExecutionNode,
    ExecutionNodeFlags, ExecutionNodeKey, ExecutionReference, ExecutionReferenceKind,
    ExecutionSource, ExecutionTable, ExecutionTableAlignment, ExecutionTableCell,
    ExecutionTableCellFlags, ExecutionTableCellKey, ExecutionTableDataKind, ExecutionTableKey,
    ExecutionTableLayoutKind, ExecutionTableRow, ExecutionTableRowKey, ExecutionTableRowKind,
    ExecutionWord, ExecutionWordKey, ExecutionWrapper, ExecutionWrapperKind, FlushOutcome,
    FragmentKey, FragmentRole, GeometryKind, GeometryOriginKind, GeometryUnit,
    NativeExecutionReport, PoolRange, RawDocument,
};
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::{
    collections::BTreeMap,
    mem::{MaybeUninit, align_of, offset_of, size_of},
    ops::Range,
    os::raw::{c_char, c_void},
    path::PathBuf,
    ptr::NonNull,
};

const NONE: u32 = u32::MAX;

#[repr(C)]
#[derive(Clone, Copy)]
struct CSourceRecord {
    key: u32,
    parent: u32,
    include_node: u32,
    flags: u32,
    path_start: u32,
    path_length: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CNodeRecord {
    key: u32,
    parent: u32,
    source: u32,
    line: u32,
    column: u32,
    kind: u32,
    flags: u32,
    macro_start: u32,
    macro_length: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CBufferGenerationRecord {
    key: u32,
    buffer: u32,
    generation: u32,
    capacity: u32,
    extent: u32,
    close_reason: u32,
    reserved: u32,
    open_sequence: u64,
    close_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CWordRecord {
    key: u32,
    node: u32,
    source: u32,
    operand_start: u32,
    operand_length: u32,
    role: u32,
    wrapper: u32,
    enter_atom: u32,
    leave_atom: u32,
    reserved: u32,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CAtomRecord {
    key: u32,
    buffer: u32,
    generation: u32,
    buffer_generation: u32,
    slot: u32,
    kind: u32,
    role: u32,
    input_scalar: u32,
    display_scalar: u32,
    width_bu: i64,
    node: u32,
    source: u32,
    operand_start: u32,
    operand_length: u32,
    font: u32,
    wrapper: u32,
    replaced_by: u32,
    disposition: u32,
    sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CFragmentRecord {
    key: u32,
    node: u32,
    buffer: u32,
    generation: u32,
    buffer_generation: u32,
    atom_ref_start: u32,
    atom_ref_length: u32,
    device_line: u32,
    role: u32,
    wrapper: u32,
    reserved: u32,
    start_bu: i64,
    end_bu: i64,
    sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CFragmentAtomRecord {
    fragment: u32,
    atom: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CFlushRecord {
    key: u32,
    node: u32,
    buffer: u32,
    generation: u32,
    buffer_generation: u32,
    scan_start: u32,
    scan_end: u32,
    accepted_start: u32,
    accepted_end: u32,
    consumed_start: u32,
    consumed_end: u32,
    tail_discarded_start: u32,
    tail_discarded_end: u32,
    remaining_start: u32,
    remaining_end: u32,
    fragment_start: u32,
    fragment_length: u32,
    flags_before: u32,
    flags_after: u32,
    boundary: u32,
    outcome: u32,
    leading_bu: i64,
    content_bu: i64,
    field_bu: i64,
    target_bu: i64,
    taboff_before: i64,
    taboff_after: i64,
    visual_before: i64,
    visual_after: i64,
    sequence: u64,
    outcome_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CBoundaryRecord {
    key: u32,
    node: u32,
    parent: u32,
    request: u32,
    effect: u32,
    flags_before: u32,
    flags_after: u32,
    control: u32,
    line_before: i64,
    line_after: i64,
    visual_before: i64,
    visual_after: i64,
    direct_device_lines: u32,
    wrapper: u32,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CControlRecord {
    key: u32,
    node: u32,
    parent: u32,
    wrapper: u32,
    request: u32,
    buffer: u32,
    generation_before: u32,
    generation_after: u32,
    flags_before: u32,
    flags_after: u32,
    atom_start: u32,
    atom_length: u32,
    fragment_start: u32,
    fragment_length: u32,
    flush_start: u32,
    flush_length: u32,
    boundary_start: u32,
    boundary_length: u32,
    geometry_start: u32,
    geometry_length: u32,
    wrapper_start: u32,
    wrapper_length: u32,
    reserved: u32,
    line_before: i64,
    line_after: i64,
    visual_before: i64,
    visual_after: i64,
    column_before: i64,
    column_after: i64,
    extent_before: i64,
    extent_after: i64,
    offset_before: i64,
    offset_after: i64,
    rmargin_before: i64,
    rmargin_after: i64,
    maxrmargin_before: i64,
    maxrmargin_after: i64,
    taboff_before: i64,
    taboff_after: i64,
    temporary_indent_before: i64,
    temporary_indent_after: i64,
    skip_vertical_before: i64,
    skip_vertical_after: i64,
    minimum_blank_before: i64,
    minimum_blank_after: i64,
    trailing_blank_before: i64,
    trailing_blank_after: i64,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CGeometryRecord {
    key: u32,
    node: u32,
    related: u32,
    kind: u32,
    unit: u32,
    origin_kind: u32,
    origin_key: u32,
    reserved: u32,
    requested: i64,
    effective: i64,
    before: i64,
    after: i64,
    sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CWrapperRecord {
    key: u32,
    parent: u32,
    node: u32,
    kind: u32,
    target_start: u32,
    target_length: u32,
    enter_atom: u32,
    leave_atom: u32,
    detail: u32,
    flags: u32,
    state_before: u32,
    state_after: u32,
    depth_before: u32,
    depth_after: u32,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CReferenceRecord {
    key: u32,
    parent: u32,
    owner_node: u32,
    target_node: u32,
    kind: u32,
    primary_start: u32,
    primary_length: u32,
    secondary_start: u32,
    secondary_length: u32,
    enter_atom: u32,
    label_start_atom: u32,
    leave_atom: u32,
    affinity: u32,
    flags: u32,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CAnchorRecord {
    key: u32,
    node: u32,
    target_start: u32,
    target_length: u32,
    device_line: u32,
    atom_cursor: u32,
    fragment_cursor: u32,
    affinity: u32,
    reserved: u32,
    sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CTableRecord {
    key: u32,
    first_row_node: u32,
    row_start: u32,
    row_length: u32,
    cell_start: u32,
    cell_length: u32,
    logical_columns: u32,
    flags: u32,
    enter_atom: u32,
    leave_atom: u32,
    enter_fragment: u32,
    leave_fragment: u32,
    enter_flush: u32,
    leave_flush: u32,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CTableRowRecord {
    key: u32,
    table: u32,
    node: u32,
    ordinal: u32,
    kind: u32,
    logical_columns: u32,
    cell_start: u32,
    cell_length: u32,
    enter_atom: u32,
    leave_atom: u32,
    enter_fragment: u32,
    leave_fragment: u32,
    enter_flush: u32,
    leave_flush: u32,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CTableCellRecord {
    key: u32,
    row: u32,
    node: u32,
    ordinal: u32,
    data_ordinal: u32,
    logical_column: u32,
    column_span: u32,
    row_span: u32,
    layout_kind: u32,
    data_kind: u32,
    alignment: u32,
    font: u32,
    flags: u32,
    buffer: u32,
    buffer_generation: u32,
    reserved: u32,
    enter_atom: u32,
    leave_atom: u32,
    offset_bu: i64,
    rmargin_bu: i64,
    coloff_before_bu: i64,
    coloff_after_bu: i64,
    enter_sequence: u64,
    leave_sequence: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct CDiagnosticRecord {
    code: u32,
    node: u32,
    message_start: u32,
    message_length: u32,
    sequence: u64,
}

type CountFn = unsafe extern "C" fn(*const CExecutionReport) -> usize;
type SizeFn = unsafe extern "C" fn() -> usize;
type FieldCountFn = unsafe extern "C" fn() -> u32;
type OffsetFn = unsafe extern "C" fn(u32) -> usize;
type CopyFn = unsafe extern "C" fn(
    *const CExecutionReport,
    usize,
    *mut c_void,
    usize,
    usize,
    usize,
    *mut usize,
) -> i32;

struct RecordApi {
    name: &'static str,
    count: CountFn,
    size: SizeFn,
    align: SizeFn,
    field_count: FieldCountFn,
    offset: OffsetFn,
    copy: CopyFn,
}

macro_rules! declare_record_api {
    ($count:ident, $size:ident, $align:ident, $fields:ident, $offset:ident, $copy:ident) => {
        unsafe extern "C" {
            fn $count(report: *const CExecutionReport) -> usize;
            fn $size() -> usize;
            fn $align() -> usize;
            fn $fields() -> u32;
            fn $offset(field: u32) -> usize;
            fn $copy(
                report: *const CExecutionReport,
                start: usize,
                destination: *mut c_void,
                destination_bytes: usize,
                element_size: usize,
                count: usize,
                written: *mut usize,
            ) -> i32;
        }
    };
}

declare_record_api!(
    mant_mandoc_execution_source_count,
    mant_mandoc_execution_source_size,
    mant_mandoc_execution_source_align,
    mant_mandoc_execution_source_field_count,
    mant_mandoc_execution_source_offset,
    mant_mandoc_execution_copy_sources
);
declare_record_api!(
    mant_mandoc_execution_node_count,
    mant_mandoc_execution_node_size,
    mant_mandoc_execution_node_align,
    mant_mandoc_execution_node_field_count,
    mant_mandoc_execution_node_offset,
    mant_mandoc_execution_copy_nodes
);
declare_record_api!(
    mant_mandoc_execution_buffer_generation_count,
    mant_mandoc_execution_buffer_generation_size,
    mant_mandoc_execution_buffer_generation_align,
    mant_mandoc_execution_buffer_generation_field_count,
    mant_mandoc_execution_buffer_generation_offset,
    mant_mandoc_execution_copy_buffer_generations
);
declare_record_api!(
    mant_mandoc_execution_word_count,
    mant_mandoc_execution_word_size,
    mant_mandoc_execution_word_align,
    mant_mandoc_execution_word_field_count,
    mant_mandoc_execution_word_offset,
    mant_mandoc_execution_copy_words
);
declare_record_api!(
    mant_mandoc_execution_atom_count,
    mant_mandoc_execution_atom_size,
    mant_mandoc_execution_atom_align,
    mant_mandoc_execution_atom_field_count,
    mant_mandoc_execution_atom_offset,
    mant_mandoc_execution_copy_atoms
);
declare_record_api!(
    mant_mandoc_execution_fragment_count,
    mant_mandoc_execution_fragment_size,
    mant_mandoc_execution_fragment_align,
    mant_mandoc_execution_fragment_field_count,
    mant_mandoc_execution_fragment_offset,
    mant_mandoc_execution_copy_fragments
);
declare_record_api!(
    mant_mandoc_execution_fragment_atom_count,
    mant_mandoc_execution_fragment_atom_size,
    mant_mandoc_execution_fragment_atom_align,
    mant_mandoc_execution_fragment_atom_field_count,
    mant_mandoc_execution_fragment_atom_offset,
    mant_mandoc_execution_copy_fragment_atoms
);
declare_record_api!(
    mant_mandoc_execution_flush_count,
    mant_mandoc_execution_flush_size,
    mant_mandoc_execution_flush_align,
    mant_mandoc_execution_flush_field_count,
    mant_mandoc_execution_flush_offset,
    mant_mandoc_execution_copy_flushes
);
declare_record_api!(
    mant_mandoc_execution_boundary_count,
    mant_mandoc_execution_boundary_size,
    mant_mandoc_execution_boundary_align,
    mant_mandoc_execution_boundary_field_count,
    mant_mandoc_execution_boundary_offset,
    mant_mandoc_execution_copy_boundaries
);
declare_record_api!(
    mant_mandoc_execution_control_count,
    mant_mandoc_execution_control_size,
    mant_mandoc_execution_control_align,
    mant_mandoc_execution_control_field_count,
    mant_mandoc_execution_control_offset,
    mant_mandoc_execution_copy_controls
);
declare_record_api!(
    mant_mandoc_execution_geometry_count,
    mant_mandoc_execution_geometry_size,
    mant_mandoc_execution_geometry_align,
    mant_mandoc_execution_geometry_field_count,
    mant_mandoc_execution_geometry_offset,
    mant_mandoc_execution_copy_geometries
);
declare_record_api!(
    mant_mandoc_execution_wrapper_count,
    mant_mandoc_execution_wrapper_size,
    mant_mandoc_execution_wrapper_align,
    mant_mandoc_execution_wrapper_field_count,
    mant_mandoc_execution_wrapper_offset,
    mant_mandoc_execution_copy_wrappers
);
declare_record_api!(
    mant_mandoc_execution_reference_count,
    mant_mandoc_execution_reference_size,
    mant_mandoc_execution_reference_align,
    mant_mandoc_execution_reference_field_count,
    mant_mandoc_execution_reference_offset,
    mant_mandoc_execution_copy_references
);
declare_record_api!(
    mant_mandoc_execution_anchor_count,
    mant_mandoc_execution_anchor_size,
    mant_mandoc_execution_anchor_align,
    mant_mandoc_execution_anchor_field_count,
    mant_mandoc_execution_anchor_offset,
    mant_mandoc_execution_copy_anchors
);
declare_record_api!(
    mant_mandoc_execution_table_count,
    mant_mandoc_execution_table_size,
    mant_mandoc_execution_table_align,
    mant_mandoc_execution_table_field_count,
    mant_mandoc_execution_table_offset,
    mant_mandoc_execution_copy_tables
);
declare_record_api!(
    mant_mandoc_execution_table_row_count,
    mant_mandoc_execution_table_row_size,
    mant_mandoc_execution_table_row_align,
    mant_mandoc_execution_table_row_field_count,
    mant_mandoc_execution_table_row_offset,
    mant_mandoc_execution_copy_table_rows
);
declare_record_api!(
    mant_mandoc_execution_table_cell_count,
    mant_mandoc_execution_table_cell_size,
    mant_mandoc_execution_table_cell_align,
    mant_mandoc_execution_table_cell_field_count,
    mant_mandoc_execution_table_cell_offset,
    mant_mandoc_execution_copy_table_cells
);
declare_record_api!(
    mant_mandoc_execution_diagnostic_count,
    mant_mandoc_execution_diagnostic_size,
    mant_mandoc_execution_diagnostic_align,
    mant_mandoc_execution_diagnostic_field_count,
    mant_mandoc_execution_diagnostic_offset,
    mant_mandoc_execution_copy_diagnostics
);

unsafe extern "C" {
    fn mant_mandoc_execution_status(report: *const CExecutionReport) -> i32;
    fn mant_mandoc_execution_error(report: *const CExecutionReport) -> *const c_char;
    fn mant_mandoc_execution_pool_length(report: *const CExecutionReport) -> usize;
    fn mant_mandoc_execution_work_count(report: *const CExecutionReport) -> u64;
    fn mant_mandoc_execution_record_count(report: *const CExecutionReport) -> u64;
    fn mant_mandoc_execution_allocated_record_bytes(report: *const CExecutionReport) -> u64;
    fn mant_mandoc_execution_copy_pool(
        report: *const CExecutionReport,
        start: usize,
        destination: *mut c_void,
        length: usize,
        written: *mut usize,
    ) -> i32;
}

// Rust does not provide stable identifier concatenation, so keep these small
// constructors explicit and let the compiler type-check every symbol.
macro_rules! api {
    ($name:literal, $count:ident, $size:ident, $align:ident, $fields:ident, $offset:ident, $copy:ident) => {
        RecordApi {
            name: $name,
            count: $count,
            size: $size,
            align: $align,
            field_count: $fields,
            offset: $offset,
            copy: $copy,
        }
    };
}

pub(super) fn copy_executed_document(
    pointer: *mut CDocument,
    limits: ExecutionLimits,
) -> Result<(RawDocument, NativeExecutionReport), (ExecutionErrorKind, String)> {
    let handle = DocumentHandle(NonNull::new(pointer).ok_or_else(|| {
        (
            ExecutionErrorKind::Allocation,
            "libmandoc could not allocate an execution document".to_owned(),
        )
    })?);
    let document = handle.0.as_ptr();
    let report = unsafe { raw::mant_mandoc_document_execution(document) };
    if report.is_null() {
        let message =
            unsafe { super::owned::optional_string(raw::mant_mandoc_document_error(document)) }
                .unwrap_or_else(|| "libmandoc produced no execution report".to_owned());
        return Err((ExecutionErrorKind::Allocation, message));
    }
    let status = unsafe { mant_mandoc_execution_status(report) };
    if status != 1 {
        let kind = match status {
            2 => ExecutionErrorKind::Unsupported,
            3 => ExecutionErrorKind::Budget,
            4 => ExecutionErrorKind::Allocation,
            6 => ExecutionErrorKind::Cancelled,
            _ => ExecutionErrorKind::Native,
        };
        let message = unsafe { super::owned::optional_string(mant_mandoc_execution_error(report)) }
            .unwrap_or_else(|| "native execution failed without a diagnostic".to_owned());
        return Err((kind, message));
    }
    let execution_node_count = unsafe { mant_mandoc_execution_node_count(report) };
    validate_execution_node_transfer_count(execution_node_count, limits)
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    let mut ast_keys = reserved_vec(execution_node_count, "AST node-key")
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    let raw_document = copy_document_from_handle(&handle, Some(&mut ast_keys))
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    if raw_document.node_truncated || raw_document.equation_truncated {
        return Err((
            ExecutionErrorKind::Transfer,
            "native execution cannot transfer a truncated syntax tree".to_owned(),
        ));
    }
    let execution = unsafe { copy_report(report, limits) }
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    validate_execution_ast_bindings(&raw_document.document.root, &execution)
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    if ast_keys.len() != execution.nodes.len() {
        return Err((
            ExecutionErrorKind::Transfer,
            "owned syntax tree and execution report node identities differ".to_owned(),
        ));
    }
    Ok((raw_document, execution))
}

fn validate_execution_node_transfer_count(
    node_count: usize,
    limits: ExecutionLimits,
) -> Result<(), String> {
    if u64::try_from(node_count).map_or(true, |count| count > limits.max_nodes)
        || node_count > (isize::MAX as usize) / size_of::<u32>()
    {
        return Err("native execution node transfer exceeds its declared limit".to_owned());
    }
    Ok(())
}

fn stable_ast_node_flags(flags: crate::NodeFlags) -> u32 {
    u32::from(flags.generated)
        | u32::from(flags.sentence_end) << 1
        | u32::from(flags.no_print) << 2
        | u32::from(flags.no_fill) << 3
        | u32::from(flags.deep_link_target) << 4
        | u32::from(flags.permalink) << 5
        | u32::from(flags.line_start) << 6
        | u32::from(flags.delimiter_open) << 7
        | u32::from(flags.delimiter_close) << 8
        | u32::from(flags.synopsis_pretty) << 9
}

fn validate_execution_ast_bindings(
    root: &crate::Node,
    report: &NativeExecutionReport,
) -> Result<(), String> {
    let ast_nodes = collect_execution_ast_nodes(root, report)?;
    validate_execution_table_ast_bindings(&ast_nodes, report)
}

fn collect_execution_ast_nodes<'a>(
    root: &'a crate::Node,
    report: &NativeExecutionReport,
) -> Result<Vec<&'a crate::Node>, String> {
    let mut ast_nodes = reserved_vec(report.nodes.len(), "AST execution binding")?;
    let mut pending = reserved_vec(1, "AST execution traversal")?;
    pending.push((root, None));
    while let Some((node, parent)) = pending.pop() {
        let key = node
            .execution_node_key
            .ok_or_else(|| "executed AST node has no report-local identity".to_owned())?;
        if key as usize != ast_nodes.len() {
            return Err("executed AST node identities are not dense DFS keys".to_owned());
        }
        let origin = report
            .nodes
            .get(key as usize)
            .ok_or_else(|| "executed AST node key is out of range".to_owned())?;
        if origin.key.0 != key
            || origin.parent.map(|key| key.0) != parent
            || origin.source != 0
            || origin.kind != node.kind
            || origin.flags != ExecutionNodeFlags(stable_ast_node_flags(node.flags))
            || origin.macro_name != node.macro_name
            || origin.line != node.line
            || origin.column != node.column
        {
            return Err("owned syntax node does not match its execution origin".to_owned());
        }
        ast_nodes.push(node);
        pending
            .try_reserve(node.children.len())
            .map_err(|_| "could not allocate AST execution traversal".to_owned())?;
        pending.extend(node.children.iter().rev().map(|child| (child, Some(key))));
    }
    if ast_nodes.len() != report.nodes.len() {
        return Err("owned syntax tree and execution report node counts differ".to_owned());
    }
    Ok(ast_nodes)
}

fn validate_execution_table_ast_bindings(
    ast_nodes: &[&crate::Node],
    report: &NativeExecutionReport,
) -> Result<(), String> {
    let mut bound_rows = reserved_filled_vec(false, ast_nodes.len(), "AST table-row binding")?;
    for row in &report.table_rows {
        let ast_row = ast_nodes
            .get(row.node.0 as usize)
            .ok_or_else(|| "execution table row has no matching AST node".to_owned())?;
        if bound_rows[row.node.0 as usize] {
            return Err("owned AST table row has multiple execution rows".to_owned());
        }
        bound_rows[row.node.0 as usize] = true;
        let expected_row_kind = match ast_row.table_row_kind.as_ref() {
            Some(crate::TableRowKind::Data | crate::TableRowKind::LayoutRule { .. }) => {
                ExecutionTableRowKind::Data
            }
            Some(crate::TableRowKind::HorizontalRule) => ExecutionTableRowKind::HorizontalRule,
            Some(crate::TableRowKind::DoubleHorizontalRule) => {
                ExecutionTableRowKind::DoubleHorizontalRule
            }
            None => {
                return Err("execution table row is not a table row in the owned AST".to_owned());
            }
        };
        if ast_row.kind != crate::NodeKind::Table || row.kind != expected_row_kind {
            return Err("execution table row kind does not match the owned AST".to_owned());
        }
        let cells = &report.table_cells[row.cells.start as usize..row.cells.end as usize];
        let mut logical_column = 0_u32;
        for (expected_data_ordinal, (cell, ast_cell)) in
            cells.iter().zip(&ast_row.table_cells).enumerate()
        {
            let expected_kind = match ast_cell.kind {
                crate::TableCellKind::Text => ExecutionTableDataKind::Text,
                crate::TableCellKind::Empty => ExecutionTableDataKind::None,
                crate::TableCellKind::HorizontalRule => ExecutionTableDataKind::HorizontalRule,
                crate::TableCellKind::DoubleHorizontalRule => {
                    ExecutionTableDataKind::DoubleHorizontalRule
                }
                crate::TableCellKind::IsolatedHorizontalRule => {
                    ExecutionTableDataKind::IsolatedHorizontalRule
                }
                crate::TableCellKind::IsolatedDoubleHorizontalRule => {
                    ExecutionTableDataKind::IsolatedDoubleHorizontalRule
                }
            };
            let expected_alignment = match ast_cell.alignment {
                crate::TableAlignment::Left => matches!(
                    cell.alignment,
                    ExecutionTableAlignment::None
                        | ExecutionTableAlignment::Left
                        | ExecutionTableAlignment::Long
                ),
                crate::TableAlignment::Center => cell.alignment == ExecutionTableAlignment::Center,
                crate::TableAlignment::Right => matches!(
                    cell.alignment,
                    ExecutionTableAlignment::Right | ExecutionTableAlignment::Numeric
                ),
            };
            if usize::try_from(cell.data_ordinal).ok() != Some(expected_data_ordinal)
                || cell.node != row.node
                || cell.logical_column != logical_column
                || cell.column_span != u32::from(ast_cell.column_span)
                || cell.row_span != u32::from(ast_cell.row_span)
                || cell.data_kind != expected_kind
                || !expected_alignment
                || cell.flags.contains(ExecutionTableCellFlags::TEXT_BLOCK) != ast_cell.text_block
                || cell
                    .flags
                    .contains(ExecutionTableCellFlags::SOURCE_RECOVERY_SAFE)
                    != ast_cell.source_recovery_safe
                || cell
                    .flags
                    .contains(ExecutionTableCellFlags::VERTICAL_CONTINUATION)
                    != ast_cell.vertical_continuation
            {
                return Err("execution table cell does not match the owned AST row".to_owned());
            }
            logical_column = logical_column
                .checked_add(cell.column_span)
                .ok_or_else(|| "execution table logical column overflow".to_owned())?;
        }
        if cells.len() != ast_row.table_cells.len() || logical_column > row.logical_columns {
            return Err("execution table cells do not match the owned AST row".to_owned());
        }
    }
    if ast_nodes.iter().enumerate().any(|(key, node)| {
        node.table_row_kind.is_some() != bound_rows.get(key).copied().unwrap_or(false)
    }) {
        return Err("owned AST and execution report table-row sets differ".to_owned());
    }
    Ok(())
}

pub(super) fn native_limits(limits: ExecutionLimits) -> CExecutionLimits {
    CExecutionLimits {
        abi_version: 2,
        abi_size: u32::try_from(size_of::<CExecutionLimits>())
            .expect("execution limits ABI size fits in u32"),
        max_nodes: limits.max_nodes,
        max_depth: limits.max_depth,
        max_work: limits.max_work,
        max_records: limits.max_records,
        max_pool_bytes: limits.max_pool_bytes,
        max_buffer_cells: limits.max_buffer_cells,
        max_report_bytes: limits.max_report_bytes,
    }
}

pub(super) fn validate_limits_layout() -> Result<(), String> {
    let offsets = [
        offset_of!(CExecutionLimits, abi_version),
        offset_of!(CExecutionLimits, abi_size),
        offset_of!(CExecutionLimits, max_nodes),
        offset_of!(CExecutionLimits, max_depth),
        offset_of!(CExecutionLimits, max_work),
        offset_of!(CExecutionLimits, max_records),
        offset_of!(CExecutionLimits, max_pool_bytes),
        offset_of!(CExecutionLimits, max_buffer_cells),
        offset_of!(CExecutionLimits, max_report_bytes),
    ];
    validate_record_layout(
        "execution-limits",
        unsafe { raw::mant_mandoc_execution_limits_size() },
        unsafe { raw::mant_mandoc_execution_limits_align() },
        unsafe { raw::mant_mandoc_execution_limits_field_count() },
        size_of::<CExecutionLimits>(),
        align_of::<CExecutionLimits>(),
        &offsets,
        |field| unsafe { raw::mant_mandoc_execution_limits_offset(field) },
    )
}

unsafe fn copy_report(
    report: *const CExecutionReport,
    limits: ExecutionLimits,
) -> Result<NativeExecutionReport, String> {
    let work_units = unsafe { mant_mandoc_execution_work_count(report) };
    let record_count = unsafe { mant_mandoc_execution_record_count(report) };
    let record_bytes = unsafe { mant_mandoc_execution_allocated_record_bytes(report) };
    let buffer_cells = unsafe { raw::mant_mandoc_execution_buffer_cell_count(report) };
    let pool_length = unsafe { mant_mandoc_execution_pool_length(report) };
    if work_units > limits.max_work
        || record_count > limits.max_records
        || record_bytes > limits.max_report_bytes
        || buffer_cells > limits.max_buffer_cells
        || u64::try_from(pool_length).map_or(true, |length| length > limits.max_pool_bytes)
        || pool_length > isize::MAX as usize
    {
        return Err("native execution transfer exceeds its declared limits".to_owned());
    }
    let expected_records = usize::try_from(record_count)
        .map_err(|_| "native execution record count exceeds addressable memory".to_owned())?;
    let node_count = unsafe { mant_mandoc_execution_node_count(report) };
    validate_execution_node_transfer_count(node_count, limits)?;
    let records = unsafe { copy_raw_records(report, expected_records) }?;
    let pool = unsafe { copy_pool(report, pool_length) }?;
    let mut owned = convert_report(pool, work_units, record_count, buffer_cells, records)?;
    owned.record_bytes = record_bytes;
    Ok(owned)
}

struct RawRecords {
    sources: Vec<CSourceRecord>,
    nodes: Vec<CNodeRecord>,
    buffer_generations: Vec<CBufferGenerationRecord>,
    words: Vec<CWordRecord>,
    atoms: Vec<CAtomRecord>,
    fragments: Vec<CFragmentRecord>,
    fragment_atoms: Vec<CFragmentAtomRecord>,
    flushes: Vec<CFlushRecord>,
    boundaries: Vec<CBoundaryRecord>,
    controls: Vec<CControlRecord>,
    geometry: Vec<CGeometryRecord>,
    wrappers: Vec<CWrapperRecord>,
    references: Vec<CReferenceRecord>,
    anchors: Vec<CAnchorRecord>,
    tables: Vec<CTableRecord>,
    table_rows: Vec<CTableRowRecord>,
    table_cells: Vec<CTableCellRecord>,
    diagnostics: Vec<CDiagnosticRecord>,
}

macro_rules! copy_record_table {
    ($function:ident, $type:ty, $offsets:ident, $name:literal, $count:ident, $size:ident, $align:ident, $fields:ident, $offset:ident, $copy:ident) => {
        unsafe fn $function(
            report: *const CExecutionReport,
            remaining: &mut usize,
        ) -> Result<Vec<$type>, String> {
            unsafe {
                copy_table(
                    report,
                    &api!($name, $count, $size, $align, $fields, $offset, $copy),
                    &$offsets(),
                    remaining,
                )
            }
        }
    };
}

copy_record_table!(
    copy_sources,
    CSourceRecord,
    source_offsets,
    "source",
    mant_mandoc_execution_source_count,
    mant_mandoc_execution_source_size,
    mant_mandoc_execution_source_align,
    mant_mandoc_execution_source_field_count,
    mant_mandoc_execution_source_offset,
    mant_mandoc_execution_copy_sources
);
copy_record_table!(
    copy_nodes,
    CNodeRecord,
    node_offsets,
    "node",
    mant_mandoc_execution_node_count,
    mant_mandoc_execution_node_size,
    mant_mandoc_execution_node_align,
    mant_mandoc_execution_node_field_count,
    mant_mandoc_execution_node_offset,
    mant_mandoc_execution_copy_nodes
);
copy_record_table!(
    copy_buffer_generations,
    CBufferGenerationRecord,
    buffer_generation_offsets,
    "buffer-generation",
    mant_mandoc_execution_buffer_generation_count,
    mant_mandoc_execution_buffer_generation_size,
    mant_mandoc_execution_buffer_generation_align,
    mant_mandoc_execution_buffer_generation_field_count,
    mant_mandoc_execution_buffer_generation_offset,
    mant_mandoc_execution_copy_buffer_generations
);
copy_record_table!(
    copy_words,
    CWordRecord,
    word_offsets,
    "word",
    mant_mandoc_execution_word_count,
    mant_mandoc_execution_word_size,
    mant_mandoc_execution_word_align,
    mant_mandoc_execution_word_field_count,
    mant_mandoc_execution_word_offset,
    mant_mandoc_execution_copy_words
);
copy_record_table!(
    copy_atoms,
    CAtomRecord,
    atom_offsets,
    "atom",
    mant_mandoc_execution_atom_count,
    mant_mandoc_execution_atom_size,
    mant_mandoc_execution_atom_align,
    mant_mandoc_execution_atom_field_count,
    mant_mandoc_execution_atom_offset,
    mant_mandoc_execution_copy_atoms
);
copy_record_table!(
    copy_fragments,
    CFragmentRecord,
    fragment_offsets,
    "fragment",
    mant_mandoc_execution_fragment_count,
    mant_mandoc_execution_fragment_size,
    mant_mandoc_execution_fragment_align,
    mant_mandoc_execution_fragment_field_count,
    mant_mandoc_execution_fragment_offset,
    mant_mandoc_execution_copy_fragments
);
copy_record_table!(
    copy_fragment_atoms,
    CFragmentAtomRecord,
    fragment_atom_offsets,
    "fragment-atom",
    mant_mandoc_execution_fragment_atom_count,
    mant_mandoc_execution_fragment_atom_size,
    mant_mandoc_execution_fragment_atom_align,
    mant_mandoc_execution_fragment_atom_field_count,
    mant_mandoc_execution_fragment_atom_offset,
    mant_mandoc_execution_copy_fragment_atoms
);
copy_record_table!(
    copy_flushes,
    CFlushRecord,
    flush_offsets,
    "flush",
    mant_mandoc_execution_flush_count,
    mant_mandoc_execution_flush_size,
    mant_mandoc_execution_flush_align,
    mant_mandoc_execution_flush_field_count,
    mant_mandoc_execution_flush_offset,
    mant_mandoc_execution_copy_flushes
);
copy_record_table!(
    copy_boundaries,
    CBoundaryRecord,
    boundary_offsets,
    "boundary",
    mant_mandoc_execution_boundary_count,
    mant_mandoc_execution_boundary_size,
    mant_mandoc_execution_boundary_align,
    mant_mandoc_execution_boundary_field_count,
    mant_mandoc_execution_boundary_offset,
    mant_mandoc_execution_copy_boundaries
);
copy_record_table!(
    copy_controls,
    CControlRecord,
    control_offsets,
    "control",
    mant_mandoc_execution_control_count,
    mant_mandoc_execution_control_size,
    mant_mandoc_execution_control_align,
    mant_mandoc_execution_control_field_count,
    mant_mandoc_execution_control_offset,
    mant_mandoc_execution_copy_controls
);
copy_record_table!(
    copy_geometry,
    CGeometryRecord,
    geometry_offsets,
    "geometry",
    mant_mandoc_execution_geometry_count,
    mant_mandoc_execution_geometry_size,
    mant_mandoc_execution_geometry_align,
    mant_mandoc_execution_geometry_field_count,
    mant_mandoc_execution_geometry_offset,
    mant_mandoc_execution_copy_geometries
);
copy_record_table!(
    copy_wrappers,
    CWrapperRecord,
    wrapper_offsets,
    "wrapper",
    mant_mandoc_execution_wrapper_count,
    mant_mandoc_execution_wrapper_size,
    mant_mandoc_execution_wrapper_align,
    mant_mandoc_execution_wrapper_field_count,
    mant_mandoc_execution_wrapper_offset,
    mant_mandoc_execution_copy_wrappers
);
copy_record_table!(
    copy_references,
    CReferenceRecord,
    reference_offsets,
    "reference",
    mant_mandoc_execution_reference_count,
    mant_mandoc_execution_reference_size,
    mant_mandoc_execution_reference_align,
    mant_mandoc_execution_reference_field_count,
    mant_mandoc_execution_reference_offset,
    mant_mandoc_execution_copy_references
);
copy_record_table!(
    copy_anchors,
    CAnchorRecord,
    anchor_offsets,
    "anchor",
    mant_mandoc_execution_anchor_count,
    mant_mandoc_execution_anchor_size,
    mant_mandoc_execution_anchor_align,
    mant_mandoc_execution_anchor_field_count,
    mant_mandoc_execution_anchor_offset,
    mant_mandoc_execution_copy_anchors
);
copy_record_table!(
    copy_tables,
    CTableRecord,
    table_offsets,
    "table",
    mant_mandoc_execution_table_count,
    mant_mandoc_execution_table_size,
    mant_mandoc_execution_table_align,
    mant_mandoc_execution_table_field_count,
    mant_mandoc_execution_table_offset,
    mant_mandoc_execution_copy_tables
);
copy_record_table!(
    copy_table_rows,
    CTableRowRecord,
    table_row_offsets,
    "table-row",
    mant_mandoc_execution_table_row_count,
    mant_mandoc_execution_table_row_size,
    mant_mandoc_execution_table_row_align,
    mant_mandoc_execution_table_row_field_count,
    mant_mandoc_execution_table_row_offset,
    mant_mandoc_execution_copy_table_rows
);
copy_record_table!(
    copy_table_cells,
    CTableCellRecord,
    table_cell_offsets,
    "table-cell",
    mant_mandoc_execution_table_cell_count,
    mant_mandoc_execution_table_cell_size,
    mant_mandoc_execution_table_cell_align,
    mant_mandoc_execution_table_cell_field_count,
    mant_mandoc_execution_table_cell_offset,
    mant_mandoc_execution_copy_table_cells
);
copy_record_table!(
    copy_diagnostics,
    CDiagnosticRecord,
    diagnostic_offsets,
    "diagnostic",
    mant_mandoc_execution_diagnostic_count,
    mant_mandoc_execution_diagnostic_size,
    mant_mandoc_execution_diagnostic_align,
    mant_mandoc_execution_diagnostic_field_count,
    mant_mandoc_execution_diagnostic_offset,
    mant_mandoc_execution_copy_diagnostics
);

unsafe fn copy_raw_records(
    report: *const CExecutionReport,
    expected_records: usize,
) -> Result<RawRecords, String> {
    let mut remaining = expected_records;
    let records = RawRecords {
        sources: unsafe { copy_sources(report, &mut remaining) }?,
        nodes: unsafe { copy_nodes(report, &mut remaining) }?,
        buffer_generations: unsafe { copy_buffer_generations(report, &mut remaining) }?,
        words: unsafe { copy_words(report, &mut remaining) }?,
        atoms: unsafe { copy_atoms(report, &mut remaining) }?,
        fragments: unsafe { copy_fragments(report, &mut remaining) }?,
        fragment_atoms: unsafe { copy_fragment_atoms(report, &mut remaining) }?,
        flushes: unsafe { copy_flushes(report, &mut remaining) }?,
        boundaries: unsafe { copy_boundaries(report, &mut remaining) }?,
        controls: unsafe { copy_controls(report, &mut remaining) }?,
        geometry: unsafe { copy_geometry(report, &mut remaining) }?,
        wrappers: unsafe { copy_wrappers(report, &mut remaining) }?,
        references: unsafe { copy_references(report, &mut remaining) }?,
        anchors: unsafe { copy_anchors(report, &mut remaining) }?,
        tables: unsafe { copy_tables(report, &mut remaining) }?,
        table_rows: unsafe { copy_table_rows(report, &mut remaining) }?,
        table_cells: unsafe { copy_table_cells(report, &mut remaining) }?,
        diagnostics: unsafe { copy_diagnostics(report, &mut remaining) }?,
    };
    if remaining != 0 {
        return Err("native execution record accounting mismatch".to_owned());
    }
    Ok(records)
}

unsafe fn copy_table<T: Copy>(
    report: *const CExecutionReport,
    api: &RecordApi,
    offsets: &[usize],
    remaining: &mut usize,
) -> Result<Vec<T>, String> {
    let native_size = unsafe { (api.size)() };
    let native_align = unsafe { (api.align)() };
    let native_fields = unsafe { (api.field_count)() };
    validate_record_layout(
        api.name,
        native_size,
        native_align,
        native_fields,
        size_of::<T>(),
        align_of::<T>(),
        offsets,
        |field| unsafe { (api.offset)(field) },
    )?;
    let count = unsafe { (api.count)(report) };
    if count > *remaining {
        return Err(format!(
            "libmandoc {} record count exceeds the sealed report",
            api.name
        ));
    }
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or_else(|| format!("libmandoc {} record byte count overflow", api.name))?;
    if bytes > isize::MAX as usize {
        return Err(format!(
            "libmandoc {} record byte count exceeds addressable memory",
            api.name
        ));
    }
    let mut records = Vec::<MaybeUninit<T>>::new();
    records
        .try_reserve_exact(count)
        .map_err(|_| format!("could not allocate {} record transfer", api.name))?;
    let mut written = 0;
    if unsafe {
        (api.copy)(
            report,
            0,
            records.as_mut_ptr().cast(),
            bytes,
            size_of::<T>(),
            count,
            &raw mut written,
        )
    } != 1
        || written != count
    {
        return Err(format!(
            "libmandoc rejected the {} record transfer",
            api.name
        ));
    }
    unsafe { records.set_len(count) };
    let pointer = records.as_mut_ptr().cast::<T>();
    let length = records.len();
    let capacity = records.capacity();
    std::mem::forget(records);
    *remaining -= count;
    Ok(unsafe { Vec::from_raw_parts(pointer, length, capacity) })
}

#[allow(clippy::too_many_arguments)]
fn validate_record_layout(
    name: &str,
    native_size: usize,
    native_align: usize,
    native_fields: u32,
    expected_size: usize,
    expected_align: usize,
    expected_offsets: &[usize],
    mut native_offset: impl FnMut(u32) -> usize,
) -> Result<(), String> {
    if native_size != expected_size
        || native_align != expected_align
        || usize::try_from(native_fields).ok() != Some(expected_offsets.len())
    {
        return Err(format!("libmandoc {name} record ABI mismatch"));
    }
    for (field, expected) in expected_offsets.iter().copied().enumerate() {
        let field =
            u32::try_from(field).map_err(|_| format!("libmandoc {name} field count overflow"))?;
        if native_offset(field) != expected {
            return Err(format!(
                "libmandoc {name} record offset mismatch at field {field}"
            ));
        }
    }
    Ok(())
}

unsafe fn copy_pool(
    report: *const CExecutionReport,
    expected_length: usize,
) -> Result<Vec<u8>, String> {
    let length = unsafe { mant_mandoc_execution_pool_length(report) };
    if length != expected_length || length > isize::MAX as usize {
        return Err("native execution byte-pool length changed during transfer".to_owned());
    }
    let mut pool = Vec::new();
    pool.try_reserve_exact(length)
        .map_err(|_| "could not allocate execution byte-pool transfer".to_owned())?;
    pool.resize(length, 0);
    let mut written = 0;
    if unsafe {
        mant_mandoc_execution_copy_pool(
            report,
            0,
            pool.as_mut_ptr().cast(),
            length,
            &raw mut written,
        )
    } != 1
        || written != length
    {
        return Err("libmandoc rejected the execution byte-pool transfer".to_owned());
    }
    Ok(pool)
}

fn option(value: u32) -> Option<u32> {
    (value != NONE).then_some(value)
}
fn node_key(value: u32, nodes: usize, field: &str) -> Result<ExecutionNodeKey, String> {
    if usize::try_from(value).is_ok_and(|key| key < nodes) {
        Ok(ExecutionNodeKey(value))
    } else {
        Err(format!("invalid execution node key in {field}"))
    }
}
fn node_is_within(
    nodes: &[ExecutionNode],
    mut node: ExecutionNodeKey,
    ancestor: ExecutionNodeKey,
) -> bool {
    loop {
        if node == ancestor {
            return true;
        }
        let Some(parent) = nodes[node.0 as usize].parent else {
            return false;
        };
        node = parent;
    }
}
fn optional_node_key(
    value: u32,
    nodes: usize,
    field: &str,
) -> Result<Option<ExecutionNodeKey>, String> {
    option(value)
        .map(|key| node_key(key, nodes, field))
        .transpose()
}
fn pool_range(start: u32, length: u32, pool: &[u8], field: &str) -> Result<PoolRange, String> {
    let range = PoolRange { start, length };
    if range
        .as_range()
        .is_some_and(|range| range.end <= pool.len())
    {
        Ok(range)
    } else {
        Err(format!("invalid execution pool range in {field}"))
    }
}
fn optional_pool(
    start: u32,
    length: u32,
    pool: &[u8],
    field: &str,
) -> Result<Option<PoolRange>, String> {
    if start == NONE {
        if length == 0 {
            Ok(None)
        } else {
            Err(format!("invalid absent execution pool range in {field}"))
        }
    } else {
        pool_range(start, length, pool, field).map(Some)
    }
}
fn range(start: u32, end: u32, field: &str) -> Result<std::ops::Range<u32>, String> {
    (start <= end)
        .then_some(start..end)
        .ok_or_else(|| format!("invalid execution range in {field}"))
}

const fn terminal_tail_scalar(scalar: u32) -> bool {
    matches!(scalar, 9 | 32 | 10 | 30 | 29 | 26)
}
fn counted_range(start: u32, length: u32, field: &str) -> Result<Range<u32>, String> {
    start
        .checked_add(length)
        .map(|end| start..end)
        .ok_or_else(|| format!("invalid execution counted range in {field}"))
}
fn exact_sequence_range<T>(
    values: &[T],
    selected: &Range<u32>,
    enter_sequence: u64,
    leave_sequence: u64,
    sequence: impl Fn(&T) -> u64,
) -> bool {
    let Ok(start) = usize::try_from(selected.start) else {
        return false;
    };
    let Ok(end) = usize::try_from(selected.end) else {
        return false;
    };
    if start > end || end > values.len() || enter_sequence >= leave_sequence {
        return false;
    }
    if start > 0 && sequence(&values[start - 1]) >= enter_sequence {
        return false;
    }
    if start < values.len() && sequence(&values[start]) <= enter_sequence {
        return false;
    }
    if end > 0 && sequence(&values[end - 1]) >= leave_sequence {
        return false;
    }
    if end < values.len() && sequence(&values[end]) <= leave_sequence {
        return false;
    }
    true
}

#[derive(Clone, Copy)]
enum GenerationEventKind {
    Open(u32),
    Close(u32),
    Check(Option<u32>),
}

#[derive(Clone, Copy)]
struct GenerationEvent {
    sequence: u64,
    buffer: u32,
    kind: GenerationEventKind,
}

fn validate_control_generations(
    generations: &[ExecutionBufferGeneration],
    controls: &[ExecutionControl],
) -> Result<(), String> {
    let event_count = generations
        .len()
        .checked_add(controls.len())
        .and_then(|count| count.checked_mul(2))
        .ok_or_else(|| "execution generation checkpoint count overflow".to_owned())?;
    let mut events = reserved_vec(event_count, "generation checkpoints")?;
    for generation in generations {
        events.push(GenerationEvent {
            sequence: generation.open_sequence,
            buffer: generation.buffer,
            kind: GenerationEventKind::Open(generation.key),
        });
        events.push(GenerationEvent {
            sequence: generation.close_sequence,
            buffer: generation.buffer,
            kind: GenerationEventKind::Close(generation.key),
        });
    }
    for control in controls {
        events.push(GenerationEvent {
            sequence: control.enter_sequence,
            buffer: control.buffer,
            kind: GenerationEventKind::Check(control.generation_before),
        });
        events.push(GenerationEvent {
            sequence: control.leave_sequence,
            buffer: control.buffer,
            kind: GenerationEventKind::Check(control.generation_after),
        });
    }
    events.sort_unstable_by_key(|event| event.sequence);
    if events
        .windows(2)
        .any(|pair| pair[0].sequence == pair[1].sequence)
    {
        return Err("execution generation checkpoints share a sequence".to_owned());
    }
    let mut active = BTreeMap::<u32, u32>::new();
    for event in events {
        match event.kind {
            GenerationEventKind::Open(generation) => {
                if active.insert(event.buffer, generation).is_some() {
                    return Err("execution buffer generations overlap".to_owned());
                }
            }
            GenerationEventKind::Close(generation) => {
                if active.remove(&event.buffer) != Some(generation) {
                    return Err("execution buffer generation close is inconsistent".to_owned());
                }
            }
            GenerationEventKind::Check(generation) => {
                if active.get(&event.buffer).copied() != generation {
                    return Err("execution control buffer generation is inconsistent".to_owned());
                }
            }
        }
    }
    Ok(())
}

fn validate_active_wrapper_owners(
    wrappers: &[ExecutionWrapper],
    boundaries: &[ExecutionBoundary],
    controls: &[ExecutionControl],
) -> Result<(), String> {
    let event_count = boundaries
        .len()
        .checked_add(controls.len())
        .ok_or_else(|| "execution wrapper checkpoint count overflow".to_owned())?;
    let mut events = reserved_vec(event_count, "wrapper checkpoints")?;
    events.extend(
        boundaries
            .iter()
            .map(|boundary| (boundary.enter_sequence, boundary.wrapper, false)),
    );
    events.extend(
        controls
            .iter()
            .map(|control| (control.enter_sequence, Some(control.wrapper), true)),
    );
    events.sort_unstable_by_key(|event| event.0);
    let mut active = reserved_vec(wrappers.len(), "active node wrappers")?;
    let mut cursor = 0_usize;
    for (sequence, owner, is_control) in events {
        while wrappers
            .get(cursor)
            .is_some_and(|wrapper| wrapper.enter_sequence < sequence)
        {
            let wrapper = &wrappers[cursor];
            while active.last().is_some_and(|parent| {
                wrappers[*parent as usize].leave_sequence <= wrapper.enter_sequence
            }) {
                active.pop();
            }
            if wrapper.parent != active.last().copied() {
                return Err("execution wrapper parent is not its active node owner".to_owned());
            }
            if is_structural_wrapper(wrapper.kind) {
                active.push(wrapper.key);
            }
            cursor += 1;
        }
        while active
            .last()
            .is_some_and(|wrapper| wrappers[*wrapper as usize].leave_sequence <= sequence)
        {
            active.pop();
        }
        if owner != active.last().copied() {
            return Err(if is_control {
                "execution control does not match its active wrapper".to_owned()
            } else {
                "execution boundary does not match its active wrapper".to_owned()
            });
        }
    }
    Ok(())
}

fn is_structural_wrapper(kind: ExecutionWrapperKind) -> bool {
    matches!(
        kind,
        ExecutionWrapperKind::Node
            | ExecutionWrapperKind::Heading
            | ExecutionWrapperKind::MdocListItem
    )
}

fn is_mdoc_list_item(nodes: &[ExecutionNode], node: ExecutionNodeKey) -> bool {
    let item = &nodes[node.0 as usize];
    item.kind == crate::NodeKind::Block
        && item.macro_name.as_deref() == Some("It")
        && item.parent.is_some_and(|body| {
            let body = &nodes[body.0 as usize];
            body.kind == crate::NodeKind::Body
                && body.parent.is_some_and(|list| {
                    let list = &nodes[list.0 as usize];
                    list.kind == crate::NodeKind::Block && list.macro_name.as_deref() == Some("Bl")
                })
        })
}

fn expected_heading_kind(node: &ExecutionNode) -> Option<ExecutionHeadingKind> {
    if node.kind != crate::NodeKind::Head {
        return None;
    }
    match node.macro_name.as_deref() {
        Some("SH") => Some(ExecutionHeadingKind::ManSection),
        Some("SS") => Some(ExecutionHeadingKind::ManSubsection),
        Some("Sh") => Some(ExecutionHeadingKind::MdocSection),
        Some("Ss") => Some(ExecutionHeadingKind::MdocSubsection),
        _ => None,
    }
}
fn dense(key: u32, index: usize, field: &str) -> Result<(), String> {
    (usize::try_from(key).ok() == Some(index))
        .then_some(())
        .ok_or_else(|| format!("non-dense execution key in {field}"))
}

fn reserved_vec<T>(count: usize, field: &str) -> Result<Vec<T>, String> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| format!("could not allocate converted execution {field} records"))?;
    Ok(values)
}

fn reserved_filled_vec<T: Clone>(value: T, count: usize, field: &str) -> Result<Vec<T>, String> {
    let mut values = reserved_vec(count, field)?;
    values.resize(count, value);
    Ok(values)
}

fn copy_bytes(value: &[u8], field: &str) -> Result<Vec<u8>, String> {
    let mut copied = reserved_vec(value.len(), field)?;
    copied.extend_from_slice(value);
    Ok(copied)
}

fn copy_utf8(value: &[u8], field: &str) -> Result<String, String> {
    let value =
        std::str::from_utf8(value).map_err(|_| format!("execution {field} is not UTF-8"))?;
    let mut copied = String::new();
    copied
        .try_reserve_exact(value.len())
        .map_err(|_| format!("could not allocate execution {field}"))?;
    copied.push_str(value);
    Ok(copied)
}

#[allow(clippy::too_many_lines)]
fn convert_report(
    pool: Vec<u8>,
    work_units: u64,
    record_count: u64,
    buffer_cells: u64,
    records: RawRecords,
) -> Result<NativeExecutionReport, String> {
    let RawRecords {
        sources: source_records,
        nodes: node_records,
        buffer_generations: buffer_generation_records,
        words: word_records,
        atoms: atom_records,
        fragments: fragment_records,
        fragment_atoms: fragment_atom_records,
        flushes: flush_records,
        boundaries: boundary_records,
        controls: control_records,
        geometry: geometry_records,
        wrappers: wrapper_records,
        references: reference_records,
        anchors: anchor_records,
        tables: table_records,
        table_rows: table_row_records,
        table_cells: table_cell_records,
        diagnostics: diagnostic_records,
    } = records;
    let node_count = node_records.len();
    let source_count = source_records.len();
    let word_count = word_records.len();
    let atom_count = atom_records.len();
    let buffer_generation_count = buffer_generation_records.len();
    let fragment_count = fragment_records.len();
    let wrapper_count = wrapper_records.len();
    let reference_count = reference_records.len();
    let table_count = table_records.len();
    let table_row_count = table_row_records.len();
    let table_cell_count = table_cell_records.len();
    let expected_records = [
        source_count,
        node_count,
        buffer_generation_count,
        word_count,
        atom_count,
        fragment_count,
        fragment_atom_records.len(),
        flush_records.len(),
        boundary_records.len(),
        control_records.len(),
        geometry_records.len(),
        wrapper_count,
        reference_count,
        anchor_records.len(),
        table_count,
        table_row_count,
        table_cell_count,
        diagnostic_records.len(),
    ]
    .into_iter()
    .try_fold(0_u64, |total, count| {
        total
            .checked_add(
                u64::try_from(count).map_err(|_| "execution record count overflow".to_owned())?,
            )
            .ok_or_else(|| "execution record count overflow".to_owned())
    })?;
    if record_count != expected_records {
        return Err("native execution record accounting mismatch".to_owned());
    }
    if source_count != 1 {
        return Err("native execution report must contain exactly one root source".to_owned());
    }
    let mut sources = reserved_vec(source_count, "source")?;
    for (index, value) in source_records.into_iter().enumerate() {
        dense(value.key, index, "source")?;
        if value.flags != 0 {
            return Err("unknown execution source flags".to_owned());
        }
        let path = pool_range(value.path_start, value.path_length, &pool, "source path")?;
        #[cfg(unix)]
        let path = PathBuf::from(OsString::from_vec(copy_bytes(
            &pool[path.as_range().unwrap()],
            "source path",
        )?));
        #[cfg(not(unix))]
        let path = PathBuf::from(copy_utf8(&pool[path.as_range().unwrap()], "source path")?);
        let parent = option(value.parent);
        if parent.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= source_count)
        }) {
            return Err("invalid parent execution source".to_owned());
        }
        let include_node = option(value.include_node)
            .map(|key| node_key(key, node_count, "source include node"))
            .transpose()?;
        if index == 0 && (parent.is_some() || include_node.is_some()) {
            return Err("root execution source has an include relationship".to_owned());
        }
        sources.push(ExecutionSource {
            key: value.key,
            parent,
            include_node,
            path,
        });
    }
    let mut nodes = reserved_vec(node_count, "node")?;
    for (index, value) in node_records.into_iter().enumerate() {
        dense(value.key, index, "node")?;
        if usize::try_from(value.source)
            .ok()
            .is_none_or(|key| key >= source_count)
        {
            return Err("invalid execution node source".to_owned());
        }
        let parent = option(value.parent)
            .map(|key| node_key(key, node_count, "node parent"))
            .transpose()?;
        if parent.is_some_and(|key| usize::try_from(key.0).ok().is_none_or(|key| key >= index)) {
            return Err("execution node parent is not an earlier node".to_owned());
        }
        let kind = match value.kind {
            0 => crate::NodeKind::Root,
            1 => crate::NodeKind::Block,
            2 => crate::NodeKind::Head,
            3 => crate::NodeKind::Body,
            4 => crate::NodeKind::Tail,
            5 => crate::NodeKind::Element,
            6 => crate::NodeKind::Text,
            7 => crate::NodeKind::Comment,
            8 => crate::NodeKind::Table,
            9 => crate::NodeKind::Equation,
            _ => return Err("unknown execution node kind".to_owned()),
        };
        if value.flags & !0x03ff != 0 {
            return Err("unknown execution node flags".to_owned());
        }
        let macro_range =
            optional_pool(value.macro_start, value.macro_length, &pool, "node macro")?;
        let macro_name = macro_range
            .map(|range| copy_utf8(&pool[range.as_range().unwrap()], "macro name"))
            .transpose()?;
        nodes.push(ExecutionNode {
            key: ExecutionNodeKey(value.key),
            parent,
            source: value.source,
            line: value.line,
            column: value.column,
            kind,
            flags: ExecutionNodeFlags(value.flags),
            macro_name,
        });
    }
    let mut buffer_generations = reserved_vec(buffer_generation_count, "buffer-generation")?;
    let mut latest_generation = BTreeMap::<u32, (u32, u32, BufferCloseReason, u64)>::new();
    for (index, value) in buffer_generation_records.iter().copied().enumerate() {
        dense(value.key, index, "buffer-generation")?;
        if value.reserved != 0 || value.extent > value.capacity {
            return Err("invalid execution buffer-generation extent".to_owned());
        }
        let close_reason = match value.close_reason {
            1 => BufferCloseReason::Reset,
            2 => BufferCloseReason::ReportEnd,
            _ => return Err("unknown execution buffer close reason".to_owned()),
        };
        if value.open_sequence >= value.close_sequence {
            return Err("invalid execution buffer-generation lifetime".to_owned());
        }
        if let Some((
            previous_generation,
            previous_capacity,
            previous_close_reason,
            previous_close,
        )) = latest_generation.get(&value.buffer).copied()
        {
            if value.generation != previous_generation.saturating_add(1)
                || value.capacity < previous_capacity
                || previous_close_reason != BufferCloseReason::Reset
                || value.open_sequence <= previous_close
            {
                return Err("non-contiguous execution buffer-generation lifetime".to_owned());
            }
        } else if value.generation != 0 {
            return Err("execution buffer generation does not begin at zero".to_owned());
        }
        latest_generation.insert(
            value.buffer,
            (
                value.generation,
                value.capacity,
                close_reason,
                value.close_sequence,
            ),
        );
        buffer_generations.push(ExecutionBufferGeneration {
            key: value.key,
            buffer: value.buffer,
            generation: value.generation,
            capacity: value.capacity,
            extent: value.extent,
            close_reason,
            open_sequence: value.open_sequence,
            close_sequence: value.close_sequence,
        });
    }
    let accounted_buffer_cells = latest_generation
        .values()
        .try_fold(0_u64, |total, (_, capacity, _, _)| {
            total.checked_add(u64::from(*capacity))
        });
    if accounted_buffer_cells != Some(buffer_cells) {
        return Err("native execution buffer capacity accounting mismatch".to_owned());
    }
    let mut words = reserved_vec(word_count, "word")?;
    let mut atom_words = reserved_filled_vec(None, atom_count, "word atom coverage")?;
    for (index, value) in word_records.iter().copied().enumerate() {
        dense(value.key, index, "word")?;
        if index != 0 {
            let previous = word_records[index - 1];
            if previous.leave_sequence >= value.enter_sequence
                || previous.leave_atom > value.enter_atom
            {
                return Err("execution formatter words are not serial".to_owned());
            }
        }
        if value.reserved != 0 {
            return Err("non-zero reserved execution word field".to_owned());
        }
        let node = optional_node_key(value.node, node_count, "word")?;
        if usize::try_from(value.source)
            .ok()
            .is_none_or(|key| key >= source_count)
            || node.map_or(0, |key| nodes[key.0 as usize].source) != value.source
        {
            return Err("execution word source does not match its node".to_owned());
        }
        let operand = pool_range(
            value.operand_start,
            value.operand_length,
            &pool,
            "word operand",
        )?;
        let role = match value.role {
            1 => AtomRole::Authored,
            4 => AtomRole::MacroGenerated,
            5 => AtomRole::DeviceGenerated,
            6 => AtomRole::TableCellPayload,
            _ => return Err("unknown execution word role".to_owned()),
        };
        if node.is_some_and(|node| {
            nodes[node.0 as usize]
                .flags
                .contains(ExecutionNodeFlags::GENERATED)
        }) && role == AtomRole::Authored
        {
            return Err("generated execution word is marked as authored".to_owned());
        }
        let wrapper = option(value.wrapper);
        if wrapper.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= wrapper_count)
        }) {
            return Err("invalid execution word wrapper".to_owned());
        }
        let atoms = range(value.enter_atom, value.leave_atom, "word atoms")?;
        if !exact_sequence_range(
            &atom_records,
            &atoms,
            value.enter_sequence,
            value.leave_sequence,
            |atom| atom.sequence,
        ) {
            return Err("execution word atom interval is inconsistent".to_owned());
        }
        for owner in atom_words
            .get_mut(atoms.start as usize..atoms.end as usize)
            .ok_or_else(|| "execution word atom interval is out of bounds".to_owned())?
        {
            if owner.replace(ExecutionWordKey(value.key)).is_some() {
                return Err("overlapping execution word atom intervals".to_owned());
            }
        }
        words.push(ExecutionWord {
            key: ExecutionWordKey(value.key),
            node,
            source: value.source,
            operand,
            role,
            wrapper,
            atoms,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    let mut atoms = reserved_vec(atom_count, "atom")?;
    let mut replacement_predecessors =
        reserved_filled_vec(false, atom_count, "atom replacement coverage")?;
    for (index, value) in atom_records.iter().copied().enumerate() {
        dense(value.key, index, "atom")?;
        let kind = match value.kind {
            1 => AtomKind::Glyph,
            2 => AtomKind::BreakableSpace,
            3 => AtomKind::NonBreakingSpace,
            4 => AtomKind::BreakableHyphen,
            5 => AtomKind::ZeroWidth,
            6 => AtomKind::Tab,
            7 => AtomKind::TabReference,
            8 => AtomKind::Backspace,
            9 => AtomKind::WordEndBreak,
            10 => AtomKind::BreakPoint,
            _ => return Err("unknown execution atom kind".to_owned()),
        };
        let role = match value.role {
            1 => AtomRole::Authored,
            2 => AtomRole::ImplicitSpace,
            3 => AtomRole::FontDecoration,
            4 => AtomRole::MacroGenerated,
            5 => AtomRole::DeviceGenerated,
            6 => AtomRole::TableCellPayload,
            _ => return Err("unknown execution atom role".to_owned()),
        };
        let font = match value.font {
            0 => ExecutionFont::Roman,
            1 => ExecutionFont::Bold,
            2 => ExecutionFont::Underline,
            3 => ExecutionFont::BoldUnderline,
            _ => return Err("unknown execution font".to_owned()),
        };
        let disposition = match value.disposition {
            1 => return Err("sealed execution report contains a buffered atom".to_owned()),
            2 => AtomDisposition::Emitted,
            3 => AtomDisposition::Consumed,
            4 => AtomDisposition::Replaced,
            5 => AtomDisposition::TrailingDiscard,
            _ => return Err("unknown execution atom disposition".to_owned()),
        };
        let node = optional_node_key(value.node, node_count, "atom")?;
        if usize::try_from(value.source)
            .ok()
            .is_none_or(|key| key >= source_count)
        {
            return Err("invalid execution atom source".to_owned());
        }
        if node.map_or(0, |key| nodes[key.0 as usize].source) != value.source {
            return Err("execution atom source does not match its node".to_owned());
        }
        let wrapper = option(value.wrapper);
        if wrapper.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= wrapper_count)
        }) {
            return Err("invalid execution atom wrapper".to_owned());
        }
        match atom_words[index] {
            Some(word_key) => {
                let word = &words[word_key.0 as usize];
                let operand = optional_pool(
                    value.operand_start,
                    value.operand_length,
                    &pool,
                    "atom operand",
                )?;
                if operand != Some(word.operand)
                    || node != word.node
                    || value.source != word.source
                    || wrapper != word.wrapper
                    || (role != word.role
                        && !matches!(role, AtomRole::ImplicitSpace | AtomRole::FontDecoration))
                {
                    return Err("execution atom does not match its formatter word".to_owned());
                }
            }
            None if value.operand_start != NONE || value.operand_length != 0 => {
                return Err("execution atom operand has no formatter word".to_owned());
            }
            None => {}
        }
        let replaced_by = option(value.replaced_by);
        if replaced_by.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= atom_count)
        }) {
            return Err("invalid replacing execution atom".to_owned());
        }
        if char::from_u32(value.input_scalar).is_none()
            || char::from_u32(value.display_scalar).is_none()
        {
            return Err("invalid Unicode scalar in execution atom".to_owned());
        }
        let buffer = option(value.buffer);
        let generation = option(value.generation);
        let buffer_generation = option(value.buffer_generation);
        let slot = option(value.slot);
        match (buffer, generation, buffer_generation, slot) {
            (Some(buffer), Some(generation), Some(generation_key), Some(slot)) => {
                let fact =
                    buffer_generations
                        .get(usize::try_from(generation_key).map_err(|_| {
                            "execution atom buffer-generation key overflow".to_owned()
                        })?)
                        .ok_or_else(|| "invalid execution atom buffer-generation".to_owned())?;
                if fact.buffer != buffer
                    || fact.generation != generation
                    || slot >= fact.extent
                    || value.sequence <= fact.open_sequence
                    || value.sequence >= fact.close_sequence
                {
                    return Err("execution atom is outside its buffer generation".to_owned());
                }
            }
            (None, None, None, None) => {
                if disposition != AtomDisposition::Emitted {
                    return Err("direct execution atom is not emitted".to_owned());
                }
            }
            _ => return Err("partial execution atom buffer identity".to_owned()),
        }
        if (disposition == AtomDisposition::Replaced) != replaced_by.is_some() {
            return Err("execution atom replacement disposition is inconsistent".to_owned());
        }
        if let Some(replacement) = replaced_by {
            if replacement <= value.key {
                return Err("execution atom replacement is not later than its source".to_owned());
            }
            let replacement_key = usize::try_from(replacement)
                .map_err(|_| "execution atom replacement key overflow".to_owned())?;
            if replacement_predecessors[replacement_key] {
                return Err("execution atom has multiple replacement predecessors".to_owned());
            }
            replacement_predecessors[replacement_key] = true;
            let replacement = &atom_records[replacement_key];
            if replacement.buffer != value.buffer
                || replacement.generation != value.generation
                || replacement.buffer_generation != value.buffer_generation
                || replacement.slot != value.slot
                || replacement.sequence <= value.sequence
            {
                return Err("execution atom replacement changed buffer identity".to_owned());
            }
        }
        atoms.push(ExecutionAtom {
            key: AtomKey(value.key),
            buffer,
            generation,
            buffer_generation,
            slot,
            kind,
            role,
            input_scalar: value.input_scalar,
            display_scalar: value.display_scalar,
            width_bu: value.width_bu,
            node,
            source: value.source,
            operand: optional_pool(
                value.operand_start,
                value.operand_length,
                &pool,
                "atom operand",
            )?,
            font,
            wrapper,
            replaced_by: replaced_by.map(AtomKey),
            disposition,
            sequence: value.sequence,
        });
    }
    for pair in &fragment_atom_records {
        if usize::try_from(pair.fragment)
            .ok()
            .is_none_or(|key| key >= fragment_count)
            || usize::try_from(pair.atom)
                .ok()
                .is_none_or(|key| key >= atom_count)
        {
            return Err("invalid execution fragment-atom reference".to_owned());
        }
    }
    let mut live_atoms = reserved_vec(atom_count, "live execution atom locations")?;
    live_atoms.extend(
        atoms
            .iter()
            .enumerate()
            .filter_map(|(key, atom)| Some((atom.buffer_generation?, atom.slot?, key))),
    );
    live_atoms.sort_unstable_by_key(|&(generation, slot, key)| (generation, slot, key));

    // Validate the complete write history of each fixed-CVS buffer slot.
    // Replacement edges join immediately adjacent writes. Consumed or
    // discarded atoms end an occupancy epoch, allowing a later write to reuse
    // the cleared slot without a replacement edge. The final write is the
    // current occupant used for dense field validation below.
    let mut current_occupants = reserved_vec(live_atoms.len(), "current execution atom locations")?;
    let mut group_start = 0;
    while group_start < live_atoms.len() {
        let (generation, slot, _) = live_atoms[group_start];
        let mut group_end = group_start + 1;
        while group_end < live_atoms.len()
            && live_atoms[group_end].0 == generation
            && live_atoms[group_end].1 == slot
        {
            group_end += 1;
        }
        for pair in live_atoms[group_start..group_end].windows(2) {
            let current = &atoms[pair[0].2];
            match current.disposition {
                AtomDisposition::Replaced if current.replaced_by == Some(atoms[pair[1].2].key) => {}
                AtomDisposition::Consumed | AtomDisposition::TrailingDiscard => {}
                _ => {
                    return Err("execution atom slot history is inconsistent".to_owned());
                }
            }
        }
        if atoms[live_atoms[group_end - 1].2].disposition == AtomDisposition::Replaced {
            return Err("execution atom slot history is inconsistent".to_owned());
        }
        current_occupants.push(live_atoms[group_end - 1]);
        group_start = group_end;
    }
    let live_atoms = current_occupants;
    let mut fragment_ref_coverage = reserved_filled_vec(
        false,
        fragment_atom_records.len(),
        "fragment reference coverage",
    )?;
    let mut referenced_atoms = reserved_filled_vec(false, atom_count, "fragment atom coverage")?;
    let mut fragments = reserved_vec(fragment_count, "fragment")?;
    for (index, value) in fragment_records.into_iter().enumerate() {
        dense(value.key, index, "fragment")?;
        if value.reserved != 0 {
            return Err("non-zero reserved execution fragment field".to_owned());
        }
        if value.start_bu < 0 || value.end_bu < 0 {
            return Err("invalid execution fragment geometry".to_owned());
        }
        let start = usize::try_from(value.atom_ref_start)
            .map_err(|_| "fragment atom range overflow".to_owned())?;
        let end = start
            .checked_add(
                usize::try_from(value.atom_ref_length)
                    .map_err(|_| "fragment atom range overflow".to_owned())?,
            )
            .ok_or_else(|| "fragment atom range overflow".to_owned())?;
        let refs = fragment_atom_records
            .get(start..end)
            .ok_or_else(|| "invalid fragment atom range".to_owned())?;
        if refs.len() != 1 {
            return Err("execution fragment must have exactly one origin atom".to_owned());
        }
        if fragment_ref_coverage[start..end]
            .iter()
            .any(|covered| *covered)
        {
            return Err("overlapping execution fragment-atom ranges".to_owned());
        }
        fragment_ref_coverage[start..end].fill(true);
        if refs.iter().any(|pair| pair.fragment != value.key) {
            return Err("fragment atom range contains another fragment".to_owned());
        }
        let atom_key = usize::try_from(refs[0].atom)
            .map_err(|_| "execution fragment atom key overflow".to_owned())?;
        if referenced_atoms[atom_key] {
            return Err("execution atom is referenced by multiple fragments".to_owned());
        }
        referenced_atoms[atom_key] = true;
        let origin_atom = &atom_records[atom_key];
        if origin_atom.disposition != 2
            || origin_atom.buffer != value.buffer
            || origin_atom.generation != value.generation
            || origin_atom.buffer_generation != value.buffer_generation
            || origin_atom.node != value.node
        {
            return Err("fragment atom belongs to another execution origin".to_owned());
        }
        let buffer = option(value.buffer);
        let generation = option(value.generation);
        let buffer_generation = option(value.buffer_generation);
        match (buffer, generation, buffer_generation) {
            (Some(buffer), Some(generation), Some(generation_key)) => {
                let fact = buffer_generations
                    .get(usize::try_from(generation_key).map_err(|_| {
                        "execution fragment buffer-generation key overflow".to_owned()
                    })?)
                    .ok_or_else(|| "invalid execution fragment buffer-generation".to_owned())?;
                if fact.buffer != buffer
                    || fact.generation != generation
                    || value.sequence <= fact.open_sequence
                    || value.sequence >= fact.close_sequence
                {
                    return Err("execution fragment is outside its buffer generation".to_owned());
                }
            }
            (None, None, None) => {}
            _ => return Err("partial execution fragment buffer identity".to_owned()),
        }
        let wrapper = option(value.wrapper);
        if wrapper.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= wrapper_count)
        }) {
            return Err("invalid execution fragment wrapper".to_owned());
        }
        let role = match value.role {
            1 => FragmentRole::Content,
            2 => FragmentRole::FontDecoration,
            3 => FragmentRole::MarginDecoration,
            4 => FragmentRole::PageDecoration,
            _ => return Err("unknown execution fragment role".to_owned()),
        };
        let mut fragment_atoms = reserved_vec(refs.len(), "fragment atom keys")?;
        fragment_atoms.extend(refs.iter().map(|pair| AtomKey(pair.atom)));
        fragments.push(ExecutionFragment {
            key: FragmentKey(value.key),
            node: optional_node_key(value.node, node_count, "fragment")?,
            buffer,
            generation,
            buffer_generation,
            atoms: fragment_atoms,
            device_line: value.device_line,
            role,
            start_bu: value.start_bu,
            end_bu: value.end_bu,
            wrapper,
            sequence: value.sequence,
        });
    }
    if fragment_ref_coverage.iter().any(|covered| !covered) {
        return Err("unclaimed execution fragment-atom record".to_owned());
    }
    if atoms
        .iter()
        .zip(&referenced_atoms)
        .any(|(atom, referenced)| atom.buffer.is_none() && !referenced)
    {
        return Err("direct execution atom has no fragment".to_owned());
    }
    let mut fragment_flush_outcomes =
        reserved_filled_vec(None::<u64>, fragment_count, "flush fragment coverage")?;
    let mut flushes = reserved_vec(flush_records.len(), "flush")?;
    let mut pending_flushes = reserved_filled_vec(
        None::<usize>,
        buffer_generations.len(),
        "pending flush continuation",
    )?;
    let mut terminal_flushes =
        reserved_filled_vec(false, buffer_generations.len(), "terminal flush state")?;
    for (index, value) in flush_records.iter().copied().enumerate() {
        dense(value.key, index, "flush")?;
        if index != 0 && flush_records[index - 1].outcome_sequence >= value.sequence {
            return Err("execution flush order is inconsistent".to_owned());
        }
        if value.flags_before & !0x7f_ffff != 0 || value.flags_after & !0x7f_ffff != 0 {
            return Err("unknown execution terminal flags".to_owned());
        }
        let fragment_start = usize::try_from(value.fragment_start)
            .map_err(|_| "flush fragment range overflow".to_owned())?;
        let fragment_end = fragment_start
            .checked_add(
                usize::try_from(value.fragment_length)
                    .map_err(|_| "flush fragment range overflow".to_owned())?,
            )
            .ok_or_else(|| "flush fragment range overflow".to_owned())?;
        let flush_fragments = fragments
            .get(fragment_start..fragment_end)
            .ok_or_else(|| "invalid flush fragment range".to_owned())?;
        if fragment_flush_outcomes[fragment_start..fragment_end]
            .iter()
            .any(Option::is_some)
        {
            return Err("overlapping execution flush fragment ranges".to_owned());
        }
        fragment_flush_outcomes[fragment_start..fragment_end].fill(Some(value.outcome_sequence));
        if flush_fragments.iter().any(|fragment| {
            fragment.buffer != Some(value.buffer)
                || fragment.generation != Some(value.generation)
                || fragment.buffer_generation != Some(value.buffer_generation)
                || fragment.sequence <= value.sequence
                || fragment.sequence >= value.outcome_sequence
        }) {
            return Err("flush contains a fragment from another buffer generation".to_owned());
        }
        if flush_fragments.iter().any(|fragment| {
            fragment
                .atoms
                .first()
                .and_then(|key| atoms.get(key.0 as usize))
                .and_then(|atom| atom.slot)
                .is_none_or(|slot| slot < value.accepted_start || slot >= value.accepted_end)
        }) {
            return Err("flush contains a fragment outside its accepted field".to_owned());
        }
        let generation_key = usize::try_from(value.buffer_generation)
            .map_err(|_| "execution flush buffer-generation key overflow".to_owned())?;
        let generation_fact = buffer_generations
            .get(generation_key)
            .ok_or_else(|| "invalid execution flush buffer-generation".to_owned())?;
        if generation_fact.buffer != value.buffer
            || generation_fact.generation != value.generation
            || value.sequence <= generation_fact.open_sequence
            || value.outcome_sequence <= value.sequence
            || value.outcome_sequence >= generation_fact.close_sequence
        {
            return Err("execution flush is outside its buffer generation".to_owned());
        }
        if terminal_flushes[generation_key] {
            return Err("execution flush follows a terminal field".to_owned());
        }
        let first_flush = pending_flushes[generation_key].is_none();
        if let Some(previous) = pending_flushes[generation_key].take() {
            let previous = &flush_records[previous];
            if value.scan_start != previous.remaining_start
                || value.remaining_end != previous.remaining_end
                || previous.outcome_sequence >= value.sequence
                || value.taboff_before != previous.taboff_after
            {
                return Err("execution flush does not resume its remaining field".to_owned());
            }
        } else if value.scan_start != 0 {
            return Err("execution buffer generation does not start at slot zero".to_owned());
        }
        if value.leading_bu < 0
            || value.content_bu < 0
            || value.field_bu < 0
            || value.target_bu < 0
            || value.taboff_before < 0
            || value.taboff_after < 0
            || value.visual_before < 0
            || value.visual_after < 0
        {
            return Err("negative execution flush geometry".to_owned());
        }
        if value.scan_start != value.accepted_start
            || value.scan_start != value.consumed_start
            || value.accepted_start > value.accepted_end
            || value.accepted_start != value.consumed_start
            || value.accepted_end != value.consumed_end
            || value.accepted_end != value.tail_discarded_start
            || value.accepted_end > value.scan_end
            || value.scan_end > value.remaining_end
            || value.tail_discarded_start > value.tail_discarded_end
            || value.tail_discarded_end != value.remaining_start
            || value.remaining_start > value.remaining_end
            || value.remaining_end > generation_fact.extent
        {
            return Err("inconsistent execution flush ranges".to_owned());
        }
        let outcome = match value.outcome {
            1 => FlushOutcome::NoContent,
            2 => FlushOutcome::Exhausted,
            3 => FlushOutcome::Wrapped,
            4 => FlushOutcome::DeferredColumn,
            _ => return Err("unknown execution flush outcome".to_owned()),
        };
        let first_live = live_atoms.partition_point(|&(generation, slot, _)| {
            (generation, slot) < (value.buffer_generation, value.accepted_start)
        });
        let mut next_slot = value.accepted_start;
        for &(generation, slot, atom_key) in &live_atoms[first_live..] {
            if generation != value.buffer_generation || slot >= value.tail_discarded_end {
                break;
            }
            if atoms[atom_key].sequence >= value.sequence {
                return Err("execution field atom was created after its flush began".to_owned());
            }
            let disposition = atoms[atom_key].disposition;
            if slot < value.accepted_end {
                if slot != next_slot
                    || !matches!(
                        disposition,
                        AtomDisposition::Emitted
                            | AtomDisposition::Consumed
                            | AtomDisposition::TrailingDiscard
                    )
                {
                    return Err("accepted execution field has an invalid atom fate".to_owned());
                }
                next_slot += 1;
            } else if matches!(outcome, FlushOutcome::NoContent | FlushOutcome::Exhausted) {
                if slot != next_slot
                    || !terminal_tail_scalar(atoms[atom_key].display_scalar)
                    || disposition != AtomDisposition::TrailingDiscard
                {
                    return Err("terminal execution field tail was not discarded".to_owned());
                }
                next_slot += 1;
            } else {
                if slot != next_slot
                    || atoms[atom_key].display_scalar != u32::from(b' ')
                    || disposition != AtomDisposition::Consumed
                {
                    return Err("continuing execution field tail was not consumed".to_owned());
                }
                next_slot += 1;
            }
        }
        if next_slot != value.tail_discarded_end {
            return Err("execution field lacks complete atom provenance".to_owned());
        }
        if outcome == FlushOutcome::NoContent {
            if value.accepted_start != value.accepted_end
                || value.consumed_start != value.consumed_end
                || value.remaining_start != value.remaining_end
                || value.fragment_length != 0
            {
                return Err("no-content flush emitted or consumed a field".to_owned());
            }
        } else if value.accepted_start == value.accepted_end {
            return Err("content flush accepted an empty field".to_owned());
        }
        if matches!(
            outcome,
            FlushOutcome::Wrapped | FlushOutcome::DeferredColumn
        ) && value.remaining_start == value.remaining_end
        {
            return Err("continuing execution flush has no remaining field".to_owned());
        }
        if matches!(outcome, FlushOutcome::NoContent | FlushOutcome::Exhausted) {
            if value.remaining_start != value.remaining_end
                || generation_fact.close_reason != BufferCloseReason::Reset
            {
                return Err("terminal execution flush retains a field".to_owned());
            }
            terminal_flushes[generation_key] = true;
        } else {
            pending_flushes[generation_key] = Some(index);
        }
        if first_flush {
            let first_live = live_atoms.partition_point(|&(generation, slot, _)| {
                (generation, slot) < (value.buffer_generation, 0)
            });
            let mut next_slot = 0;
            for &(generation, slot, atom_key) in &live_atoms[first_live..] {
                if generation != value.buffer_generation || slot >= value.remaining_end {
                    break;
                }
                if slot != next_slot || atoms[atom_key].sequence >= value.sequence {
                    return Err("execution field atom was created after its flush began".to_owned());
                }
                next_slot += 1;
            }
            if next_slot != value.remaining_end {
                return Err("execution field lacks complete atom provenance".to_owned());
            }
        }
        if outcome == FlushOutcome::Wrapped {
            let next = flush_records
                .get(index + 1)
                .ok_or_else(|| "wrapped execution flush has no successor".to_owned())?;
            if next.buffer_generation != value.buffer_generation
                || next.scan_start != value.remaining_start
                || next.remaining_end != value.remaining_end
                || value.outcome_sequence >= next.sequence
                || next.taboff_before != value.taboff_after
            {
                return Err("wrapped execution flush successor is inconsistent".to_owned());
            }
        }
        let boundary = option(value.boundary);
        if boundary.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= boundary_records.len())
        }) {
            return Err("invalid execution flush boundary".to_owned());
        }
        flushes.push(ExecutionFlush {
            key: value.key,
            node: optional_node_key(value.node, node_count, "flush")?,
            buffer: value.buffer,
            generation: value.generation,
            buffer_generation: value.buffer_generation,
            scanned: range(value.scan_start, value.scan_end, "flush scan")?,
            accepted: range(value.accepted_start, value.accepted_end, "flush accepted")?,
            consumed: range(value.consumed_start, value.consumed_end, "flush consumed")?,
            tail_discarded: range(
                value.tail_discarded_start,
                value.tail_discarded_end,
                "flush tail discard",
            )?,
            remaining: range(
                value.remaining_start,
                value.remaining_end,
                "flush remaining",
            )?,
            fragments: range(
                value.fragment_start,
                value
                    .fragment_start
                    .checked_add(value.fragment_length)
                    .ok_or_else(|| "flush fragment range overflow".to_owned())?,
                "flush fragments",
            )?,
            flags_before: value.flags_before,
            flags_after: value.flags_after,
            boundary,
            outcome,
            leading_bu: value.leading_bu,
            content_bu: value.content_bu,
            field_bu: value.field_bu,
            target_bu: value.target_bu,
            taboff_before: value.taboff_before,
            taboff_after: value.taboff_after,
            visual_before: value.visual_before,
            visual_after: value.visual_after,
            sequence: value.sequence,
            outcome_sequence: value.outcome_sequence,
        });
    }
    if pending_flushes.iter().any(Option::is_some) {
        return Err("execution flush continuation is incomplete".to_owned());
    }
    if fragments.iter().enumerate().any(|(index, fragment)| {
        fragment.buffer_generation.is_some() != fragment_flush_outcomes[index].is_some()
    }) {
        return Err("execution fragments are not exactly partitioned by flushes".to_owned());
    }
    let mut boundaries = reserved_vec(boundary_records.len(), "boundary")?;
    for (index, value) in boundary_records.into_iter().enumerate() {
        dense(value.key, index, "boundary")?;
        let request = match value.request {
            1 => BoundaryRequest::Newline,
            2 => BoundaryRequest::VerticalSpace,
            3 => BoundaryRequest::Endline,
            4 => BoundaryRequest::DeviceEndline,
            _ => return Err("unknown execution boundary request".to_owned()),
        };
        let effect = match value.effect {
            0 => BoundaryEffect::NoOutput,
            1 => BoundaryEffect::Flushed,
            2 => BoundaryEffect::EndedLine,
            3 => BoundaryEffect::AddedVerticalSpace,
            _ => return Err("unknown execution boundary effect".to_owned()),
        };
        let parent = option(value.parent);
        if parent.is_some_and(|key| usize::try_from(key).ok().is_none_or(|key| key >= index)) {
            return Err("execution boundary parent is not an earlier boundary".to_owned());
        }
        if value.flags_before & !0x7f_ffff != 0 || value.flags_after & !0x7f_ffff != 0 {
            return Err("unknown execution boundary terminal flags".to_owned());
        }
        let control = option(value.control);
        if control.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= control_records.len())
        }) {
            return Err("invalid execution boundary control owner".to_owned());
        }
        let wrapper = option(value.wrapper);
        if wrapper.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= wrapper_records.len())
        }) {
            return Err("invalid execution boundary wrapper".to_owned());
        }
        let node = optional_node_key(value.node, node_count, "boundary")?;
        if wrapper.is_none() != node.is_none() {
            return Err("execution boundary node and wrapper presence differ".to_owned());
        }
        if let Some(wrapper) = wrapper {
            let wrapper = &wrapper_records[wrapper as usize];
            if !matches!(wrapper.kind, 1 | 3 | 4)
                || wrapper.node != value.node
                || wrapper.enter_sequence >= value.enter_sequence
                || wrapper.leave_sequence <= value.leave_sequence
            {
                return Err("execution boundary does not match its active wrapper".to_owned());
            }
        }
        if let Some(control) = control {
            let control = &control_records[control as usize];
            if control.enter_sequence >= value.enter_sequence
                || control.leave_sequence <= value.leave_sequence
            {
                return Err("execution boundary escapes its control owner".to_owned());
            }
        }
        boundaries.push(ExecutionBoundary {
            key: value.key,
            node,
            parent,
            request,
            effect,
            flags_before: value.flags_before,
            flags_after: value.flags_after,
            control,
            wrapper,
            line_before: value.line_before,
            line_after: value.line_after,
            visual_before: value.visual_before,
            visual_after: value.visual_after,
            direct_device_lines: value.direct_device_lines,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    let mut boundary_child_leave = reserved_filled_vec(0_u64, boundaries.len(), "boundary child")?;
    let mut direct_boundary_lines =
        reserved_filled_vec(0_u32, boundaries.len(), "boundary direct line")?;
    let mut subtree_boundary_lines =
        reserved_filled_vec(0_u32, boundaries.len(), "boundary subtree line")?;
    let mut root_boundary_leave = 0_u64;
    for boundary in &boundaries {
        if boundary.enter_sequence >= boundary.leave_sequence
            || boundary.line_before < 0
            || boundary.line_after < 0
            || boundary.line_after < boundary.line_before
            || boundary.visual_before < 0
            || boundary.visual_after < 0
        {
            return Err("invalid execution boundary interval or effect".to_owned());
        }
        let last_leave = if let Some(parent) = boundary.parent {
            let parent = boundaries
                .get(parent as usize)
                .ok_or_else(|| "invalid execution boundary parent".to_owned())?;
            if parent.enter_sequence >= boundary.enter_sequence
                || parent.leave_sequence <= boundary.leave_sequence
            {
                return Err("execution boundary escapes its parent interval".to_owned());
            }
            &mut boundary_child_leave[parent.key as usize]
        } else {
            &mut root_boundary_leave
        };
        if *last_leave != 0 && boundary.enter_sequence <= *last_leave {
            return Err("execution boundary siblings overlap".to_owned());
        }
        *last_leave = boundary.leave_sequence;
        if boundary.request == BoundaryRequest::DeviceEndline
            && let Some(parent) = boundary.parent
        {
            direct_boundary_lines[parent as usize] = direct_boundary_lines[parent as usize]
                .checked_add(1)
                .ok_or_else(|| "execution boundary direct line count overflow".to_owned())?;
        }
    }
    for boundary in boundaries.iter().rev() {
        let key = boundary.key as usize;
        let lines = if boundary.request == BoundaryRequest::DeviceEndline {
            1
        } else {
            subtree_boundary_lines[key]
        };
        if let Some(parent) = boundary.parent {
            subtree_boundary_lines[parent as usize] = subtree_boundary_lines[parent as usize]
                .checked_add(lines)
                .ok_or_else(|| "execution boundary subtree line count overflow".to_owned())?;
        }
    }
    for boundary in &boundaries {
        if boundary.direct_device_lines != direct_boundary_lines[boundary.key as usize] {
            return Err("execution boundary direct line count is inconsistent".to_owned());
        }
        let line_delta = boundary
            .line_after
            .checked_sub(boundary.line_before)
            .ok_or_else(|| "execution boundary line delta overflow".to_owned())?;
        let expected_effect = if boundary.request == BoundaryRequest::DeviceEndline {
            if boundary.line_before == i64::MAX || boundary.line_after != boundary.line_before + 1 {
                return Err("invalid device endline boundary".to_owned());
            }
            BoundaryEffect::EndedLine
        } else if u32::try_from(line_delta).ok()
            != Some(subtree_boundary_lines[boundary.key as usize])
        {
            return Err("execution boundary line delta is inconsistent".to_owned());
        } else if boundary.request == BoundaryRequest::VerticalSpace
            && boundary.direct_device_lines != 0
        {
            BoundaryEffect::AddedVerticalSpace
        } else if boundary.line_after > boundary.line_before {
            BoundaryEffect::EndedLine
        } else if boundary.visual_after != boundary.visual_before {
            BoundaryEffect::Flushed
        } else {
            BoundaryEffect::NoOutput
        };
        if boundary.effect != expected_effect {
            return Err("execution boundary effect is inconsistent".to_owned());
        }
    }
    let mut controls = reserved_vec(control_records.len(), "control")?;
    for (index, value) in control_records.into_iter().enumerate() {
        dense(value.key, index, "control")?;
        if value.reserved != 0
            || value.flags_before & !0x7f_ffff != 0
            || value.flags_after & !0x7f_ffff != 0
        {
            return Err("invalid execution control flags or reserved field".to_owned());
        }
        let request = match value.request {
            1 => ExecutionControlRequest::Break,
            2 => ExecutionControlRequest::Center,
            3 => ExecutionControlRequest::Fill,
            4 => ExecutionControlRequest::Font,
            5 => ExecutionControlRequest::LineLength,
            6 => ExecutionControlRequest::MarginCharacter,
            7 => ExecutionControlRequest::NoFill,
            8 => ExecutionControlRequest::PageOffset,
            9 => ExecutionControlRequest::RightJustify,
            10 => ExecutionControlRequest::VerticalSpace,
            11 => ExecutionControlRequest::TabStops,
            12 => ExecutionControlRequest::TemporaryIndent,
            _ => return Err("unknown execution control request".to_owned()),
        };
        let node = node_key(value.node, node_count, "control")?;
        let expected_macro = match request {
            ExecutionControlRequest::Break => "br",
            ExecutionControlRequest::Center => "ce",
            ExecutionControlRequest::Fill => "fi",
            ExecutionControlRequest::Font => "ft",
            ExecutionControlRequest::LineLength => "ll",
            ExecutionControlRequest::MarginCharacter => "mc",
            ExecutionControlRequest::NoFill => "nf",
            ExecutionControlRequest::PageOffset => "po",
            ExecutionControlRequest::RightJustify => "rj",
            ExecutionControlRequest::VerticalSpace => "sp",
            ExecutionControlRequest::TabStops => "ta",
            ExecutionControlRequest::TemporaryIndent => "ti",
        };
        if nodes[node.0 as usize].macro_name.as_deref() != Some(expected_macro) {
            return Err("execution control request does not match its node macro".to_owned());
        }
        let parent = option(value.parent);
        if parent.is_some_and(|key| usize::try_from(key).ok().is_none_or(|key| key >= index)) {
            return Err("execution control parent is not an earlier control".to_owned());
        }
        let wrapper = usize::try_from(value.wrapper)
            .ok()
            .filter(|&key| key < wrapper_count)
            .ok_or_else(|| "invalid execution control wrapper".to_owned())?;
        let wrapper_record = &wrapper_records[wrapper];
        let atom_end = value
            .atom_start
            .checked_add(value.atom_length)
            .ok_or_else(|| "invalid execution control atom range".to_owned())?;
        if !matches!(wrapper_record.kind, 1 | 3 | 4)
            || !node_is_within(
                &nodes,
                node,
                node_key(wrapper_record.node, node_count, "control wrapper")?,
            )
            || wrapper_record.enter_sequence >= value.enter_sequence
            || wrapper_record.leave_sequence <= value.leave_sequence
            || wrapper_record.enter_atom > value.atom_start
            || wrapper_record.leave_atom < atom_end
        {
            return Err("execution control does not match its node wrapper".to_owned());
        }
        let generation_before = option(value.generation_before);
        let generation_after = option(value.generation_after);
        for (generation, checkpoint) in [
            (generation_before, value.enter_sequence),
            (generation_after, value.leave_sequence),
        ] {
            if generation.is_some_and(|generation| {
                buffer_generations
                    .get(generation as usize)
                    .is_none_or(|entry| {
                        entry.buffer != value.buffer
                            || entry.open_sequence >= checkpoint
                            || entry.close_sequence <= checkpoint
                    })
            }) {
                return Err("execution control buffer generation is inconsistent".to_owned());
            }
        }
        let atoms = counted_range(value.atom_start, value.atom_length, "control atoms")?;
        let fragments = counted_range(
            value.fragment_start,
            value.fragment_length,
            "control fragments",
        )?;
        let flush_range = counted_range(value.flush_start, value.flush_length, "control flushes")?;
        let boundary_range = counted_range(
            value.boundary_start,
            value.boundary_length,
            "control boundaries",
        )?;
        let geometry_range = counted_range(
            value.geometry_start,
            value.geometry_length,
            "control geometry",
        )?;
        let wrapper_range = counted_range(
            value.wrapper_start,
            value.wrapper_length,
            "control wrappers",
        )?;
        if atoms.end as usize > atom_count
            || fragments.end as usize > fragment_count
            || flush_range.end as usize > flushes.len()
            || boundary_range.end as usize > boundaries.len()
            || geometry_range.end as usize > geometry_records.len()
            || wrapper_range.end as usize > wrapper_count
            || value.enter_sequence >= value.leave_sequence
            || [
                value.line_before,
                value.line_after,
                value.visual_before,
                value.visual_after,
                value.column_before,
                value.column_after,
                value.extent_before,
                value.extent_after,
                value.offset_before,
                value.offset_after,
                value.rmargin_before,
                value.rmargin_after,
                value.maxrmargin_before,
                value.maxrmargin_after,
                value.taboff_before,
                value.taboff_after,
                value.minimum_blank_before,
                value.minimum_blank_after,
                value.trailing_blank_before,
                value.trailing_blank_after,
            ]
            .into_iter()
            .any(|value| value < 0)
        {
            return Err("invalid execution control range or state".to_owned());
        }
        controls.push(ExecutionControl {
            key: value.key,
            node,
            parent,
            wrapper: value.wrapper,
            request,
            buffer: value.buffer,
            generation_before,
            generation_after,
            flags_before: value.flags_before,
            flags_after: value.flags_after,
            atoms,
            fragments,
            flushes: flush_range,
            boundaries: boundary_range,
            geometry: geometry_range,
            wrappers: wrapper_range,
            line_before: value.line_before,
            line_after: value.line_after,
            visual_before: value.visual_before,
            visual_after: value.visual_after,
            column_before: value.column_before,
            column_after: value.column_after,
            extent_before: value.extent_before,
            extent_after: value.extent_after,
            offset_before: value.offset_before,
            offset_after: value.offset_after,
            rmargin_before: value.rmargin_before,
            rmargin_after: value.rmargin_after,
            maxrmargin_before: value.maxrmargin_before,
            maxrmargin_after: value.maxrmargin_after,
            taboff_before: value.taboff_before,
            taboff_after: value.taboff_after,
            temporary_indent_before: value.temporary_indent_before,
            temporary_indent_after: value.temporary_indent_after,
            skip_vertical_before: value.skip_vertical_before,
            skip_vertical_after: value.skip_vertical_after,
            minimum_blank_before: value.minimum_blank_before,
            minimum_blank_after: value.minimum_blank_after,
            trailing_blank_before: value.trailing_blank_before,
            trailing_blank_after: value.trailing_blank_after,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    let mut control_child_leave = reserved_filled_vec(0_u64, controls.len(), "control child")?;
    let mut root_control_leave = 0_u64;
    let mut active_controls = reserved_vec(controls.len(), "control nesting")?;
    for control in &controls {
        while active_controls.last().is_some_and(|parent| {
            controls[*parent as usize].leave_sequence <= control.enter_sequence
        }) {
            active_controls.pop();
        }
        if control.parent != active_controls.last().copied() {
            return Err("execution control parent is not its active owner".to_owned());
        }
        let last_leave = if let Some(parent) = control.parent {
            let parent = controls
                .get(parent as usize)
                .ok_or_else(|| "invalid execution control parent".to_owned())?;
            if parent.enter_sequence >= control.enter_sequence
                || parent.leave_sequence <= control.leave_sequence
            {
                return Err("execution control escapes its parent interval".to_owned());
            }
            &mut control_child_leave[parent.key as usize]
        } else {
            &mut root_control_leave
        };
        if *last_leave != 0 && control.enter_sequence <= *last_leave {
            return Err("execution control siblings overlap".to_owned());
        }
        *last_leave = control.leave_sequence;
        if !exact_sequence_range(
            &atoms,
            &control.atoms,
            control.enter_sequence,
            control.leave_sequence,
            |v| v.sequence,
        ) || !exact_sequence_range(
            &fragments,
            &control.fragments,
            control.enter_sequence,
            control.leave_sequence,
            |v| v.sequence,
        ) || !exact_sequence_range(
            &flushes,
            &control.flushes,
            control.enter_sequence,
            control.leave_sequence,
            |v| v.sequence,
        ) || !exact_sequence_range(
            &boundaries,
            &control.boundaries,
            control.enter_sequence,
            control.leave_sequence,
            |v| v.enter_sequence,
        ) || !exact_sequence_range(
            &geometry_records,
            &control.geometry,
            control.enter_sequence,
            control.leave_sequence,
            |v| v.sequence,
        ) || !exact_sequence_range(
            &wrapper_records,
            &control.wrappers,
            control.enter_sequence,
            control.leave_sequence,
            |v| v.enter_sequence,
        ) {
            return Err("execution control record ranges do not match its interval".to_owned());
        }
        active_controls.push(control.key);
    }
    validate_control_generations(&buffer_generations, &controls)?;
    active_controls.clear();
    let mut control_cursor = 0_usize;
    let mut last_boundary_enter = None;
    for boundary in &boundaries {
        if last_boundary_enter.is_some_and(|enter| enter >= boundary.enter_sequence) {
            return Err("execution boundaries are not in entry order".to_owned());
        }
        while controls
            .get(control_cursor)
            .is_some_and(|control| control.enter_sequence < boundary.enter_sequence)
        {
            let control = &controls[control_cursor];
            while active_controls.last().is_some_and(|parent| {
                controls[*parent as usize].leave_sequence <= control.enter_sequence
            }) {
                active_controls.pop();
            }
            active_controls.push(control.key);
            control_cursor += 1;
        }
        while active_controls.last().is_some_and(|owner| {
            controls[*owner as usize].leave_sequence <= boundary.enter_sequence
        }) {
            active_controls.pop();
        }
        if boundary.control != active_controls.last().copied() {
            return Err("execution boundary does not match its active control".to_owned());
        }
        last_boundary_enter = Some(boundary.enter_sequence);
    }
    let mut geometry = reserved_vec(geometry_records.len(), "geometry")?;
    let mut glyph_geometry_coverage =
        reserved_filled_vec(false, fragment_count, "glyph geometry coverage")?;
    for (index, value) in geometry_records.into_iter().enumerate() {
        dense(value.key, index, "geometry")?;
        if value.reserved != 0 {
            return Err("non-zero reserved execution geometry field".to_owned());
        }
        let kind = match value.kind {
            1 => GeometryKind::Advance,
            2 => GeometryKind::Glyph,
            3 => GeometryKind::Endline,
            4 => GeometryKind::Field,
            _ => return Err("unknown execution geometry kind".to_owned()),
        };
        let unit = match value.unit {
            1 => GeometryUnit::Basic,
            2 => GeometryUnit::BufferSlot,
            3 => GeometryUnit::DeviceLine,
            _ => return Err("unknown execution geometry unit".to_owned()),
        };
        let origin_kind = match value.origin_kind {
            0 => GeometryOriginKind::None,
            1 => GeometryOriginKind::Atom,
            2 => GeometryOriginKind::Flush,
            3 => GeometryOriginKind::Boundary,
            4 => GeometryOriginKind::Fragment,
            _ => return Err("unknown execution geometry origin kind".to_owned()),
        };
        let node = optional_node_key(value.node, node_count, "geometry")?;
        match kind {
            GeometryKind::Advance => {
                let effective = value
                    .after
                    .checked_sub(value.before)
                    .filter(|_| value.before >= 0 && value.after >= 0);
                let origin_flush = option(value.origin_key)
                    .and_then(|key| usize::try_from(key).ok())
                    .and_then(|key| flushes.get(key));
                if unit != GeometryUnit::Basic
                    || value.requested < 0
                    || effective != Some(value.effective)
                    || !matches!(
                        origin_kind,
                        GeometryOriginKind::None | GeometryOriginKind::Flush
                    )
                    || (origin_kind == GeometryOriginKind::None)
                        != option(value.origin_key).is_none()
                    || option(value.related) != option(value.origin_key)
                    || (origin_kind == GeometryOriginKind::Flush
                        && origin_flush.is_none_or(|flush| {
                            node != flush.node
                                || value.sequence <= flush.sequence
                                || value.sequence >= flush.outcome_sequence
                        }))
                {
                    return Err("invalid execution advance geometry relationship".to_owned());
                }
            }
            GeometryKind::Glyph => {
                let origin_atom = option(value.origin_key)
                    .and_then(|key| usize::try_from(key).ok().filter(|key| *key < atom_count));
                let related_fragment = option(value.related).and_then(|key| {
                    usize::try_from(key)
                        .ok()
                        .filter(|key| *key < fragment_count)
                });
                let Some(origin_atom) = origin_atom else {
                    return Err("invalid execution glyph geometry relationship".to_owned());
                };
                let Some(related_fragment) = related_fragment else {
                    return Err("invalid execution glyph geometry relationship".to_owned());
                };
                let fragment = &fragments[related_fragment];
                let Some(fragment_width) = fragment.end_bu.checked_sub(fragment.start_bu) else {
                    return Err("invalid execution glyph geometry relationship".to_owned());
                };
                if unit != GeometryUnit::Basic
                    || origin_kind != GeometryOriginKind::Atom
                    || glyph_geometry_coverage[related_fragment]
                    || fragment.atoms.as_slice() != [AtomKey(value.origin_key)]
                    || atoms[origin_atom].disposition != AtomDisposition::Emitted
                    || node != fragment.node
                    || value.requested != fragment_width
                    || value.effective != value.requested
                    || value.before != fragment.start_bu
                    || value.after != fragment.end_bu
                    || value.sequence <= fragment.sequence
                    || fragment_flush_outcomes[related_fragment]
                        .is_some_and(|outcome| value.sequence >= outcome)
                {
                    return Err("invalid execution glyph geometry relationship".to_owned());
                }
                glyph_geometry_coverage[related_fragment] = true;
            }
            GeometryKind::Endline => {
                let boundary = option(value.origin_key)
                    .and_then(|key| usize::try_from(key).ok())
                    .and_then(|key| boundaries.get(key));
                if unit != GeometryUnit::DeviceLine
                    || value.requested != 1
                    || value.effective != 1
                    || value.before < 0
                    || value.after < 0
                    || origin_kind != GeometryOriginKind::Boundary
                    || option(value.origin_key) != option(value.related)
                    || boundary.is_none_or(|boundary| {
                        boundary.request != BoundaryRequest::DeviceEndline
                            || boundary.effect != BoundaryEffect::EndedLine
                            || node != boundary.node
                            || value.before != boundary.line_before
                            || value.after != boundary.line_after
                            || value.sequence <= boundary.enter_sequence
                            || value.sequence >= boundary.leave_sequence
                    })
                {
                    return Err("invalid execution endline geometry relationship".to_owned());
                }
            }
            GeometryKind::Field => {
                let flush = option(value.origin_key)
                    .and_then(|key| usize::try_from(key).ok())
                    .and_then(|key| flushes.get(key));
                if unit != GeometryUnit::Basic
                    || value.before < 0
                    || value.after < 0
                    || origin_kind != GeometryOriginKind::Flush
                    || option(value.related) != option(value.origin_key)
                    || flush.is_none_or(|flush| {
                        node != flush.node
                            || value.sequence <= flush.sequence
                            || value.sequence >= flush.outcome_sequence
                    })
                {
                    return Err("invalid execution field geometry relationship".to_owned());
                }
            }
        }
        geometry.push(ExecutionGeometry {
            key: value.key,
            node,
            related: option(value.related),
            kind,
            unit,
            origin_kind,
            origin_key: option(value.origin_key),
            requested: value.requested,
            effective: value.effective,
            before: value.before,
            after: value.after,
            sequence: value.sequence,
        });
    }
    if glyph_geometry_coverage.iter().any(|covered| !covered) {
        return Err("execution fragment has no unique glyph geometry".to_owned());
    }
    let mut wrappers: Vec<ExecutionWrapper> = reserved_vec(wrapper_count, "wrapper")?;
    let mut heading_nodes = reserved_filled_vec(false, node_count, "heading node ownership")?;
    let mut mdoc_list_item_nodes =
        reserved_filled_vec(false, node_count, "mdoc list item ownership")?;
    let mut last_wrapper_child_leave =
        reserved_filled_vec(None::<u64>, wrapper_count, "wrapper sibling order")?;
    let mut last_root_wrapper_leave = None;
    for (index, value) in wrapper_records.into_iter().enumerate() {
        dense(value.key, index, "wrapper")?;
        if index != 0 && wrappers[index - 1].enter_sequence >= value.enter_sequence {
            return Err("execution wrappers are not in entry order".to_owned());
        }
        let parent = option(value.parent);
        if parent.is_some_and(|key| usize::try_from(key).ok().is_none_or(|key| key >= index)) {
            return Err("execution wrapper parent is not an earlier wrapper".to_owned());
        }
        let node = optional_node_key(value.node, node_count, "wrapper")?;
        let kind = match value.kind {
            1 => ExecutionWrapperKind::Node,
            2 => ExecutionWrapperKind::Font,
            3 => ExecutionWrapperKind::Heading,
            4 => ExecutionWrapperKind::MdocListItem,
            _ => return Err("unknown execution wrapper kind".to_owned()),
        };
        let heading_kind = match (kind, value.detail) {
            (ExecutionWrapperKind::Heading, 1) => Some(ExecutionHeadingKind::ManSection),
            (ExecutionWrapperKind::Heading, 2) => Some(ExecutionHeadingKind::ManSubsection),
            (ExecutionWrapperKind::Heading, 3) => Some(ExecutionHeadingKind::MdocSection),
            (ExecutionWrapperKind::Heading, 4) => Some(ExecutionHeadingKind::MdocSubsection),
            (ExecutionWrapperKind::Heading, _) => {
                return Err("invalid execution heading kind".to_owned());
            }
            (ExecutionWrapperKind::MdocListItem, _) | (_, 0) => None,
            _ => return Err("unexpected execution wrapper heading kind".to_owned()),
        };
        let mdoc_list_kind = match (kind, value.detail) {
            (ExecutionWrapperKind::MdocListItem, 1) => Some(ExecutionMdocListKind::Bullet),
            (ExecutionWrapperKind::MdocListItem, 2) => Some(ExecutionMdocListKind::Dash),
            (ExecutionWrapperKind::MdocListItem, 3) => Some(ExecutionMdocListKind::Enum),
            (ExecutionWrapperKind::MdocListItem, 4) => Some(ExecutionMdocListKind::Hang),
            (ExecutionWrapperKind::MdocListItem, 5) => Some(ExecutionMdocListKind::Hyphen),
            (ExecutionWrapperKind::MdocListItem, 6) => Some(ExecutionMdocListKind::Item),
            (ExecutionWrapperKind::MdocListItem, 7) => Some(ExecutionMdocListKind::Overhang),
            (ExecutionWrapperKind::MdocListItem, 8) => Some(ExecutionMdocListKind::Inset),
            (ExecutionWrapperKind::MdocListItem, 9) => Some(ExecutionMdocListKind::Diagnostic),
            (ExecutionWrapperKind::MdocListItem, 10) => Some(ExecutionMdocListKind::Tag),
            (ExecutionWrapperKind::MdocListItem, 11) => Some(ExecutionMdocListKind::Column),
            (ExecutionWrapperKind::MdocListItem, _) => {
                return Err("invalid execution mdoc list kind".to_owned());
            }
            (_, _) => None,
        };
        if value.enter_atom as usize > atom_count
            || value.leave_atom == NONE
            || value.leave_atom as usize > atom_count
            || value.enter_atom > value.leave_atom
        {
            return Err("invalid execution wrapper atom range".to_owned());
        }
        match kind {
            ExecutionWrapperKind::Node
                if node.is_none()
                    || value.flags != 0
                    || value.target_start != NONE
                    || value.target_length != 0
                    || value.state_before != 0
                    || value.state_after != 0
                    || value.depth_before != 0
                    || value.depth_after != 0
                    || value.enter_sequence >= value.leave_sequence =>
            {
                return Err("invalid execution node wrapper".to_owned());
            }
            ExecutionWrapperKind::Font
                if value.flags != 0
                    || value.target_start != NONE
                    || value.target_length != 0
                    || value.enter_atom != value.leave_atom
                    || value.enter_sequence != value.leave_sequence
                    || value.state_before > 3
                    || value.state_after > 3
                    || value
                        .depth_before
                        .checked_add(1)
                        .is_none_or(|maximum| value.depth_after > maximum) =>
            {
                return Err("invalid execution font transition".to_owned());
            }
            ExecutionWrapperKind::Heading
                if node.is_none()
                    || value.flags != 0
                    || value.state_before != 0
                    || value.state_after != 0
                    || value.depth_before != 0
                    || value.depth_after != 0
                    || value.enter_sequence >= value.leave_sequence
                    || parent.is_none()
                    || node.is_none_or(|key| {
                        expected_heading_kind(&nodes[key.0 as usize]) != heading_kind
                    }) =>
            {
                return Err("invalid execution heading wrapper".to_owned());
            }
            ExecutionWrapperKind::MdocListItem
                if node.is_none()
                    || value.flags & !1 != 0
                    || value.target_start != NONE
                    || value.target_length != 0
                    || value.state_before >= 1 << 23
                    || value.state_after >= 1 << 23
                    || value.depth_before != 0
                    || value.depth_after != 0
                    || value.enter_sequence >= value.leave_sequence
                    || parent.is_none()
                    || node.is_none_or(|key| !is_mdoc_list_item(&nodes, key)) =>
            {
                return Err("invalid execution mdoc list item wrapper".to_owned());
            }
            _ => {}
        }
        let last_sibling_leave = if let Some(parent) = parent {
            let parent = usize::try_from(parent)
                .map_err(|_| "execution wrapper parent key overflow".to_owned())?;
            let parent_wrapper = &wrappers[parent];
            if parent_wrapper.enter_sequence >= value.enter_sequence
                || parent_wrapper.leave_sequence <= value.leave_sequence
            {
                return Err("execution wrapper is outside its parent".to_owned());
            }
            if matches!(
                kind,
                ExecutionWrapperKind::Heading | ExecutionWrapperKind::MdocListItem
            ) && (parent_wrapper.kind != ExecutionWrapperKind::Node
                || parent_wrapper.node != node)
            {
                return Err("execution heading is not nested in its head node".to_owned());
            }
            &mut last_wrapper_child_leave[parent]
        } else {
            &mut last_root_wrapper_leave
        };
        if last_sibling_leave.is_some_and(|leave| value.enter_sequence <= leave) {
            return Err("overlapping execution wrapper siblings".to_owned());
        }
        *last_sibling_leave = Some(value.leave_sequence);
        if kind == ExecutionWrapperKind::Heading {
            let node = node.expect("validated heading node").0 as usize;
            if heading_nodes[node] {
                return Err("duplicate execution heading wrapper".to_owned());
            }
            heading_nodes[node] = true;
        } else if kind == ExecutionWrapperKind::MdocListItem {
            let node = node.expect("validated mdoc list item node").0 as usize;
            if mdoc_list_item_nodes[node] {
                return Err("duplicate execution mdoc list item wrapper".to_owned());
            }
            mdoc_list_item_nodes[node] = true;
        }
        wrappers.push(ExecutionWrapper {
            key: value.key,
            parent,
            node,
            kind,
            target: optional_pool(
                value.target_start,
                value.target_length,
                &pool,
                "wrapper target",
            )?,
            enter_atom: value.enter_atom,
            leave_atom: value.leave_atom,
            heading_kind,
            mdoc_list_kind,
            flags: value.flags,
            state_before: value.state_before,
            state_after: value.state_after,
            depth_before: value.depth_before,
            depth_after: value.depth_after,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    if nodes
        .iter()
        .enumerate()
        .any(|(index, node)| expected_heading_kind(node).is_some() != heading_nodes[index])
    {
        return Err("missing execution heading wrapper".to_owned());
    }
    if nodes
        .iter()
        .enumerate()
        .any(|(index, node)| is_mdoc_list_item(&nodes, node.key) != mdoc_list_item_nodes[index])
    {
        return Err("missing execution mdoc list item wrapper".to_owned());
    }
    validate_active_wrapper_owners(&wrappers, &boundaries, &controls)?;
    // Upstream roff_term_pre_mc() stores an authored margin character and a
    // later term_newln() emits it inside another node wrapper.  Preserve that
    // one evidenced delayed-source edge without allowing arbitrary events to
    // escape their source-node subtree.
    let mut margin_control_nodes = reserved_filled_vec(false, node_count, "margin controls")?;
    for control in &controls {
        if control.request == ExecutionControlRequest::MarginCharacter {
            margin_control_nodes[control.node.0 as usize] = true;
        }
    }
    for index in 0..node_count {
        if let Some(parent) = nodes[index].parent {
            margin_control_nodes[index] |= margin_control_nodes[parent.0 as usize];
        }
    }
    let mut margin_atoms = reserved_filled_vec(false, atom_count, "margin atoms")?;
    for fragment in &fragments {
        if fragment.role == FragmentRole::MarginDecoration {
            for atom in &fragment.atoms {
                margin_atoms[atom.0 as usize] = true;
            }
        }
    }
    for word in &words {
        if let Some(wrapper_key) = word.wrapper {
            let wrapper = &wrappers[wrapper_key as usize];
            let delayed_margin = word.node.is_some_and(|node| {
                margin_control_nodes[node.0 as usize]
                    && !word.atoms.is_empty()
                    && word.atoms.clone().all(|atom| margin_atoms[atom as usize])
            });
            if !is_structural_wrapper(wrapper.kind)
                || word.enter_sequence <= wrapper.enter_sequence
                || word.leave_sequence >= wrapper.leave_sequence
                || word.atoms.start < wrapper.enter_atom
                || word.atoms.end > wrapper.leave_atom
                || word.node.is_none_or(|node| {
                    !node_is_within(&nodes, node, wrapper.node.expect("validated node wrapper"))
                        && !delayed_margin
                })
            {
                return Err("execution word is outside its node wrapper".to_owned());
            }
        }
    }
    for atom in &atoms {
        if let Some(wrapper_key) = atom.wrapper {
            let wrapper = &wrappers[wrapper_key as usize];
            let delayed_margin = atom.node.is_some_and(|node| {
                margin_control_nodes[node.0 as usize] && margin_atoms[atom.key.0 as usize]
            });
            if !is_structural_wrapper(wrapper.kind)
                || atom.sequence <= wrapper.enter_sequence
                || atom.sequence >= wrapper.leave_sequence
                || atom.key.0 < wrapper.enter_atom
                || atom.key.0 >= wrapper.leave_atom
                || atom.node.is_none_or(|node| {
                    !node_is_within(&nodes, node, wrapper.node.expect("validated node wrapper"))
                        && !delayed_margin
                })
            {
                return Err("execution atom is outside its node wrapper".to_owned());
            }
        }
    }
    for fragment in &fragments {
        if let Some(wrapper_key) = fragment.wrapper {
            let wrapper = &wrappers[wrapper_key as usize];
            // A buffered atom can be emitted after its source node wrapper
            // unwinds.  The fragment wrapper therefore identifies the later
            // execution interval, while the referenced atom carries the AST
            // source owner; requiring that owner to be below this wrapper
            // would reject the delayed flushes performed by term_flushln().
            if !is_structural_wrapper(wrapper.kind)
                || fragment.sequence <= wrapper.enter_sequence
                || fragment.sequence >= wrapper.leave_sequence
            {
                return Err("execution fragment is outside its node wrapper".to_owned());
            }
        }
    }
    if atoms
        .windows(2)
        .any(|pair| pair[0].sequence > pair[1].sequence)
        || fragments
            .windows(2)
            .any(|pair| pair[0].sequence > pair[1].sequence)
    {
        return Err("native execution event keys are not in execution order".to_owned());
    }
    let mut references: Vec<ExecutionReference> =
        reserved_vec(reference_records.len(), "reference")?;
    let mut last_child_leave =
        reserved_filled_vec(None::<u64>, reference_count, "reference sibling order")?;
    let mut last_root_leave = None;
    for (index, value) in reference_records.into_iter().enumerate() {
        dense(value.key, index, "reference")?;
        if value.flags != 0
            || value.enter_atom > value.label_start_atom
            || value.label_start_atom > value.leave_atom
            || value.leave_atom as usize > atom_count
            || value.leave_sequence <= value.enter_sequence
            || (index != 0 && references[index - 1].enter_sequence >= value.enter_sequence)
        {
            return Err("invalid execution semantic reference".to_owned());
        }
        let kind = match value.kind {
            1 => ExecutionReferenceKind::ExternalUri,
            2 => ExecutionReferenceKind::Email,
            3 => ExecutionReferenceKind::Manual,
            4 => ExecutionReferenceKind::SameDocumentSection,
            _ => return Err("unknown execution semantic reference kind".to_owned()),
        };
        let affinity = match value.affinity {
            1 => ExecutionAffinity::Inline,
            _ => return Err("invalid execution semantic reference affinity".to_owned()),
        };
        let owner_node = node_key(value.owner_node, node_count, "reference owner")?;
        let target_node = node_key(value.target_node, node_count, "reference target")?;
        let execution_atoms = value.enter_atom..value.leave_atom;
        let label_atoms = value.label_start_atom..value.leave_atom;
        let previous_atom = value
            .enter_atom
            .checked_sub(1)
            .and_then(|key| atoms.get(key as usize));
        let first_atom = atoms.get(value.enter_atom as usize);
        let last_atom = value
            .leave_atom
            .checked_sub(1)
            .and_then(|key| atoms.get(key as usize));
        let next_atom = atoms.get(value.leave_atom as usize);
        if !node_is_within(&nodes, target_node, owner_node)
            || previous_atom.is_some_and(|atom| atom.sequence >= value.enter_sequence)
            || (value.enter_atom < value.leave_atom
                && first_atom.is_none_or(|atom| atom.sequence <= value.enter_sequence))
            || (value.enter_atom == value.leave_atom
                && first_atom.is_some_and(|atom| atom.sequence <= value.leave_sequence))
            || (value.enter_atom < value.leave_atom
                && last_atom.is_none_or(|atom| atom.sequence >= value.leave_sequence))
            || next_atom.is_some_and(|atom| atom.sequence <= value.leave_sequence)
            || atoms[value.enter_atom as usize..value.leave_atom as usize]
                .iter()
                .any(|atom| {
                    atom.sequence <= value.enter_sequence || atom.sequence >= value.leave_sequence
                })
            || atoms[value.enter_atom as usize..value.label_start_atom as usize]
                .iter()
                .any(|atom| atom.role != AtomRole::ImplicitSpace)
            || atoms
                .get(value.label_start_atom as usize)
                .is_some_and(|atom| {
                    value.label_start_atom < value.leave_atom
                        && atom.role == AtomRole::ImplicitSpace
                })
        {
            return Err("invalid execution semantic reference relationship".to_owned());
        }
        let secondary = optional_pool(
            value.secondary_start,
            value.secondary_length,
            &pool,
            "reference secondary target",
        )?;
        if kind != ExecutionReferenceKind::Manual && secondary.is_some() {
            return Err("invalid execution semantic reference components".to_owned());
        }
        let parent = option(value.parent);
        let last_sibling_leave = if let Some(parent) = parent {
            let parent_index = usize::try_from(parent)
                .ok()
                .filter(|parent| *parent < index)
                .ok_or_else(|| "invalid execution reference parent".to_owned())?;
            let parent_reference = &references[parent_index];
            if parent_reference.enter_sequence >= value.enter_sequence
                || parent_reference.leave_sequence <= value.leave_sequence
                || parent_reference.execution_atoms.start > execution_atoms.start
                || parent_reference.execution_atoms.end < execution_atoms.end
                || parent_reference.atoms.start > label_atoms.start
                || parent_reference.atoms.end < label_atoms.end
            {
                return Err("invalid execution reference nesting".to_owned());
            }
            &mut last_child_leave[parent_index]
        } else {
            &mut last_root_leave
        };
        if last_sibling_leave.is_some_and(|sequence| value.enter_sequence <= sequence) {
            return Err("overlapping execution reference siblings".to_owned());
        }
        *last_sibling_leave = Some(value.leave_sequence);
        references.push(ExecutionReference {
            key: value.key,
            parent,
            owner_node,
            target_node,
            kind,
            primary: pool_range(
                value.primary_start,
                value.primary_length,
                &pool,
                "reference primary target",
            )?,
            secondary,
            execution_atoms,
            atoms: label_atoms,
            affinity,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    let mut anchors = reserved_vec(anchor_records.len(), "anchor")?;
    for (index, value) in anchor_records.into_iter().enumerate() {
        dense(value.key, index, "anchor")?;
        if value.reserved != 0 {
            return Err("non-zero reserved execution anchor field".to_owned());
        }
        if value.device_line == 0
            || value.atom_cursor as usize > atom_count
            || value.fragment_cursor as usize > fragment_count
            || value.affinity != 2
            || value
                .atom_cursor
                .checked_sub(1)
                .and_then(|key| atoms.get(key as usize))
                .is_some_and(|atom| atom.sequence >= value.sequence)
            || atoms
                .get(value.atom_cursor as usize)
                .is_some_and(|atom| atom.sequence <= value.sequence)
            || value
                .fragment_cursor
                .checked_sub(1)
                .and_then(|key| fragments.get(key as usize))
                .is_some_and(|fragment| fragment.sequence >= value.sequence)
            || fragments
                .get(value.fragment_cursor as usize)
                .is_some_and(|fragment| fragment.sequence <= value.sequence)
        {
            return Err("invalid execution anchor attachment".to_owned());
        }
        anchors.push(ExecutionAnchor {
            key: value.key,
            node: node_key(value.node, node_count, "anchor")?,
            target: pool_range(
                value.target_start,
                value.target_length,
                &pool,
                "anchor target",
            )?,
            device_line: value.device_line,
            atom_cursor: value.atom_cursor,
            fragment_cursor: value.fragment_cursor,
            affinity: ExecutionAffinity::BeforeOutput,
            sequence: value.sequence,
        });
    }

    let mut tables = reserved_vec(table_count, "table")?;
    let mut expected_row_cursor = 0_u32;
    let mut expected_cell_cursor = 0_u32;
    let mut previous_table_leave = None;
    for (index, value) in table_records.into_iter().enumerate() {
        dense(value.key, index, "table")?;
        let rows = counted_range(value.row_start, value.row_length, "table rows")?;
        let cells = counted_range(value.cell_start, value.cell_length, "table cells")?;
        let table_atoms = range(value.enter_atom, value.leave_atom, "table atoms")?;
        let table_fragments = range(
            value.enter_fragment,
            value.leave_fragment,
            "table fragments",
        )?;
        let table_flushes = range(value.enter_flush, value.leave_flush, "table flushes")?;
        if value.flags != 0
            || value.logical_columns == 0
            || rows.is_empty()
            || rows.start != expected_row_cursor
            || cells.start != expected_cell_cursor
            || rows.end as usize > table_row_count
            || cells.end as usize > table_cell_count
            || table_atoms.end as usize > atom_count
            || table_fragments.end as usize > fragment_count
            || table_flushes.end as usize > flush_records.len()
            || value.enter_sequence >= value.leave_sequence
            || previous_table_leave.is_some_and(|leave| value.enter_sequence <= leave)
            || !exact_sequence_range(
                &atoms,
                &table_atoms,
                value.enter_sequence,
                value.leave_sequence,
                |atom| atom.sequence,
            )
        {
            return Err("invalid execution table envelope".to_owned());
        }
        if !exact_sequence_range(
            &fragments,
            &table_fragments,
            value.enter_sequence,
            value.leave_sequence,
            |fragment| fragment.sequence,
        ) || !exact_sequence_range(
            &flushes,
            &table_flushes,
            value.enter_sequence,
            value.leave_sequence,
            |flush| flush.sequence,
        ) {
            return Err("execution table cursors do not match its lifetime".to_owned());
        }
        expected_row_cursor = rows.end;
        expected_cell_cursor = cells.end;
        previous_table_leave = Some(value.leave_sequence);
        tables.push(ExecutionTable {
            key: ExecutionTableKey(value.key),
            first_row_node: node_key(value.first_row_node, node_count, "table first row")?,
            rows,
            cells,
            logical_columns: value.logical_columns,
            flags: value.flags,
            atoms: table_atoms,
            fragments: table_fragments,
            flushes: table_flushes,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    if expected_row_cursor as usize != table_row_count
        || expected_cell_cursor as usize != table_cell_count
    {
        return Err("execution table ranges do not partition rows and cells".to_owned());
    }

    let mut table_rows = reserved_vec(table_row_count, "table-row")?;
    let mut next_cell_by_table = reserved_filled_vec(0_u32, table_count, "table cell cursor")?;
    let mut next_ordinal_by_table = reserved_filled_vec(0_u32, table_count, "table row ordinal")?;
    let mut previous_row_leave = reserved_filled_vec(None::<u64>, table_count, "table row order")?;
    for (index, value) in table_row_records.into_iter().enumerate() {
        dense(value.key, index, "table-row")?;
        let table_index = usize::try_from(value.table)
            .ok()
            .filter(|table| *table < table_count)
            .ok_or_else(|| "invalid execution table-row parent".to_owned())?;
        let table = &tables[table_index];
        let node = node_key(value.node, node_count, "table-row node")?;
        let cells = counted_range(value.cell_start, value.cell_length, "table-row cells")?;
        let row_atoms = range(value.enter_atom, value.leave_atom, "table-row atoms")?;
        let row_fragments = range(
            value.enter_fragment,
            value.leave_fragment,
            "table-row fragments",
        )?;
        let row_flushes = range(value.enter_flush, value.leave_flush, "table-row flushes")?;
        let kind = match value.kind {
            1 => ExecutionTableRowKind::Data,
            2 => ExecutionTableRowKind::HorizontalRule,
            3 => ExecutionTableRowKind::DoubleHorizontalRule,
            _ => return Err("unknown execution table-row kind".to_owned()),
        };
        if value.ordinal != next_ordinal_by_table[table_index]
            || value.logical_columns != table.logical_columns
            || value.key < table.rows.start
            || value.key >= table.rows.end
            || cells.start != next_cell_by_table[table_index].max(table.cells.start)
            || cells.end > table.cells.end
            || row_atoms.start < table.atoms.start
            || row_atoms.end > table.atoms.end
            || row_fragments.start < table.fragments.start
            || row_fragments.end > table.fragments.end
            || row_flushes.start < table.flushes.start
            || row_flushes.end > table.flushes.end
            || value.enter_sequence <= table.enter_sequence
            || value.leave_sequence >= table.leave_sequence
            || previous_row_leave[table_index].is_some_and(|leave| value.enter_sequence <= leave)
            || (kind != ExecutionTableRowKind::Data && !cells.is_empty())
            || nodes[node.0 as usize].kind != crate::NodeKind::Table
            || !exact_sequence_range(
                &atoms,
                &row_atoms,
                value.enter_sequence,
                value.leave_sequence,
                |atom| atom.sequence,
            )
            || !exact_sequence_range(
                &fragments,
                &row_fragments,
                value.enter_sequence,
                value.leave_sequence,
                |fragment| fragment.sequence,
            )
            || !exact_sequence_range(
                &flushes,
                &row_flushes,
                value.enter_sequence,
                value.leave_sequence,
                |flush| flush.sequence,
            )
        {
            return Err("invalid execution table-row relationship".to_owned());
        }
        if value.ordinal == 0 && node != table.first_row_node {
            return Err("execution table first-row node does not match".to_owned());
        }
        next_ordinal_by_table[table_index] = value
            .ordinal
            .checked_add(1)
            .ok_or_else(|| "execution table-row ordinal overflow".to_owned())?;
        next_cell_by_table[table_index] = cells.end;
        previous_row_leave[table_index] = Some(value.leave_sequence);
        table_rows.push(ExecutionTableRow {
            key: ExecutionTableRowKey(value.key),
            table: ExecutionTableKey(value.table),
            node,
            ordinal: value.ordinal,
            kind,
            logical_columns: value.logical_columns,
            cells,
            atoms: row_atoms,
            fragments: row_fragments,
            flushes: row_flushes,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    for (table_index, table) in tables.iter().enumerate() {
        if next_ordinal_by_table[table_index] != table.rows.end - table.rows.start
            || next_cell_by_table[table_index].max(table.cells.start) != table.cells.end
        {
            return Err("execution table rows do not fill their parent ranges".to_owned());
        }
    }

    let mut table_cells = reserved_vec(table_cell_count, "table-cell")?;
    let mut next_ordinal_by_row =
        reserved_filled_vec(0_u32, table_row_count, "table-cell ordinal")?;
    let mut next_column_by_row = reserved_filled_vec(0_u32, table_row_count, "table-cell column")?;
    let mut next_data_ordinal_by_row =
        reserved_filled_vec(0_u32, table_row_count, "table data-cell ordinal")?;
    let mut previous_cell_leave =
        reserved_filled_vec(None::<u64>, table_row_count, "table-cell order")?;
    let mut generation_cell = reserved_filled_vec(
        None::<ExecutionTableCellKey>,
        buffer_generation_count,
        "table generation owner",
    )?;
    for (index, value) in table_cell_records.into_iter().enumerate() {
        dense(value.key, index, "table-cell")?;
        let row_index = usize::try_from(value.row)
            .ok()
            .filter(|row| *row < table_row_count)
            .ok_or_else(|| "invalid execution table-cell parent".to_owned())?;
        let row = &table_rows[row_index];
        let node = node_key(value.node, node_count, "table-cell node")?;
        let cell_atoms = range(value.enter_atom, value.leave_atom, "table-cell atoms")?;
        let layout_kind = match value.layout_kind {
            1 => ExecutionTableLayoutKind::Center,
            2 => ExecutionTableLayoutKind::Right,
            3 => ExecutionTableLayoutKind::Left,
            4 => ExecutionTableLayoutKind::Numeric,
            5 => ExecutionTableLayoutKind::Span,
            6 => ExecutionTableLayoutKind::Long,
            7 => ExecutionTableLayoutKind::Down,
            8 => ExecutionTableLayoutKind::HorizontalRule,
            9 => ExecutionTableLayoutKind::DoubleHorizontalRule,
            _ => return Err("unknown execution table layout kind".to_owned()),
        };
        let data_kind = match value.data_kind {
            1 => ExecutionTableDataKind::None,
            2 => ExecutionTableDataKind::Text,
            3 => ExecutionTableDataKind::HorizontalRule,
            4 => ExecutionTableDataKind::DoubleHorizontalRule,
            5 => ExecutionTableDataKind::IsolatedHorizontalRule,
            6 => ExecutionTableDataKind::IsolatedDoubleHorizontalRule,
            _ => return Err("unknown execution table data kind".to_owned()),
        };
        let alignment = match value.alignment {
            0 => ExecutionTableAlignment::None,
            1 => ExecutionTableAlignment::Left,
            2 => ExecutionTableAlignment::Center,
            3 => ExecutionTableAlignment::Right,
            4 => ExecutionTableAlignment::Numeric,
            5 => ExecutionTableAlignment::Long,
            _ => return Err("unknown execution table alignment".to_owned()),
        };
        let font = match value.font {
            0 => ExecutionFont::Roman,
            1 => ExecutionFont::Bold,
            2 => ExecutionFont::Underline,
            3 => ExecutionFont::BoldUnderline,
            _ => return Err("unknown execution table font".to_owned()),
        };
        if value.data_ordinal != next_data_ordinal_by_row[row_index] {
            return Err("non-contiguous execution table data-cell ordinal".to_owned());
        }
        next_data_ordinal_by_row[row_index] = next_data_ordinal_by_row[row_index]
            .checked_add(1)
            .ok_or_else(|| "execution table data-cell ordinal overflow".to_owned())?;
        let expected_alignment = match layout_kind {
            ExecutionTableLayoutKind::Left => ExecutionTableAlignment::Left,
            ExecutionTableLayoutKind::Center => ExecutionTableAlignment::Center,
            ExecutionTableLayoutKind::Right => ExecutionTableAlignment::Right,
            ExecutionTableLayoutKind::Numeric => ExecutionTableAlignment::Numeric,
            ExecutionTableLayoutKind::Long => ExecutionTableAlignment::Long,
            ExecutionTableLayoutKind::Span
            | ExecutionTableLayoutKind::Down
            | ExecutionTableLayoutKind::HorizontalRule
            | ExecutionTableLayoutKind::DoubleHorizontalRule => ExecutionTableAlignment::None,
        };
        if value.reserved != 0
            || value.flags & !0x01ff != 0
            || value.ordinal != next_ordinal_by_row[row_index]
            || value.key < row.cells.start
            || value.key >= row.cells.end
            || value.logical_column < next_column_by_row[row_index]
            || value.column_span == 0
            || value.row_span == 0
            || value
                .logical_column
                .checked_add(value.column_span)
                .is_none_or(|end| end > row.logical_columns)
            || alignment != expected_alignment
            || node != row.node
            || cell_atoms.start < row.atoms.start
            || cell_atoms.end > row.atoms.end
            || value.enter_sequence <= row.enter_sequence
            || value.leave_sequence >= row.leave_sequence
            || previous_cell_leave[row_index].is_some_and(|leave| value.enter_sequence <= leave)
            || value.offset_bu < 0
            || value.rmargin_bu < value.offset_bu
            || value.coloff_before_bu < 0
            || value.coloff_after_bu < value.coloff_before_bu
            || !exact_sequence_range(
                &atoms,
                &cell_atoms,
                value.enter_sequence,
                value.leave_sequence,
                |atom| atom.sequence,
            )
        {
            return Err("invalid execution table-cell relationship".to_owned());
        }
        let buffer = option(value.buffer);
        let buffer_generation = option(value.buffer_generation);
        match (buffer, buffer_generation) {
            (Some(buffer), Some(generation_key)) => {
                let generation_index = usize::try_from(generation_key)
                    .ok()
                    .filter(|key| *key < buffer_generation_count)
                    .ok_or_else(|| "invalid table-cell buffer generation".to_owned())?;
                let generation = &buffer_generations[generation_index];
                if cell_atoms.is_empty()
                    || generation.buffer != buffer
                    || generation.open_sequence <= value.enter_sequence
                    || generation.open_sequence >= value.leave_sequence
                    || generation.close_sequence >= row.leave_sequence
                    || generation_cell[generation_index].is_some()
                    || atoms[cell_atoms.start as usize..cell_atoms.end as usize]
                        .iter()
                        .any(|atom| {
                            atom.node != Some(node)
                                || atom.buffer != Some(buffer)
                                || atom.buffer_generation != Some(generation_key)
                        })
                {
                    return Err("invalid execution table-cell buffer relationship".to_owned());
                }
                generation_cell[generation_index] = Some(ExecutionTableCellKey(value.key));
            }
            (None, None) if !cell_atoms.is_empty() => {
                return Err("table cell with atoms has no buffer generation".to_owned());
            }
            (None, None) => {}
            _ => return Err("partial execution table-cell buffer identity".to_owned()),
        }
        next_ordinal_by_row[row_index] = value
            .ordinal
            .checked_add(1)
            .ok_or_else(|| "execution table-cell ordinal overflow".to_owned())?;
        next_column_by_row[row_index] = value.logical_column + value.column_span;
        previous_cell_leave[row_index] = Some(value.leave_sequence);
        table_cells.push(ExecutionTableCell {
            key: ExecutionTableCellKey(value.key),
            row: ExecutionTableRowKey(value.row),
            node,
            ordinal: value.ordinal,
            data_ordinal: value.data_ordinal,
            logical_column: value.logical_column,
            column_span: value.column_span,
            row_span: value.row_span,
            layout_kind,
            data_kind,
            alignment,
            font,
            flags: ExecutionTableCellFlags(value.flags),
            buffer,
            buffer_generation,
            atoms: cell_atoms,
            offset_bu: value.offset_bu,
            rmargin_bu: value.rmargin_bu,
            coloff_before_bu: value.coloff_before_bu,
            coloff_after_bu: value.coloff_after_bu,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    for (row_index, row) in table_rows.iter().enumerate() {
        if next_ordinal_by_row[row_index] != row.cells.end - row.cells.start {
            return Err("execution table cells do not fill their row range".to_owned());
        }
    }
    for flush in &flushes {
        let Some(owner) = generation_cell
            .get(flush.buffer_generation as usize)
            .and_then(|owner| *owner)
        else {
            continue;
        };
        let cell = &table_cells[owner.0 as usize];
        let row = &table_rows[cell.row.0 as usize];
        let table = &tables[row.table.0 as usize];
        if flush.key < row.flushes.start
            || flush.key >= row.flushes.end
            || flush.key < table.flushes.start
            || flush.key >= table.flushes.end
        {
            return Err("table-cell flush escapes its row or table".to_owned());
        }
    }
    for fragment in &fragments {
        let Some(owner) = fragment
            .buffer_generation
            .and_then(|generation| generation_cell.get(generation as usize))
            .and_then(|owner| *owner)
        else {
            continue;
        };
        let cell = &table_cells[owner.0 as usize];
        let row = &table_rows[cell.row.0 as usize];
        let table = &tables[row.table.0 as usize];
        if fragment.key.0 < row.fragments.start
            || fragment.key.0 >= row.fragments.end
            || fragment.key.0 < table.fragments.start
            || fragment.key.0 >= table.fragments.end
        {
            return Err("table-cell fragment escapes its row or table".to_owned());
        }
    }
    for atom in &atoms {
        let owner = atom
            .buffer_generation
            .and_then(|generation| generation_cell.get(generation as usize))
            .and_then(|owner| *owner);
        let Some(owner) = owner else {
            if atom.role == AtomRole::TableCellPayload {
                return Err("table-cell payload atom has no cell owner".to_owned());
            }
            continue;
        };
        let cell = &table_cells[owner.0 as usize];
        if atom.key.0 < cell.atoms.start
            || atom.key.0 >= cell.atoms.end
            || atom.buffer != cell.buffer
        {
            return Err("table-cell atom escapes its cell".to_owned());
        }
    }

    let mut diagnostics = reserved_vec(diagnostic_records.len(), "diagnostic")?;
    for value in diagnostic_records {
        diagnostics.push(ExecutionDiagnostic {
            code: value.code,
            node: option(value.node)
                .map(|key| node_key(key, node_count, "diagnostic"))
                .transpose()?,
            message: pool_range(
                value.message_start,
                value.message_length,
                &pool,
                "diagnostic message",
            )?,
            sequence: value.sequence,
        });
    }
    validate_event_sequences(
        &words,
        &atoms,
        &buffer_generations,
        &fragments,
        &flushes,
        &boundaries,
        &controls,
        &geometry,
        &wrappers,
        &references,
        &anchors,
        &tables,
        &table_rows,
        &table_cells,
        &diagnostics,
    )?;
    Ok(NativeExecutionReport {
        pool,
        work_units,
        record_count,
        record_bytes: 0,
        buffer_cells,
        sources,
        nodes,
        buffer_generations,
        words,
        atoms,
        fragments,
        flushes,
        boundaries,
        controls,
        geometry,
        wrappers,
        references,
        anchors,
        tables,
        table_rows,
        table_cells,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn validate_event_sequences(
    words: &[ExecutionWord],
    atoms: &[ExecutionAtom],
    buffer_generations: &[ExecutionBufferGeneration],
    fragments: &[ExecutionFragment],
    flushes: &[ExecutionFlush],
    boundaries: &[ExecutionBoundary],
    controls: &[ExecutionControl],
    geometry: &[ExecutionGeometry],
    wrappers: &[ExecutionWrapper],
    references: &[ExecutionReference],
    anchors: &[ExecutionAnchor],
    tables: &[ExecutionTable],
    table_rows: &[ExecutionTableRow],
    table_cells: &[ExecutionTableCell],
    diagnostics: &[ExecutionDiagnostic],
) -> Result<(), String> {
    let generation_sequences = buffer_generations
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let flush_sequences = flushes
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let control_sequences = controls
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let wrapper_sequences = wrappers
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let reference_sequences = references
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let table_sequences = tables
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let table_row_sequences = table_rows
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let table_cell_sequences = table_cells
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let sequence_count = [
        words
            .len()
            .checked_mul(2)
            .ok_or_else(|| "native execution sequence count overflow".to_owned())?,
        atoms.len(),
        generation_sequences,
        fragments.len(),
        flush_sequences,
        boundaries
            .len()
            .checked_mul(2)
            .ok_or_else(|| "native execution sequence count overflow".to_owned())?,
        control_sequences,
        geometry.len(),
        wrapper_sequences,
        reference_sequences,
        anchors.len(),
        table_sequences,
        table_row_sequences,
        table_cell_sequences,
        diagnostics.len(),
    ]
    .into_iter()
    .try_fold(0_usize, usize::checked_add)
    .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let mut seen = reserved_vec(sequence_count, "event sequences")?;
    for word in words {
        seen.push(word.enter_sequence);
        seen.push(word.leave_sequence);
    }
    for generation in buffer_generations {
        seen.push(generation.open_sequence);
        seen.push(generation.close_sequence);
    }
    for atom in atoms {
        seen.push(atom.sequence);
        if atom.replaced_by.is_some_and(|key| {
            atoms
                .get(key.0 as usize)
                .is_none_or(|replacement| replacement.sequence <= atom.sequence)
        }) {
            return Err("execution atom replacement is not later in execution order".to_owned());
        }
    }
    for fragment in fragments {
        seen.push(fragment.sequence);
        if fragment.atoms.iter().any(|key| {
            atoms
                .get(key.0 as usize)
                .is_none_or(|atom| atom.sequence >= fragment.sequence)
        }) {
            return Err("execution fragment precedes one of its atoms".to_owned());
        }
    }
    for flush in flushes {
        seen.push(flush.sequence);
        seen.push(flush.outcome_sequence);
    }
    for boundary in boundaries {
        seen.push(boundary.enter_sequence);
        seen.push(boundary.leave_sequence);
    }
    for control in controls {
        seen.push(control.enter_sequence);
        seen.push(control.leave_sequence);
    }
    for fact in geometry {
        seen.push(fact.sequence);
    }
    for wrapper in wrappers {
        seen.push(wrapper.enter_sequence);
        if wrapper.leave_sequence != wrapper.enter_sequence {
            seen.push(wrapper.leave_sequence);
        }
    }
    for reference in references {
        seen.push(reference.enter_sequence);
        seen.push(reference.leave_sequence);
    }
    for anchor in anchors {
        seen.push(anchor.sequence);
    }
    for table in tables {
        seen.push(table.enter_sequence);
        seen.push(table.leave_sequence);
    }
    for row in table_rows {
        seen.push(row.enter_sequence);
        seen.push(row.leave_sequence);
    }
    for cell in table_cells {
        seen.push(cell.enter_sequence);
        seen.push(cell.leave_sequence);
    }
    for diagnostic in diagnostics {
        seen.push(diagnostic.sequence);
    }
    seen.sort_unstable();
    if seen.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err("duplicate native execution sequence".to_owned());
    }
    Ok(())
}

fn source_offsets() -> [usize; 6] {
    [
        offset_of!(CSourceRecord, key),
        offset_of!(CSourceRecord, parent),
        offset_of!(CSourceRecord, include_node),
        offset_of!(CSourceRecord, flags),
        offset_of!(CSourceRecord, path_start),
        offset_of!(CSourceRecord, path_length),
    ]
}
fn node_offsets() -> [usize; 9] {
    [
        offset_of!(CNodeRecord, key),
        offset_of!(CNodeRecord, parent),
        offset_of!(CNodeRecord, source),
        offset_of!(CNodeRecord, line),
        offset_of!(CNodeRecord, column),
        offset_of!(CNodeRecord, kind),
        offset_of!(CNodeRecord, flags),
        offset_of!(CNodeRecord, macro_start),
        offset_of!(CNodeRecord, macro_length),
    ]
}
fn buffer_generation_offsets() -> [usize; 9] {
    [
        offset_of!(CBufferGenerationRecord, key),
        offset_of!(CBufferGenerationRecord, buffer),
        offset_of!(CBufferGenerationRecord, generation),
        offset_of!(CBufferGenerationRecord, capacity),
        offset_of!(CBufferGenerationRecord, extent),
        offset_of!(CBufferGenerationRecord, close_reason),
        offset_of!(CBufferGenerationRecord, reserved),
        offset_of!(CBufferGenerationRecord, open_sequence),
        offset_of!(CBufferGenerationRecord, close_sequence),
    ]
}
fn word_offsets() -> [usize; 12] {
    [
        offset_of!(CWordRecord, key),
        offset_of!(CWordRecord, node),
        offset_of!(CWordRecord, source),
        offset_of!(CWordRecord, operand_start),
        offset_of!(CWordRecord, operand_length),
        offset_of!(CWordRecord, role),
        offset_of!(CWordRecord, wrapper),
        offset_of!(CWordRecord, enter_atom),
        offset_of!(CWordRecord, leave_atom),
        offset_of!(CWordRecord, reserved),
        offset_of!(CWordRecord, enter_sequence),
        offset_of!(CWordRecord, leave_sequence),
    ]
}
fn atom_offsets() -> [usize; 19] {
    [
        offset_of!(CAtomRecord, key),
        offset_of!(CAtomRecord, buffer),
        offset_of!(CAtomRecord, generation),
        offset_of!(CAtomRecord, buffer_generation),
        offset_of!(CAtomRecord, slot),
        offset_of!(CAtomRecord, kind),
        offset_of!(CAtomRecord, role),
        offset_of!(CAtomRecord, input_scalar),
        offset_of!(CAtomRecord, display_scalar),
        offset_of!(CAtomRecord, width_bu),
        offset_of!(CAtomRecord, node),
        offset_of!(CAtomRecord, source),
        offset_of!(CAtomRecord, operand_start),
        offset_of!(CAtomRecord, operand_length),
        offset_of!(CAtomRecord, font),
        offset_of!(CAtomRecord, wrapper),
        offset_of!(CAtomRecord, replaced_by),
        offset_of!(CAtomRecord, disposition),
        offset_of!(CAtomRecord, sequence),
    ]
}
fn fragment_offsets() -> [usize; 14] {
    [
        offset_of!(CFragmentRecord, key),
        offset_of!(CFragmentRecord, node),
        offset_of!(CFragmentRecord, buffer),
        offset_of!(CFragmentRecord, generation),
        offset_of!(CFragmentRecord, buffer_generation),
        offset_of!(CFragmentRecord, atom_ref_start),
        offset_of!(CFragmentRecord, atom_ref_length),
        offset_of!(CFragmentRecord, device_line),
        offset_of!(CFragmentRecord, role),
        offset_of!(CFragmentRecord, wrapper),
        offset_of!(CFragmentRecord, reserved),
        offset_of!(CFragmentRecord, start_bu),
        offset_of!(CFragmentRecord, end_bu),
        offset_of!(CFragmentRecord, sequence),
    ]
}
fn fragment_atom_offsets() -> [usize; 2] {
    [
        offset_of!(CFragmentAtomRecord, fragment),
        offset_of!(CFragmentAtomRecord, atom),
    ]
}
fn flush_offsets() -> [usize; 31] {
    [
        offset_of!(CFlushRecord, key),
        offset_of!(CFlushRecord, node),
        offset_of!(CFlushRecord, buffer),
        offset_of!(CFlushRecord, generation),
        offset_of!(CFlushRecord, buffer_generation),
        offset_of!(CFlushRecord, scan_start),
        offset_of!(CFlushRecord, scan_end),
        offset_of!(CFlushRecord, accepted_start),
        offset_of!(CFlushRecord, accepted_end),
        offset_of!(CFlushRecord, consumed_start),
        offset_of!(CFlushRecord, consumed_end),
        offset_of!(CFlushRecord, tail_discarded_start),
        offset_of!(CFlushRecord, tail_discarded_end),
        offset_of!(CFlushRecord, remaining_start),
        offset_of!(CFlushRecord, remaining_end),
        offset_of!(CFlushRecord, fragment_start),
        offset_of!(CFlushRecord, fragment_length),
        offset_of!(CFlushRecord, flags_before),
        offset_of!(CFlushRecord, flags_after),
        offset_of!(CFlushRecord, boundary),
        offset_of!(CFlushRecord, outcome),
        offset_of!(CFlushRecord, leading_bu),
        offset_of!(CFlushRecord, content_bu),
        offset_of!(CFlushRecord, field_bu),
        offset_of!(CFlushRecord, target_bu),
        offset_of!(CFlushRecord, taboff_before),
        offset_of!(CFlushRecord, taboff_after),
        offset_of!(CFlushRecord, visual_before),
        offset_of!(CFlushRecord, visual_after),
        offset_of!(CFlushRecord, sequence),
        offset_of!(CFlushRecord, outcome_sequence),
    ]
}
fn boundary_offsets() -> [usize; 16] {
    [
        offset_of!(CBoundaryRecord, key),
        offset_of!(CBoundaryRecord, node),
        offset_of!(CBoundaryRecord, parent),
        offset_of!(CBoundaryRecord, request),
        offset_of!(CBoundaryRecord, effect),
        offset_of!(CBoundaryRecord, flags_before),
        offset_of!(CBoundaryRecord, flags_after),
        offset_of!(CBoundaryRecord, control),
        offset_of!(CBoundaryRecord, line_before),
        offset_of!(CBoundaryRecord, line_after),
        offset_of!(CBoundaryRecord, visual_before),
        offset_of!(CBoundaryRecord, visual_after),
        offset_of!(CBoundaryRecord, direct_device_lines),
        offset_of!(CBoundaryRecord, wrapper),
        offset_of!(CBoundaryRecord, enter_sequence),
        offset_of!(CBoundaryRecord, leave_sequence),
    ]
}
fn control_offsets() -> [usize; 49] {
    [
        offset_of!(CControlRecord, key),
        offset_of!(CControlRecord, node),
        offset_of!(CControlRecord, parent),
        offset_of!(CControlRecord, wrapper),
        offset_of!(CControlRecord, request),
        offset_of!(CControlRecord, buffer),
        offset_of!(CControlRecord, generation_before),
        offset_of!(CControlRecord, generation_after),
        offset_of!(CControlRecord, flags_before),
        offset_of!(CControlRecord, flags_after),
        offset_of!(CControlRecord, atom_start),
        offset_of!(CControlRecord, atom_length),
        offset_of!(CControlRecord, fragment_start),
        offset_of!(CControlRecord, fragment_length),
        offset_of!(CControlRecord, flush_start),
        offset_of!(CControlRecord, flush_length),
        offset_of!(CControlRecord, boundary_start),
        offset_of!(CControlRecord, boundary_length),
        offset_of!(CControlRecord, geometry_start),
        offset_of!(CControlRecord, geometry_length),
        offset_of!(CControlRecord, wrapper_start),
        offset_of!(CControlRecord, wrapper_length),
        offset_of!(CControlRecord, reserved),
        offset_of!(CControlRecord, line_before),
        offset_of!(CControlRecord, line_after),
        offset_of!(CControlRecord, visual_before),
        offset_of!(CControlRecord, visual_after),
        offset_of!(CControlRecord, column_before),
        offset_of!(CControlRecord, column_after),
        offset_of!(CControlRecord, extent_before),
        offset_of!(CControlRecord, extent_after),
        offset_of!(CControlRecord, offset_before),
        offset_of!(CControlRecord, offset_after),
        offset_of!(CControlRecord, rmargin_before),
        offset_of!(CControlRecord, rmargin_after),
        offset_of!(CControlRecord, maxrmargin_before),
        offset_of!(CControlRecord, maxrmargin_after),
        offset_of!(CControlRecord, taboff_before),
        offset_of!(CControlRecord, taboff_after),
        offset_of!(CControlRecord, temporary_indent_before),
        offset_of!(CControlRecord, temporary_indent_after),
        offset_of!(CControlRecord, skip_vertical_before),
        offset_of!(CControlRecord, skip_vertical_after),
        offset_of!(CControlRecord, minimum_blank_before),
        offset_of!(CControlRecord, minimum_blank_after),
        offset_of!(CControlRecord, trailing_blank_before),
        offset_of!(CControlRecord, trailing_blank_after),
        offset_of!(CControlRecord, enter_sequence),
        offset_of!(CControlRecord, leave_sequence),
    ]
}
fn geometry_offsets() -> [usize; 13] {
    [
        offset_of!(CGeometryRecord, key),
        offset_of!(CGeometryRecord, node),
        offset_of!(CGeometryRecord, related),
        offset_of!(CGeometryRecord, kind),
        offset_of!(CGeometryRecord, unit),
        offset_of!(CGeometryRecord, origin_kind),
        offset_of!(CGeometryRecord, origin_key),
        offset_of!(CGeometryRecord, reserved),
        offset_of!(CGeometryRecord, requested),
        offset_of!(CGeometryRecord, effective),
        offset_of!(CGeometryRecord, before),
        offset_of!(CGeometryRecord, after),
        offset_of!(CGeometryRecord, sequence),
    ]
}
fn wrapper_offsets() -> [usize; 16] {
    [
        offset_of!(CWrapperRecord, key),
        offset_of!(CWrapperRecord, parent),
        offset_of!(CWrapperRecord, node),
        offset_of!(CWrapperRecord, kind),
        offset_of!(CWrapperRecord, target_start),
        offset_of!(CWrapperRecord, target_length),
        offset_of!(CWrapperRecord, enter_atom),
        offset_of!(CWrapperRecord, leave_atom),
        offset_of!(CWrapperRecord, detail),
        offset_of!(CWrapperRecord, flags),
        offset_of!(CWrapperRecord, state_before),
        offset_of!(CWrapperRecord, state_after),
        offset_of!(CWrapperRecord, depth_before),
        offset_of!(CWrapperRecord, depth_after),
        offset_of!(CWrapperRecord, enter_sequence),
        offset_of!(CWrapperRecord, leave_sequence),
    ]
}
fn reference_offsets() -> [usize; 16] {
    [
        offset_of!(CReferenceRecord, key),
        offset_of!(CReferenceRecord, parent),
        offset_of!(CReferenceRecord, owner_node),
        offset_of!(CReferenceRecord, target_node),
        offset_of!(CReferenceRecord, kind),
        offset_of!(CReferenceRecord, primary_start),
        offset_of!(CReferenceRecord, primary_length),
        offset_of!(CReferenceRecord, secondary_start),
        offset_of!(CReferenceRecord, secondary_length),
        offset_of!(CReferenceRecord, enter_atom),
        offset_of!(CReferenceRecord, label_start_atom),
        offset_of!(CReferenceRecord, leave_atom),
        offset_of!(CReferenceRecord, affinity),
        offset_of!(CReferenceRecord, flags),
        offset_of!(CReferenceRecord, enter_sequence),
        offset_of!(CReferenceRecord, leave_sequence),
    ]
}
fn anchor_offsets() -> [usize; 10] {
    [
        offset_of!(CAnchorRecord, key),
        offset_of!(CAnchorRecord, node),
        offset_of!(CAnchorRecord, target_start),
        offset_of!(CAnchorRecord, target_length),
        offset_of!(CAnchorRecord, device_line),
        offset_of!(CAnchorRecord, atom_cursor),
        offset_of!(CAnchorRecord, fragment_cursor),
        offset_of!(CAnchorRecord, affinity),
        offset_of!(CAnchorRecord, reserved),
        offset_of!(CAnchorRecord, sequence),
    ]
}
fn table_offsets() -> [usize; 16] {
    [
        offset_of!(CTableRecord, key),
        offset_of!(CTableRecord, first_row_node),
        offset_of!(CTableRecord, row_start),
        offset_of!(CTableRecord, row_length),
        offset_of!(CTableRecord, cell_start),
        offset_of!(CTableRecord, cell_length),
        offset_of!(CTableRecord, logical_columns),
        offset_of!(CTableRecord, flags),
        offset_of!(CTableRecord, enter_atom),
        offset_of!(CTableRecord, leave_atom),
        offset_of!(CTableRecord, enter_fragment),
        offset_of!(CTableRecord, leave_fragment),
        offset_of!(CTableRecord, enter_flush),
        offset_of!(CTableRecord, leave_flush),
        offset_of!(CTableRecord, enter_sequence),
        offset_of!(CTableRecord, leave_sequence),
    ]
}
fn table_row_offsets() -> [usize; 16] {
    [
        offset_of!(CTableRowRecord, key),
        offset_of!(CTableRowRecord, table),
        offset_of!(CTableRowRecord, node),
        offset_of!(CTableRowRecord, ordinal),
        offset_of!(CTableRowRecord, kind),
        offset_of!(CTableRowRecord, logical_columns),
        offset_of!(CTableRowRecord, cell_start),
        offset_of!(CTableRowRecord, cell_length),
        offset_of!(CTableRowRecord, enter_atom),
        offset_of!(CTableRowRecord, leave_atom),
        offset_of!(CTableRowRecord, enter_fragment),
        offset_of!(CTableRowRecord, leave_fragment),
        offset_of!(CTableRowRecord, enter_flush),
        offset_of!(CTableRowRecord, leave_flush),
        offset_of!(CTableRowRecord, enter_sequence),
        offset_of!(CTableRowRecord, leave_sequence),
    ]
}
fn table_cell_offsets() -> [usize; 24] {
    [
        offset_of!(CTableCellRecord, key),
        offset_of!(CTableCellRecord, row),
        offset_of!(CTableCellRecord, node),
        offset_of!(CTableCellRecord, ordinal),
        offset_of!(CTableCellRecord, data_ordinal),
        offset_of!(CTableCellRecord, logical_column),
        offset_of!(CTableCellRecord, column_span),
        offset_of!(CTableCellRecord, row_span),
        offset_of!(CTableCellRecord, layout_kind),
        offset_of!(CTableCellRecord, data_kind),
        offset_of!(CTableCellRecord, alignment),
        offset_of!(CTableCellRecord, font),
        offset_of!(CTableCellRecord, flags),
        offset_of!(CTableCellRecord, buffer),
        offset_of!(CTableCellRecord, buffer_generation),
        offset_of!(CTableCellRecord, reserved),
        offset_of!(CTableCellRecord, enter_atom),
        offset_of!(CTableCellRecord, leave_atom),
        offset_of!(CTableCellRecord, offset_bu),
        offset_of!(CTableCellRecord, rmargin_bu),
        offset_of!(CTableCellRecord, coloff_before_bu),
        offset_of!(CTableCellRecord, coloff_after_bu),
        offset_of!(CTableCellRecord, enter_sequence),
        offset_of!(CTableCellRecord, leave_sequence),
    ]
}
fn diagnostic_offsets() -> [usize; 5] {
    [
        offset_of!(CDiagnosticRecord, code),
        offset_of!(CDiagnosticRecord, node),
        offset_of!(CDiagnosticRecord, message_start),
        offset_of!(CDiagnosticRecord, message_length),
        offset_of!(CDiagnosticRecord, sequence),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    type InvalidHeadingCase = (
        Option<fn(&mut CNodeRecord)>,
        Option<fn(&mut CWrapperRecord)>,
    );

    #[test]
    fn node_transfer_budget_is_checked_before_ast_key_allocation() {
        let limits = ExecutionLimits {
            max_nodes: 1,
            ..ExecutionLimits::default()
        };
        assert_eq!(
            validate_execution_node_transfer_count(2, limits).unwrap_err(),
            "native execution node transfer exceeds its declared limit"
        );

        let limits = ExecutionLimits {
            max_nodes: u64::MAX,
            ..ExecutionLimits::default()
        };
        assert_eq!(
            validate_execution_node_transfer_count(usize::MAX, limits).unwrap_err(),
            "native execution node transfer exceeds its declared limit"
        );
    }

    #[test]
    fn record_layout_validation_rejects_each_abi_dimension() {
        let offsets = [0, 4, 8];
        let valid = |field: u32| offsets[field as usize];
        validate_record_layout("test", 12, 4, 3, 12, 4, &offsets, valid).unwrap();
        for result in [
            validate_record_layout("test", 16, 4, 3, 12, 4, &offsets, valid),
            validate_record_layout("test", 12, 8, 3, 12, 4, &offsets, valid),
            validate_record_layout("test", 12, 4, 2, 12, 4, &offsets, valid),
        ] {
            assert_eq!(result.unwrap_err(), "libmandoc test record ABI mismatch");
        }
        assert_eq!(
            validate_record_layout("test", 12, 4, 3, 12, 4, &offsets, |field| {
                valid(field) + usize::from(field == 1)
            })
            .unwrap_err(),
            "libmandoc test record offset mismatch at field 1"
        );
    }

    #[test]
    fn ast_binding_rejects_reassigned_execution_origins() {
        let source = br".TH PROBE 1
.SH NAME
probe \- test
.SH DESCRIPTION
body
";
        let mut report = crate::Parser::new(crate::ParseOptions::default())
            .with_input_format(crate::InputFormat::Man)
            .execute_bytes("probe.1", source, ExecutionLimits::default())
            .unwrap();
        let root = &report.document.root;
        validate_execution_ast_bindings(root, &report.execution).unwrap();

        let original = report.execution.nodes[0].line;
        report.execution.nodes[0].line = original.saturating_add(1);
        assert_eq!(
            validate_execution_ast_bindings(root, &report.execution).unwrap_err(),
            "owned syntax node does not match its execution origin"
        );
        report.execution.nodes[0].line = original;

        let section = report
            .execution
            .nodes
            .iter()
            .position(|node| node.macro_name.as_deref() == Some("SH"))
            .expect("section node");
        report.execution.nodes[section].parent = None;
        assert_eq!(
            validate_execution_ast_bindings(root, &report.execution).unwrap_err(),
            "owned syntax node does not match its execution origin"
        );
    }

    fn root_source() -> CSourceRecord {
        CSourceRecord {
            key: 0,
            parent: NONE,
            include_node: NONE,
            flags: 0,
            path_start: 0,
            path_length: 1,
        }
    }

    fn raw_records() -> RawRecords {
        RawRecords {
            sources: vec![root_source()],
            nodes: Vec::new(),
            buffer_generations: vec![CBufferGenerationRecord {
                key: 0,
                buffer: 0,
                generation: 0,
                capacity: 2,
                extent: 2,
                close_reason: 2,
                reserved: 0,
                open_sequence: 0,
                close_sequence: 10,
            }],
            words: Vec::new(),
            atoms: Vec::new(),
            fragments: Vec::new(),
            fragment_atoms: Vec::new(),
            flushes: Vec::new(),
            boundaries: Vec::new(),
            controls: Vec::new(),
            geometry: Vec::new(),
            wrappers: Vec::new(),
            references: Vec::new(),
            anchors: Vec::new(),
            tables: Vec::new(),
            table_rows: Vec::new(),
            table_cells: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn atom() -> CAtomRecord {
        CAtomRecord {
            key: 0,
            buffer: 0,
            generation: 0,
            buffer_generation: 0,
            slot: 0,
            kind: 1,
            role: 1,
            input_scalar: u32::from('x'),
            display_scalar: u32::from('x'),
            width_bu: 24,
            node: NONE,
            source: 0,
            operand_start: NONE,
            operand_length: 0,
            font: 0,
            wrapper: NONE,
            replaced_by: NONE,
            disposition: 2,
            sequence: 1,
        }
    }

    fn consumed_atom(key: u32, slot: u32, sequence: u64) -> CAtomRecord {
        CAtomRecord {
            key,
            slot,
            disposition: 3,
            sequence,
            ..atom()
        }
    }

    fn empty_word() -> CWordRecord {
        CWordRecord {
            key: 0,
            node: NONE,
            source: 0,
            operand_start: 0,
            operand_length: 0,
            role: 5,
            wrapper: NONE,
            enter_atom: 0,
            leave_atom: 0,
            reserved: 0,
            enter_sequence: 1,
            leave_sequence: 2,
        }
    }

    fn direct_atom() -> CAtomRecord {
        CAtomRecord {
            buffer: NONE,
            generation: NONE,
            buffer_generation: NONE,
            slot: NONE,
            ..atom()
        }
    }

    fn fragment() -> CFragmentRecord {
        CFragmentRecord {
            key: 0,
            node: NONE,
            buffer: 0,
            generation: 0,
            buffer_generation: 0,
            atom_ref_start: 0,
            atom_ref_length: 1,
            device_line: 0,
            role: 1,
            wrapper: NONE,
            reserved: 0,
            start_bu: 0,
            end_bu: 24,
            sequence: 2,
        }
    }

    fn wrapper() -> CWrapperRecord {
        CWrapperRecord {
            key: 0,
            parent: NONE,
            node: NONE,
            kind: 2,
            target_start: NONE,
            target_length: 0,
            enter_atom: 0,
            leave_atom: 0,
            detail: 0,
            flags: 0,
            state_before: 0,
            state_after: 0,
            depth_before: 0,
            depth_after: 0,
            enter_sequence: 1,
            leave_sequence: 1,
        }
    }

    fn records_with_mdoc_list_item_wrapper() -> (Vec<u8>, RawRecords) {
        let pool = b"xBlIt".to_vec();
        let mut records = raw_records();
        records.nodes.extend([
            CNodeRecord {
                key: 0,
                kind: 1,
                macro_start: 1,
                macro_length: 2,
                ..node()
            },
            CNodeRecord {
                key: 1,
                parent: 0,
                kind: 3,
                ..node()
            },
            CNodeRecord {
                key: 2,
                parent: 1,
                kind: 1,
                macro_start: 3,
                macro_length: 2,
                ..node()
            },
        ]);
        let node_wrapper = |key, parent, node, enter_sequence, leave_sequence| CWrapperRecord {
            key,
            parent,
            node,
            kind: 1,
            enter_sequence,
            leave_sequence,
            ..wrapper()
        };
        records.wrappers.extend([
            node_wrapper(0, NONE, 0, 1, 8),
            node_wrapper(1, 0, 1, 2, 7),
            node_wrapper(2, 1, 2, 3, 6),
            CWrapperRecord {
                key: 3,
                parent: 2,
                node: 2,
                kind: 4,
                detail: 1,
                enter_sequence: 4,
                leave_sequence: 5,
                ..wrapper()
            },
        ]);
        (pool, records)
    }

    fn assert_heading_cardinality_rejected(
        head: CNodeRecord,
        parent: CWrapperRecord,
        heading: CWrapperRecord,
    ) {
        let mut missing = raw_records();
        missing.nodes.push(head);
        missing.wrappers.push(parent);
        assert_eq!(
            rejection_with_pool(b"xShNAME".to_vec(), missing),
            "missing execution heading wrapper"
        );

        let mut duplicate = raw_records();
        duplicate.nodes.push(head);
        let mut duplicate_parent = parent;
        duplicate_parent.leave_sequence = 6;
        let mut second_heading = heading;
        second_heading.key = 2;
        second_heading.enter_sequence = 4;
        second_heading.leave_sequence = 5;
        duplicate
            .wrappers
            .extend([duplicate_parent, heading, second_heading]);
        assert_eq!(
            rejection_with_pool(b"xShNAME".to_vec(), duplicate),
            "duplicate execution heading wrapper"
        );
    }

    fn node() -> CNodeRecord {
        CNodeRecord {
            key: 0,
            parent: NONE,
            source: 0,
            line: 1,
            column: 1,
            kind: 5,
            flags: 0,
            macro_start: NONE,
            macro_length: 0,
        }
    }

    fn control() -> CControlRecord {
        CControlRecord {
            key: 0,
            node: 0,
            parent: NONE,
            wrapper: 0,
            request: 1,
            buffer: 0,
            generation_before: 0,
            generation_after: 0,
            flags_before: 0,
            flags_after: 0,
            atom_start: 0,
            atom_length: 0,
            fragment_start: 0,
            fragment_length: 0,
            flush_start: 0,
            flush_length: 0,
            boundary_start: 0,
            boundary_length: 0,
            geometry_start: 0,
            geometry_length: 0,
            wrapper_start: 1,
            wrapper_length: 0,
            reserved: 0,
            line_before: 0,
            line_after: 0,
            visual_before: 0,
            visual_after: 0,
            column_before: 0,
            column_after: 0,
            extent_before: 0,
            extent_after: 0,
            offset_before: 0,
            offset_after: 0,
            rmargin_before: 24,
            rmargin_after: 24,
            maxrmargin_before: 24,
            maxrmargin_after: 24,
            taboff_before: 0,
            taboff_after: 0,
            temporary_indent_before: 0,
            temporary_indent_after: 0,
            skip_vertical_before: 0,
            skip_vertical_after: 0,
            minimum_blank_before: 0,
            minimum_blank_after: 0,
            trailing_blank_before: 0,
            trailing_blank_after: 0,
            enter_sequence: 2,
            leave_sequence: 3,
        }
    }

    fn boundary() -> CBoundaryRecord {
        CBoundaryRecord {
            key: 0,
            node: NONE,
            parent: NONE,
            request: 1,
            effect: 0,
            flags_before: 0,
            flags_after: 0,
            control: NONE,
            line_before: 0,
            line_after: 0,
            visual_before: 0,
            visual_after: 0,
            direct_device_lines: 0,
            wrapper: NONE,
            enter_sequence: 1,
            leave_sequence: 2,
        }
    }

    fn control_records() -> RawRecords {
        let mut records = raw_records();
        let mut owner = node();
        owner.macro_start = 1;
        owner.macro_length = 2;
        records.nodes.push(owner);
        let mut scope = wrapper();
        scope.kind = 1;
        scope.node = 0;
        scope.enter_sequence = 1;
        scope.leave_sequence = 4;
        records.wrappers.push(scope);
        records.controls.push(control());
        records
    }

    fn reference() -> CReferenceRecord {
        CReferenceRecord {
            key: 0,
            parent: NONE,
            owner_node: 0,
            target_node: 0,
            kind: 1,
            primary_start: 0,
            primary_length: 1,
            secondary_start: NONE,
            secondary_length: 0,
            enter_atom: 0,
            label_start_atom: 0,
            leave_atom: 0,
            affinity: 1,
            flags: 0,
            enter_sequence: 1,
            leave_sequence: 2,
        }
    }

    fn anchor() -> CAnchorRecord {
        CAnchorRecord {
            key: 0,
            node: 0,
            target_start: 0,
            target_length: 1,
            device_line: 1,
            atom_cursor: 0,
            fragment_cursor: 0,
            affinity: 2,
            reserved: 0,
            sequence: 1,
        }
    }

    fn flush() -> CFlushRecord {
        CFlushRecord {
            key: 0,
            node: NONE,
            buffer: 0,
            generation: 0,
            buffer_generation: 0,
            scan_start: 0,
            scan_end: 2,
            accepted_start: 0,
            accepted_end: 2,
            consumed_start: 0,
            consumed_end: 2,
            tail_discarded_start: 2,
            tail_discarded_end: 2,
            remaining_start: 2,
            remaining_end: 2,
            fragment_start: 0,
            fragment_length: 0,
            flags_before: 0,
            flags_after: 0,
            boundary: NONE,
            outcome: 2,
            leading_bu: 0,
            content_bu: 48,
            field_bu: 48,
            target_bu: 48,
            taboff_before: 0,
            taboff_after: 0,
            visual_before: 0,
            visual_after: 48,
            sequence: 1,
            outcome_sequence: 2,
        }
    }

    fn glyph_geometry() -> CGeometryRecord {
        CGeometryRecord {
            key: 0,
            node: NONE,
            related: 0,
            kind: 2,
            unit: 1,
            origin_kind: 1,
            origin_key: 0,
            reserved: 0,
            requested: 24,
            effective: 24,
            before: 0,
            after: 24,
            sequence: 4,
        }
    }

    fn records_with_emitted_fragment() -> RawRecords {
        let mut records = raw_records();
        // A completed `term_flushln()` resets its buffer generation.  Keep
        // this common emitted-fragment fixture faithful to that native
        // lifetime so tests aimed at later relationships are not rejected by
        // the field-continuation validator first.
        records.buffer_generations[0].close_reason = 1;
        records.buffer_generations[0].extent = 1;
        records.atoms.push(atom());
        let mut fragment = fragment();
        fragment.sequence = 3;
        records.fragments.push(fragment);
        records.fragment_atoms.push(CFragmentAtomRecord {
            fragment: 0,
            atom: 0,
        });
        let mut flush = flush();
        flush.scan_end = 1;
        flush.accepted_end = 1;
        flush.consumed_end = 1;
        flush.tail_discarded_start = 1;
        flush.tail_discarded_end = 1;
        flush.remaining_start = 1;
        flush.remaining_end = 1;
        flush.fragment_length = 1;
        flush.sequence = 2;
        flush.outcome_sequence = 5;
        records.flushes.push(flush);
        records.geometry.push(glyph_geometry());
        records
    }

    fn records_with_reference_atom() -> RawRecords {
        let mut records = records_with_emitted_fragment();
        records.nodes.push(node());
        records.buffer_generations[0].close_sequence = 20;
        records.atoms[0].sequence = 2;
        records.flushes[0].sequence = 4;
        records.flushes[0].outcome_sequence = 7;
        records.fragments[0].sequence = 5;
        records.geometry[0].sequence = 6;
        let mut reference = reference();
        reference.leave_atom = 1;
        reference.enter_sequence = 1;
        reference.leave_sequence = 3;
        records.references.push(reference);
        records
    }

    fn records_with_word_atom() -> RawRecords {
        let mut records = records_with_emitted_fragment();
        records.atoms[0].operand_start = 0;
        records.atoms[0].operand_length = 1;
        records.atoms[0].sequence = 2;
        records.flushes[0].sequence = 4;
        records.flushes[0].outcome_sequence = 8;
        records.fragments[0].sequence = 5;
        records.geometry[0].sequence = 6;
        records.words.push(CWordRecord {
            operand_length: 1,
            role: 1,
            enter_atom: 0,
            leave_atom: 1,
            enter_sequence: 1,
            leave_sequence: 3,
            ..empty_word()
        });
        records
    }

    fn records_with_empty_table_cell() -> RawRecords {
        let mut records = raw_records();
        records.nodes.push(CNodeRecord { kind: 8, ..node() });
        records.tables.push(CTableRecord {
            key: 0,
            first_row_node: 0,
            row_start: 0,
            row_length: 1,
            cell_start: 0,
            cell_length: 1,
            logical_columns: 1,
            flags: 0,
            enter_atom: 0,
            leave_atom: 0,
            enter_fragment: 0,
            leave_fragment: 0,
            enter_flush: 0,
            leave_flush: 0,
            enter_sequence: 1,
            leave_sequence: 6,
        });
        records.table_rows.push(CTableRowRecord {
            key: 0,
            table: 0,
            node: 0,
            ordinal: 0,
            kind: 1,
            logical_columns: 1,
            cell_start: 0,
            cell_length: 1,
            enter_atom: 0,
            leave_atom: 0,
            enter_fragment: 0,
            leave_fragment: 0,
            enter_flush: 0,
            leave_flush: 0,
            enter_sequence: 2,
            leave_sequence: 5,
        });
        records.table_cells.push(CTableCellRecord {
            key: 0,
            row: 0,
            node: 0,
            ordinal: 0,
            data_ordinal: 0,
            logical_column: 0,
            column_span: 1,
            row_span: 1,
            layout_kind: 3,
            data_kind: 2,
            alignment: 1,
            font: 0,
            flags: 0,
            buffer: NONE,
            buffer_generation: NONE,
            reserved: 0,
            enter_atom: 0,
            leave_atom: 0,
            offset_bu: 0,
            rmargin_bu: 24,
            coloff_before_bu: 0,
            coloff_after_bu: 24,
            enter_sequence: 3,
            leave_sequence: 4,
        });
        records
    }

    fn record_count(records: &RawRecords) -> u64 {
        [
            records.sources.len(),
            records.nodes.len(),
            records.buffer_generations.len(),
            records.words.len(),
            records.atoms.len(),
            records.fragments.len(),
            records.fragment_atoms.len(),
            records.flushes.len(),
            records.boundaries.len(),
            records.controls.len(),
            records.geometry.len(),
            records.wrappers.len(),
            records.references.len(),
            records.anchors.len(),
            records.tables.len(),
            records.table_rows.len(),
            records.table_cells.len(),
            records.diagnostics.len(),
        ]
        .into_iter()
        .map(|count| u64::try_from(count).unwrap())
        .sum()
    }

    fn rejection(records: RawRecords) -> String {
        rejection_with_pool(b"x".to_vec(), records)
    }

    fn rejection_with_pool(pool: Vec<u8>, records: RawRecords) -> String {
        let count = record_count(&records);
        let buffer_cells = records
            .buffer_generations
            .iter()
            .map(|generation| u64::from(generation.capacity))
            .max()
            .unwrap_or(0);
        convert_report(pool, 0, count, buffer_cells, records).unwrap_err()
    }

    #[test]
    fn convert_report_preserves_and_validates_zero_output_words() {
        let mut records = raw_records();
        records.words.push(empty_word());
        let count = record_count(&records);
        let report = convert_report(b"x".to_vec(), 0, count, 2, records).unwrap();
        assert_eq!(report.words.len(), 1);
        assert!(report.words[0].atoms.is_empty());
        assert_eq!(
            report.pool_bytes(report.words[0].operand),
            Some(b"".as_slice())
        );

        let mut records = raw_records();
        let mut invalid = empty_word();
        invalid.role = 2;
        records.words.push(invalid);
        assert_eq!(rejection(records), "unknown execution word role");

        let mut records = raw_records();
        let mut first = empty_word();
        first.enter_sequence = 1;
        first.leave_sequence = 4;
        let mut nested = empty_word();
        nested.key = 1;
        nested.enter_sequence = 2;
        nested.leave_sequence = 3;
        records.words.extend([first, nested]);
        assert_eq!(
            rejection(records),
            "execution formatter words are not serial"
        );

        let mut records = raw_records();
        let mut later = empty_word();
        later.enter_sequence = 4;
        later.leave_sequence = 5;
        let mut earlier = empty_word();
        earlier.key = 1;
        earlier.enter_sequence = 1;
        earlier.leave_sequence = 2;
        records.words.extend([later, earlier]);
        assert_eq!(
            rejection(records),
            "execution formatter words are not serial"
        );

        let mut records = raw_records();
        let mut invalid = empty_word();
        invalid.leave_atom = 1;
        records.words.push(invalid);
        assert_eq!(
            rejection(records),
            "execution word atom interval is inconsistent"
        );

        let records = records_with_word_atom();
        let count = record_count(&records);
        let report = convert_report(b"x".to_vec(), 0, count, 2, records).unwrap();
        assert_eq!(report.words[0].atoms, 0..1);

        let mut records = records_with_word_atom();
        records.atoms[0].operand_length = 0;
        assert_eq!(
            rejection(records),
            "execution atom does not match its formatter word"
        );

        let mut records = records_with_word_atom();
        records.nodes.push(CNodeRecord { flags: 1, ..node() });
        records.words[0].node = 0;
        records.atoms[0].node = 0;
        assert_eq!(
            rejection(records),
            "generated execution word is marked as authored"
        );
    }

    #[test]
    fn convert_report_rejects_unknown_discriminants() {
        let mut records = raw_records();
        let mut value = atom();
        value.kind = u32::MAX;
        records.atoms.push(value);

        assert_eq!(rejection(records), "unknown execution atom kind");
    }

    #[test]
    fn convert_report_rejects_unowned_table_payload_atoms() {
        let mut records = records_with_emitted_fragment();
        records.atoms[0].role = 6;

        assert_eq!(
            rejection(records),
            "table-cell payload atom has no cell owner"
        );
    }

    #[test]
    fn convert_report_rejects_invalid_foreign_keys() {
        let mut records = raw_records();
        let mut value = atom();
        value.source = 1;
        records.atoms.push(value);

        assert_eq!(rejection(records), "invalid execution atom source");
    }

    #[test]
    fn convert_report_rejects_invalid_record_ranges() {
        let mut records = raw_records();
        records.atoms.push(atom());
        let mut value = fragment();
        value.atom_ref_start = 1;
        records.fragments.push(value);
        records.fragment_atoms.push(CFragmentAtomRecord {
            fragment: 0,
            atom: 0,
        });

        assert_eq!(rejection(records), "invalid fragment atom range");
    }

    #[test]
    fn convert_report_accepts_an_empty_table_cell_without_a_buffer() {
        let records = records_with_empty_table_cell();
        let count = record_count(&records);
        let report = convert_report(b"x".to_vec(), 0, count, 2, records).unwrap();

        assert_eq!(report.tables.len(), 1);
        assert_eq!(report.table_rows.len(), 1);
        assert_eq!(report.table_cells.len(), 1);
        assert_eq!(report.table_cells[0].buffer, None);
        assert_eq!(report.table_cells[0].buffer_generation, None);
        assert_eq!(report.table_cells[0].data_ordinal, 0);
    }

    #[test]
    fn convert_report_rejects_partial_table_cell_buffer_identity() {
        for mutate in [
            |cell: &mut CTableCellRecord| cell.buffer = 0,
            |cell: &mut CTableCellRecord| cell.buffer_generation = 0,
        ] {
            let mut records = records_with_empty_table_cell();
            mutate(&mut records.table_cells[0]);
            assert_eq!(
                rejection(records),
                "partial execution table-cell buffer identity"
            );
        }
    }

    #[test]
    fn convert_report_rejects_malformed_table_relationships() {
        let mut records = records_with_empty_table_cell();
        records.table_cells[0].data_ordinal = 1;
        assert_eq!(
            rejection(records),
            "non-contiguous execution table data-cell ordinal"
        );

        let mut records = records_with_empty_table_cell();
        records.table_cells[0].column_span = 2;
        assert_eq!(
            rejection(records),
            "invalid execution table-cell relationship"
        );

        let mut records = records_with_empty_table_cell();
        records.table_rows[0].table = 1;
        assert_eq!(rejection(records), "invalid execution table-row parent");
    }

    #[test]
    fn convert_report_rejects_stale_fragment_generations() {
        let mut records = raw_records();
        records.atoms.push(atom());
        let mut value = fragment();
        value.generation += 1;
        records.fragments.push(value);
        records.fragment_atoms.push(CFragmentAtomRecord {
            fragment: 0,
            atom: 0,
        });

        assert_eq!(
            rejection(records),
            "fragment atom belongs to another execution origin"
        );
    }

    #[test]
    fn convert_report_rejects_fragment_node_reassignment() {
        let mut records = records_with_emitted_fragment();
        records.nodes.push(node());
        records.atoms[0].node = 0;

        assert_eq!(
            rejection(records),
            "fragment atom belongs to another execution origin"
        );
    }

    #[test]
    fn convert_report_rejects_unbalanced_wrappers() {
        let mut records = raw_records();
        let mut value = wrapper();
        value.depth_after = 2;
        records.wrappers.push(value);
        assert_eq!(rejection(records), "invalid execution font transition");

        let mut records = raw_records();
        let mut value = wrapper();
        value.leave_sequence = 0;
        records.wrappers.push(value);
        assert_eq!(rejection(records), "invalid execution font transition");
    }

    #[test]
    fn convert_report_rejects_invalid_wrapper_variants() {
        for mutate in [
            |value: &mut CWrapperRecord| value.detail = 1,
            |value: &mut CWrapperRecord| value.flags = 1,
            |value: &mut CWrapperRecord| value.state_before = 1,
            |value: &mut CWrapperRecord| value.depth_after = 1,
            |value: &mut CWrapperRecord| value.target_start = 0,
        ] {
            let mut records = raw_records();
            let mut value = wrapper();
            value.kind = 1;
            value.node = 0;
            value.leave_sequence = 2;
            records.nodes.push(node());
            mutate(&mut value);
            records.wrappers.push(value);
            assert!(matches!(
                rejection(records).as_str(),
                "invalid execution wrapper atom range"
                    | "invalid execution node wrapper"
                    | "unexpected execution wrapper heading kind"
            ));
        }

        for mutate in [
            |value: &mut CWrapperRecord| value.leave_atom = 1,
            |value: &mut CWrapperRecord| value.leave_sequence = 3,
            |value: &mut CWrapperRecord| value.depth_before = u32::MAX,
            |value: &mut CWrapperRecord| value.state_after = 4,
        ] {
            let mut records = raw_records();
            let mut value = wrapper();
            mutate(&mut value);
            records.wrappers.push(value);
            assert!(matches!(
                rejection(records).as_str(),
                "invalid execution wrapper atom range" | "invalid execution font transition"
            ));
        }
    }

    #[test]
    fn convert_report_rejects_invalid_mdoc_list_item_wrappers() {
        let (pool, records) = records_with_mdoc_list_item_wrapper();
        let count = record_count(&records);
        let report = convert_report(pool, 0, count, 2, records).unwrap();
        assert_eq!(
            report.wrappers[3].mdoc_list_kind,
            Some(ExecutionMdocListKind::Bullet)
        );

        let (pool, mut records) = records_with_mdoc_list_item_wrapper();
        records.wrappers[3].detail = 12;
        assert_eq!(
            rejection_with_pool(pool, records),
            "invalid execution mdoc list kind"
        );

        let (pool, mut records) = records_with_mdoc_list_item_wrapper();
        records.wrappers.pop();
        assert_eq!(
            rejection_with_pool(pool, records),
            "missing execution mdoc list item wrapper"
        );

        let (pool, mut records) = records_with_mdoc_list_item_wrapper();
        records.buffer_generations[0].close_sequence = 20;
        records.wrappers[2].leave_sequence = 10;
        records.wrappers[1].leave_sequence = 11;
        records.wrappers[0].leave_sequence = 12;
        records.wrappers[3].leave_sequence = 5;
        records.wrappers.push(CWrapperRecord {
            key: 4,
            parent: 2,
            node: 2,
            kind: 4,
            detail: 1,
            enter_sequence: 6,
            leave_sequence: 7,
            ..wrapper()
        });
        assert_eq!(
            rejection_with_pool(pool, records),
            "duplicate execution mdoc list item wrapper"
        );
    }

    #[test]
    fn convert_report_requires_typed_heading_scopes_inside_head_nodes() {
        let mut records = raw_records();
        let mut head = node();
        head.kind = 2;
        head.macro_start = 1;
        head.macro_length = 2;
        records.nodes.push(head);
        let mut parent = wrapper();
        parent.kind = 1;
        parent.node = 0;
        parent.enter_sequence = 1;
        parent.leave_sequence = 4;
        let heading = CWrapperRecord {
            key: 1,
            parent: 0,
            node: 0,
            kind: 3,
            target_start: 3,
            target_length: 4,
            enter_atom: 0,
            leave_atom: 0,
            detail: 3,
            flags: 0,
            state_before: 0,
            state_after: 0,
            depth_before: 0,
            depth_after: 0,
            enter_sequence: 2,
            leave_sequence: 3,
        };
        records.wrappers.extend([parent, heading]);
        let count = record_count(&records);
        let report = convert_report(b"xShNAME".to_vec(), 0, count, 2, records).unwrap();
        assert_eq!(report.wrappers[1].kind, ExecutionWrapperKind::Heading);
        assert_eq!(
            report.wrappers[1].heading_kind,
            Some(ExecutionHeadingKind::MdocSection)
        );
        assert_eq!(
            report.pool_bytes(report.wrappers[1].target.unwrap()),
            Some(b"NAME".as_slice())
        );

        let invalid_cases: [InvalidHeadingCase; 7] = [
            (None, Some(|value: &mut CWrapperRecord| value.detail = 0)),
            (None, Some(|value: &mut CWrapperRecord| value.parent = NONE)),
            (
                None,
                Some(|value: &mut CWrapperRecord| value.state_before = 1),
            ),
            (
                None,
                Some(|value: &mut CWrapperRecord| value.leave_sequence = 2),
            ),
            (Some(|value: &mut CNodeRecord| value.kind = 5), None),
            (
                Some(|value: &mut CNodeRecord| value.macro_start = NONE),
                None,
            ),
            (Some(|value: &mut CNodeRecord| value.macro_start = 3), None),
        ];
        for (mutate_node, mutate_wrapper) in invalid_cases {
            let mut records = raw_records();
            let mut invalid_head = head;
            if let Some(mutate) = mutate_node {
                mutate(&mut invalid_head);
            }
            records.nodes.push(invalid_head);
            records.wrappers.push(parent);
            let mut invalid = heading;
            if let Some(mutate) = mutate_wrapper {
                mutate(&mut invalid);
            }
            records.wrappers.push(invalid);
            let error = rejection_with_pool(b"xShNAME".to_vec(), records);
            assert!(
                matches!(
                    error.as_str(),
                    "invalid execution heading kind"
                        | "invalid execution heading wrapper"
                        | "invalid absent execution pool range in node macro"
                        | "execution wrapper parent is not its active node owner"
                        | "duplicate native execution event sequence"
                ),
                "{error}"
            );
        }

        assert_heading_cardinality_rejected(head, parent, heading);
    }

    #[test]
    fn convert_report_rejects_overlapping_or_misordered_node_wrappers() {
        let mut records = raw_records();
        records.nodes.push(node());
        let mut outer = wrapper();
        outer.kind = 1;
        outer.node = 0;
        outer.enter_sequence = 1;
        outer.leave_sequence = 9;
        let mut overlapping = outer;
        overlapping.key = 1;
        overlapping.enter_sequence = 2;
        overlapping.leave_sequence = 8;
        records.wrappers.extend([outer, overlapping]);
        assert_eq!(rejection(records), "overlapping execution wrapper siblings");

        let mut records = raw_records();
        records.nodes.push(node());
        let mut later = wrapper();
        later.kind = 1;
        later.node = 0;
        later.enter_sequence = 2;
        later.leave_sequence = 3;
        let mut earlier = later;
        earlier.key = 1;
        earlier.enter_sequence = 1;
        earlier.leave_sequence = 4;
        records.wrappers.extend([later, earlier]);
        assert_eq!(
            rejection(records),
            "execution wrappers are not in entry order"
        );
    }

    #[test]
    fn convert_report_rejects_atoms_from_another_node_wrapper_subtree() {
        let mut records = records_with_emitted_fragment();
        records.nodes.push(node());
        records.nodes.push(CNodeRecord { key: 1, ..node() });
        records.buffer_generations[0].close_sequence = 20;
        records.atoms[0].node = 1;
        records.atoms[0].wrapper = 0;
        records.atoms[0].sequence = 2;
        records.flushes[0].node = 1;
        records.flushes[0].sequence = 3;
        records.flushes[0].outcome_sequence = 6;
        records.fragments[0].node = 1;
        records.fragments[0].sequence = 4;
        records.geometry[0].node = 1;
        records.geometry[0].sequence = 5;
        let mut value = wrapper();
        value.kind = 1;
        value.node = 0;
        value.leave_atom = 1;
        value.enter_sequence = 1;
        value.leave_sequence = 10;
        records.wrappers.push(value);
        assert_eq!(
            rejection(records),
            "execution atom is outside its node wrapper"
        );
    }

    #[test]
    fn convert_report_allows_buffered_fragments_to_flush_in_another_node_wrapper() {
        let mut records = records_with_emitted_fragment();
        records.nodes.push(node());
        records.nodes.push(CNodeRecord { key: 1, ..node() });
        records.buffer_generations[0].close_sequence = 20;
        records.atoms[0].node = 1;
        records.atoms[0].sequence = 2;
        records.flushes[0].node = 1;
        records.flushes[0].sequence = 3;
        records.flushes[0].outcome_sequence = 6;
        records.fragments[0].node = 1;
        records.fragments[0].wrapper = 0;
        records.fragments[0].sequence = 4;
        records.geometry[0].node = 1;
        records.geometry[0].sequence = 5;
        let mut value = wrapper();
        value.kind = 1;
        value.node = 0;
        value.leave_atom = 1;
        value.enter_sequence = 1;
        value.leave_sequence = 10;
        records.wrappers.push(value);

        let count = record_count(&records);
        let report = convert_report(b"x".to_vec(), 2, count, 2, records).unwrap();
        assert_eq!(report.fragments[0].node, Some(ExecutionNodeKey(1)));
        assert_eq!(report.fragments[0].wrapper, Some(0));
    }

    #[test]
    fn convert_report_rejects_invalid_semantic_references() {
        let mut records = raw_records();
        records.nodes.push(node());
        let mut value = reference();
        value.kind = u32::MAX;
        records.references.push(value);
        assert_eq!(
            rejection(records),
            "unknown execution semantic reference kind"
        );

        let mut records = raw_records();
        records.nodes.push(node());
        let mut value = reference();
        value.leave_atom = 1;
        records.references.push(value);
        assert_eq!(rejection(records), "invalid execution semantic reference");

        let mut records = raw_records();
        records.nodes.push(node());
        let mut value = reference();
        value.owner_node = 1;
        records.references.push(value);
        assert_eq!(
            rejection(records),
            "invalid execution node key in reference owner"
        );

        let mut records = raw_records();
        records.nodes.push(node());
        let mut unrelated = node();
        unrelated.key = 1;
        records.nodes.push(unrelated);
        let mut value = reference();
        value.target_node = 1;
        records.references.push(value);
        assert_eq!(
            rejection(records),
            "invalid execution semantic reference relationship"
        );

        let mut records = records_with_reference_atom();
        records.references[0].enter_atom = 1;
        records.references[0].label_start_atom = 1;
        records.references[0].leave_atom = 1;
        assert_eq!(
            rejection(records),
            "invalid execution semantic reference relationship"
        );

        let mut records = records_with_reference_atom();
        records.references[0].label_start_atom = 1;
        assert_eq!(
            rejection(records),
            "invalid execution semantic reference relationship"
        );

        let mut records = raw_records();
        records.nodes.push(node());
        let mut outer = reference();
        outer.enter_sequence = 1;
        outer.leave_sequence = 6;
        let mut overlapping = reference();
        overlapping.key = 1;
        overlapping.enter_sequence = 2;
        overlapping.leave_sequence = 5;
        records.references.extend([outer, overlapping]);
        assert_eq!(
            rejection(records),
            "overlapping execution reference siblings"
        );

        let mut records = raw_records();
        records.nodes.push(node());
        let mut value = reference();
        value.secondary_start = 0;
        value.secondary_length = 1;
        records.references.push(value);
        assert_eq!(
            rejection(records),
            "invalid execution semantic reference components"
        );

        let mut records = records_with_emitted_fragment();
        records.nodes.push(node());
        let mut value = reference();
        value.leave_atom = 1;
        value.enter_sequence = 6;
        value.leave_sequence = 7;
        records.references.push(value);
        assert_eq!(
            rejection(records),
            "invalid execution semantic reference relationship"
        );
    }

    #[test]
    fn convert_report_rejects_invalid_anchor_attachments() {
        for value in [
            CAnchorRecord {
                device_line: 0,
                ..anchor()
            },
            CAnchorRecord {
                affinity: 1,
                ..anchor()
            },
        ] {
            let mut records = raw_records();
            records.nodes.push(node());
            records.anchors.push(value);
            assert_eq!(rejection(records), "invalid execution anchor attachment");
        }

        let mut records = records_with_emitted_fragment();
        records.nodes.push(node());
        records.anchors.push(CAnchorRecord {
            sequence: 6,
            ..anchor()
        });
        assert_eq!(rejection(records), "invalid execution anchor attachment");
    }

    #[test]
    fn convert_report_rejects_record_count_mismatches() {
        let records = raw_records();
        let actual = record_count(&records);

        assert_eq!(
            convert_report(b"x".to_vec(), 0, actual + 1, 0, records).unwrap_err(),
            "native execution record accounting mismatch"
        );
    }

    #[test]
    fn convert_report_validates_control_origin_wrapper_and_shape() {
        let records = control_records();
        let count = record_count(&records);
        let report = convert_report(b"xbr".to_vec(), 0, count, 2, records).unwrap();
        assert_eq!(report.controls.len(), 1);
        assert_eq!(report.controls[0].request, ExecutionControlRequest::Break);
        assert_eq!(report.controls[0].wrapper, 0);

        let mut records = control_records();
        records.controls[0].request = u32::MAX;
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "unknown execution control request"
        );

        let records = control_records();
        assert_eq!(
            rejection_with_pool(b"xmc".to_vec(), records),
            "execution control request does not match its node macro"
        );

        let mut records = control_records();
        records.controls[0].wrapper = 1;
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "invalid execution control wrapper"
        );

        let mut records = control_records();
        records.wrappers[0].kind = 2;
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "execution control does not match its node wrapper"
        );

        let mut records = control_records();
        records.controls[0].parent = 0;
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "execution control parent is not an earlier control"
        );

        let mut records = control_records();
        records.controls[0].reserved = 1;
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "invalid execution control flags or reserved field"
        );

        let mut records = control_records();
        records.buffer_generations[0].close_sequence = 2;
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "execution control buffer generation is inconsistent"
        );

        let mut records = control_records();
        records.controls[0].generation_after = NONE;
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "execution control buffer generation is inconsistent"
        );

        let mut records = control_records();
        records.buffer_generations[0].close_sequence = 10;
        records.wrappers[0].leave_sequence = 8;
        records.controls[0].enter_sequence = 3;
        records.controls[0].leave_sequence = 6;
        records.controls[0].wrapper_start = 2;
        records.wrappers.push(CWrapperRecord {
            key: 1,
            parent: 0,
            node: 0,
            kind: 1,
            enter_sequence: 2,
            leave_sequence: 7,
            ..wrapper()
        });
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "execution control does not match its active wrapper"
        );
    }

    #[test]
    fn convert_report_rejects_impossible_boundary_effects_and_ownership() {
        let mut records = raw_records();
        records.boundaries.push(boundary());
        let count = record_count(&records);
        assert!(convert_report(b"x".to_vec(), 0, count, 2, records).is_ok());

        let mut records = raw_records();
        records.boundaries.push(CBoundaryRecord {
            effect: 3,
            ..boundary()
        });
        assert_eq!(
            rejection(records),
            "execution boundary effect is inconsistent"
        );

        let mut records = raw_records();
        records.boundaries.push(CBoundaryRecord {
            line_before: 10,
            line_after: 0,
            ..boundary()
        });
        assert_eq!(
            rejection(records),
            "invalid execution boundary interval or effect"
        );

        let mut records = raw_records();
        records.boundaries.push(CBoundaryRecord {
            direct_device_lines: 1,
            ..boundary()
        });
        assert_eq!(
            rejection(records),
            "execution boundary direct line count is inconsistent"
        );

        let mut records = raw_records();
        records.nodes.push(node());
        records.boundaries.push(CBoundaryRecord {
            node: 0,
            ..boundary()
        });
        assert_eq!(
            rejection(records),
            "execution boundary node and wrapper presence differ"
        );

        let mut records = control_records();
        records.wrappers[0].leave_sequence = 6;
        records.controls[0].leave_sequence = 5;
        records.controls[0].boundary_length = 1;
        records.boundaries.push(CBoundaryRecord {
            node: 0,
            wrapper: 0,
            enter_sequence: 3,
            leave_sequence: 4,
            ..boundary()
        });
        assert_eq!(
            rejection_with_pool(b"xbr".to_vec(), records),
            "execution boundary does not match its active control"
        );

        let mut records = raw_records();
        records.boundaries.extend([
            CBoundaryRecord {
                key: 0,
                request: 1,
                effect: 0,
                line_before: 0,
                line_after: 0,
                enter_sequence: 1,
                leave_sequence: 8,
                ..boundary()
            },
            CBoundaryRecord {
                key: 1,
                parent: 0,
                request: 2,
                effect: 3,
                line_before: 0,
                line_after: 1,
                direct_device_lines: 1,
                enter_sequence: 2,
                leave_sequence: 7,
                ..boundary()
            },
            CBoundaryRecord {
                key: 2,
                parent: 1,
                request: 4,
                effect: 2,
                line_before: 0,
                line_after: 1,
                enter_sequence: 3,
                leave_sequence: 6,
                ..boundary()
            },
        ]);
        assert_eq!(
            rejection(records),
            "execution boundary line delta is inconsistent"
        );
    }

    #[test]
    fn convert_report_rejects_invalid_buffer_generations() {
        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 0;
        assert_eq!(rejection(records), "unknown execution buffer close reason");

        let mut records = raw_records();
        records.buffer_generations[0].extent = 3;
        assert_eq!(
            rejection(records),
            "invalid execution buffer-generation extent"
        );

        let mut records = raw_records();
        records.atoms.push(atom());
        records.atoms[0].buffer_generation = 1;
        assert_eq!(
            rejection(records),
            "invalid execution atom buffer-generation"
        );

        let mut records = raw_records();
        records.buffer_generations.push(CBufferGenerationRecord {
            key: 1,
            buffer: 0,
            generation: 1,
            capacity: 2,
            extent: 0,
            close_reason: 2,
            reserved: 0,
            open_sequence: 11,
            close_sequence: 12,
        });
        assert_eq!(
            rejection(records),
            "non-contiguous execution buffer-generation lifetime"
        );

        let mut records = raw_records();
        records.buffer_generations[0].buffer = 1;
        let count = record_count(&records);
        let report = convert_report(b"x".to_vec(), 0, count, 2, records).unwrap();
        assert_eq!(report.buffer_generations[0].buffer, 1);
        let mut records = raw_records();
        records.buffer_generations[0].buffer = 1;
        assert_eq!(
            convert_report(b"x".to_vec(), 0, count, 3, records).unwrap_err(),
            "native execution buffer capacity accounting mismatch"
        );

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records.buffer_generations.push(CBufferGenerationRecord {
            key: 1,
            buffer: 0,
            generation: 1,
            capacity: 2,
            extent: 0,
            close_reason: 2,
            reserved: 0,
            open_sequence: 9,
            close_sequence: 12,
        });
        assert_eq!(
            rejection(records),
            "non-contiguous execution buffer-generation lifetime"
        );
    }

    #[test]
    fn convert_report_does_not_allocate_from_sparse_buffer_keys() {
        let mut records = raw_records();
        records.buffer_generations[0].buffer = u32::MAX - 1;
        let count = record_count(&records);
        let report = convert_report(b"x".to_vec(), 0, count, 2, records).unwrap();

        assert_eq!(report.buffer_generations[0].buffer, u32::MAX - 1);
    }

    #[test]
    fn convert_report_rejects_invalid_flush_facts() {
        let mut records = raw_records();
        let mut value = flush();
        value.outcome = u32::MAX;
        records.flushes.push(value);
        assert_eq!(rejection(records), "unknown execution flush outcome");

        let mut records = raw_records();
        let mut value = flush();
        value.remaining_start = 1;
        records.flushes.push(value);
        assert_eq!(rejection(records), "inconsistent execution flush ranges");

        let mut records = raw_records();
        let mut value = flush();
        value.outcome_sequence = value.sequence;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "execution flush is outside its buffer generation"
        );

        let mut records = records_with_emitted_fragment();
        records.flushes[0].outcome = 1;
        assert_eq!(
            rejection(records),
            "no-content flush emitted or consumed a field"
        );

        let mut records = raw_records();
        let mut value = flush();
        value.accepted_end = 0;
        value.consumed_end = 0;
        value.tail_discarded_start = 0;
        value.tail_discarded_end = 0;
        value.remaining_start = 0;
        records.flushes.push(value);
        assert_eq!(rejection(records), "content flush accepted an empty field");

        let mut records = records_with_emitted_fragment();
        records.buffer_generations[0].extent = 2;
        records.atoms[0].slot = 1;
        records.flushes[0].accepted_end = 1;
        records.flushes[0].consumed_end = 1;
        records.flushes[0].tail_discarded_start = 1;
        records.flushes[0].tail_discarded_end = 2;
        records.flushes[0].remaining_start = 2;
        records.flushes[0].remaining_end = 2;
        assert_eq!(
            rejection(records),
            "flush contains a fragment outside its accepted field"
        );
    }

    #[test]
    fn convert_report_rejects_invalid_field_ranges_and_geometry() {
        let mut records = raw_records();
        let mut value = flush();
        value.field_bu = -1;
        records.flushes.push(value);
        assert_eq!(rejection(records), "negative execution flush geometry");

        let mut records = raw_records();
        let mut value = flush();
        value.scan_end = 1;
        records.flushes.push(value);
        assert_eq!(rejection(records), "inconsistent execution flush ranges");

        let mut records = raw_records();
        let mut value = flush();
        value.tail_discarded_start = 1;
        records.flushes.push(value);
        assert_eq!(rejection(records), "inconsistent execution flush ranges");

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records.atoms.push(consumed_atom(0, 0, 1));
        let mut value = flush();
        value.accepted_end = 1;
        value.consumed_end = 1;
        value.tail_discarded_start = 1;
        value.tail_discarded_end = 1;
        value.remaining_start = 1;
        value.sequence = 2;
        value.outcome_sequence = 3;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "terminal execution flush retains a field"
        );
    }

    #[test]
    fn convert_report_rejects_field_atoms_created_after_the_flush() {
        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records
            .atoms
            .extend([consumed_atom(0, 0, 3), consumed_atom(1, 1, 4)]);
        records.flushes.push(flush());

        assert_eq!(
            rejection(records),
            "execution field atom was created after its flush began"
        );

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 4)]);
        let mut first = flush();
        first.scan_end = 1;
        first.accepted_end = 1;
        first.consumed_end = 1;
        first.tail_discarded_start = 1;
        first.tail_discarded_end = 1;
        first.remaining_start = 1;
        first.outcome = 4;
        first.sequence = 2;
        first.outcome_sequence = 3;
        records.flushes.push(first);
        let mut second = flush();
        second.key = 1;
        second.scan_start = 1;
        second.accepted_start = 1;
        second.consumed_start = 1;
        second.sequence = 5;
        second.outcome_sequence = 6;
        records.flushes.push(second);
        assert_eq!(
            rejection(records),
            "execution field atom was created after its flush began"
        );
    }

    #[test]
    fn convert_report_rejects_replacements_across_a_cleared_slot_epoch() {
        let mut records = raw_records();
        records.buffer_generations[0].capacity = 1;
        records.buffer_generations[0].extent = 1;
        records.buffer_generations[0].close_reason = 1;
        let mut replaced = atom();
        replaced.disposition = 4;
        replaced.replaced_by = 2;
        records.atoms.push(replaced);
        records.atoms.push(consumed_atom(1, 0, 2));
        records.atoms.push(consumed_atom(2, 0, 3));
        let mut value = flush();
        value.scan_end = 1;
        value.accepted_end = 1;
        value.consumed_end = 1;
        value.tail_discarded_start = 1;
        value.tail_discarded_end = 1;
        value.remaining_start = 1;
        value.remaining_end = 1;
        value.sequence = 4;
        value.outcome_sequence = 5;
        records.flushes.push(value);

        assert_eq!(
            rejection(records),
            "execution atom slot history is inconsistent"
        );
    }

    #[test]
    fn convert_report_rejects_incomplete_field_continuations() {
        let mut records = raw_records();
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 2)]);
        let mut value = flush();
        value.accepted_end = 1;
        value.consumed_end = 1;
        value.tail_discarded_start = 1;
        value.tail_discarded_end = 1;
        value.remaining_start = 1;
        value.outcome = 3;
        value.sequence = 3;
        value.outcome_sequence = 4;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "wrapped execution flush has no successor"
        );

        let mut records = raw_records();
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 2)]);
        let mut value = flush();
        value.accepted_end = 1;
        value.consumed_end = 1;
        value.tail_discarded_start = 1;
        value.tail_discarded_end = 1;
        value.remaining_start = 1;
        value.outcome = 4;
        value.sequence = 3;
        value.outcome_sequence = 4;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "execution flush continuation is incomplete"
        );

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 2)]);
        let mut first = flush();
        first.sequence = 3;
        first.outcome_sequence = 4;
        records.flushes.push(first);
        let mut second = flush();
        second.key = 1;
        second.sequence = 5;
        second.outcome_sequence = 6;
        records.flushes.push(second);
        assert_eq!(
            rejection(records),
            "execution flush follows a terminal field"
        );
    }

    #[test]
    fn convert_report_rejects_inconsistent_field_chain_order() {
        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        let mut value = flush();
        value.scan_start = 1;
        value.accepted_start = 1;
        value.consumed_start = 1;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "execution buffer generation does not start at slot zero"
        );

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 2)]);
        let mut first = flush();
        first.accepted_end = 1;
        first.consumed_end = 1;
        first.tail_discarded_start = 1;
        first.tail_discarded_end = 1;
        first.remaining_start = 1;
        first.outcome = 4;
        first.sequence = 5;
        first.outcome_sequence = 6;
        records.flushes.push(first);
        let mut second = flush();
        second.key = 1;
        second.scan_start = 1;
        second.accepted_start = 1;
        second.consumed_start = 1;
        second.sequence = 3;
        second.outcome_sequence = 4;
        records.flushes.push(second);
        assert_eq!(rejection(records), "execution flush order is inconsistent");

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 2)]);
        let mut first = flush();
        first.accepted_end = 1;
        first.consumed_end = 1;
        first.tail_discarded_start = 1;
        first.tail_discarded_end = 1;
        first.remaining_start = 1;
        first.outcome = 4;
        first.sequence = 3;
        first.outcome_sequence = 4;
        records.flushes.push(first);
        let mut second = flush();
        second.key = 1;
        second.scan_start = 1;
        second.accepted_start = 1;
        second.consumed_start = 1;
        second.sequence = 5;
        second.outcome_sequence = 6;
        second.taboff_before = 24;
        records.flushes.push(second);
        assert_eq!(
            rejection(records),
            "execution flush does not resume its remaining field"
        );
    }

    #[test]
    fn convert_report_rejects_invalid_field_tail_atom_fates() {
        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records.flushes.push(flush());
        assert_eq!(
            rejection(records),
            "execution field lacks complete atom provenance"
        );

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        let mut accepted = atom();
        accepted.disposition = 3;
        records.atoms.push(accepted);
        let mut tail = atom();
        tail.key = 1;
        tail.slot = 1;
        tail.input_scalar = u32::from(b' ');
        tail.display_scalar = u32::from(b' ');
        tail.disposition = 3;
        tail.sequence = 2;
        records.atoms.push(tail);
        let mut value = flush();
        value.accepted_end = 1;
        value.consumed_end = 1;
        value.tail_discarded_start = 1;
        value.tail_discarded_end = 2;
        value.sequence = 3;
        value.outcome_sequence = 4;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "terminal execution field tail was not discarded"
        );

        let mut records = raw_records();
        records.buffer_generations[0].capacity = 3;
        records.buffer_generations[0].extent = 3;
        let mut accepted = atom();
        accepted.disposition = 3;
        records.atoms.push(accepted);
        let mut tail = atom();
        tail.key = 1;
        tail.slot = 1;
        tail.disposition = 3;
        tail.sequence = 2;
        records.atoms.push(tail);
        let mut value = flush();
        value.scan_end = 2;
        value.accepted_end = 1;
        value.consumed_end = 1;
        value.tail_discarded_start = 1;
        value.tail_discarded_end = 2;
        value.remaining_start = 2;
        value.remaining_end = 3;
        value.outcome = 3;
        value.sequence = 3;
        value.outcome_sequence = 4;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "continuing execution field tail was not consumed"
        );

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        let mut accepted = atom();
        accepted.disposition = 3;
        records.atoms.push(accepted);
        let mut value = flush();
        value.accepted_end = 1;
        value.consumed_end = 1;
        value.tail_discarded_start = 1;
        value.tail_discarded_end = 2;
        value.sequence = 2;
        value.outcome_sequence = 3;
        records.flushes.push(value);
        assert_eq!(
            rejection(records),
            "execution field lacks complete atom provenance"
        );
    }

    #[test]
    fn convert_report_rejects_inconsistent_atom_replacement_graphs() {
        let mut records = raw_records();
        let mut value = atom();
        value.disposition = 4;
        records.atoms.push(value);
        assert_eq!(
            rejection(records),
            "execution atom replacement disposition is inconsistent"
        );

        let mut records = raw_records();
        let mut source = atom();
        source.disposition = 3;
        source.replaced_by = 1;
        records.atoms.push(source);
        let mut replacement = atom();
        replacement.key = 1;
        replacement.disposition = 3;
        replacement.sequence = 2;
        records.atoms.push(replacement);
        assert_eq!(
            rejection(records),
            "execution atom replacement disposition is inconsistent"
        );

        let mut records = raw_records();
        for (key, sequence) in [(0, 1), (1, 2)] {
            let mut source = atom();
            source.key = key;
            source.disposition = 4;
            source.replaced_by = 2;
            source.sequence = sequence;
            records.atoms.push(source);
        }
        let mut replacement = atom();
        replacement.key = 2;
        replacement.disposition = 3;
        replacement.sequence = 3;
        records.atoms.push(replacement);
        assert_eq!(
            rejection(records),
            "execution atom has multiple replacement predecessors"
        );

        let mut records = raw_records();
        let mut direct = direct_atom();
        direct.disposition = 3;
        records.atoms.push(direct);
        assert_eq!(rejection(records), "direct execution atom is not emitted");

        let mut records = raw_records();
        let mut source = direct_atom();
        source.disposition = 4;
        source.replaced_by = 1;
        records.atoms.push(source);
        let mut replacement = direct_atom();
        replacement.key = 1;
        replacement.disposition = 3;
        replacement.sequence = 2;
        records.atoms.push(replacement);
        assert_eq!(rejection(records), "direct execution atom is not emitted");

        let mut records = raw_records();
        records.atoms.push(direct_atom());
        assert_eq!(rejection(records), "direct execution atom has no fragment");
    }

    #[test]
    fn convert_report_rejects_incomplete_emission_relationships() {
        let mut records = records_with_emitted_fragment();
        records.atoms[0].disposition = 3;
        assert_eq!(
            rejection(records),
            "fragment atom belongs to another execution origin"
        );

        let mut records = records_with_emitted_fragment();
        records.flushes.clear();
        assert_eq!(
            rejection(records),
            "execution fragments are not exactly partitioned by flushes"
        );
    }

    #[test]
    fn convert_report_rejects_inexact_fragment_geometry() {
        let mut records = records_with_emitted_fragment();
        records.geometry.clear();
        assert_eq!(
            rejection(records),
            "execution fragment has no unique glyph geometry"
        );

        let mut records = records_with_emitted_fragment();
        records.geometry[0].before = 1;
        assert_eq!(
            rejection(records),
            "invalid execution glyph geometry relationship"
        );

        let mut records = records_with_emitted_fragment();
        records.fragments[0].start_bu = i64::MIN;
        records.fragments[0].end_bu = i64::MAX;
        assert_eq!(rejection(records), "invalid execution fragment geometry");

        let mut records = records_with_emitted_fragment();
        records.geometry[0].sequence = records.flushes[0].outcome_sequence;
        assert_eq!(
            rejection(records),
            "invalid execution glyph geometry relationship"
        );

        let mut records = records_with_emitted_fragment();
        let mut duplicate = records.geometry[0];
        duplicate.key = 1;
        duplicate.sequence = 6;
        records.geometry.push(duplicate);
        assert_eq!(
            rejection(records),
            "invalid execution glyph geometry relationship"
        );
    }

    #[test]
    fn convert_report_rejects_geometry_reassigned_to_another_node() {
        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records.nodes.push(node());
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 2)]);
        let mut flush_record = flush();
        flush_record.node = 0;
        flush_record.sequence = 3;
        flush_record.outcome_sequence = 6;
        records.flushes.push(flush_record);
        records.geometry.push(CGeometryRecord {
            key: 0,
            node: NONE,
            related: 0,
            kind: 1,
            unit: 1,
            origin_kind: 2,
            origin_key: 0,
            reserved: 0,
            requested: 24,
            effective: 24,
            before: 0,
            after: 24,
            sequence: 4,
        });
        assert_eq!(
            rejection(records),
            "invalid execution advance geometry relationship"
        );

        let mut records = raw_records();
        records.nodes.push(node());
        records.boundaries.push(CBoundaryRecord {
            key: 0,
            node: NONE,
            parent: NONE,
            request: 4,
            effect: 2,
            flags_before: 0,
            flags_after: 0,
            control: NONE,
            line_before: 0,
            line_after: 1,
            visual_before: 24,
            visual_after: 0,
            direct_device_lines: 0,
            wrapper: NONE,
            enter_sequence: 1,
            leave_sequence: 3,
        });
        records.geometry.push(CGeometryRecord {
            key: 0,
            node: 0,
            related: 0,
            kind: 3,
            unit: 3,
            origin_kind: 3,
            origin_key: 0,
            reserved: 0,
            requested: 1,
            effective: 1,
            before: 0,
            after: 1,
            sequence: 2,
        });
        assert_eq!(
            rejection(records),
            "invalid execution endline geometry relationship"
        );

        let mut records = raw_records();
        records.buffer_generations[0].close_reason = 1;
        records.nodes.push(node());
        records
            .atoms
            .extend([consumed_atom(0, 0, 1), consumed_atom(1, 1, 2)]);
        let mut flush_record = flush();
        flush_record.node = 0;
        flush_record.sequence = 3;
        flush_record.outcome_sequence = 6;
        records.flushes.push(flush_record);
        records.geometry.push(CGeometryRecord {
            key: 0,
            node: NONE,
            related: 0,
            kind: 4,
            unit: 1,
            origin_kind: 2,
            origin_key: 0,
            reserved: 0,
            requested: 0,
            effective: 0,
            before: 0,
            after: 0,
            sequence: 4,
        });
        assert_eq!(
            rejection(records),
            "invalid execution field geometry relationship"
        );
    }

    #[test]
    fn report_transfer_capacity_failure_is_reported() {
        assert_eq!(
            reserved_vec::<u8>(usize::MAX, "probe").unwrap_err(),
            "could not allocate converted execution probe records"
        );
        assert_eq!(
            reserved_filled_vec(0_u8, usize::MAX, "probe").unwrap_err(),
            "could not allocate converted execution probe records"
        );
    }

    #[test]
    fn convert_report_rejects_unclaimed_fragment_references() {
        let mut records = raw_records();
        records.atoms.push(atom());
        records.fragment_atoms.push(CFragmentAtomRecord {
            fragment: 0,
            atom: 0,
        });
        assert_eq!(
            rejection(records),
            "invalid execution fragment-atom reference"
        );
    }

    #[test]
    fn convert_report_rejects_duplicate_event_sequences() {
        let mut records = raw_records();
        let mut first = atom();
        first.disposition = 3;
        records.atoms.push(first);
        let mut value = atom();
        value.key = 1;
        value.slot = 1;
        value.disposition = 3;
        records.atoms.push(value);

        assert_eq!(rejection(records), "duplicate native execution sequence");
    }
}
