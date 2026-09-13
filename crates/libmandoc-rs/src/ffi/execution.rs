//! Strict, all-or-error transfer of the sealed native execution report.

use super::{
    owned::copy_document_from_handle,
    raw::{self, CDocument, CExecutionLimits, CExecutionReport},
    session::DocumentHandle,
};
use crate::{
    AtomDisposition, AtomKey, AtomKind, AtomRole, BoundaryEffect, BoundaryRequest,
    BufferCloseReason, ExecutionAffinity, ExecutionAnchor, ExecutionAtom, ExecutionBoundary,
    ExecutionBufferGeneration, ExecutionDiagnostic, ExecutionErrorKind, ExecutionFlush,
    ExecutionFont, ExecutionFragment, ExecutionGeometry, ExecutionLimits, ExecutionNode,
    ExecutionNodeKey, ExecutionReference, ExecutionReferenceKind, ExecutionSource,
    ExecutionWrapper, ExecutionWrapperKind, FlushOutcome, FragmentKey, FragmentRole, GeometryKind,
    GeometryOriginKind, GeometryUnit, NativeExecutionReport, PoolRange, RawDocument,
};
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
use std::{
    mem::{MaybeUninit, align_of, offset_of, size_of},
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
    reserved: u32,
    line_before: i64,
    line_after: i64,
    visual_before: i64,
    visual_after: i64,
    sequence: u64,
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
    affinity: u32,
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
            _ => ExecutionErrorKind::Native,
        };
        let message = unsafe { super::owned::optional_string(mant_mandoc_execution_error(report)) }
            .unwrap_or_else(|| "native execution failed without a diagnostic".to_owned());
        return Err((kind, message));
    }
    let execution_node_count = unsafe { mant_mandoc_execution_node_count(report) };
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
    let execution = unsafe { copy_report(report) }
        .map_err(|message| (ExecutionErrorKind::Transfer, message))?;
    if ast_keys.len() != execution.nodes.len()
        || !ast_keys
            .iter()
            .zip(&execution.nodes)
            .all(|(actual, expected)| *actual == expected.key.0)
    {
        return Err((
            ExecutionErrorKind::Transfer,
            "owned syntax tree and execution report node identities differ".to_owned(),
        ));
    }
    Ok((raw_document, execution))
}

pub(super) fn native_limits(limits: ExecutionLimits) -> CExecutionLimits {
    CExecutionLimits {
        abi_version: 1,
        abi_size: u32::try_from(size_of::<CExecutionLimits>())
            .expect("execution limits ABI size fits in u32"),
        max_nodes: limits.max_nodes,
        max_depth: limits.max_depth,
        max_work: limits.max_work,
        max_records: limits.max_records,
        max_pool_bytes: limits.max_pool_bytes,
        max_buffer_cells: limits.max_buffer_cells,
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

unsafe fn copy_report(report: *const CExecutionReport) -> Result<NativeExecutionReport, String> {
    let records = unsafe { copy_raw_records(report) }?;
    let pool = unsafe { copy_pool(report) }?;
    let work_units = unsafe { mant_mandoc_execution_work_count(report) };
    let record_count = unsafe { mant_mandoc_execution_record_count(report) };
    let buffer_cells = unsafe { raw::mant_mandoc_execution_buffer_cell_count(report) };
    convert_report(pool, work_units, record_count, buffer_cells, records)
}

struct RawRecords {
    sources: Vec<CSourceRecord>,
    nodes: Vec<CNodeRecord>,
    buffer_generations: Vec<CBufferGenerationRecord>,
    atoms: Vec<CAtomRecord>,
    fragments: Vec<CFragmentRecord>,
    fragment_atoms: Vec<CFragmentAtomRecord>,
    flushes: Vec<CFlushRecord>,
    boundaries: Vec<CBoundaryRecord>,
    geometry: Vec<CGeometryRecord>,
    wrappers: Vec<CWrapperRecord>,
    references: Vec<CReferenceRecord>,
    anchors: Vec<CAnchorRecord>,
    diagnostics: Vec<CDiagnosticRecord>,
}

macro_rules! copy_record_table {
    ($function:ident, $type:ty, $offsets:ident, $name:literal, $count:ident, $size:ident, $align:ident, $fields:ident, $offset:ident, $copy:ident) => {
        unsafe fn $function(report: *const CExecutionReport) -> Result<Vec<$type>, String> {
            unsafe {
                copy_table(
                    report,
                    &api!($name, $count, $size, $align, $fields, $offset, $copy),
                    &$offsets(),
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

unsafe fn copy_raw_records(report: *const CExecutionReport) -> Result<RawRecords, String> {
    Ok(RawRecords {
        sources: unsafe { copy_sources(report) }?,
        nodes: unsafe { copy_nodes(report) }?,
        buffer_generations: unsafe { copy_buffer_generations(report) }?,
        atoms: unsafe { copy_atoms(report) }?,
        fragments: unsafe { copy_fragments(report) }?,
        fragment_atoms: unsafe { copy_fragment_atoms(report) }?,
        flushes: unsafe { copy_flushes(report) }?,
        boundaries: unsafe { copy_boundaries(report) }?,
        geometry: unsafe { copy_geometry(report) }?,
        wrappers: unsafe { copy_wrappers(report) }?,
        references: unsafe { copy_references(report) }?,
        anchors: unsafe { copy_anchors(report) }?,
        diagnostics: unsafe { copy_diagnostics(report) }?,
    })
}

unsafe fn copy_table<T: Copy>(
    report: *const CExecutionReport,
    api: &RecordApi,
    offsets: &[usize],
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
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or_else(|| format!("libmandoc {} record byte count overflow", api.name))?;
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

unsafe fn copy_pool(report: *const CExecutionReport) -> Result<Vec<u8>, String> {
    let length = unsafe { mant_mandoc_execution_pool_length(report) };
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
        atoms: atom_records,
        fragments: fragment_records,
        fragment_atoms: fragment_atom_records,
        flushes: flush_records,
        boundaries: boundary_records,
        geometry: geometry_records,
        wrappers: wrapper_records,
        references: reference_records,
        anchors: anchor_records,
        diagnostics: diagnostic_records,
    } = records;
    let node_count = node_records.len();
    let source_count = source_records.len();
    let atom_count = atom_records.len();
    let buffer_generation_count = buffer_generation_records.len();
    let fragment_count = fragment_records.len();
    let wrapper_count = wrapper_records.len();
    let reference_count = reference_records.len();
    let expected_records = [
        source_count,
        node_count,
        buffer_generation_count,
        atom_count,
        fragment_count,
        fragment_atom_records.len(),
        flush_records.len(),
        boundary_records.len(),
        geometry_records.len(),
        wrapper_count,
        reference_count,
        anchor_records.len(),
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
        if value.kind > 9 || value.flags & !0x03ff != 0 {
            return Err("unknown execution node kind or flags".to_owned());
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
            kind: value.kind,
            flags: value.flags,
            macro_name,
        });
    }
    let buffer_identity_count =
        buffer_generation_records
            .iter()
            .try_fold(0_usize, |count, generation| {
                usize::try_from(generation.buffer)
                    .ok()
                    .and_then(|buffer| buffer.checked_add(1))
                    .map(|next| count.max(next))
            });
    let Some(buffer_identity_count) = buffer_identity_count else {
        return Err("invalid execution buffer identity".to_owned());
    };
    let mut buffer_generations = reserved_vec(buffer_generation_count, "buffer-generation")?;
    let mut latest_generation = reserved_filled_vec(
        None::<(u32, u32, BufferCloseReason, u64)>,
        buffer_identity_count,
        "buffer-generation state",
    )?;
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
        let state = latest_generation
            .get_mut(
                usize::try_from(value.buffer)
                    .ok()
                    .filter(|buffer| *buffer < buffer_identity_count)
                    .ok_or_else(|| "invalid execution buffer identity".to_owned())?,
            )
            .expect("checked buffer generation state");
        if let Some((
            previous_generation,
            previous_capacity,
            previous_close_reason,
            previous_close,
        )) = *state
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
        *state = Some((
            value.generation,
            value.capacity,
            close_reason,
            value.close_sequence,
        ));
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
    let accounted_buffer_cells = latest_generation.iter().try_fold(0_u64, |total, state| {
        let Some((_, capacity, _, _)) = state else {
            return None;
        };
        total.checked_add(u64::from(*capacity))
    });
    if latest_generation.iter().any(Option::is_none) {
        return Err("execution buffer identities are not dense".to_owned());
    }
    if accounted_buffer_cells != Some(buffer_cells) {
        return Err("native execution buffer capacity accounting mismatch".to_owned());
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
        let wrapper = option(value.wrapper);
        if wrapper.is_some_and(|key| {
            usize::try_from(key)
                .ok()
                .is_none_or(|key| key >= wrapper_count)
        }) {
            return Err("invalid execution atom wrapper".to_owned());
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
        {
            return Err("fragment atom belongs to another buffer generation".to_owned());
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
    for (index, value) in flush_records.iter().copied().enumerate() {
        dense(value.key, index, "flush")?;
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
        let generation_fact = buffer_generations
            .get(
                usize::try_from(value.buffer_generation)
                    .map_err(|_| "execution flush buffer-generation key overflow".to_owned())?,
            )
            .ok_or_else(|| "invalid execution flush buffer-generation".to_owned())?;
        if generation_fact.buffer != value.buffer
            || generation_fact.generation != value.generation
            || value.sequence <= generation_fact.open_sequence
            || value.outcome_sequence <= value.sequence
            || value.outcome_sequence >= generation_fact.close_sequence
        {
            return Err("execution flush is outside its buffer generation".to_owned());
        }
        if value.scan_start != value.accepted_start
            || value.scan_start != value.consumed_start
            || value.accepted_start > value.accepted_end
            || value.accepted_start != value.consumed_start
            || value.accepted_end != value.consumed_end
            || value.accepted_end != value.remaining_start
            || value.remaining_start > value.remaining_end
            || value.scan_end != value.remaining_end
            || value.scan_end != generation_fact.extent
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
        if outcome == FlushOutcome::NoContent {
            if value.accepted_start != value.accepted_end
                || value.consumed_start != value.consumed_end
                || value.fragment_length != 0
            {
                return Err("no-content flush emitted or consumed a field".to_owned());
            }
        } else if value.accepted_start == value.accepted_end {
            return Err("content flush accepted an empty field".to_owned());
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
    for (index, flush) in flushes.iter().enumerate() {
        if flush.outcome == FlushOutcome::Wrapped {
            let Some(next) = flushes.get(index + 1) else {
                return Err("wrapped execution flush has no successor".to_owned());
            };
            if next.buffer_generation != flush.buffer_generation
                || next.scanned.start < flush.remaining.start
                || next.scanned.start > flush.remaining.end
            {
                return Err("wrapped execution flush successor is inconsistent".to_owned());
            }
        }
    }
    if fragments.iter().enumerate().any(|(index, fragment)| {
        fragment.buffer_generation.is_some() != fragment_flush_outcomes[index].is_some()
    }) {
        return Err("execution fragments are not exactly partitioned by flushes".to_owned());
    }
    let mut boundaries = reserved_vec(boundary_records.len(), "boundary")?;
    for (index, value) in boundary_records.into_iter().enumerate() {
        dense(value.key, index, "boundary")?;
        if value.reserved != 0 {
            return Err("non-zero reserved execution boundary field".to_owned());
        }
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
        boundaries.push(ExecutionBoundary {
            key: value.key,
            node: optional_node_key(value.node, node_count, "boundary")?,
            parent,
            request,
            effect,
            flags_before: value.flags_before,
            flags_after: value.flags_after,
            line_before: value.line_before,
            line_after: value.line_after,
            visual_before: value.visual_before,
            visual_after: value.visual_after,
            sequence: value.sequence,
        });
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
        match kind {
            GeometryKind::Advance => {
                if unit != GeometryUnit::Basic
                    || !matches!(
                        origin_kind,
                        GeometryOriginKind::None | GeometryOriginKind::Flush
                    )
                    || (origin_kind == GeometryOriginKind::None)
                        != option(value.origin_key).is_none()
                    || option(value.origin_key).is_some_and(|key| {
                        usize::try_from(key)
                            .ok()
                            .is_none_or(|key| key >= flushes.len())
                    })
                    || option(value.related) != option(value.origin_key)
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
                if unit != GeometryUnit::Basic
                    || origin_kind != GeometryOriginKind::Atom
                    || glyph_geometry_coverage[related_fragment]
                    || fragment.atoms.as_slice() != [AtomKey(value.origin_key)]
                    || atoms[origin_atom].disposition != AtomDisposition::Emitted
                    || optional_node_key(value.node, node_count, "glyph geometry")? != fragment.node
                    || value.requested != fragment.end_bu - fragment.start_bu
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
                if unit != GeometryUnit::DeviceLine
                    || origin_kind != GeometryOriginKind::Boundary
                    || option(value.origin_key) != option(value.related)
                    || option(value.related).is_none_or(|key| {
                        usize::try_from(key)
                            .ok()
                            .is_none_or(|key| key >= boundaries.len())
                    })
                {
                    return Err("invalid execution endline geometry relationship".to_owned());
                }
            }
            GeometryKind::Field => {
                if unit != GeometryUnit::Basic
                    || origin_kind != GeometryOriginKind::Flush
                    || option(value.origin_key).is_none_or(|key| {
                        usize::try_from(key)
                            .ok()
                            .is_none_or(|key| key >= flushes.len())
                    })
                    || option(value.related) != option(value.origin_key)
                {
                    return Err("invalid execution field geometry relationship".to_owned());
                }
            }
        }
        geometry.push(ExecutionGeometry {
            key: value.key,
            node: optional_node_key(value.node, node_count, "geometry")?,
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
    let mut wrappers = reserved_vec(wrapper_count, "wrapper")?;
    for (index, value) in wrapper_records.into_iter().enumerate() {
        dense(value.key, index, "wrapper")?;
        let parent = option(value.parent);
        if parent.is_some_and(|key| usize::try_from(key).ok().is_none_or(|key| key >= index)) {
            return Err("execution wrapper parent is not an earlier wrapper".to_owned());
        }
        if value.leave_sequence < value.enter_sequence
            || value.depth_after > value.depth_before.saturating_add(1)
        {
            return Err("unbalanced execution wrapper".to_owned());
        }
        let node = optional_node_key(value.node, node_count, "wrapper")?;
        let kind = match value.kind {
            1 => ExecutionWrapperKind::Node,
            2 => ExecutionWrapperKind::Font,
            _ => return Err("unknown execution wrapper kind".to_owned()),
        };
        if value.enter_atom as usize > atom_count
            || value.leave_atom == NONE
            || value.leave_atom as usize > atom_count
            || value.enter_atom > value.leave_atom
        {
            return Err("invalid execution wrapper atom range".to_owned());
        }
        if kind == ExecutionWrapperKind::Node && node.is_none() {
            return Err("node wrapper has no execution node".to_owned());
        }
        if kind == ExecutionWrapperKind::Font
            && (value.state_before > 3 || value.state_after > 3 || value.target_start != NONE)
        {
            return Err("invalid execution font transition".to_owned());
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
            affinity: value.affinity,
            flags: value.flags,
            state_before: value.state_before,
            state_after: value.state_after,
            depth_before: value.depth_before,
            depth_after: value.depth_after,
            enter_sequence: value.enter_sequence,
            leave_sequence: value.leave_sequence,
        });
    }
    for wrapper in &wrappers {
        if let Some(parent) = wrapper.parent {
            let parent = &wrappers[parent as usize];
            if parent.enter_sequence >= wrapper.enter_sequence
                || parent.leave_sequence <= wrapper.leave_sequence
            {
                return Err("execution wrapper is outside its parent".to_owned());
            }
        }
        if wrapper.kind == ExecutionWrapperKind::Node
            && (wrapper.enter_atom as usize > atom_count
                || wrapper.leave_atom as usize > atom_count)
        {
            return Err("invalid node wrapper atom range".to_owned());
        }
    }
    for atom in &atoms {
        if let Some(wrapper_key) = atom.wrapper {
            let wrapper = &wrappers[wrapper_key as usize];
            if wrapper.kind != ExecutionWrapperKind::Node
                || atom.sequence <= wrapper.enter_sequence
                || atom.sequence >= wrapper.leave_sequence
                || atom.key.0 < wrapper.enter_atom
                || atom.key.0 >= wrapper.leave_atom
            {
                return Err("execution atom is outside its node wrapper".to_owned());
            }
        }
    }
    for fragment in &fragments {
        if let Some(wrapper_key) = fragment.wrapper {
            let wrapper = &wrappers[wrapper_key as usize];
            if wrapper.kind != ExecutionWrapperKind::Node
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
        &atoms,
        &buffer_generations,
        &fragments,
        &flushes,
        &boundaries,
        &geometry,
        &wrappers,
        &references,
        &anchors,
        &diagnostics,
    )?;
    Ok(NativeExecutionReport {
        pool,
        work_units,
        record_count,
        buffer_cells,
        sources,
        nodes,
        buffer_generations,
        atoms,
        fragments,
        flushes,
        boundaries,
        geometry,
        wrappers,
        references,
        anchors,
        diagnostics,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_event_sequences(
    atoms: &[ExecutionAtom],
    buffer_generations: &[ExecutionBufferGeneration],
    fragments: &[ExecutionFragment],
    flushes: &[ExecutionFlush],
    boundaries: &[ExecutionBoundary],
    geometry: &[ExecutionGeometry],
    wrappers: &[ExecutionWrapper],
    references: &[ExecutionReference],
    anchors: &[ExecutionAnchor],
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
    let wrapper_sequences = wrappers
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let reference_sequences = references
        .len()
        .checked_mul(2)
        .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let sequence_count = [
        atoms.len(),
        generation_sequences,
        fragments.len(),
        flush_sequences,
        boundaries.len(),
        geometry.len(),
        wrapper_sequences,
        reference_sequences,
        anchors.len(),
        diagnostics.len(),
    ]
    .into_iter()
    .try_fold(0_usize, usize::checked_add)
    .ok_or_else(|| "native execution sequence count overflow".to_owned())?;
    let mut seen = reserved_vec(sequence_count, "event sequences")?;
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
        seen.push(boundary.sequence);
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
fn flush_offsets() -> [usize; 29] {
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
fn boundary_offsets() -> [usize; 13] {
    [
        offset_of!(CBoundaryRecord, key),
        offset_of!(CBoundaryRecord, node),
        offset_of!(CBoundaryRecord, parent),
        offset_of!(CBoundaryRecord, request),
        offset_of!(CBoundaryRecord, effect),
        offset_of!(CBoundaryRecord, flags_before),
        offset_of!(CBoundaryRecord, flags_after),
        offset_of!(CBoundaryRecord, reserved),
        offset_of!(CBoundaryRecord, line_before),
        offset_of!(CBoundaryRecord, line_after),
        offset_of!(CBoundaryRecord, visual_before),
        offset_of!(CBoundaryRecord, visual_after),
        offset_of!(CBoundaryRecord, sequence),
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
        offset_of!(CWrapperRecord, affinity),
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
            atoms: Vec::new(),
            fragments: Vec::new(),
            fragment_atoms: Vec::new(),
            flushes: Vec::new(),
            boundaries: Vec::new(),
            geometry: Vec::new(),
            wrappers: Vec::new(),
            references: Vec::new(),
            anchors: Vec::new(),
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
            affinity: 0,
            flags: 0,
            state_before: 0,
            state_after: 0,
            depth_before: 0,
            depth_after: 0,
            enter_sequence: 1,
            leave_sequence: 2,
        }
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
        records.atoms.push(atom());
        let mut fragment = fragment();
        fragment.sequence = 3;
        records.fragments.push(fragment);
        records.fragment_atoms.push(CFragmentAtomRecord {
            fragment: 0,
            atom: 0,
        });
        let mut flush = flush();
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

    fn record_count(records: &RawRecords) -> u64 {
        [
            records.sources.len(),
            records.nodes.len(),
            records.buffer_generations.len(),
            records.atoms.len(),
            records.fragments.len(),
            records.fragment_atoms.len(),
            records.flushes.len(),
            records.boundaries.len(),
            records.geometry.len(),
            records.wrappers.len(),
            records.references.len(),
            records.anchors.len(),
            records.diagnostics.len(),
        ]
        .into_iter()
        .map(|count| u64::try_from(count).unwrap())
        .sum()
    }

    fn rejection(records: RawRecords) -> String {
        let count = record_count(&records);
        let buffer_cells = records
            .buffer_generations
            .iter()
            .map(|generation| u64::from(generation.capacity))
            .max()
            .unwrap_or(0);
        convert_report(b"x".to_vec(), 0, count, buffer_cells, records).unwrap_err()
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
            "fragment atom belongs to another buffer generation"
        );
    }

    #[test]
    fn convert_report_rejects_unbalanced_wrappers() {
        let mut records = raw_records();
        let mut value = wrapper();
        value.depth_after = 2;
        records.wrappers.push(value);
        assert_eq!(rejection(records), "unbalanced execution wrapper");

        let mut records = raw_records();
        let mut value = wrapper();
        value.leave_sequence = 0;
        records.wrappers.push(value);
        assert_eq!(rejection(records), "unbalanced execution wrapper");
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
        assert_eq!(
            rejection(records),
            "execution buffer identities are not dense"
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
        value.remaining_start = 0;
        records.flushes.push(value);
        assert_eq!(rejection(records), "content flush accepted an empty field");

        let mut records = records_with_emitted_fragment();
        records.flushes[0].scan_start = 1;
        records.flushes[0].accepted_start = 1;
        records.flushes[0].consumed_start = 1;
        assert_eq!(
            rejection(records),
            "flush contains a fragment outside its accepted field"
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
            "fragment atom belongs to another buffer generation"
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
