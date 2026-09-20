//! Checked ownership transfer for the versioned structured-rendering ABI.
#![allow(dead_code)]

use super::guard::NativeSessionGuard;
use crate::{InputFormat, SourceBundle};
use std::{ffi::c_void, ptr::NonNull};

pub(super) const STATUS_OK: u32 = 0;
pub(super) const STATUS_INVALID_INPUT: u32 = 1;
pub(super) const STATUS_REENTRANT: u32 = 2;
pub(super) const STATUS_BUDGET: u32 = 3;
pub(super) const STATUS_BUILDER_ALLOC: u32 = 4;
pub(super) const STATUS_NATIVE: u32 = 5;
pub(super) const STATUS_RELATION: u32 = 6;
pub(super) const STATUS_UNSUPPORTED: u32 = 7;

const IDENTITY_BUNDLE_MEMBER: u32 = 2;
const FORMAT_MAN: u32 = 1;
const FORMAT_MDOC: u32 = 2;
const PROFILE_UTF8: u32 = 1;
const PROFILE_ASCII: u32 = 2;
const COORD_NATIVE_NORMALIZED_BYTES: u32 = 2;
const DIAGNOSTIC_STYLE: u32 = 1;
const DIAGNOSTIC_UNSUPPORTED: u32 = 4;
const DIAGNOSTIC_CODE_NATIVE_LAST: u32 = 210;
const PROVENANCE_AUTHORED: u32 = 1;
const PROVENANCE_GENERATED: u32 = 2;
const PROVENANCE_UNKNOWN: u32 = 3;
const ATOM_TEXT: u32 = 1;
const ATOM_WHITESPACE: u32 = 2;
const ATOM_BREAK_OPPORTUNITY: u32 = 3;
const ATOM_HARD_BREAK: u32 = 4;
const STYLE_MASK: u32 = 1 | 2 | 4 | 8;
const OWNER_KIND_LAST: u32 = 7;
const ROOT_HEADING: u32 = 1;
const ROOT_BODY: u32 = 3;
const ROOT_KIND_LAST: u32 = 5;
const BLOCK_HEADING: u32 = 1;
const BLOCK_PARAGRAPH: u32 = 2;
const BLOCK_LIST: u32 = 3;
const BLOCK_DEFINITION_LIST: u32 = 4;
const BLOCK_TABLE: u32 = 5;
const BLOCK_INDENTED: u32 = 6;
const BLOCK_FIXED_DISPLAY: u32 = 7;
const BLOCK_VERTICAL_SPACE: u32 = 8;
const BLOCK_THEMATIC_BREAK: u32 = 9;
const RESOLVE_NOT_FOUND: u32 = 1;
const RESOLVE_DENIED: u32 = 2;
const RESOLVE_PANIC: u32 = 4;
const RESOLVE_INVALID: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BytesView {
    ptr: *const u8,
    len: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SliceView {
    ptr: *const c_void,
    count: u32,
    stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InputSourceView {
    identity_kind: u32,
    format: u32,
    logical_name: BytesView,
    resolver_name: BytesView,
    source_bytes: BytesView,
    reserved: u32,
}

type ResolveFn = unsafe extern "C" fn(*mut c_void, u32, BytesView, *mut u32) -> u32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct InputView {
    sources: SliceView,
    root_input: u32,
    profile: u32,
    width: u32,
    resolve: Option<ResolveFn>,
    resolve_context: *mut c_void,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct FailureView {
    status: u32,
    stage: u32,
    limit_kind: u32,
    observed: u64,
    allowed: u64,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct ProbeMetrics {
    collector_events: u64,
    logical_events: u64,
    buffer_writes: u64,
    cursor_moves: u64,
    truncates: u64,
    consumes: u64,
    partial_consumes: u64,
    continued_consumes: u64,
    resets: u64,
    peak_columns: u64,
    peak_slots: u64,
    rendered_bytes: u64,
    builder_allocated_bytes: u64,
    content_bytes: u64,
    source_count: u64,
    token_count: u64,
    slot_capacity: u64,
    sidecar_allocated_bytes: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Limits {
    max_input_sources: u64,
    max_sources: u64,
    max_source_path_bytes: u64,
    max_decoded_source_bytes_per_source: u64,
    max_decoded_source_bytes_total: u64,
    max_source_map_entries: u64,
    max_source_map_bytes: u64,
    max_builder_operations: u64,
    max_builder_allocated_bytes: u64,
    max_content_bytes: u64,
    max_owners: u64,
    max_blocks: u64,
    max_content_atoms: u64,
    max_content_refs: u64,
    max_content_points: u64,
    max_links: u64,
    max_tables: u64,
    max_table_rows: u64,
    max_table_cells: u64,
    max_fixed_views: u64,
    max_fixed_lines: u64,
    max_placements: u64,
    max_decorations: u64,
    max_forms: u64,
    max_name_hints: u64,
    max_relations: u64,
    max_connection_atoms: u64,
    max_annotation_runs: u64,
    max_annotation_mutations: u64,
    max_relation_edges: u64,
    max_diagnostics: u64,
    max_transfer_objects: u64,
    max_transfer_edges: u64,
    max_transfer_bytes: u64,
    max_nesting_depth: u64,
    max_include_depth: u64,
    reserved: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_sources: 4_096,
            max_sources: 4_096,
            max_source_path_bytes: 4 * 1024 * 1024,
            max_decoded_source_bytes_per_source: 64 * 1024 * 1024,
            max_decoded_source_bytes_total: 256 * 1024 * 1024,
            max_source_map_entries: 4_194_304,
            max_source_map_bytes: 64 * 1024 * 1024,
            max_builder_operations: 67_108_864,
            max_builder_allocated_bytes: 256 * 1024 * 1024,
            max_content_bytes: 128 * 1024 * 1024,
            max_owners: 1_048_576,
            max_blocks: 1_048_576,
            max_content_atoms: 4_194_304,
            max_content_refs: 8_388_608,
            max_content_points: 1_048_576,
            max_links: 1_048_576,
            max_tables: 1_048_576,
            max_table_rows: 1_048_576,
            max_table_cells: 4_194_304,
            max_fixed_views: 1_048_576,
            max_fixed_lines: 1_048_576,
            max_placements: 8_388_608,
            max_decorations: 8_388_608,
            max_forms: 1_048_576,
            max_name_hints: 1_048_576,
            max_relations: 4_194_304,
            max_connection_atoms: 4_194_304,
            max_annotation_runs: 4_194_304,
            max_annotation_mutations: 16_777_216,
            max_relation_edges: 8_388_608,
            max_diagnostics: 65_536,
            max_transfer_objects: 16_777_216,
            max_transfer_edges: 16_777_216,
            max_transfer_bytes: 512 * 1024 * 1024,
            max_nesting_depth: 256,
            max_include_depth: 64,
            reserved: 0,
        }
    }
}

impl Limits {
    fn is_valid(&self) -> bool {
        let values = [
            self.max_input_sources,
            self.max_sources,
            self.max_source_path_bytes,
            self.max_decoded_source_bytes_per_source,
            self.max_decoded_source_bytes_total,
            self.max_source_map_entries,
            self.max_source_map_bytes,
            self.max_builder_operations,
            self.max_builder_allocated_bytes,
            self.max_content_bytes,
            self.max_owners,
            self.max_blocks,
            self.max_content_atoms,
            self.max_content_refs,
            self.max_content_points,
            self.max_links,
            self.max_tables,
            self.max_table_rows,
            self.max_table_cells,
            self.max_fixed_views,
            self.max_fixed_lines,
            self.max_placements,
            self.max_decorations,
            self.max_forms,
            self.max_name_hints,
            self.max_relations,
            self.max_connection_atoms,
            self.max_annotation_runs,
            self.max_annotation_mutations,
            self.max_relation_edges,
            self.max_diagnostics,
            self.max_transfer_objects,
            self.max_transfer_edges,
            self.max_transfer_bytes,
            self.max_nesting_depth,
            self.max_include_depth,
        ];
        self.reserved == 0
            && values.iter().all(|value| *value != 0)
            && u32::try_from(self.max_input_sources).is_ok()
            && u32::try_from(self.max_sources).is_ok()
            && u32::try_from(self.max_diagnostics).is_ok()
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct MetadataView {
    macroset: u32,
    presence_flags: u32,
    title: BytesView,
    section: BytesView,
    volume: BytesView,
    operating_system: BytesView,
    architecture: BytesView,
    name: BytesView,
    date: BytesView,
    alias_target: BytesView,
    has_body: u8,
    reserved_bytes: [u8; 3],
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SourceView {
    key: u32,
    identity_kind: u32,
    format: u32,
    coordinate_kind: u32,
    logical_name: BytesView,
    decoded_length: u64,
    hash_present: u8,
    hash: [u8; 32],
    reserved_bytes: [u8; 7],
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SpanView {
    line_column_present: u8,
    byte_range_present: u8,
    reserved_bytes: [u8; 2],
    source: u32,
    line_start: u32,
    column_start: u32,
    line_end: u32,
    column_end: u32,
    byte_start: u64,
    byte_end: u64,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProvenanceView {
    kind: u32,
    authored_span: u32,
    generated_trigger_span: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct OwnerView {
    key: u32,
    kind: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ContentRootView {
    key: u32,
    owner: u32,
    ordinal: u32,
    kind: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ContentAtomView {
    key: u32,
    root: u32,
    ordinal: u32,
    owner: u32,
    kind: u32,
    style_flags: u32,
    role: u32,
    link: u32,
    text: BytesView,
    display_override_present: u8,
    display_reserved_bytes: [u8; 7],
    display_override: BytesView,
    whitespace_breakable: u8,
    reserved_bytes: [u8; 3],
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ContentRefView {
    atom: u32,
    byte_start: u32,
    byte_end: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ContentPointView {
    key: u32,
    root: u32,
    ordinal: u32,
    owner: u32,
    boundary_kind: u32,
    atom_boundary: u32,
    atom: u32,
    byte_offset: u32,
    scalar_boundary: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinkView {
    key: u32,
    owner: u32,
    target_kind: u32,
    target_a: BytesView,
    target_b_present: u8,
    target_b_reserved_bytes: [u8; 7],
    target_b: BytesView,
    title_present: u8,
    title_reserved_bytes: [u8; 7],
    title: BytesView,
    first_label_ref: u32,
    label_ref_count: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct BlockView {
    key: u32,
    owner: u32,
    kind: u32,
    parent: u32,
    ordinal: u32,
    provenance: u32,
    root: u32,
    table: u32,
    fixed_view: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TableView {
    key: u32,
    block: u32,
    fixed_view: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TableRowView {
    key: u32,
    table: u32,
    ordinal: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct TableCellView {
    key: u32,
    row: u32,
    column: u32,
    owner: u32,
    kind: u32,
    alignment: u32,
    row_span: u32,
    column_span: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedView {
    key: u32,
    owner: u32,
    block: u32,
    table: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FixedLineView {
    key: u32,
    view: u32,
    ordinal: u32,
    total_columns: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PlacementView {
    key: u32,
    line: u32,
    ordinal: u32,
    target_kind: u32,
    atom: u32,
    byte_start: u32,
    byte_end: u32,
    point: u32,
    scalar_start: u32,
    scalar_end: u32,
    column_start: u32,
    column_end: u32,
    cell_map_kind: u32,
    cell_map_value: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DecorationView {
    key: u32,
    line: u32,
    ordinal: u32,
    kind: u32,
    text: BytesView,
    column_start: u32,
    column_end: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct FormView {
    key: u32,
    owner: u32,
    role: u32,
    first_ref: u32,
    ref_count: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct NameHintView {
    key: u32,
    form: u32,
    first_ref: u32,
    ref_count: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RelationView {
    key: u32,
    owner: u32,
    kind: u32,
    target_owner: u32,
    provenance: u32,
    reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct DiagnosticView {
    level: u32,
    code: u32,
    message: BytesView,
    span: u32,
    owner: u32,
    reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ResultView {
    root_source: u32,
    profile: u32,
    width: u32,
    metadata: MetadataView,
    sources: SliceView,
    spans: SliceView,
    provenances: SliceView,
    owners: SliceView,
    content_roots: SliceView,
    content_atoms: SliceView,
    content_refs: SliceView,
    content_points: SliceView,
    links: SliceView,
    blocks: SliceView,
    tables: SliceView,
    table_rows: SliceView,
    table_cells: SliceView,
    fixed_views: SliceView,
    fixed_lines: SliceView,
    placements: SliceView,
    decorations: SliceView,
    forms: SliceView,
    name_hints: SliceView,
    relations: SliceView,
    diagnostics: SliceView,
    reserved: u32,
}

#[repr(C)]
struct ResultHandleRaw {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn mant_structured_abi_version() -> u32;
    fn mant_structured_discriminant_fingerprint() -> u64;
    fn mant_structured_render(
        input: *const InputView,
        limits: *const Limits,
        result: *mut *mut ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    fn mant_structured_result_check(
        result: *const ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    fn mant_structured_result_view(result: *const ResultHandleRaw, view: *mut ResultView) -> u32;
    fn mant_structured_result_free(result: *mut ResultHandleRaw);
    fn mant_structured_view_size(kind: u32) -> usize;
    fn mant_structured_view_align(kind: u32) -> usize;
    fn mant_structured_view_offset(kind: u32, field: u32) -> usize;
    fn mant_structured_test_fail_after(successful_allocations: u64);
    fn mant_structured_probe(
        input: *const InputView,
        limits: *const Limits,
        metrics: *mut ProbeMetrics,
        failure: *mut FailureView,
    ) -> u32;
}

struct ResultHandle(NonNull<ResultHandleRaw>);

impl Drop for ResultHandle {
    fn drop(&mut self) {
        unsafe { mant_structured_result_free(self.0.as_ptr()) };
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedSource {
    pub(crate) key: u32,
    pub(crate) identity_kind: u32,
    pub(crate) format: u32,
    pub(crate) coordinate_kind: u32,
    pub(crate) logical_name: String,
    pub(crate) decoded_length: u64,
    pub(crate) hash: Option<[u8; 32]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedSpan {
    pub(crate) source: u32,
    pub(crate) line_columns: Option<(u32, u32, u32, u32)>,
    pub(crate) byte_range: Option<std::ops::Range<u64>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnedProvenance {
    Authored { span: u32 },
    Generated { trigger_span: Option<u32> },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedOwner {
    pub(crate) key: u32,
    pub(crate) kind: u32,
    pub(crate) provenance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedContentRoot {
    pub(crate) key: u32,
    pub(crate) owner: u32,
    pub(crate) ordinal: u32,
    pub(crate) kind: u32,
    pub(crate) provenance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedContentAtom {
    pub(crate) key: u32,
    pub(crate) root: u32,
    pub(crate) ordinal: u32,
    pub(crate) owner: u32,
    pub(crate) kind: u32,
    pub(crate) style_flags: u32,
    pub(crate) role: Option<u32>,
    pub(crate) link: Option<u32>,
    pub(crate) text: String,
    pub(crate) display_override: Option<String>,
    pub(crate) whitespace_breakable: bool,
    pub(crate) provenance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedContentRef {
    pub(crate) atom: u32,
    pub(crate) bytes: std::ops::Range<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedLink {
    pub(crate) key: u32,
    pub(crate) owner: u32,
    pub(crate) target_kind: u32,
    pub(crate) target_a: String,
    pub(crate) target_b: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) first_label_ref: u32,
    pub(crate) label_ref_count: u32,
    pub(crate) provenance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedBlock {
    pub(crate) key: u32,
    pub(crate) owner: u32,
    pub(crate) kind: u32,
    pub(crate) parent: Option<u32>,
    pub(crate) ordinal: u32,
    pub(crate) provenance: u32,
    pub(crate) root: Option<u32>,
    pub(crate) table: Option<u32>,
    pub(crate) fixed_view: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedDiagnostic {
    pub(crate) level: u32,
    pub(crate) code: u32,
    pub(crate) message: String,
    pub(crate) span: Option<u32>,
    pub(crate) owner: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedMetadata {
    pub(crate) macroset: u32,
    pub(crate) title: Option<String>,
    pub(crate) section: Option<String>,
    pub(crate) volume: Option<String>,
    pub(crate) operating_system: Option<String>,
    pub(crate) architecture: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) date: Option<String>,
    pub(crate) alias_target: Option<String>,
    pub(crate) has_body: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedStructuredDocument {
    pub(crate) root_source: u32,
    pub(crate) profile: u32,
    pub(crate) width: u32,
    pub(crate) metadata: OwnedMetadata,
    pub(crate) sources: Vec<OwnedSource>,
    pub(crate) spans: Vec<OwnedSpan>,
    pub(crate) provenances: Vec<OwnedProvenance>,
    pub(crate) owners: Vec<OwnedOwner>,
    pub(crate) content_roots: Vec<OwnedContentRoot>,
    pub(crate) content_atoms: Vec<OwnedContentAtom>,
    pub(crate) content_refs: Vec<OwnedContentRef>,
    pub(crate) links: Vec<OwnedLink>,
    pub(crate) blocks: Vec<OwnedBlock>,
    pub(crate) diagnostics: Vec<OwnedDiagnostic>,
}

struct StructuredSlices<'a> {
    sources: &'a [SourceView],
    spans: &'a [SpanView],
    provenances: &'a [ProvenanceView],
    owners: &'a [OwnerView],
    content_roots: &'a [ContentRootView],
    content_atoms: &'a [ContentAtomView],
    content_refs: &'a [ContentRefView],
    content_points: &'a [ContentPointView],
    links: &'a [LinkView],
    blocks: &'a [BlockView],
    tables: &'a [TableView],
    table_rows: &'a [TableRowView],
    table_cells: &'a [TableCellView],
    fixed_views: &'a [FixedView],
    fixed_lines: &'a [FixedLineView],
    placements: &'a [PlacementView],
    decorations: &'a [DecorationView],
    forms: &'a [FormView],
    name_hints: &'a [NameHintView],
    relations: &'a [RelationView],
    diagnostics: &'a [DiagnosticView],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NativeStructuredError {
    pub(crate) status: u32,
    pub(crate) stage: u32,
    pub(crate) limit_kind: u32,
    pub(crate) observed: u64,
    pub(crate) allowed: u64,
}

struct InputStorage<'a> {
    _bundle: &'a SourceBundle,
    descriptors: Vec<InputSourceView>,
    source_count: u32,
    root_input: u32,
}

impl<'a> InputStorage<'a> {
    fn new(
        root: &str,
        bundle: &'a SourceBundle,
        format: InputFormat,
        limits: &Limits,
    ) -> Result<Self, NativeStructuredError> {
        let invalid = || NativeStructuredError {
            status: STATUS_INVALID_INPUT,
            stage: 1,
            limit_kind: 0,
            observed: 0,
            allowed: 0,
        };
        let budget = |limit_kind, observed, allowed| NativeStructuredError {
            status: STATUS_BUDGET,
            stage: 1,
            limit_kind,
            observed,
            allowed,
        };
        if !limits.is_valid() {
            return Err(invalid());
        }
        let format = match format {
            InputFormat::Man => FORMAT_MAN,
            InputFormat::Mdoc => FORMAT_MDOC,
            InputFormat::Auto => return Err(invalid()),
        };
        let source_count = u64::try_from(bundle.sources().count()).map_err(|_| invalid())?;
        if source_count > limits.max_input_sources {
            return Err(budget(1, source_count, limits.max_input_sources));
        }
        if source_count > limits.max_source_map_entries {
            return Err(budget(6, source_count, limits.max_source_map_entries));
        }
        let mut root_input = None;
        let mut path_bytes = 0_u64;
        let mut decoded_bytes = 0_u64;
        for (index, (name, bytes)) in bundle.sources().enumerate() {
            if name == root {
                root_input = Some(u32::try_from(index + 1).map_err(|_| invalid())?);
            }
            let name_len = u64::try_from(name.len()).map_err(|_| invalid())?;
            path_bytes = path_bytes.checked_add(name_len).ok_or_else(invalid)?;
            if path_bytes > limits.max_source_path_bytes {
                return Err(budget(3, path_bytes, limits.max_source_path_bytes));
            }
            let source_len = u64::try_from(bytes.len()).map_err(|_| invalid())?;
            if source_len > limits.max_decoded_source_bytes_per_source {
                return Err(budget(
                    4,
                    source_len,
                    limits.max_decoded_source_bytes_per_source,
                ));
            }
            decoded_bytes = decoded_bytes.checked_add(source_len).ok_or_else(invalid)?;
            if decoded_bytes > limits.max_decoded_source_bytes_total {
                return Err(budget(
                    5,
                    decoded_bytes,
                    limits.max_decoded_source_bytes_total,
                ));
            }
        }
        let root_input = root_input.ok_or_else(invalid)?;
        let source_count = u32::try_from(source_count).map_err(|_| invalid())?;
        let mut descriptors = Vec::new();
        descriptors
            .try_reserve_exact(source_count as usize)
            .map_err(|_| NativeStructuredError {
                status: STATUS_BUILDER_ALLOC,
                stage: 1,
                limit_kind: 0,
                observed: u64::from(source_count),
                allowed: limits.max_input_sources,
            })?;
        for (name, bytes) in bundle.sources() {
            descriptors.push(InputSourceView {
                identity_kind: IDENTITY_BUNDLE_MEMBER,
                format,
                logical_name: BytesView {
                    ptr: name.as_ptr(),
                    len: name.len() as u64,
                },
                resolver_name: BytesView {
                    ptr: name.as_ptr(),
                    len: name.len() as u64,
                },
                source_bytes: if bytes.is_empty() {
                    BytesView::default()
                } else {
                    BytesView {
                        ptr: bytes.as_ptr(),
                        len: bytes.len() as u64,
                    }
                },
                reserved: 0,
            });
        }
        Ok(Self {
            _bundle: bundle,
            descriptors,
            source_count,
            root_input,
        })
    }

    fn view(&self, width: u32, profile: u32) -> InputView {
        InputView {
            sources: SliceView {
                ptr: self.descriptors.as_ptr().cast(),
                count: self.source_count,
                stride: u32::try_from(std::mem::size_of::<InputSourceView>())
                    .expect("input-source ABI size fits u32"),
            },
            root_input: self.root_input,
            profile,
            width,
            resolve: None,
            resolve_context: std::ptr::null_mut(),
            reserved: 0,
        }
    }
}

pub(crate) fn render_prelude(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    width: u32,
    limits: &Limits,
) -> Result<OwnedStructuredDocument, NativeStructuredError> {
    render_prelude_profile(root, bundle, format, PROFILE_UTF8, width, limits)
}

fn render_prelude_profile(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    profile: u32,
    width: u32,
    limits: &Limits,
) -> Result<OwnedStructuredDocument, NativeStructuredError> {
    let _guard = NativeSessionGuard::enter().map_err(|_| NativeStructuredError {
        status: STATUS_REENTRANT,
        stage: 1,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?;
    if unsafe { mant_structured_abi_version() } != 1 {
        return Err(relation_error());
    }
    let storage = InputStorage::new(root, bundle, format, limits)?;
    let input = storage.view(width, profile);
    let mut pointer = std::ptr::null_mut();
    let mut failure = FailureView::default();
    let status = unsafe {
        mant_structured_render(&raw const input, limits, &raw mut pointer, &raw mut failure)
    };
    if status != STATUS_OK {
        if !pointer.is_null() {
            unsafe { mant_structured_result_free(pointer) };
        }
        return Err(error_from_failure(status, failure)?);
    }
    let handle = ResultHandle(NonNull::new(pointer).ok_or(NativeStructuredError {
        status: STATUS_RELATION,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?);
    let mut checked = FailureView::default();
    let status = unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut checked) };
    if status != STATUS_OK {
        return Err(error_from_failure(status, checked)?);
    }
    let mut view = ResultView::default();
    let status = unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) };
    if status != STATUS_OK || view.reserved != 0 || view.metadata.reserved != 0 {
        return Err(NativeStructuredError {
            status: STATUS_RELATION,
            stage: 6,
            limit_kind: 0,
            observed: 0,
            allowed: 0,
        });
    }
    copy_structured_document(&handle, &view, limits)
}

#[cfg(test)]
fn probe_structured(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    width: u32,
    limits: &Limits,
) -> Result<ProbeMetrics, NativeStructuredError> {
    let _guard = NativeSessionGuard::enter().map_err(|_| NativeStructuredError {
        status: STATUS_REENTRANT,
        stage: 1,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?;
    let storage = InputStorage::new(root, bundle, format, limits)?;
    let input = storage.view(width, PROFILE_UTF8);
    let mut metrics = ProbeMetrics::default();
    let mut failure = FailureView::default();
    let status = unsafe {
        mant_structured_probe(&raw const input, limits, &raw mut metrics, &raw mut failure)
    };
    if status != STATUS_UNSUPPORTED {
        return Err(error_from_failure(status, failure)?);
    }
    let error = error_from_failure(status, failure)?;
    if error.stage != 4 || error.limit_kind != 0 {
        return Err(relation_error());
    }
    Ok(metrics)
}

pub(crate) fn render_structured(
    root: &str,
    bundle: &SourceBundle,
    format: InputFormat,
    profile: crate::structured::StructuredProfile,
    width: u32,
    limits: &crate::structured::StructuredLimits,
) -> Result<crate::structured::StructuredDocument, crate::structured::StructuredError> {
    use crate::structured::StructuredProfile;

    let native_profile = match profile {
        StructuredProfile::Utf8 => PROFILE_UTF8,
        StructuredProfile::Ascii => PROFILE_ASCII,
    };
    let raw = render_prelude_profile(
        root,
        bundle,
        format,
        native_profile,
        width,
        &raw_limits(limits),
    )
    .map_err(|error| semantic_error(&error))?;
    semantic_document(raw)
}

#[allow(clippy::too_many_lines)]
fn semantic_document(
    raw: OwnedStructuredDocument,
) -> Result<crate::structured::StructuredDocument, crate::structured::StructuredError> {
    use crate::structured::{
        ContentAtom, ContentAtomKey, ContentAtomKind, ContentOwner, ContentOwnerKind, ContentRef,
        ContentRoot, ContentRootKey, ContentRootKind, LineColumn, LineColumns, LinkOccurrence,
        LinkOccurrenceKey, NativeBlock, NativeBlockKey, NativeBlockKind, NativeDiagnostic,
        NativeLinkTarget, NativeRole, OwnerKey, Provenance, ProvenanceKey, SourceCoordinates,
        SourceIdentity, SourceKey, SourceRecord, SourceSpan, SpanKey, StructuredDiagnosticCode,
        StructuredDiagnosticLevel, StructuredDocument, StructuredMetadata, StructuredProfile,
        StructuredStyle,
    };

    let OwnedStructuredDocument {
        root_source,
        profile,
        width,
        metadata,
        sources,
        spans,
        provenances,
        owners,
        content_roots,
        content_atoms,
        content_refs,
        links,
        blocks,
        diagnostics,
    } = raw;

    let root_source = SourceKey::new(root_source)
        .ok_or_else(|| semantic_invalid("native root source key is absent"))?;
    let profile = match profile {
        PROFILE_UTF8 => StructuredProfile::Utf8,
        PROFILE_ASCII => StructuredProfile::Ascii,
        _ => return Err(semantic_invalid("native profile discriminator is unknown")),
    };

    let OwnedMetadata {
        macroset,
        title,
        section,
        volume,
        operating_system,
        architecture,
        name,
        date,
        alias_target,
        has_body,
    } = metadata;
    let metadata = StructuredMetadata {
        macro_set: semantic_source_format(macroset)?,
        title,
        section,
        volume,
        operating_system,
        architecture,
        name,
        date,
        alias_target,
        has_body,
    };

    let mut typed_sources = Vec::new();
    typed_sources
        .try_reserve_exact(sources.len())
        .map_err(semantic_allocation)?;
    for source in sources {
        let identity = match source.identity_kind {
            1 => SourceIdentity::Path(source.logical_name),
            IDENTITY_BUNDLE_MEMBER => SourceIdentity::BundleMember(source.logical_name),
            3 => SourceIdentity::Anonymous(source.logical_name),
            _ => return Err(semantic_invalid("native source identity kind is unknown")),
        };
        typed_sources.push(SourceRecord {
            key: SourceKey::new(source.key)
                .ok_or_else(|| semantic_invalid("native source key is absent"))?,
            identity,
            format: semantic_source_format(source.format)?,
            decoded_byte_len: source.decoded_length,
            content_sha256: source.hash,
            coordinates: match source.coordinate_kind {
                1 => SourceCoordinates::DecodedUtf8Bytes,
                COORD_NATIVE_NORMALIZED_BYTES => SourceCoordinates::NativeNormalizedBytes,
                _ => {
                    return Err(semantic_invalid(
                        "native source coordinate discriminator is unknown",
                    ));
                }
            },
        });
    }

    let mut typed_spans = Vec::new();
    typed_spans
        .try_reserve_exact(spans.len())
        .map_err(semantic_allocation)?;
    for span in spans {
        let line_columns = match span.line_columns {
            None => None,
            Some((line, column, end_line, end_column)) => {
                let start = LineColumn::new(line, column)
                    .ok_or_else(|| semantic_invalid("native source span start is invalid"))?;
                let end = match (end_line, end_column) {
                    (0, 0) => None,
                    (0, _) | (_, 0) => {
                        return Err(semantic_invalid("native source span end is incomplete"));
                    }
                    (line, column) => Some(
                        LineColumn::new(line, column)
                            .ok_or_else(|| semantic_invalid("native source span end is invalid"))?,
                    ),
                };
                Some(LineColumns::new(start, end))
            }
        };
        typed_spans.push(SourceSpan {
            source: SourceKey::new(span.source)
                .ok_or_else(|| semantic_invalid("native span source key is absent"))?,
            line_columns,
            byte_range: span.byte_range,
        });
    }

    let mut typed_provenances = Vec::new();
    typed_provenances
        .try_reserve_exact(provenances.len())
        .map_err(semantic_allocation)?;
    for provenance in provenances {
        typed_provenances.push(match provenance {
            OwnedProvenance::Authored { span } => Provenance::Authored {
                span: SpanKey::new(span)
                    .ok_or_else(|| semantic_invalid("native authored span key is absent"))?,
            },
            OwnedProvenance::Generated { trigger_span } => Provenance::Generated {
                trigger: trigger_span
                    .map(|span| {
                        SpanKey::new(span).ok_or_else(|| {
                            semantic_invalid("native generated trigger span key is absent")
                        })
                    })
                    .transpose()?,
            },
            OwnedProvenance::Unknown => Provenance::Unknown,
        });
    }

    let mut typed_owners = Vec::new();
    typed_owners
        .try_reserve_exact(owners.len())
        .map_err(semantic_allocation)?;
    for owner in owners {
        let kind = match owner.kind {
            1 => ContentOwnerKind::Document,
            2 => ContentOwnerKind::Section,
            3 => ContentOwnerKind::Paragraph,
            4 => ContentOwnerKind::ListItem,
            5 => ContentOwnerKind::DefinitionItem,
            6 => ContentOwnerKind::TableCell,
            7 => ContentOwnerKind::FixedDisplay,
            _ => return Err(semantic_invalid("native content owner kind is unknown")),
        };
        typed_owners.push(ContentOwner {
            key: OwnerKey::new(owner.key)
                .ok_or_else(|| semantic_invalid("native owner key is absent"))?,
            kind,
            provenance: ProvenanceKey::new(owner.provenance)
                .ok_or_else(|| semantic_invalid("native owner provenance key is absent"))?,
        });
    }

    let mut typed_roots = Vec::new();
    typed_roots
        .try_reserve_exact(content_roots.len())
        .map_err(semantic_allocation)?;
    for root in content_roots {
        let kind = match root.kind {
            1 => ContentRootKind::Heading,
            2 => ContentRootKind::Term,
            3 => ContentRootKind::Body,
            4 => ContentRootKind::Cell,
            5 => ContentRootKind::FixedBody,
            _ => return Err(semantic_invalid("native content root kind is unknown")),
        };
        typed_roots.push(ContentRoot {
            key: ContentRootKey::new(root.key)
                .ok_or_else(|| semantic_invalid("native content root key is absent"))?,
            owner: OwnerKey::new(root.owner)
                .ok_or_else(|| semantic_invalid("native content root owner key is absent"))?,
            ordinal: root.ordinal,
            kind,
            provenance: ProvenanceKey::new(root.provenance)
                .ok_or_else(|| semantic_invalid("native content root provenance key is absent"))?,
        });
    }

    let mut typed_atoms = Vec::new();
    typed_atoms
        .try_reserve_exact(content_atoms.len())
        .map_err(semantic_allocation)?;
    for atom in content_atoms {
        let kind = match atom.kind {
            ATOM_TEXT => ContentAtomKind::Text {
                text: atom.text,
                display_override: atom.display_override,
            },
            ATOM_WHITESPACE => ContentAtomKind::Whitespace {
                text: atom.text,
                display_override: atom.display_override,
                breakable: atom.whitespace_breakable,
            },
            ATOM_BREAK_OPPORTUNITY => ContentAtomKind::BreakOpportunity,
            ATOM_HARD_BREAK => ContentAtomKind::HardBreak,
            _ => return Err(semantic_invalid("native content atom kind is unknown")),
        };
        let role = atom
            .role
            .map(|role| match role {
                1 => Ok(NativeRole::Flag),
                2 => Ok(NativeRole::EnvironmentVariable),
                3 => Ok(NativeRole::Argument),
                4 => Ok(NativeRole::CommandOrDirective),
                5 => Ok(NativeRole::Path),
                _ => Err(semantic_invalid("native content role is unknown")),
            })
            .transpose()?;
        typed_atoms.push(ContentAtom {
            key: ContentAtomKey::new(atom.key)
                .ok_or_else(|| semantic_invalid("native content atom key is absent"))?,
            root: ContentRootKey::new(atom.root)
                .ok_or_else(|| semantic_invalid("native content atom root key is absent"))?,
            ordinal: atom.ordinal,
            owner: OwnerKey::new(atom.owner)
                .ok_or_else(|| semantic_invalid("native content atom owner key is absent"))?,
            kind,
            style: StructuredStyle::from_flags([
                atom.style_flags & 1 != 0,
                atom.style_flags & 2 != 0,
                atom.style_flags & 4 != 0,
                atom.style_flags & 8 != 0,
            ]),
            role,
            link: atom
                .link
                .map(|link| {
                    LinkOccurrenceKey::new(link)
                        .ok_or_else(|| semantic_invalid("native atom link key is absent"))
                })
                .transpose()?,
            provenance: ProvenanceKey::new(atom.provenance)
                .ok_or_else(|| semantic_invalid("native atom provenance key is absent"))?,
        });
    }

    let mut typed_refs = Vec::new();
    typed_refs
        .try_reserve_exact(content_refs.len())
        .map_err(semantic_allocation)?;
    for content_ref in content_refs {
        typed_refs.push(ContentRef {
            atom: ContentAtomKey::new(content_ref.atom)
                .ok_or_else(|| semantic_invalid("native content reference atom key is absent"))?,
            bytes: content_ref.bytes,
        });
    }

    let mut typed_links = Vec::new();
    typed_links
        .try_reserve_exact(links.len())
        .map_err(semantic_allocation)?;
    for link in links {
        let target = match (link.target_kind, link.target_b) {
            (1, None) => NativeLinkTarget::External(link.target_a),
            (2, None) => NativeLinkTarget::Email(link.target_a),
            (3, None) => NativeLinkTarget::Document(link.target_a),
            (4, Some(section)) => NativeLinkTarget::Manual {
                name: link.target_a,
                section,
            },
            (5, None) => NativeLinkTarget::Section(link.target_a),
            _ => return Err(semantic_invalid("native link target shape is invalid")),
        };
        let first = link
            .first_label_ref
            .checked_sub(1)
            .ok_or_else(|| semantic_invalid("native link label reference key is absent"))?;
        let start = usize::try_from(first)
            .map_err(|_| semantic_invalid("native link label start does not fit usize"))?;
        let count = usize::try_from(link.label_ref_count)
            .map_err(|_| semantic_invalid("native link label count does not fit usize"))?;
        let end = start
            .checked_add(count)
            .ok_or_else(|| semantic_invalid("native link label range overflows usize"))?;
        typed_links.push(LinkOccurrence {
            key: LinkOccurrenceKey::new(link.key)
                .ok_or_else(|| semantic_invalid("native link key is absent"))?,
            owner: OwnerKey::new(link.owner)
                .ok_or_else(|| semantic_invalid("native link owner key is absent"))?,
            target,
            title: link.title,
            label_refs: start..end,
            provenance: ProvenanceKey::new(link.provenance)
                .ok_or_else(|| semantic_invalid("native link provenance key is absent"))?,
        });
    }

    let mut typed_blocks = Vec::new();
    typed_blocks
        .try_reserve_exact(blocks.len())
        .map_err(semantic_allocation)?;
    for block in blocks {
        let kind = match block.kind {
            BLOCK_HEADING => NativeBlockKind::Heading,
            BLOCK_PARAGRAPH => NativeBlockKind::Paragraph,
            BLOCK_LIST => NativeBlockKind::List,
            BLOCK_DEFINITION_LIST => NativeBlockKind::DefinitionList,
            BLOCK_TABLE => NativeBlockKind::Table,
            BLOCK_INDENTED => NativeBlockKind::Indented,
            BLOCK_FIXED_DISPLAY => NativeBlockKind::FixedDisplay,
            BLOCK_VERTICAL_SPACE => NativeBlockKind::VerticalSpace,
            BLOCK_THEMATIC_BREAK => NativeBlockKind::ThematicBreak,
            _ => return Err(semantic_invalid("native block kind is unknown")),
        };
        typed_blocks.push(NativeBlock {
            key: NativeBlockKey::new(block.key)
                .ok_or_else(|| semantic_invalid("native block key is absent"))?,
            owner: OwnerKey::new(block.owner)
                .ok_or_else(|| semantic_invalid("native block owner key is absent"))?,
            kind,
            parent: block
                .parent
                .map(|parent| {
                    NativeBlockKey::new(parent)
                        .ok_or_else(|| semantic_invalid("native block parent key is absent"))
                })
                .transpose()?,
            ordinal: block.ordinal,
            provenance: ProvenanceKey::new(block.provenance)
                .ok_or_else(|| semantic_invalid("native block provenance key is absent"))?,
            root: block
                .root
                .map(|root| {
                    ContentRootKey::new(root)
                        .ok_or_else(|| semantic_invalid("native block root key is absent"))
                })
                .transpose()?,
        });
    }

    let mut typed_diagnostics = Vec::new();
    typed_diagnostics
        .try_reserve_exact(diagnostics.len())
        .map_err(semantic_allocation)?;
    for diagnostic in diagnostics {
        let level = match diagnostic.level {
            1 => StructuredDiagnosticLevel::Style,
            2 => StructuredDiagnosticLevel::Warning,
            3 => StructuredDiagnosticLevel::Error,
            4 => StructuredDiagnosticLevel::Unsupported,
            _ => return Err(semantic_invalid("native diagnostic level is unknown")),
        };
        typed_diagnostics.push(NativeDiagnostic {
            level,
            code: StructuredDiagnosticCode::from_native_ordinal(diagnostic.code)
                .ok_or_else(|| semantic_invalid("native diagnostic code is unknown"))?,
            message: diagnostic.message,
            span: diagnostic
                .span
                .map(|span| {
                    SpanKey::new(span)
                        .ok_or_else(|| semantic_invalid("native diagnostic span key is absent"))
                })
                .transpose()?,
            owner: diagnostic
                .owner
                .map(|owner| {
                    OwnerKey::new(owner)
                        .ok_or_else(|| semantic_invalid("native diagnostic owner key is absent"))
                })
                .transpose()?,
        });
    }

    Ok(StructuredDocument {
        root_source,
        profile,
        width,
        metadata,
        sources: typed_sources,
        spans: typed_spans,
        provenances: typed_provenances,
        owners: typed_owners,
        content_roots: typed_roots,
        content_atoms: typed_atoms,
        content_refs: typed_refs,
        links: typed_links,
        blocks: typed_blocks,
        diagnostics: typed_diagnostics,
    })
}

fn semantic_source_format(
    format: u32,
) -> Result<crate::structured::SourceFormat, crate::structured::StructuredError> {
    match format {
        FORMAT_MAN => Ok(crate::structured::SourceFormat::Man),
        FORMAT_MDOC => Ok(crate::structured::SourceFormat::Mdoc),
        _ => Err(semantic_invalid("native source format is unknown")),
    }
}

fn semantic_invalid(message: &str) -> crate::structured::StructuredError {
    crate::structured::StructuredError::new(
        crate::structured::StructuredErrorKind::InvalidResult,
        crate::structured::StructuredStage::Check,
        None,
        0,
        0,
        message,
    )
}

fn semantic_allocation(_: std::collections::TryReserveError) -> crate::structured::StructuredError {
    crate::structured::StructuredError::new(
        crate::structured::StructuredErrorKind::BuilderAllocation,
        crate::structured::StructuredStage::Check,
        None,
        0,
        0,
        "structured semantic transfer allocation failed",
    )
}

fn semantic_error(error: &NativeStructuredError) -> crate::structured::StructuredError {
    use crate::structured::{
        StructuredError, StructuredErrorKind, StructuredLimitKind, StructuredStage,
    };

    let kind = match error.status {
        STATUS_INVALID_INPUT => StructuredErrorKind::InvalidInput,
        STATUS_REENTRANT => StructuredErrorKind::Reentrant,
        STATUS_BUDGET => StructuredErrorKind::Budget,
        STATUS_BUILDER_ALLOC => StructuredErrorKind::BuilderAllocation,
        STATUS_NATIVE => StructuredErrorKind::Native,
        STATUS_RELATION => StructuredErrorKind::InvalidResult,
        STATUS_UNSUPPORTED => StructuredErrorKind::Unsupported,
        _ => return semantic_invalid("native failure status discriminator is unknown"),
    };
    let stage = match error.stage {
        1 => StructuredStage::Marshal,
        2 => StructuredStage::Resolve,
        3 => StructuredStage::Parse,
        4 => StructuredStage::Render,
        5 => StructuredStage::Finalize,
        6 => StructuredStage::Check,
        _ => return semantic_invalid("native failure stage discriminator is unknown"),
    };
    let limit = match error.limit_kind {
        0 => None,
        1 => Some(StructuredLimitKind::InputSources),
        2 => Some(StructuredLimitKind::Sources),
        3 => Some(StructuredLimitKind::SourcePathBytes),
        4 => Some(StructuredLimitKind::DecodedSourceBytesPerSource),
        5 => Some(StructuredLimitKind::DecodedSourceBytesTotal),
        6 => Some(StructuredLimitKind::SourceMapEntries),
        7 => Some(StructuredLimitKind::SourceMapBytes),
        8 => Some(StructuredLimitKind::BuilderOperations),
        9 => Some(StructuredLimitKind::BuilderAllocatedBytes),
        10 => Some(StructuredLimitKind::ContentBytes),
        11 => Some(StructuredLimitKind::Owners),
        12 => Some(StructuredLimitKind::Blocks),
        13 => Some(StructuredLimitKind::ContentAtoms),
        14 => Some(StructuredLimitKind::ContentRefs),
        15 => Some(StructuredLimitKind::ContentPoints),
        16 => Some(StructuredLimitKind::Links),
        17 => Some(StructuredLimitKind::Tables),
        18 => Some(StructuredLimitKind::TableRows),
        19 => Some(StructuredLimitKind::TableCells),
        20 => Some(StructuredLimitKind::FixedViews),
        21 => Some(StructuredLimitKind::FixedLines),
        22 => Some(StructuredLimitKind::Placements),
        23 => Some(StructuredLimitKind::Decorations),
        24 => Some(StructuredLimitKind::Forms),
        25 => Some(StructuredLimitKind::NameHints),
        26 => Some(StructuredLimitKind::Relations),
        27 => Some(StructuredLimitKind::ConnectionAtoms),
        28 => Some(StructuredLimitKind::AnnotationRuns),
        29 => Some(StructuredLimitKind::AnnotationMutations),
        30 => Some(StructuredLimitKind::RelationEdges),
        31 => Some(StructuredLimitKind::Diagnostics),
        32 => Some(StructuredLimitKind::TransferObjects),
        33 => Some(StructuredLimitKind::TransferEdges),
        34 => Some(StructuredLimitKind::TransferBytes),
        35 => Some(StructuredLimitKind::NestingDepth),
        36 => Some(StructuredLimitKind::IncludeDepth),
        _ => return semantic_invalid("native failure limit discriminator is unknown"),
    };
    StructuredError::new(
        kind,
        stage,
        limit,
        error.observed,
        error.allowed,
        format!("native structured rendering failed at {stage:?} with {kind:?}"),
    )
}

fn raw_limits(limits: &crate::structured::StructuredLimits) -> Limits {
    Limits {
        max_input_sources: limits.max_input_sources,
        max_sources: limits.max_sources,
        max_source_path_bytes: limits.max_source_path_bytes,
        max_decoded_source_bytes_per_source: limits.max_decoded_source_bytes_per_source,
        max_decoded_source_bytes_total: limits.max_decoded_source_bytes_total,
        max_source_map_entries: limits.max_source_map_entries,
        max_source_map_bytes: limits.max_source_map_bytes,
        max_builder_operations: limits.max_builder_operations,
        max_builder_allocated_bytes: limits.max_builder_allocated_bytes,
        max_content_bytes: limits.max_content_bytes,
        max_owners: limits.max_owners,
        max_blocks: limits.max_blocks,
        max_content_atoms: limits.max_content_atoms,
        max_content_refs: limits.max_content_refs,
        max_content_points: limits.max_content_points,
        max_links: limits.max_links,
        max_tables: limits.max_tables,
        max_table_rows: limits.max_table_rows,
        max_table_cells: limits.max_table_cells,
        max_fixed_views: limits.max_fixed_views,
        max_fixed_lines: limits.max_fixed_lines,
        max_placements: limits.max_placements,
        max_decorations: limits.max_decorations,
        max_forms: limits.max_forms,
        max_name_hints: limits.max_name_hints,
        max_relations: limits.max_relations,
        max_connection_atoms: limits.max_connection_atoms,
        max_annotation_runs: limits.max_annotation_runs,
        max_annotation_mutations: limits.max_annotation_mutations,
        max_relation_edges: limits.max_relation_edges,
        max_diagnostics: limits.max_diagnostics,
        max_transfer_objects: limits.max_transfer_objects,
        max_transfer_edges: limits.max_transfer_edges,
        max_transfer_bytes: limits.max_transfer_bytes,
        max_nesting_depth: limits.max_nesting_depth,
        max_include_depth: limits.max_include_depth,
        reserved: 0,
    }
}

fn error_from_failure(
    status: u32,
    failure: FailureView,
) -> Result<NativeStructuredError, NativeStructuredError> {
    if !(STATUS_INVALID_INPUT..=STATUS_UNSUPPORTED).contains(&status)
        || failure.status != status
        || !(1..=6).contains(&failure.stage)
        || failure.limit_kind > 36
        || failure.reserved != 0
        || (status == STATUS_BUDGET
            && (failure.limit_kind == 0 || failure.observed <= failure.allowed))
        || (status != STATUS_BUDGET && failure.limit_kind != 0)
    {
        return Err(relation_error());
    }
    Ok(NativeStructuredError {
        status,
        stage: failure.stage,
        limit_kind: failure.limit_kind,
        observed: failure.observed,
        allowed: failure.allowed,
    })
}

#[allow(clippy::too_many_lines)] // Keeps the frozen C-to-owned transfer audit in one sequence.
fn copy_structured_document(
    handle: &ResultHandle,
    view: &ResultView,
    limits: &Limits,
) -> Result<OwnedStructuredDocument, NativeStructuredError> {
    if !(PROFILE_UTF8..=PROFILE_ASCII).contains(&view.profile)
        || view.width == 0
        || view.reserved != 0
    {
        return Err(relation_error());
    }
    let slices = StructuredSlices {
        sources: checked_slice::<SourceView>(view.sources, handle)?,
        spans: checked_slice::<SpanView>(view.spans, handle)?,
        provenances: checked_slice::<ProvenanceView>(view.provenances, handle)?,
        owners: checked_slice::<OwnerView>(view.owners, handle)?,
        content_roots: checked_slice::<ContentRootView>(view.content_roots, handle)?,
        content_atoms: checked_slice::<ContentAtomView>(view.content_atoms, handle)?,
        content_refs: checked_slice::<ContentRefView>(view.content_refs, handle)?,
        content_points: checked_slice::<ContentPointView>(view.content_points, handle)?,
        links: checked_slice::<LinkView>(view.links, handle)?,
        blocks: checked_slice::<BlockView>(view.blocks, handle)?,
        tables: checked_slice::<TableView>(view.tables, handle)?,
        table_rows: checked_slice::<TableRowView>(view.table_rows, handle)?,
        table_cells: checked_slice::<TableCellView>(view.table_cells, handle)?,
        fixed_views: checked_slice::<FixedView>(view.fixed_views, handle)?,
        fixed_lines: checked_slice::<FixedLineView>(view.fixed_lines, handle)?,
        placements: checked_slice::<PlacementView>(view.placements, handle)?,
        decorations: checked_slice::<DecorationView>(view.decorations, handle)?,
        forms: checked_slice::<FormView>(view.forms, handle)?,
        name_hints: checked_slice::<NameHintView>(view.name_hints, handle)?,
        relations: checked_slice::<RelationView>(view.relations, handle)?,
        diagnostics: checked_slice::<DiagnosticView>(view.diagnostics, handle)?,
    };
    validate_metadata(view.metadata)?;
    transfer_preflight(view, &slices, limits)?;
    validate_structured_relations(view, &slices)?;

    let mut owned_sources = Vec::new();
    let mut owned_spans = Vec::new();
    let mut owned_provenances = Vec::new();
    let mut owned_owners = Vec::new();
    let mut owned_roots = Vec::new();
    let mut owned_atoms = Vec::new();
    let mut owned_refs = Vec::new();
    let mut owned_links = Vec::new();
    let mut owned_blocks = Vec::new();
    let mut owned_diagnostics = Vec::new();
    owned_sources
        .try_reserve_exact(slices.sources.len())
        .map_err(alloc_error)?;
    owned_spans
        .try_reserve_exact(slices.spans.len())
        .map_err(alloc_error)?;
    owned_provenances
        .try_reserve_exact(slices.provenances.len())
        .map_err(alloc_error)?;
    owned_owners
        .try_reserve_exact(slices.owners.len())
        .map_err(alloc_error)?;
    owned_roots
        .try_reserve_exact(slices.content_roots.len())
        .map_err(alloc_error)?;
    owned_atoms
        .try_reserve_exact(slices.content_atoms.len())
        .map_err(alloc_error)?;
    owned_refs
        .try_reserve_exact(slices.content_refs.len())
        .map_err(alloc_error)?;
    owned_links
        .try_reserve_exact(slices.links.len())
        .map_err(alloc_error)?;
    owned_blocks
        .try_reserve_exact(slices.blocks.len())
        .map_err(alloc_error)?;
    owned_diagnostics
        .try_reserve_exact(slices.diagnostics.len())
        .map_err(alloc_error)?;

    for source in slices.sources {
        owned_sources.push(OwnedSource {
            key: source.key,
            identity_kind: source.identity_kind,
            format: source.format,
            coordinate_kind: source.coordinate_kind,
            logical_name: copy_string(source.logical_name)?,
            decoded_length: source.decoded_length,
            hash: (source.hash_present == 1).then_some(source.hash),
        });
    }

    for span in slices.spans {
        owned_spans.push(OwnedSpan {
            source: span.source,
            line_columns: (span.line_column_present == 1).then_some((
                span.line_start,
                span.column_start,
                span.line_end,
                span.column_end,
            )),
            byte_range: (span.byte_range_present == 1).then_some(span.byte_start..span.byte_end),
        });
    }

    for provenance in slices.provenances {
        owned_provenances.push(match provenance.kind {
            PROVENANCE_AUTHORED => OwnedProvenance::Authored {
                span: provenance.authored_span,
            },
            PROVENANCE_GENERATED => OwnedProvenance::Generated {
                trigger_span: (provenance.generated_trigger_span != 0)
                    .then_some(provenance.generated_trigger_span),
            },
            PROVENANCE_UNKNOWN => OwnedProvenance::Unknown,
            _ => unreachable!("validated provenance kind"),
        });
    }

    for owner in slices.owners {
        owned_owners.push(OwnedOwner {
            key: owner.key,
            kind: owner.kind,
            provenance: owner.provenance,
        });
    }
    for root in slices.content_roots {
        owned_roots.push(OwnedContentRoot {
            key: root.key,
            owner: root.owner,
            ordinal: root.ordinal,
            kind: root.kind,
            provenance: root.provenance,
        });
    }
    for atom in slices.content_atoms {
        owned_atoms.push(OwnedContentAtom {
            key: atom.key,
            root: atom.root,
            ordinal: atom.ordinal,
            owner: atom.owner,
            kind: atom.kind,
            style_flags: atom.style_flags,
            role: (atom.role != 0).then_some(atom.role),
            link: (atom.link != 0).then_some(atom.link),
            text: copy_string(atom.text)?,
            display_override: if atom.display_override_present == 1 {
                Some(copy_string(atom.display_override)?)
            } else {
                None
            },
            whitespace_breakable: atom.whitespace_breakable == 1,
            provenance: atom.provenance,
        });
    }
    for content_ref in slices.content_refs {
        owned_refs.push(OwnedContentRef {
            atom: content_ref.atom,
            bytes: content_ref.byte_start..content_ref.byte_end,
        });
    }
    for link in slices.links {
        owned_links.push(OwnedLink {
            key: link.key,
            owner: link.owner,
            target_kind: link.target_kind,
            target_a: copy_string(link.target_a)?,
            target_b: (link.target_b_present == 1)
                .then(|| copy_string(link.target_b))
                .transpose()?,
            title: (link.title_present == 1)
                .then(|| copy_string(link.title))
                .transpose()?,
            first_label_ref: link.first_label_ref,
            label_ref_count: link.label_ref_count,
            provenance: link.provenance,
        });
    }
    for block in slices.blocks {
        owned_blocks.push(OwnedBlock {
            key: block.key,
            owner: block.owner,
            kind: block.kind,
            parent: (block.parent != 0).then_some(block.parent),
            ordinal: block.ordinal,
            provenance: block.provenance,
            root: (block.root != 0).then_some(block.root),
            table: (block.table != 0).then_some(block.table),
            fixed_view: (block.fixed_view != 0).then_some(block.fixed_view),
        });
    }
    for diagnostic in slices.diagnostics {
        owned_diagnostics.push(OwnedDiagnostic {
            level: diagnostic.level,
            code: diagnostic.code,
            message: copy_string(diagnostic.message)?,
            span: (diagnostic.span != 0).then_some(diagnostic.span),
            owner: (diagnostic.owner != 0).then_some(diagnostic.owner),
        });
    }

    Ok(OwnedStructuredDocument {
        root_source: view.root_source,
        profile: view.profile,
        width: view.width,
        metadata: OwnedMetadata {
            macroset: view.metadata.macroset,
            title: copy_optional_string(view.metadata, 1 << 0, view.metadata.title)?,
            section: copy_optional_string(view.metadata, 1 << 1, view.metadata.section)?,
            volume: copy_optional_string(view.metadata, 1 << 2, view.metadata.volume)?,
            operating_system: copy_optional_string(
                view.metadata,
                1 << 3,
                view.metadata.operating_system,
            )?,
            architecture: copy_optional_string(view.metadata, 1 << 4, view.metadata.architecture)?,
            name: copy_optional_string(view.metadata, 1 << 5, view.metadata.name)?,
            date: copy_optional_string(view.metadata, 1 << 6, view.metadata.date)?,
            alias_target: copy_optional_string(view.metadata, 1 << 7, view.metadata.alias_target)?,
            has_body: view.metadata.has_body == 1,
        },
        sources: owned_sources,
        spans: owned_spans,
        provenances: owned_provenances,
        owners: owned_owners,
        content_roots: owned_roots,
        content_atoms: owned_atoms,
        content_refs: owned_refs,
        links: owned_links,
        blocks: owned_blocks,
        diagnostics: owned_diagnostics,
    })
}

fn validate_metadata(metadata: MetadataView) -> Result<(), NativeStructuredError> {
    if !(FORMAT_MAN..=FORMAT_MDOC).contains(&metadata.macroset)
        || metadata.presence_flags & !0xff != 0
        || metadata.has_body > 1
        || metadata.reserved_bytes != [0; 3]
        || metadata.reserved != 0
    {
        return Err(relation_error());
    }
    for (flag, field) in [
        (1 << 0, metadata.title),
        (1 << 1, metadata.section),
        (1 << 2, metadata.volume),
        (1 << 3, metadata.operating_system),
        (1 << 4, metadata.architecture),
        (1 << 5, metadata.name),
        (1 << 6, metadata.date),
        (1 << 7, metadata.alias_target),
    ] {
        validate_utf8_view(field)?;
        if metadata.presence_flags & flag == 0 && (field.len != 0 || !field.ptr.is_null()) {
            return Err(relation_error());
        }
    }
    Ok(())
}

fn valid_span(span: &SpanView) -> bool {
    let line_valid = if span.line_column_present == 0 {
        span.line_start == 0 && span.column_start == 0 && span.line_end == 0 && span.column_end == 0
    } else {
        span.line_start != 0
            && span.column_start != 0
            && ((span.line_end == 0 && span.column_end == 0)
                || (span.line_end >= span.line_start
                    && span.column_end != 0
                    && (span.line_end != span.line_start || span.column_end >= span.column_start)))
    };
    let bytes_valid = span.byte_range_present == 0 && span.byte_start == 0 && span.byte_end == 0;
    line_valid && bytes_valid
}

fn utf8_boundary(view: BytesView, offset: u32) -> bool {
    let Ok(length) = usize::try_from(view.len) else {
        return false;
    };
    let offset = offset as usize;
    if view.ptr.is_null() || offset > length {
        return false;
    }
    let bytes = unsafe { std::slice::from_raw_parts(view.ptr, length) };
    std::str::from_utf8(bytes).is_ok_and(|text| text.is_char_boundary(offset))
}

#[allow(clippy::too_many_lines)]
fn validate_structured_relations(
    view: &ResultView,
    slices: &StructuredSlices<'_>,
) -> Result<(), NativeStructuredError> {
    if !slices.content_points.is_empty()
        || !slices.tables.is_empty()
        || !slices.table_rows.is_empty()
        || !slices.table_cells.is_empty()
        || !slices.fixed_views.is_empty()
        || !slices.fixed_lines.is_empty()
        || !slices.placements.is_empty()
        || !slices.decorations.is_empty()
        || !slices.forms.is_empty()
        || !slices.name_hints.is_empty()
        || !slices.relations.is_empty()
    {
        return Err(relation_error());
    }

    for (index, source) in slices.sources.iter().enumerate() {
        if source.key != dense_key(index)?
            || source.reserved != 0
            || !(1..=3).contains(&source.identity_kind)
            || !(FORMAT_MAN..=FORMAT_MDOC).contains(&source.format)
            || source.coordinate_kind != COORD_NATIVE_NORMALIZED_BYTES
            || source.hash_present > 1
            || source.reserved_bytes != [0; 7]
            || (source.hash_present == 0 && source.hash != [0; 32])
        {
            return Err(relation_error());
        }
    }
    if view.root_source != 1 || slices.sources.is_empty() {
        return Err(relation_error());
    }
    if view.metadata.macroset != slices.sources[view.root_source as usize - 1].format {
        return Err(relation_error());
    }

    for span in slices.spans {
        if span.reserved != 0
            || span.reserved_bytes != [0; 2]
            || span.line_column_present > 1
            || span.byte_range_present > 1
            || span.source == 0
            || span.source as usize > slices.sources.len()
            || !valid_span(span)
        {
            return Err(relation_error());
        }
    }
    for provenance in slices.provenances {
        let active = match provenance.kind {
            PROVENANCE_AUTHORED => {
                provenance.authored_span != 0
                    && provenance.authored_span as usize <= slices.spans.len()
                    && provenance.generated_trigger_span == 0
            }
            PROVENANCE_GENERATED => {
                provenance.authored_span == 0
                    && provenance.generated_trigger_span as usize <= slices.spans.len()
            }
            PROVENANCE_UNKNOWN => {
                provenance.authored_span == 0 && provenance.generated_trigger_span == 0
            }
            _ => false,
        };
        if provenance.reserved != 0 || !active {
            return Err(relation_error());
        }
    }

    for (index, owner) in slices.owners.iter().enumerate() {
        if owner.key != dense_key(index)?
            || !(1..=OWNER_KIND_LAST).contains(&owner.kind)
            || !valid_required_key(owner.provenance, slices.provenances.len())
            || owner.reserved != 0
        {
            return Err(relation_error());
        }
    }

    let mut previous_owner = 0_u32;
    let mut expected_root_ordinal = 0_u32;
    for (index, root) in slices.content_roots.iter().enumerate() {
        if root.owner != previous_owner {
            if root.owner < previous_owner {
                return Err(relation_error());
            }
            previous_owner = root.owner;
            expected_root_ordinal = 0;
        }
        if root.key != dense_key(index)?
            || !valid_required_key(root.owner, slices.owners.len())
            || !(1..=ROOT_KIND_LAST).contains(&root.kind)
            || !valid_required_key(root.provenance, slices.provenances.len())
            || root.reserved != 0
            || root.ordinal != expected_root_ordinal
        {
            return Err(relation_error());
        }
        expected_root_ordinal = expected_root_ordinal
            .checked_add(1)
            .ok_or_else(relation_error)?;
    }

    let mut previous_root = 0_u32;
    let mut expected_atom_ordinal = 0_u32;
    for (index, atom) in slices.content_atoms.iter().enumerate() {
        let root = atom
            .root
            .checked_sub(1)
            .and_then(|index| slices.content_roots.get(index as usize));
        let text_length = validate_utf8_view(atom.text)?;
        let display_valid = match atom.display_override_present {
            0 => atom.display_override.ptr.is_null() && atom.display_override.len == 0,
            1 => {
                atom.display_override.len != 0 && validate_utf8_view(atom.display_override).is_ok()
            }
            _ => false,
        };
        let kind_valid = match atom.kind {
            ATOM_TEXT => text_length != 0 && atom.whitespace_breakable == 0,
            ATOM_WHITESPACE => text_length != 0 && atom.whitespace_breakable <= 1,
            ATOM_BREAK_OPPORTUNITY | ATOM_HARD_BREAK => {
                text_length == 0
                    && atom.display_override_present == 0
                    && atom.whitespace_breakable == 0
                    && atom.style_flags == 0
                    && atom.role == 0
                    && atom.link == 0
            }
            _ => false,
        };
        if atom.root != previous_root {
            previous_root = atom.root;
            expected_atom_ordinal = 0;
        }
        if atom.key != dense_key(index)?
            || root.is_none()
            || root.is_some_and(|root| root.owner != atom.owner)
            || atom.style_flags & !STYLE_MASK != 0
            || atom.role > 5
            || atom.link as usize > slices.links.len()
            || !display_valid
            || !kind_valid
            || atom.display_reserved_bytes != [0; 7]
            || atom.reserved_bytes != [0; 3]
            || !valid_required_key(atom.provenance, slices.provenances.len())
            || atom.reserved != 0
            || atom.ordinal != expected_atom_ordinal
        {
            return Err(relation_error());
        }
        expected_atom_ordinal = expected_atom_ordinal
            .checked_add(1)
            .ok_or_else(relation_error)?;
    }

    for content_ref in slices.content_refs {
        let atom = content_ref
            .atom
            .checked_sub(1)
            .and_then(|index| slices.content_atoms.get(index as usize));
        if content_ref.reserved != 0
            || atom.is_none()
            || atom.is_some_and(|atom| !matches!(atom.kind, ATOM_TEXT | ATOM_WHITESPACE))
            || content_ref.byte_start >= content_ref.byte_end
            || atom.is_some_and(|atom| u64::from(content_ref.byte_end) > atom.text.len)
            || atom.is_some_and(|atom| {
                !utf8_boundary(atom.text, content_ref.byte_start)
                    || !utf8_boundary(atom.text, content_ref.byte_end)
            })
        {
            return Err(relation_error());
        }
    }
    let mut next_label_ref = 0_usize;
    let mut previous_label_atom = 0_u32;
    for (index, link) in slices.links.iter().enumerate() {
        let label_start = link.first_label_ref.checked_sub(1).map(|key| key as usize);
        let label_end =
            label_start.and_then(|start| start.checked_add(link.label_ref_count as usize));
        let target_b_valid = match link.target_b_present {
            0 => link.target_b.ptr.is_null() && link.target_b.len == 0 && link.target_kind != 4,
            1 => {
                validate_utf8_view(link.target_b).is_ok()
                    && link.target_b.len != 0
                    && link.target_kind == 4
            }
            _ => false,
        };
        let title_valid = match link.title_present {
            0 => link.title.ptr.is_null() && link.title.len == 0,
            1 => validate_utf8_view(link.title).is_ok(),
            _ => false,
        };
        if link.key != dense_key(index)?
            || !valid_required_key(link.owner, slices.owners.len())
            || !(1..=5).contains(&link.target_kind)
            || validate_utf8_view(link.target_a).is_err()
            || link.target_a.len == 0
            || !target_b_valid
            || !title_valid
            || link.target_b_reserved_bytes != [0; 7]
            || link.title_reserved_bytes != [0; 7]
            || link.label_ref_count == 0
            || label_start != Some(next_label_ref)
            || label_end.is_none_or(|end| end > slices.content_refs.len())
            || !valid_required_key(link.provenance, slices.provenances.len())
            || link.reserved != 0
        {
            return Err(relation_error());
        }
        for content_ref in &slices.content_refs[label_start.unwrap()..label_end.unwrap()] {
            if content_ref.atom <= previous_label_atom
                || slices.content_atoms[content_ref.atom as usize - 1].link != link.key
            {
                return Err(relation_error());
            }
            previous_label_atom = content_ref.atom;
        }
        next_label_ref = label_end.unwrap();
    }
    if next_label_ref != slices.content_refs.len() {
        return Err(relation_error());
    }
    let mut refs = slices.content_refs.iter();
    for atom in slices.content_atoms.iter().filter(|atom| atom.link != 0) {
        if refs
            .next()
            .is_none_or(|content_ref| content_ref.atom != atom.key)
        {
            return Err(relation_error());
        }
    }
    if refs.next().is_some() {
        return Err(relation_error());
    }

    if slices.blocks.len() != slices.content_roots.len() {
        return Err(relation_error());
    }
    let mut top_level_ordinal = 0_u32;
    let mut current_parent = 0_u32;
    let mut child_ordinal = 0_u32;
    for (index, block) in slices.blocks.iter().enumerate() {
        let parent = (block.parent != 0)
            .then(|| slices.blocks.get(block.parent as usize - 1))
            .flatten();
        let block_root = &slices.content_roots[index];
        let expected_ordinal = if block.parent == 0 {
            let ordinal = top_level_ordinal;
            top_level_ordinal = top_level_ordinal
                .checked_add(1)
                .ok_or_else(relation_error)?;
            current_parent = block.key;
            child_ordinal = 0;
            ordinal
        } else {
            if block.parent != current_parent {
                return Err(relation_error());
            }
            let ordinal = child_ordinal;
            child_ordinal = child_ordinal.checked_add(1).ok_or_else(relation_error)?;
            ordinal
        };
        let payload_valid = match block.kind {
            BLOCK_HEADING => {
                block_root.kind == ROOT_HEADING
                    && block.root == block_root.key
                    && block.table == 0
                    && block.fixed_view == 0
            }
            BLOCK_PARAGRAPH => {
                block_root.kind == ROOT_BODY
                    && block.root == block_root.key
                    && block.table == 0
                    && block.fixed_view == 0
            }
            _ => false,
        };
        if block.key != dense_key(index)?
            || !valid_required_key(block.owner, slices.owners.len())
            || !valid_required_key(block.provenance, slices.provenances.len())
            || block.parent as usize > slices.blocks.len()
            || block.parent == block.key
            || parent.is_some_and(|parent| parent.owner != block.owner)
            || block_root.owner != block.owner
            || !payload_valid
            || block.reserved != 0
            || block.ordinal != expected_ordinal
        {
            return Err(relation_error());
        }
    }

    for diagnostic in slices.diagnostics {
        if diagnostic.reserved != 0
            || !(DIAGNOSTIC_STYLE..=DIAGNOSTIC_UNSUPPORTED).contains(&diagnostic.level)
            || !(1..=DIAGNOSTIC_CODE_NATIVE_LAST).contains(&diagnostic.code)
            || diagnostic.span as usize > slices.spans.len()
            || diagnostic.owner as usize > slices.owners.len()
        {
            return Err(relation_error());
        }
    }

    let has_collected_body = !slices.owners.is_empty()
        || !slices.content_roots.is_empty()
        || !slices.content_atoms.is_empty()
        || !slices.blocks.is_empty();
    if (view.metadata.has_body == 0 && has_collected_body)
        || (view.metadata.has_body == 1
            && (slices.owners.is_empty()
                || slices.content_roots.is_empty()
                || slices.blocks.is_empty()))
    {
        return Err(relation_error());
    }
    Ok(())
}

fn dense_key(index: usize) -> Result<u32, NativeStructuredError> {
    u32::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or_else(relation_error)
}

fn valid_required_key(key: u32, length: usize) -> bool {
    key != 0 && key as usize <= length
}

#[allow(clippy::too_many_lines)] // Mirrors every frozen result table and transfer counter.
fn transfer_preflight(
    view: &ResultView,
    slices: &StructuredSlices<'_>,
    limits: &Limits,
) -> Result<(), NativeStructuredError> {
    // The top-level result view and metadata are records in addition to every
    // typed table row validated below.
    let mut objects = 2_u64;
    let mut edges = 1_u64; // rootSource -> sources
    let mut bytes = 0_u64;
    for count in [
        slices.sources.len(),
        slices.spans.len(),
        slices.provenances.len(),
        slices.owners.len(),
        slices.content_roots.len(),
        slices.content_atoms.len(),
        slices.content_refs.len(),
        slices.content_points.len(),
        slices.links.len(),
        slices.blocks.len(),
        slices.tables.len(),
        slices.table_rows.len(),
        slices.table_cells.len(),
        slices.fixed_views.len(),
        slices.fixed_lines.len(),
        slices.placements.len(),
        slices.decorations.len(),
        slices.forms.len(),
        slices.name_hints.len(),
        slices.relations.len(),
        slices.diagnostics.len(),
    ] {
        objects = objects
            .checked_add(u64::try_from(count).map_err(|_| relation_error())?)
            .ok_or_else(relation_error)?;
    }
    add_edges(&mut edges, slices.spans.len())?;
    for provenance in slices.provenances {
        if provenance.kind == PROVENANCE_AUTHORED
            || (provenance.kind == PROVENANCE_GENERATED && provenance.generated_trigger_span != 0)
        {
            add_edges(&mut edges, 1)?;
        }
    }
    add_edges(&mut edges, slices.owners.len())?;
    add_edges(&mut edges, slices.content_roots.len())?; // owner
    add_edges(&mut edges, slices.content_roots.len())?; // provenance
    for _ in 0..3 {
        add_edges(&mut edges, slices.content_atoms.len())?; // root, owner, provenance
    }
    add_edges(
        &mut edges,
        slices
            .content_atoms
            .iter()
            .filter(|atom| atom.link != 0)
            .count(),
    )?;
    add_edges(&mut edges, slices.content_refs.len())?; // atom
    add_edges(&mut edges, slices.links.len())?; // owner
    add_edges(&mut edges, slices.links.len())?; // provenance
    for link in slices.links {
        add_edges(&mut edges, link.label_ref_count as usize)?;
    }
    add_edges(&mut edges, slices.blocks.len())?; // owner
    add_edges(&mut edges, slices.blocks.len())?; // provenance
    for count in [
        slices
            .blocks
            .iter()
            .filter(|block| block.parent != 0)
            .count(),
        slices.blocks.iter().filter(|block| block.root != 0).count(),
        slices
            .blocks
            .iter()
            .filter(|block| block.table != 0)
            .count(),
        slices
            .blocks
            .iter()
            .filter(|block| block.fixed_view != 0)
            .count(),
    ] {
        add_edges(&mut edges, count)?;
    }
    for diagnostic in slices.diagnostics {
        add_edges(
            &mut edges,
            usize::from(diagnostic.span != 0) + usize::from(diagnostic.owner != 0),
        )?;
    }
    add_transfer_table_bytes::<OwnedSource, crate::structured::SourceRecord, _>(
        &mut bytes,
        slices.sources,
    )?;
    add_transfer_table_bytes::<OwnedSpan, crate::structured::SourceSpan, _>(
        &mut bytes,
        slices.spans,
    )?;
    add_transfer_table_bytes::<OwnedProvenance, crate::structured::Provenance, _>(
        &mut bytes,
        slices.provenances,
    )?;
    add_transfer_table_bytes::<OwnedOwner, crate::structured::ContentOwner, _>(
        &mut bytes,
        slices.owners,
    )?;
    add_transfer_table_bytes::<OwnedContentRoot, crate::structured::ContentRoot, _>(
        &mut bytes,
        slices.content_roots,
    )?;
    add_transfer_table_bytes::<OwnedContentAtom, crate::structured::ContentAtom, _>(
        &mut bytes,
        slices.content_atoms,
    )?;
    add_transfer_table_bytes::<OwnedContentRef, crate::structured::ContentRef, _>(
        &mut bytes,
        slices.content_refs,
    )?;
    add_transfer_table_bytes::<OwnedLink, crate::structured::LinkOccurrence, _>(
        &mut bytes,
        slices.links,
    )?;
    add_transfer_table_bytes::<OwnedBlock, crate::structured::NativeBlock, _>(
        &mut bytes,
        slices.blocks,
    )?;
    add_transfer_table_bytes::<OwnedDiagnostic, crate::structured::NativeDiagnostic, _>(
        &mut bytes,
        slices.diagnostics,
    )?;
    for source in slices.sources {
        bytes = bytes
            .checked_add(validate_utf8_view(source.logical_name)?)
            .ok_or_else(relation_error)?;
    }
    for field in [
        view.metadata.title,
        view.metadata.section,
        view.metadata.volume,
        view.metadata.operating_system,
        view.metadata.architecture,
        view.metadata.name,
        view.metadata.date,
        view.metadata.alias_target,
    ] {
        bytes = bytes
            .checked_add(validate_utf8_view(field)?)
            .ok_or_else(relation_error)?;
    }
    for atom in slices.content_atoms {
        let display_bytes = match atom.display_override_present {
            0 if atom.display_override.ptr.is_null() && atom.display_override.len == 0 => 0,
            1 => validate_utf8_view(atom.display_override)?,
            _ => return Err(relation_error()),
        };
        bytes = bytes
            .checked_add(validate_utf8_view(atom.text)?)
            .and_then(|value| value.checked_add(display_bytes))
            .ok_or_else(relation_error)?;
    }
    for link in slices.links {
        bytes = bytes
            .checked_add(validate_utf8_view(link.target_a)?)
            .and_then(|value| {
                value.checked_add(if link.target_b_present == 1 {
                    validate_utf8_view(link.target_b).ok()?
                } else {
                    0
                })
            })
            .and_then(|value| {
                value.checked_add(if link.title_present == 1 {
                    validate_utf8_view(link.title).ok()?
                } else {
                    0
                })
            })
            .ok_or_else(relation_error)?;
    }
    for diagnostic in slices.diagnostics {
        bytes = bytes
            .checked_add(validate_utf8_view(diagnostic.message)?)
            .ok_or_else(relation_error)?;
    }
    if objects > limits.max_transfer_objects
        || edges > limits.max_transfer_edges
        || bytes > limits.max_transfer_bytes
    {
        return Err(NativeStructuredError {
            status: STATUS_BUDGET,
            stage: 6,
            limit_kind: if objects > limits.max_transfer_objects {
                32
            } else if edges > limits.max_transfer_edges {
                33
            } else {
                34
            },
            observed: if objects > limits.max_transfer_objects {
                objects
            } else if edges > limits.max_transfer_edges {
                edges
            } else {
                bytes
            },
            allowed: if objects > limits.max_transfer_objects {
                limits.max_transfer_objects
            } else if edges > limits.max_transfer_edges {
                limits.max_transfer_edges
            } else {
                limits.max_transfer_bytes
            },
        });
    }
    Ok(())
}

fn add_edges(edges: &mut u64, count: usize) -> Result<(), NativeStructuredError> {
    *edges = edges
        .checked_add(u64::try_from(count).map_err(|_| relation_error())?)
        .ok_or_else(relation_error)?;
    Ok(())
}

fn add_transfer_table_bytes<Raw, Typed, View>(
    bytes: &mut u64,
    rows: &[View],
) -> Result<(), NativeStructuredError> {
    let row_bytes = std::mem::size_of::<Raw>()
        .checked_add(std::mem::size_of::<Typed>())
        .ok_or_else(relation_error)?;
    let allocated = rows
        .len()
        .checked_mul(row_bytes)
        .and_then(|value| u64::try_from(value).ok())
        .ok_or_else(relation_error)?;
    *bytes = bytes.checked_add(allocated).ok_or_else(relation_error)?;
    Ok(())
}

fn validate_utf8_view(view: BytesView) -> Result<u64, NativeStructuredError> {
    if view.len == 0 {
        return if view.ptr.is_null() {
            Ok(0)
        } else {
            Err(relation_error())
        };
    }
    let length = usize::try_from(view.len).map_err(|_| relation_error())?;
    if view.ptr.is_null()
        || length > isize::MAX as usize
        || (view.ptr as usize).checked_add(length - 1).is_none()
    {
        return Err(relation_error());
    }
    let bytes = unsafe { std::slice::from_raw_parts(view.ptr, length) };
    std::str::from_utf8(bytes).map_err(|_| relation_error())?;
    Ok(view.len)
}

#[allow(clippy::needless_lifetimes)] // The explicit lifetime documents handle-bound borrowing.
fn checked_slice<'a, T>(
    view: SliceView,
    _handle: &'a ResultHandle,
) -> Result<&'a [T], NativeStructuredError> {
    if view.stride as usize != std::mem::size_of::<T>() {
        return Err(relation_error());
    }
    if view.count == 0 {
        return if view.ptr.is_null() {
            Ok(&[])
        } else {
            Err(relation_error())
        };
    }
    if view.ptr.is_null() || !(view.ptr as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return Err(relation_error());
    }
    let count = view.count as usize;
    let bytes = count
        .checked_mul(std::mem::size_of::<T>())
        .ok_or_else(relation_error)?;
    if bytes > isize::MAX as usize
        || (bytes != 0 && (view.ptr as usize).checked_add(bytes - 1).is_none())
    {
        return Err(relation_error());
    }
    Ok(unsafe { std::slice::from_raw_parts(view.ptr.cast(), count) })
}

fn copy_string(view: BytesView) -> Result<String, NativeStructuredError> {
    if view.len == 0 {
        return if view.ptr.is_null() {
            Ok(String::new())
        } else {
            Err(relation_error())
        };
    }
    let length = usize::try_from(view.len).map_err(|_| relation_error())?;
    if view.ptr.is_null() {
        return Err(relation_error());
    }
    let bytes = unsafe { std::slice::from_raw_parts(view.ptr, length) };
    let text = std::str::from_utf8(bytes).map_err(|_| relation_error())?;
    let mut owned = String::new();
    owned.try_reserve_exact(text.len()).map_err(alloc_error)?;
    owned.push_str(text);
    Ok(owned)
}

fn copy_optional_string(
    metadata: MetadataView,
    flag: u32,
    view: BytesView,
) -> Result<Option<String>, NativeStructuredError> {
    if metadata.presence_flags & flag == 0 {
        return if view.ptr.is_null() && view.len == 0 {
            Ok(None)
        } else {
            Err(relation_error())
        };
    }
    copy_string(view).map(Some)
}

fn relation_error() -> NativeStructuredError {
    NativeStructuredError {
        status: STATUS_RELATION,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    }
}

fn alloc_error(_: std::collections::TryReserveError) -> NativeStructuredError {
    NativeStructuredError {
        status: STATUS_BUILDER_ALLOC,
        stage: 6,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_failure_mapping_rejects_unknown_discriminants() {
        use crate::structured::{StructuredErrorKind, StructuredStage};

        for error in [
            NativeStructuredError {
                status: 99,
                stage: 6,
                limit_kind: 0,
                observed: 0,
                allowed: 0,
            },
            NativeStructuredError {
                status: STATUS_NATIVE,
                stage: 99,
                limit_kind: 0,
                observed: 0,
                allowed: 0,
            },
            NativeStructuredError {
                status: STATUS_BUDGET,
                stage: 6,
                limit_kind: 99,
                observed: 2,
                allowed: 1,
            },
        ] {
            let mapped = semantic_error(&error);
            assert_eq!(mapped.kind(), StructuredErrorKind::InvalidResult);
            assert_eq!(mapped.stage(), StructuredStage::Check);
            assert_eq!(mapped.limit(), None);
        }
    }

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

    #[test]
    #[allow(clippy::too_many_lines)] // One complete synthetic v1 prose result.
    fn prose_transfer_preserves_descriptor_keys_and_owned_text() {
        let source_name = String::from("root.1");
        let atom_text = String::from("owned body");
        let diagnostic_text = String::from("owned diagnostic");
        let sources = [SourceView {
            key: 1,
            identity_kind: IDENTITY_BUNDLE_MEMBER,
            format: FORMAT_MAN,
            coordinate_kind: COORD_NATIVE_NORMALIZED_BYTES,
            logical_name: BytesView {
                ptr: source_name.as_ptr(),
                len: source_name.len() as u64,
            },
            decoded_length: 64,
            ..SourceView::default()
        }];
        let spans = [SpanView {
            line_column_present: 1,
            source: 1,
            line_start: 1,
            column_start: 1,
            ..SpanView::default()
        }];
        let provenances = [ProvenanceView {
            kind: PROVENANCE_AUTHORED,
            authored_span: 1,
            ..ProvenanceView::default()
        }];
        let owners = [OwnerView {
            key: 1,
            kind: 1,
            provenance: 1,
            reserved: 0,
        }];
        let roots = [ContentRootView {
            key: 1,
            owner: 1,
            ordinal: 0,
            kind: ROOT_BODY,
            provenance: 1,
            reserved: 0,
        }];
        let atoms = [ContentAtomView {
            key: 1,
            root: 1,
            ordinal: 0,
            owner: 1,
            kind: ATOM_TEXT,
            text: BytesView {
                ptr: atom_text.as_ptr(),
                len: atom_text.len() as u64,
            },
            provenance: 1,
            ..ContentAtomView::default()
        }];
        let blocks = [BlockView {
            key: 1,
            owner: 1,
            kind: BLOCK_PARAGRAPH,
            ordinal: 0,
            provenance: 1,
            root: 1,
            ..BlockView::default()
        }];
        let diagnostics = [DiagnosticView {
            level: DIAGNOSTIC_STYLE,
            code: 1,
            message: BytesView {
                ptr: diagnostic_text.as_ptr(),
                len: diagnostic_text.len() as u64,
            },
            span: 1,
            owner: 1,
            reserved: 0,
        }];
        let empty_refs: [ContentRefView; 0] = [];
        let empty_points: [ContentPointView; 0] = [];
        let empty_links: [LinkView; 0] = [];
        let empty_tables: [TableView; 0] = [];
        let empty_rows: [TableRowView; 0] = [];
        let empty_cells: [TableCellView; 0] = [];
        let empty_fixed: [FixedView; 0] = [];
        let empty_fixed_lines: [FixedLineView; 0] = [];
        let empty_placements: [PlacementView; 0] = [];
        let empty_decorations: [DecorationView; 0] = [];
        let empty_forms: [FormView; 0] = [];
        let empty_hints: [NameHintView; 0] = [];
        let empty_relations: [RelationView; 0] = [];
        let view = ResultView {
            root_source: 1,
            profile: PROFILE_UTF8,
            width: 78,
            metadata: MetadataView {
                macroset: FORMAT_MAN,
                has_body: 1,
                ..MetadataView::default()
            },
            sources: abi_slice(&sources),
            spans: abi_slice(&spans),
            provenances: abi_slice(&provenances),
            owners: abi_slice(&owners),
            content_roots: abi_slice(&roots),
            content_atoms: abi_slice(&atoms),
            content_refs: abi_slice(&empty_refs),
            content_points: abi_slice(&empty_points),
            links: abi_slice(&empty_links),
            blocks: abi_slice(&blocks),
            tables: abi_slice(&empty_tables),
            table_rows: abi_slice(&empty_rows),
            table_cells: abi_slice(&empty_cells),
            fixed_views: abi_slice(&empty_fixed),
            fixed_lines: abi_slice(&empty_fixed_lines),
            placements: abi_slice(&empty_placements),
            decorations: abi_slice(&empty_decorations),
            forms: abi_slice(&empty_forms),
            name_hints: abi_slice(&empty_hints),
            relations: abi_slice(&empty_relations),
            diagnostics: abi_slice(&diagnostics),
            reserved: 0,
        };
        let handle = std::mem::ManuallyDrop::new(ResultHandle(NonNull::dangling()));
        let empty_sources: [SourceView; 0] = [];
        let mut missing_source = view;
        missing_source.sources = abi_slice(&empty_sources);
        let missing_source_error =
            copy_structured_document(&handle, &missing_source, &Limits::default())
                .expect_err("a root cannot exist without the first source record");
        assert_eq!(missing_source_error.status, STATUS_RELATION);

        let secondary_name = String::from("included.1");
        let two_sources = [
            sources[0],
            SourceView {
                key: 2,
                identity_kind: IDENTITY_BUNDLE_MEMBER,
                format: FORMAT_MAN,
                coordinate_kind: COORD_NATIVE_NORMALIZED_BYTES,
                logical_name: BytesView {
                    ptr: secondary_name.as_ptr(),
                    len: secondary_name.len() as u64,
                },
                decoded_length: 32,
                ..SourceView::default()
            },
        ];
        let mut nonfirst_root = view;
        nonfirst_root.root_source = 2;
        nonfirst_root.sources = abi_slice(&two_sources);
        let root_error = copy_structured_document(&handle, &nonfirst_root, &Limits::default())
            .expect_err("the native root must be the first registered source");
        assert_eq!(root_error.status, STATUS_RELATION);

        let mut exact_byte_spans = spans;
        exact_byte_spans[0].byte_range_present = 1;
        exact_byte_spans[0].byte_end = 1;
        let mut exact_byte_view = view;
        exact_byte_view.spans = abi_slice(&exact_byte_spans);
        let span_error = copy_structured_document(&handle, &exact_byte_view, &Limits::default())
            .expect_err("normalized native coordinates cannot claim exact byte offsets");
        assert_eq!(span_error.status, STATUS_RELATION);

        let object_budget = Limits {
            max_transfer_objects: 9,
            ..Limits::default()
        };
        let object_error = copy_structured_document(&handle, &view, &object_budget)
            .expect_err("ten transferred records exceed nine");
        assert_eq!((object_error.limit_kind, object_error.observed), (32, 10));
        let edge_budget = Limits {
            max_transfer_edges: 13,
            ..Limits::default()
        };
        let edge_error = copy_structured_document(&handle, &view, &edge_budget)
            .expect_err("fourteen transferred relations exceed thirteen");
        assert_eq!((edge_error.limit_kind, edge_error.observed), (33, 14));
        let table_bytes = std::mem::size_of::<OwnedSource>()
            + std::mem::size_of::<crate::structured::SourceRecord>()
            + std::mem::size_of::<OwnedSpan>()
            + std::mem::size_of::<crate::structured::SourceSpan>()
            + std::mem::size_of::<OwnedProvenance>()
            + std::mem::size_of::<crate::structured::Provenance>()
            + std::mem::size_of::<OwnedOwner>()
            + std::mem::size_of::<crate::structured::ContentOwner>()
            + std::mem::size_of::<OwnedContentRoot>()
            + std::mem::size_of::<crate::structured::ContentRoot>()
            + std::mem::size_of::<OwnedContentAtom>()
            + std::mem::size_of::<crate::structured::ContentAtom>()
            + std::mem::size_of::<OwnedBlock>()
            + std::mem::size_of::<crate::structured::NativeBlock>()
            + std::mem::size_of::<OwnedDiagnostic>()
            + std::mem::size_of::<crate::structured::NativeDiagnostic>();
        let expected_bytes = u64::try_from(
            source_name.len() + atom_text.len() + diagnostic_text.len() + table_bytes,
        )
        .unwrap();
        let byte_budget = Limits {
            max_transfer_bytes: expected_bytes - 1,
            ..Limits::default()
        };
        let byte_error = copy_structured_document(&handle, &view, &byte_budget)
            .expect_err("all copied strings are charged before allocation");
        assert_eq!(
            (byte_error.limit_kind, byte_error.observed),
            (34, expected_bytes)
        );
        let owned = copy_structured_document(&handle, &view, &Limits::default())
            .expect("valid prose view transfers");
        drop(source_name);
        drop(atom_text);
        drop(diagnostic_text);

        assert_eq!(owned.spans.len(), 1);
        assert_eq!(owned.provenances, [OwnedProvenance::Authored { span: 1 }]);
        assert_eq!(owned.content_atoms[0].text, "owned body");
        assert_eq!(owned.diagnostics[0].span, Some(1));
        assert_eq!(owned.diagnostics[0].owner, Some(1));
        assert_eq!(owned.diagnostics[0].message, "owned diagnostic");
        assert_eq!(
            diagnostic_span(&owned, &owned.diagnostics[0]),
            Some(&owned.spans[0])
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One exhaustive native ABI fingerprint table.
    fn all_frozen_view_sizes_and_alignments_match() {
        let layouts: &[(u32, usize, usize)] = &[
            (
                1,
                size_of::<InputSourceView>(),
                align_of::<InputSourceView>(),
            ),
            (2, size_of::<InputView>(), align_of::<InputView>()),
            (3, size_of::<FailureView>(), align_of::<FailureView>()),
            (4, size_of::<Limits>(), align_of::<Limits>()),
            (5, size_of::<ResultView>(), align_of::<ResultView>()),
            (6, size_of::<SourceView>(), align_of::<SourceView>()),
            (7, size_of::<SpanView>(), align_of::<SpanView>()),
            (8, size_of::<ProvenanceView>(), align_of::<ProvenanceView>()),
            (9, size_of::<OwnerView>(), align_of::<OwnerView>()),
            (
                10,
                size_of::<ContentRootView>(),
                align_of::<ContentRootView>(),
            ),
            (
                11,
                size_of::<ContentAtomView>(),
                align_of::<ContentAtomView>(),
            ),
            (
                12,
                size_of::<ContentRefView>(),
                align_of::<ContentRefView>(),
            ),
            (
                13,
                size_of::<ContentPointView>(),
                align_of::<ContentPointView>(),
            ),
            (14, size_of::<LinkView>(), align_of::<LinkView>()),
            (15, size_of::<BlockView>(), align_of::<BlockView>()),
            (16, size_of::<TableView>(), align_of::<TableView>()),
            (17, size_of::<TableRowView>(), align_of::<TableRowView>()),
            (18, size_of::<TableCellView>(), align_of::<TableCellView>()),
            (19, size_of::<FixedView>(), align_of::<FixedView>()),
            (20, size_of::<FixedLineView>(), align_of::<FixedLineView>()),
            (21, size_of::<PlacementView>(), align_of::<PlacementView>()),
            (
                22,
                size_of::<DecorationView>(),
                align_of::<DecorationView>(),
            ),
            (23, size_of::<FormView>(), align_of::<FormView>()),
            (24, size_of::<NameHintView>(), align_of::<NameHintView>()),
            (25, size_of::<RelationView>(), align_of::<RelationView>()),
            (
                26,
                size_of::<DiagnosticView>(),
                align_of::<DiagnosticView>(),
            ),
            (27, size_of::<MetadataView>(), align_of::<MetadataView>()),
        ];
        for &(kind, size, align) in layouts {
            assert_eq!(
                unsafe { mant_structured_view_size(kind) },
                size,
                "size kind {kind}"
            );
            assert_eq!(
                unsafe { mant_structured_view_align(kind) },
                align,
                "align kind {kind}"
            );
        }
        assert_eq!(unsafe { mant_structured_view_size(0) }, 0);
        assert_eq!(unsafe { mant_structured_view_align(99) }, 0);
        assert_eq!(unsafe { mant_structured_view_offset(1, 0) }, usize::MAX);

        macro_rules! offsets {
            ($kind:expr, $ty:ty; $($field:ident),+ $(,)?) => {{
                let expected = [$(std::mem::offset_of!($ty, $field)),+];
                for (index, offset) in expected.into_iter().enumerate() {
                    let field = u32::try_from(index).expect("ABI field count fits u32") + 1;
                    assert_eq!(unsafe { mant_structured_view_offset($kind, field) }, offset,
                        "offset kind {} field {}", $kind, index + 1);
                }
                let invalid = u32::try_from(expected.len()).expect("ABI field count fits u32") + 1;
                assert_eq!(unsafe { mant_structured_view_offset($kind, invalid) }, usize::MAX);
            }};
        }
        offsets!(1, InputSourceView; identity_kind, format, logical_name, resolver_name, source_bytes, reserved);
        offsets!(2, InputView; sources, root_input, profile, width, resolve, resolve_context, reserved);
        offsets!(3, FailureView; status, stage, limit_kind, observed, allowed, reserved);
        offsets!(4, Limits;
            max_input_sources, max_sources, max_source_path_bytes,
            max_decoded_source_bytes_per_source, max_decoded_source_bytes_total,
            max_source_map_entries, max_source_map_bytes, max_builder_operations,
            max_builder_allocated_bytes, max_content_bytes, max_owners, max_blocks,
            max_content_atoms, max_content_refs, max_content_points, max_links,
            max_tables, max_table_rows, max_table_cells, max_fixed_views,
            max_fixed_lines, max_placements, max_decorations, max_forms,
            max_name_hints, max_relations, max_connection_atoms, max_annotation_runs,
            max_annotation_mutations, max_relation_edges, max_diagnostics,
            max_transfer_objects, max_transfer_edges, max_transfer_bytes,
            max_nesting_depth, max_include_depth, reserved);
        offsets!(5, ResultView; root_source, profile, width, metadata, sources, spans,
            provenances, owners, content_roots, content_atoms, content_refs,
            content_points, links, blocks, tables, table_rows, table_cells,
            fixed_views, fixed_lines, placements, decorations, forms, name_hints,
            relations, diagnostics, reserved);
        offsets!(6, SourceView; key, identity_kind, format, coordinate_kind,
            logical_name, decoded_length, hash_present, hash, reserved_bytes, reserved);
        offsets!(7, SpanView; line_column_present, byte_range_present, reserved_bytes, source,
            line_start, column_start, line_end, column_end, byte_start, byte_end, reserved);
        offsets!(8, ProvenanceView; kind, authored_span, generated_trigger_span, reserved);
        offsets!(9, OwnerView; key, kind, provenance, reserved);
        offsets!(10, ContentRootView; key, owner, ordinal, kind, provenance, reserved);
        offsets!(11, ContentAtomView; key, root, ordinal, owner, kind, style_flags,
            role, link, text, display_override_present, display_reserved_bytes,
            display_override, whitespace_breakable, reserved_bytes, provenance, reserved);
        offsets!(12, ContentRefView; atom, byte_start, byte_end, reserved);
        offsets!(13, ContentPointView; key, root, ordinal, owner, boundary_kind,
            atom_boundary, atom, byte_offset, scalar_boundary, provenance, reserved);
        offsets!(14, LinkView; key, owner, target_kind, target_a, target_b_present,
            target_b_reserved_bytes, target_b, title_present, title_reserved_bytes, title, first_label_ref, label_ref_count,
            provenance, reserved);
        offsets!(15, BlockView; key, owner, kind, parent, ordinal, provenance,
            root, table, fixed_view, reserved);
        offsets!(16, TableView; key, block, fixed_view, provenance, reserved);
        offsets!(17, TableRowView; key, table, ordinal, provenance, reserved);
        offsets!(18, TableCellView; key, row, column, owner, kind, alignment,
            row_span, column_span, provenance, reserved);
        offsets!(19, FixedView; key, owner, block, table, provenance, reserved);
        offsets!(20, FixedLineView; key, view, ordinal, total_columns, reserved);
        offsets!(21, PlacementView; key, line, ordinal, target_kind, atom,
            byte_start, byte_end, point, scalar_start, scalar_end, column_start,
            column_end, cell_map_kind, cell_map_value, reserved);
        offsets!(22, DecorationView; key, line, ordinal, kind, text, column_start,
            column_end, provenance, reserved);
        offsets!(23, FormView; key, owner, role, first_ref, ref_count, provenance, reserved);
        offsets!(24, NameHintView; key, form, first_ref, ref_count, provenance, reserved);
        offsets!(25, RelationView; key, owner, kind, target_owner, provenance, reserved);
        offsets!(26, DiagnosticView; level, code, message, span, owner, reserved);
        offsets!(27, MetadataView; macroset, presence_flags, title, section, volume, operating_system,
            architecture, name, date, alias_target, has_body, reserved_bytes, reserved);

        let discriminants: &[u32] = &[
            0, 1, 2, 3, 4, 5, 6, 7, // status
            0, 1, 2, 3, 4, 5, 6, // stage
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23,
            24, 25, 26, 27, // view
            0, 1, 2, 3, // identity
            0, 1, 2, 3, // format
            0, 1, 2, // profile
            0, 1, 2, // coordinate
            0, 1, 2, 3, 4, 1, 210, // diagnostic level and native range
            0, 1, 2, 3, // provenance
            0, 1, 2, 3, 4, // atom
            0, 1, 2, 4, 8, // style bits
            0, 1, 2, 3, 4, 5, // role
            0, 1, 2, 3, 4, 5, 6, 7, // owner
            0, 1, 2, 3, 4, 5, // root
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, // block
            0, 1, 2, 3, 4, 5, // link target
            0, 1, 2, // point boundary
            0, 1, 2, // placement target
            0, 1, 2, 3, // cell map
            0, 1, 2, 3, 4, 5, // table cell
            0, 1, 2, 3, // table alignment
            0, 1, 2, 3, // decoration
            0, 1, 2, // relation
            0, 1, 2, 3, 4, 5, // resolver
            0, 1, 2, 4, 8, 16, 32, 64, 128, // metadata presence bits
        ];
        let expected = discriminants
            .iter()
            .fold(14_695_981_039_346_656_037_u64, |hash, value| {
                value.to_le_bytes().into_iter().fold(hash, |inner, byte| {
                    (inner ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
                })
            });
        assert_eq!(
            unsafe { mant_structured_discriminant_fingerprint() },
            expected
        );
    }

    #[test]
    fn empty_metadata_and_sources_survive_native_handle_release() {
        // Oracle: cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1, UTF-8/78.
        // `read.c::mparse_readmem()` enters/restores each .so source, while
        // man_validate.c::check_root() leaves a metadata-only page bodyless.
        let mut bundle = SourceBundle::new();
        bundle
            .insert("aaa-unused.1", b".TH UNUSED 1\n".to_vec())
            .unwrap();
        bundle
            .insert("middle-root.1", b".so zzz-included.1\n".to_vec())
            .unwrap();
        bundle
            .insert(
                "zzz-included.1",
                b".TH INCLUDED 1 \"2026-09-20\"\n".to_vec(),
            )
            .unwrap();
        let owned = render_prelude(
            "middle-root.1",
            &bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("empty structured document");
        assert_eq!(owned.width, 78);
        assert_eq!(owned.metadata.title.as_deref(), Some("INCLUDED"));
        assert_eq!(owned.root_source, 1);
        assert_eq!(
            owned
                .sources
                .iter()
                .map(|source| source.logical_name.as_str())
                .collect::<Vec<_>>(),
            ["middle-root.1", "zzz-included.1"]
        );
        assert_eq!(owned.sources[0].identity_kind, IDENTITY_BUNDLE_MEMBER);
        assert_eq!(owned.sources[0].format, FORMAT_MAN);
        assert_eq!(
            owned.sources[0].coordinate_kind,
            COORD_NATIVE_NORMALIZED_BYTES
        );
        assert_eq!(owned.sources[0].hash, None);
    }

    #[test]
    fn body_is_collected_into_heading_and_section_owned_prose() {
        // The registered oracle renders `body` from this exact input.
        // Pinned `man_term.c::print_man_node` supplies exact authored nodes;
        // `term.c::term_field/term_flushln` commits surviving buffer content.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "body.1",
                b".TH BODY 1 \"2026-09-20\"\n.SH NAME\nbody\n".to_vec(),
            )
            .unwrap();
        let document = render_prelude("body.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("C02b collects supported prose");
        assert_eq!(document.owners.len(), 1);
        assert_eq!(document.content_roots.len(), 2);
        assert_eq!(document.content_roots[0].kind, ROOT_HEADING);
        assert_eq!(document.content_roots[1].kind, ROOT_BODY);
        assert_eq!(document.blocks[1].parent, Some(document.blocks[0].key));
        assert!(
            document
                .content_atoms
                .iter()
                .any(|atom| atom.text == "body")
        );
    }

    #[test]
    fn input_limits_reject_before_descriptor_materialization() {
        let mut bundle = SourceBundle::new();
        bundle.insert("root.1", b".TH ROOT 1\n".to_vec()).unwrap();
        bundle
            .insert("included.1", b".TH INCLUDED 1\n".to_vec())
            .unwrap();
        let limits = Limits {
            max_input_sources: 1,
            ..Limits::default()
        };
        let error = InputStorage::new("root.1", &bundle, InputFormat::Man, &limits)
            .err()
            .expect("source count is checked before allocating descriptors");
        assert_eq!(error.status, STATUS_BUDGET);
        assert_eq!((error.limit_kind, error.observed, error.allowed), (1, 2, 1));

        for invalid_limits in [
            Limits {
                max_input_sources: 0,
                ..Limits::default()
            },
            Limits {
                max_sources: u64::from(u32::MAX) + 1,
                ..Limits::default()
            },
        ] {
            let error = InputStorage::new("root.1", &bundle, InputFormat::Man, &invalid_limits)
                .err()
                .expect("invalid limits are rejected before budget comparisons");
            assert_eq!(error.status, STATUS_INVALID_INPUT);
            assert_eq!(error.limit_kind, 0);
        }
    }

    #[test]
    fn top_level_heading_ordinals_define_document_order() {
        // Exact UTF-8/78 oracle run before this assertion rendered FIRST/body
        // before SECOND/body. Pinned `man_term.c::print_man_node` visits SH
        // blocks in document order; each `term.c::term_field/term_flushln`
        // sequence commits the corresponding heading and paragraph roots.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "order.1",
                b".TH ORDER 1 \"2026-09-21\"\n.SH FIRST\nfirst body\n.SH SECOND\nsecond body\n"
                    .to_vec(),
            )
            .unwrap();
        let document = render_prelude("order.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("multiple sections retain explicit document order");
        assert_eq!(document.blocks.len(), 4);
        assert_eq!(document.blocks[0].kind, BLOCK_HEADING);
        assert_eq!(document.blocks[0].parent, None);
        assert_eq!(document.blocks[0].ordinal, 0);
        assert_eq!(document.blocks[1].parent, Some(document.blocks[0].key));
        assert_eq!(document.blocks[1].ordinal, 0);
        assert_eq!(document.blocks[2].kind, BLOCK_HEADING);
        assert_eq!(document.blocks[2].parent, None);
        assert_eq!(document.blocks[2].ordinal, 1);
        assert_eq!(document.blocks[3].parent, Some(document.blocks[2].key));
        assert_eq!(document.blocks[3].ordinal, 0);

        let storage =
            InputStorage::new("order.1", &bundle, InputFormat::Man, &Limits::default()).unwrap();
        let (status, pointer, failure) =
            raw_render(&storage.view(78, PROFILE_UTF8), &Limits::default());
        assert_eq!(status, STATUS_OK, "{failure:?}");
        let handle = ResultHandle(NonNull::new(pointer).unwrap());
        let mut view = ResultView::default();
        assert_eq!(
            unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
            STATUS_OK
        );
        let blocks = view.blocks.ptr.cast::<BlockView>().cast_mut();
        unsafe { (*blocks.add(2)).ordinal = 0 };
        let mut failure = FailureView::default();
        assert_eq!(
            unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
            STATUS_RELATION,
            "a duplicate top-level ordinal must be rejected"
        );
        assert!(copy_structured_document(&handle, &view, &Limits::default()).is_err());

        unsafe {
            (*blocks).ordinal = 1;
            (*blocks.add(2)).ordinal = 0;
        }
        assert_eq!(
            unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
            STATUS_RELATION,
            "out-of-order top-level ordinals must be rejected"
        );
        assert!(copy_structured_document(&handle, &view, &Limits::default()).is_err());
    }

    #[test]
    fn external_link_labels_reference_shared_atoms() {
        // Oracle: registered C02b UTF-8/78 `.UR` probe, run before this
        // assertion.  `man_term.c::pre_UR/post_UR` traverses the body label
        // and emits the head target wrapper during the same native render.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "link.1",
                b".TH LINK 1 \"2026-09-20\"\n.SH NAME\n.UR https://example.com\nplain\n.B bold\n.I italic\n.UE\n"
                    .to_vec(),
            )
            .unwrap();
        let owned = render_prelude("link.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("supported external link");
        assert_eq!(owned.links.len(), 1, "{owned:?}");
        assert_eq!(owned.links[0].target_kind, 1);
        assert_eq!(owned.links[0].target_a, "https://example.com");
        let label_start = owned.links[0].first_label_ref as usize - 1;
        let label_end = label_start + owned.links[0].label_ref_count as usize;
        assert!(!owned.content_refs[label_start..label_end].is_empty());
        assert!(
            owned.content_refs[label_start..label_end]
                .iter()
                .all(|content_ref| {
                    owned.content_atoms[content_ref.atom as usize - 1].link
                        == Some(owned.links[0].key)
                })
        );
        let label_styles = owned.content_refs[label_start..label_end]
            .iter()
            .map(|content_ref| owned.content_atoms[content_ref.atom as usize - 1].style_flags)
            .collect::<Vec<_>>();
        assert!(label_styles.contains(&0), "{owned:?}");
        assert!(label_styles.iter().any(|style| style & 1 != 0), "{owned:?}");
        assert!(label_styles.iter().any(|style| style & 2 != 0), "{owned:?}");

        // Oracle: registered C02b UTF-8/78 `.Lk` probe, with traversal in
        // `mdoc_term.c::termp_lk_pre`.
        let mut mdoc = SourceBundle::new();
        mdoc.insert(
            "link.1",
            b".Dd September 20, 2026\n.Dt LINK 1\n.Os\n.Sh NAME\n.Nm link\n.Nd test\n.Sh DESCRIPTION\n.Lk https://example.com label\n"
                .to_vec(),
        )
        .unwrap();
        let owned = render_prelude("link.1", &mdoc, InputFormat::Mdoc, 78, &Limits::default())
            .expect("supported mdoc external link");
        assert_eq!(owned.links.len(), 1, "mdoc link: {owned:?}");
        assert_eq!(owned.links[0].target_a, "https://example.com");
        assert!(
            owned
                .content_atoms
                .iter()
                .any(|atom| atom.link == Some(1) && atom.style_flags & 2 != 0)
        );
    }

    #[test]
    fn native_check_rejects_empty_and_split_utf8_content_refs() {
        // Oracle: registered C02b UTF-8/78 `.UR` probe renders the authored
        // `café` label.  Pinned `term.c::encode1` retains é as one scalar;
        // label references therefore cannot start inside its UTF-8 encoding.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "link.1",
                ".TH LINK 1\n.SH TEST\n.UR https://example.com\ncafé\n.UE\n"
                    .as_bytes()
                    .to_vec(),
            )
            .unwrap();
        for split_utf8 in [false, true] {
            let limits = Limits::default();
            let storage = InputStorage::new("link.1", &bundle, InputFormat::Man, &limits).unwrap();
            let (status, pointer, failure) = raw_render(&storage.view(78, PROFILE_UTF8), &limits);
            assert_eq!(status, STATUS_OK, "{failure:?}");
            let handle = ResultHandle(NonNull::new(pointer).unwrap());
            let mut view = ResultView::default();
            assert_eq!(
                unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
                STATUS_OK
            );
            let refs = unsafe {
                std::slice::from_raw_parts_mut(
                    view.content_refs.ptr.cast::<ContentRefView>().cast_mut(),
                    view.content_refs.count as usize,
                )
            };
            let atoms = unsafe {
                std::slice::from_raw_parts(
                    view.content_atoms.ptr.cast::<ContentAtomView>(),
                    view.content_atoms.count as usize,
                )
            };
            let content_ref = refs
                .iter_mut()
                .find(|content_ref| {
                    let atom = &atoms[content_ref.atom as usize - 1];
                    let bytes = unsafe {
                        std::slice::from_raw_parts(
                            atom.text.ptr,
                            usize::try_from(atom.text.len).expect("test atom fits this platform"),
                        )
                    };
                    std::str::from_utf8(bytes).is_ok_and(|text| text.contains("café"))
                })
                .expect("authored UTF-8 label reference");
            if split_utf8 {
                content_ref.byte_start = content_ref.byte_end - 1;
            } else {
                content_ref.byte_start = content_ref.byte_end;
            }
            let mut failure = FailureView::default();
            assert_eq!(
                unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
                STATUS_RELATION
            );
        }
    }

    #[test]
    fn profile_specific_connections_keep_one_logical_model() {
        // Oracle: registered C02b ASCII/UTF-8 width-78 six-connection probe.
        // Pinned `term.c::term_word/term_fill/term_flushln/term_field` keeps
        // `\~` nonbreaking; ASCII only projects it as a display space.  `\:`
        // is an ASCII break sentinel but a UTF-8 zero-width nonbreak marker.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "connections.1",
                b".TH CONNECTIONS 1\n.SH TEST\n.ll 18n\nabcdefgh ijklmnopqrst uvwxyz\nabcdefgh-ijklmnopqrst uvwxyz\nabcdefgh\\:ijklmnopqrst uvwxyz\nabcdefgh\\%ijklmnopqrst uvwxyz\nabcdefgh\\&-ijklmnopqrst uvwxyz\nabcdefgh\\~ijklmnopqrst uvwxyz\n.B abcdefgh-ijklmnopqrst\none  two\naveryveryveryverylongword\nleft\\[em]right\n".to_vec(),
            )
            .unwrap();
        let utf8 = render_prelude_profile(
            "connections.1",
            &bundle,
            InputFormat::Man,
            PROFILE_UTF8,
            78,
            &Limits::default(),
        )
        .expect("UTF-8 connections");
        let ascii = render_prelude_profile(
            "connections.1",
            &bundle,
            InputFormat::Man,
            2,
            78,
            &Limits::default(),
        )
        .expect("ASCII connections");
        let utf8_nbsp = utf8
            .content_atoms
            .iter()
            .find(|atom| atom.text == "\u{a0}")
            .expect("UTF-8 logical NBSP");
        let ascii_nbsp = ascii
            .content_atoms
            .iter()
            .find(|atom| atom.text == "\u{a0}")
            .expect("ASCII logical NBSP");
        assert!(!utf8_nbsp.whitespace_breakable && !ascii_nbsp.whitespace_breakable);
        assert_eq!(utf8_nbsp.display_override, None);
        assert_eq!(ascii_nbsp.display_override.as_deref(), Some(" "));
        let ascii_em_dash = ascii
            .content_atoms
            .iter()
            .find(|atom| atom.text == "\u{2014}")
            .expect("ASCII profile retains the logical em dash");
        assert_eq!(ascii_em_dash.display_override.as_deref(), Some("--"));
        assert!(
            utf8.content_atoms
                .iter()
                .any(|atom| atom.text.contains('\u{2014}') && atom.display_override.is_none())
        );
        let utf8_text = utf8
            .content_atoms
            .iter()
            .map(|atom| atom.text.as_str())
            .collect::<String>();
        let ascii_text = ascii
            .content_atoms
            .iter()
            .map(|atom| atom.text.as_str())
            .collect::<String>();
        for logical in [&utf8_text, &ascii_text] {
            assert!(logical.contains("abcdefgh ijklmnopqrst uvwxyz"));
            assert!(logical.contains("abcdefgh-ijklmnopqrst uvwxyz"));
            assert!(logical.contains("abcdefghijklmnopqrst uvwxyz"));
            assert!(logical.contains("abcdefgh\u{a0}ijklmnopqrst uvwxyz"));
            assert!(logical.contains("one  two"));
            assert!(logical.contains("averyveryveryverylongword"));
        }
        assert!(
            utf8.content_atoms
                .iter()
                .any(|atom| { atom.style_flags & 1 != 0 && atom.text.contains("abcdefgh") })
        );
        let utf8_breaks = utf8
            .content_atoms
            .iter()
            .filter(|atom| atom.kind == ATOM_BREAK_OPPORTUNITY)
            .count();
        let ascii_breaks = ascii
            .content_atoms
            .iter()
            .filter(|atom| atom.kind == ATOM_BREAK_OPPORTUNITY)
            .count();
        assert_eq!(ascii_breaks, utf8_breaks + 1);
    }

    #[test]
    #[allow(clippy::too_many_lines)] // Six exact connection cases share one assertion matrix.
    fn each_connection_has_its_exact_logical_boundary() {
        // Oracle: registered C02b ASCII/UTF-8 width-78 probes were run for
        // each row before these assertions.  Pinned `roff.c::roff_parseln`
        // marks only authored in-word hyphens as `ASCII_HYPH`; `term_word`
        // maps `\:` by device, while `\%` and `\&` are ignored controls.
        for (source, expected, utf8_breaks, ascii_breaks, has_nbsp, plain_space) in [
            (
                "abcdefgh ijklmnopqrst uvwxyz",
                "abcdefgh ijklmnopqrst uvwxyz",
                0,
                0,
                false,
                true,
            ),
            (
                "abcdefgh-ijklmnopqrst uvwxyz",
                "abcdefgh-ijklmnopqrst uvwxyz",
                1,
                1,
                false,
                false,
            ),
            (
                "abcdefgh\\:ijklmnopqrst uvwxyz",
                "abcdefghijklmnopqrst uvwxyz",
                0,
                1,
                false,
                false,
            ),
            (
                "abcdefgh\\%ijklmnopqrst uvwxyz",
                "abcdefghijklmnopqrst uvwxyz",
                0,
                0,
                false,
                false,
            ),
            (
                "abcdefgh\\&-ijklmnopqrst uvwxyz",
                "abcdefgh-ijklmnopqrst uvwxyz",
                0,
                0,
                false,
                false,
            ),
            (
                "abcdefgh\\~ijklmnopqrst uvwxyz",
                "abcdefgh\u{a0}ijklmnopqrst uvwxyz",
                0,
                0,
                true,
                false,
            ),
        ] {
            let input = format!(".TH CONNECTION 1\n.SH TEST\n.ll 18n\n{source}\n");
            let mut bundle = SourceBundle::new();
            bundle.insert("connection.1", input.into_bytes()).unwrap();
            for (profile, break_count) in
                [(PROFILE_UTF8, utf8_breaks), (PROFILE_ASCII, ascii_breaks)]
            {
                let owned = render_prelude_profile(
                    "connection.1",
                    &bundle,
                    InputFormat::Man,
                    profile,
                    78,
                    &Limits::default(),
                )
                .expect("supported connection");
                let body_atoms = owned
                    .content_atoms
                    .iter()
                    .filter(|atom| owned.content_roots[atom.root as usize - 1].kind == ROOT_BODY)
                    .collect::<Vec<_>>();
                let logical = body_atoms
                    .iter()
                    .filter(|atom| atom.kind != ATOM_BREAK_OPPORTUNITY)
                    .map(|atom| atom.text.as_str())
                    .collect::<String>();
                assert_eq!(logical, expected, "profile={profile}, source={source}");
                assert_eq!(
                    body_atoms
                        .iter()
                        .filter(|atom| atom.kind == ATOM_BREAK_OPPORTUNITY)
                        .count(),
                    break_count,
                    "profile={profile}, source={source}"
                );
                if plain_space {
                    let mut logical_offset = 0;
                    let target = body_atoms
                        .iter()
                        .filter(|atom| atom.kind != ATOM_BREAK_OPPORTUNITY)
                        .find(|atom| {
                            let starts_at_boundary = logical_offset == "abcdefgh".len();
                            logical_offset += atom.text.len();
                            starts_at_boundary
                        })
                        .expect("the tested connection has an atom at its exact boundary");
                    assert_eq!(target.kind, ATOM_WHITESPACE);
                    assert_eq!(target.text, " ");
                    assert!(target.whitespace_breakable);
                }
                assert_eq!(
                    body_atoms
                        .iter()
                        .filter(|atom| atom.text == "\u{a0}" && !atom.whitespace_breakable)
                        .count(),
                    usize::from(has_nbsp),
                    "profile={profile}, source={source}"
                );
            }
        }
    }

    #[test]
    fn ascii_projection_tracks_surviving_overwritten_slots() {
        // Oracle: registered C02b ASCII/78 `\(em\h'-1m'X` prints `-X`.
        // Pinned `term.c::encode1/buffer_write` first writes both em-dash
        // projection cells, then the horizontal motion lets X overwrite the
        // second cell.  The sidecar must discard that projection fragment.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "projection.1",
                b".TH PROJECTION 1\n.SH TEST\n\\(em\\h'-1m'X\n".to_vec(),
            )
            .unwrap();
        let owned = render_prelude_profile(
            "projection.1",
            &bundle,
            InputFormat::Man,
            PROFILE_ASCII,
            78,
            &Limits::default(),
        )
        .expect("supported overwritten ASCII projection");
        let em_dash = owned
            .content_atoms
            .iter()
            .find(|atom| atom.text == "\u{2014}")
            .expect("logical em dash survives one physical cell");
        assert_eq!(em_dash.display_override.as_deref(), Some("-"));
        assert!(owned.content_atoms.iter().any(|atom| atom.text == "X"));
    }

    #[test]
    fn logical_breaks_commit_after_surviving_buffer_content() {
        // Oracle: registered C02b UTF-8/78 `.br`/`\p` probe.  In pinned
        // `term.c::term_word/term_fill/term_flushln`, `\p` marks a buffered
        // break after the word; it is not a break at the escape source byte.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "break.1",
                b".TH BREAK 1 \"2026-09-20\"\n.SH NAME\nbefore\\pafter\nmid\n.br\ntail\n".to_vec(),
            )
            .unwrap();
        let owned = render_prelude("break.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("supported logical breaks");
        let body_atoms = owned
            .content_atoms
            .iter()
            .filter(|atom| owned.content_roots[atom.root as usize - 1].kind == ROOT_BODY)
            .collect::<Vec<_>>();
        let before_after = body_atoms
            .iter()
            .position(|atom| atom.text == "beforeafter")
            .expect("sidecar coalesces the surviving word");
        // Pinned `term.c::term_flushln` consumes the automatically inserted
        // separator before it emits the delayed line end, so that separator
        // remains exactly one logical whitespace atom.
        assert_eq!(
            body_atoms[before_after + 1].kind,
            ATOM_WHITESPACE,
            "{owned:?}"
        );
        assert_eq!(body_atoms[before_after + 1].text, " ");
        assert_eq!(body_atoms[before_after + 2].kind, ATOM_HARD_BREAK);
        assert!(
            body_atoms
                .iter()
                .filter(|atom| atom.kind == ATOM_HARD_BREAK)
                .count()
                >= 2,
            "{owned:?}"
        );
    }

    #[test]
    fn delayed_breaks_remain_with_the_flushed_root_at_new_block_boundaries() {
        // Oracle: registered C02b UTF-8/78 probes run before these assertions.
        // Pinned `man_term.c::print_man_node/pre_PP/print_bvspace` and
        // `mdoc_term.c::print_mdoc_node/termp_pp_pre` enter the new node before
        // flushing the preceding buffered root, so the delayed `\p` retains
        // the root carried by its logical token.
        for (name, format, source) in [
            (
                "boundary.1",
                InputFormat::Man,
                b".TH T 1\n.SH A\nfoo\\p\n.PP\nbar\n".as_slice(),
            ),
            (
                "boundary.1",
                InputFormat::Mdoc,
                b".Dd September 21, 2026\n.Dt T 1\n.Os\n.Sh A\nfoo\\p\n.Pp\nbar\n".as_slice(),
            ),
        ] {
            let mut bundle = SourceBundle::new();
            bundle.insert(name, source.to_vec()).unwrap();
            let owned = render_prelude(name, &bundle, format, 78, &Limits::default())
                .expect("the new block flushes the preceding root");
            let foo = owned
                .content_atoms
                .iter()
                .position(|atom| atom.text == "foo")
                .expect("old root text");
            let hard_break = owned
                .content_atoms
                .iter()
                .position(|atom| atom.kind == ATOM_HARD_BREAK)
                .expect("delayed break");
            let bar = owned
                .content_atoms
                .iter()
                .position(|atom| atom.text == "bar")
                .expect("new root text");
            assert_eq!(
                owned.content_atoms[hard_break].root,
                owned.content_atoms[foo].root
            );
            assert_ne!(
                owned.content_atoms[hard_break].root,
                owned.content_atoms[bar].root
            );
            assert!(foo < hard_break && hard_break < bar, "{owned:?}");
        }
    }

    #[test]
    fn explicit_native_widths_preserve_one_logical_prose_model() {
        // Oracle: registered C02b UTF-8 probes at widths 60/78/100/120 were
        // run before this assertion.  Pinned `term.c::term_flushln` changes
        // physical rows at each width while the collector retains the same
        // authored spaces and word order.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "width.1",
                b".TH WIDTH 1\n.SH TEST\nalpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau\n"
                    .to_vec(),
            )
            .unwrap();
        let expected = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu nu xi omicron pi rho sigma tau";
        for width in [60, 78, 100, 120] {
            let owned = render_prelude(
                "width.1",
                &bundle,
                InputFormat::Man,
                width,
                &Limits::default(),
            )
            .expect("supported width-specific render");
            let logical = owned
                .content_atoms
                .iter()
                .map(|atom| atom.text.as_str())
                .collect::<String>();
            assert!(logical.contains(expected), "width={width}: {owned:?}");
        }
    }

    #[test]
    fn unsupported_geometry_still_exercises_sidecar_mutations() {
        // Oracle: registered C02b UTF-8 table (width 20), overstrike, and no-fill
        // probes were run before these assertions.  Pinned `term_flushln`
        // partially consumes multicolumn fields, while `term_word` truncates
        // the trailing overstrike backspace/blank pair.  The phase probe runs
        // that exact renderer but never publishes an incomplete document.
        let mut table = SourceBundle::new();
        table
            .insert(
                "table.1",
                b".TH TABLE 1\n.SH TEST\n.TS\ntab(:);\nl l.\nleft-side-with-many-words:right-side-with-many-words\n.TE\n".to_vec(),
            )
            .unwrap();
        let table_metrics =
            probe_structured("table.1", &table, InputFormat::Man, 20, &Limits::default())
                .expect("table probe executes then discards the unsupported result");
        assert!(table_metrics.peak_columns > 1, "{table_metrics:?}");
        assert!(table_metrics.consumes > 0, "{table_metrics:?}");
        assert!(table_metrics.partial_consumes > 0, "{table_metrics:?}");
        assert!(table_metrics.continued_consumes > 0, "{table_metrics:?}");

        let mut overstrike = SourceBundle::new();
        overstrike
            .insert(
                "over.1",
                b".TH OVER 1\n.SH TEST\nbefore \\o'ab ' after\n".to_vec(),
            )
            .unwrap();
        let overstrike_metrics = probe_structured(
            "over.1",
            &overstrike,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("overstrike probe executes then discards the unsupported result");
        assert!(overstrike_metrics.truncates > 0, "{overstrike_metrics:?}");
        let overstrike_owned = render_prelude(
            "over.1",
            &overstrike,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("the surviving logical prose remains supported");
        let overstrike_text = overstrike_owned
            .content_atoms
            .iter()
            .filter(|atom| atom.kind != ATOM_BREAK_OPPORTUNITY)
            .map(|atom| atom.text.as_str())
            .collect::<String>();
        assert!(
            overstrike_text.contains("before ab after"),
            "{overstrike_owned:?}"
        );

        let mut nofill = SourceBundle::new();
        nofill
            .insert(
                "nofill.1",
                b".TH NOFILL 1\n.SH TEST\n.nf\none  two\nthree\n.fi\n".to_vec(),
            )
            .unwrap();
        let nofill_metrics = probe_structured(
            "nofill.1",
            &nofill,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("no-fill probe executes then discards the unsupported result");
        assert!(nofill_metrics.resets > 0, "{nofill_metrics:?}");
        assert!(nofill_metrics.logical_events > 0, "{nofill_metrics:?}");
    }

    #[test]
    fn unsupported_structural_shapes_fail_whole_result() {
        // Oracle preflight covered `.SS`, no-fill, and both equation forms;
        // C02b intentionally has no complete hierarchy/fixed-view/equation
        // representation for these shapes.  Pinned
        // `eqn_term.c::term_eqn` still executes in the phase probe.
        for (name, source) in [
            (
                "subsection.1",
                b".TH UNSUP 1\n.SH TOP\n.SS CHILD\ntext\n".as_slice(),
            ),
            (
                "nofill.1",
                b".TH UNSUP 1\n.SH TOP\n.nf\ntext\n.fi\n".as_slice(),
            ),
            (
                "standalone-eqn.1",
                b".TH EQN 1\n.SH TEST\n.EQ\nx sup 2\n.EN\n".as_slice(),
            ),
            (
                "inline-eqn.1",
                b".TH EQN 1\n.SH TEST\n.EQ\ndelim $$\n.EN\nbefore $x sup 2$ after\n".as_slice(),
            ),
        ] {
            let mut bundle = SourceBundle::new();
            bundle.insert(name, source.to_vec()).unwrap();
            if name.contains("eqn") {
                let metrics =
                    probe_structured(name, &bundle, InputFormat::Man, 78, &Limits::default())
                        .expect("equation renderer executes in the discarded phase probe");
                assert!(metrics.logical_events > 0 && metrics.buffer_writes > 0);
            }
            let error = render_prelude(name, &bundle, InputFormat::Man, 78, &Limits::default())
                .expect_err("unsupported shape must not return a partial document");
            assert_eq!((error.status, error.stage), (STATUS_UNSUPPORTED, 4));
        }
        let mut recovered = SourceBundle::new();
        recovered
            .insert("ok.1", b".TH OK 1\n.SH NAME\nrecovered\n".to_vec())
            .unwrap();
        let document = render_prelude("ok.1", &recovered, InputFormat::Man, 78, &Limits::default())
            .expect("unsupported render cleanup restores the next session");
        assert!(
            document
                .content_atoms
                .iter()
                .any(|atom| atom.text == "recovered")
        );
    }

    #[test]
    fn included_content_keeps_its_result_local_source_key() {
        // Oracle: registered C02b UTF-8/78 nested/repeated include probe.
        // Pinned
        // `read.c::mparse_readmem` restores each include source key and
        // `man_term.c::print_man_node` supplies the exact authored node.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "root.1",
                b".TH ROOT 1\n.SH ROOT\nroot word\n.so middle.1\n.so leaf.1\nroot tail\n".to_vec(),
            )
            .unwrap();
        bundle
            .insert(
                "middle.1",
                b".SH MIDDLE\nmiddle word\n.so leaf.1\n".to_vec(),
            )
            .unwrap();
        bundle
            .insert("leaf.1", b".SH LEAF\nleaf word\n".to_vec())
            .unwrap();
        let owned = render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("supported included prose");
        assert_eq!(owned.sources.len(), 3, "unused sources are not synthesized");
        let leaf_roots = owned
            .content_roots
            .iter()
            .filter(|root| {
                owned
                    .content_atoms
                    .iter()
                    .filter(|atom| atom.root == root.key)
                    .map(|atom| atom.text.as_str())
                    .collect::<String>()
                    .starts_with("leaf word")
            })
            .map(|root| root.key)
            .collect::<Vec<_>>();
        assert_eq!(
            leaf_roots.len(),
            2,
            "repeated include occurrences remain distinct: {owned:?}"
        );
        let mut leaf_sources = Vec::new();
        for root in &leaf_roots {
            let atom = owned
                .content_atoms
                .iter()
                .find(|atom| atom.root == *root && atom.text == "leaf")
                .expect("each repeated leaf root retains its authored text");
            let OwnedProvenance::Authored { span } =
                owned.provenances[atom.provenance as usize - 1]
            else {
                panic!("included text must keep authored provenance");
            };
            leaf_sources.push(owned.spans[span as usize - 1].source);
        }
        assert_eq!(leaf_sources[0], leaf_sources[1]);
        assert_ne!(leaf_roots[0], leaf_roots[1]);
        assert_eq!(
            owned.sources[leaf_sources[0] as usize - 1].logical_name,
            "leaf.1"
        );
        let middle_atom = owned
            .content_atoms
            .iter()
            .find(|atom| atom.text == "middle")
            .expect("nested include body");
        let OwnedProvenance::Authored { span: middle_span } =
            owned.provenances[middle_atom.provenance as usize - 1]
        else {
            panic!("nested include body must be authored");
        };
        let middle_span = &owned.spans[middle_span as usize - 1];
        let leaf_span = owned
            .spans
            .iter()
            .find(|span| {
                span.source == leaf_sources[0]
                    && span.line_columns.is_some_and(|(line, _, _, _)| line == 2)
            })
            .expect("leaf body source position");
        assert_ne!(middle_span.source, leaf_span.source);
        assert_eq!(
            middle_span.line_columns.unwrap().0,
            leaf_span.line_columns.unwrap().0
        );
    }

    #[test]
    fn diagnostics_keep_the_emitting_source_key() {
        // The registered oracle with `-Wstyle` emits both the root `.so`
        // warning and delayed `.TH` validation warnings.  The pinned
        // man_validate.c recursion now restores each node's parse-time key.
        let mut bundle = SourceBundle::new();
        bundle
            .insert("root.1", b".so included.1\n".to_vec())
            .unwrap();
        bundle
            .insert("included.1", b".TH INCLUDED\n".to_vec())
            .unwrap();
        let owned = render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("diagnostic-only structured document");
        let diagnostic_sources = owned
            .diagnostics
            .iter()
            .filter_map(|diagnostic| diagnostic_span(&owned, diagnostic).map(|span| span.source))
            .collect::<Vec<_>>();
        assert!(
            diagnostic_sources.contains(&1),
            "missing root diagnostic: {owned:?}"
        );
        assert!(
            diagnostic_sources.contains(&2),
            "missing include diagnostic: {owned:?}"
        );
        assert!(
            owned
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(".so is fragile")),
            "native fixed diagnostic text was not retained: {owned:?}"
        );
    }

    #[test]
    fn typed_diagnostic_codes_keep_closed_native_identity() {
        use crate::structured::StructuredDiagnosticCode;

        assert_ne!(
            StructuredDiagnosticCode::from_native_ordinal(1),
            StructuredDiagnosticCode::from_native_ordinal(2)
        );
        assert!(StructuredDiagnosticCode::from_native_ordinal(0).is_none());
        assert!(StructuredDiagnosticCode::from_native_ordinal(211).is_none());
    }

    #[test]
    fn diagnostic_columns_use_the_native_normalized_source_map() {
        // Oracle: cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1, UTF-8/78.
        // For this exact line, preconv.c expands `é` before man.c reports the
        // excess argument at native-normalized column 45 (not its raw column).
        let input = ".TH TEST 1 \"date\" \"sourceé\" \"manual\" extra\n";
        let mut bundle = SourceBundle::new();
        bundle
            .insert("utf8-extra.1", input.as_bytes().to_vec())
            .unwrap();
        let owned = render_prelude(
            "utf8-extra.1",
            &bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("metadata and diagnostics survive native normalization");
        assert_eq!(owned.sources[0].decoded_length, input.len() as u64);
        assert!(owned.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("skipping excess arguments")
                && diagnostic
                    .span
                    .and_then(|key| owned.spans.get(key as usize - 1))
                    .is_some_and(|span| span.line_columns == Some((1, 45, 0, 0)))
        }));

        // Oracle for this exact control-byte input reports CHAR_BAD at 1:8.
        // read.c emits it before the completed-line observer, so the collector
        // must bind and validate the source-qualified span in two phases.
        let mut control_bundle = SourceBundle::new();
        control_bundle
            .insert("preconv-control.1", b".TH PRE\x7fCONV 1\n".to_vec())
            .unwrap();
        let control = render_prelude(
            "preconv-control.1",
            &control_bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("pre-conversion diagnostics retain their completed line map");
        assert!(control.diagnostics.iter().any(|diagnostic| {
            diagnostic.message.contains("skipping bad character")
                && diagnostic
                    .span
                    .and_then(|key| control.spans.get(key as usize - 1))
                    .is_some_and(|span| span.source == 1 && span.line_columns == Some((1, 8, 0, 0)))
        }));

        let check_limits = Limits {
            max_builder_operations: 17,
            ..Limits::default()
        };
        let check_budget = render_prelude(
            "preconv-control.1",
            &control_bundle,
            InputFormat::Man,
            78,
            &check_limits,
        )
        .expect_err("source-position validation is charged after native cleanup");
        assert_eq!(check_budget.status, STATUS_BUDGET);
        assert_eq!(check_budget.stage, 6);
        assert_eq!(check_budget.limit_kind, 8);
        assert_eq!(check_budget.observed, 18);
        assert_eq!(check_budget.allowed, 17);
    }

    #[test]
    fn controlled_builder_failure_and_budget_exhaustion_recover() {
        // The metadata-only oracle input was verified before this assertion;
        // failures below are collector policy and must not alter native facts.
        let mut bundle = SourceBundle::new();
        bundle
            .insert("root.1", b".so included.1\n".to_vec())
            .unwrap();
        bundle
            .insert("included.1", b".TH INCLUDED 1 \"2026-09-20\"\n".to_vec())
            .unwrap();

        unsafe { mant_structured_test_fail_after(0) };
        let allocation =
            render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default())
                .expect_err("injected builder allocation failure");
        assert_eq!(allocation.status, STATUS_BUILDER_ALLOC);

        unsafe { mant_structured_test_fail_after(4) };
        let mid_allocation =
            render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default())
                .expect_err("mid-session builder allocation failure");
        assert_eq!(mid_allocation.status, STATUS_BUILDER_ALLOC);

        let limits = Limits {
            max_sources: 1,
            ..Limits::default()
        };
        let budget = render_prelude("root.1", &bundle, InputFormat::Man, 78, &limits)
            .expect_err("include must exceed the source-table budget");
        assert_eq!(budget.status, STATUS_BUDGET);

        let recovered = render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("the next same-thread session must recover");
        assert_eq!(recovered.sources.len(), 2);
    }

    #[test]
    fn native_input_relations_and_transfer_budget_fail_cleanly() {
        let mut bundle = SourceBundle::new();
        bundle
            .insert("root.1", b".so included.1\n".to_vec())
            .unwrap();
        bundle
            .insert("included.1", b".TH INCLUDED 1\n".to_vec())
            .unwrap();
        let mut storage =
            InputStorage::new("root.1", &bundle, InputFormat::Man, &Limits::default()).unwrap();
        let include = storage
            .descriptors
            .iter_mut()
            .find(|source| source.logical_name.len == "included.1".len() as u64)
            .unwrap();
        include.format = FORMAT_MDOC;
        let input = storage.view(78, PROFILE_UTF8);
        let mut result = std::ptr::null_mut();
        let mut failure = FailureView::default();
        let status = unsafe {
            mant_structured_render(
                &raw const input,
                &Limits::default(),
                &raw mut result,
                &raw mut failure,
            )
        };
        assert_eq!(status, STATUS_INVALID_INPUT);
        assert!(result.is_null());
        assert_eq!(failure.status, status);
        assert_eq!(failure.stage, 1);

        let limits = Limits {
            max_transfer_objects: 1,
            ..Limits::default()
        };
        let transfer = render_prelude("root.1", &bundle, InputFormat::Man, 78, &limits)
            .expect_err("owned transfer object budget must be independent");
        assert_eq!(transfer.status, STATUS_BUDGET);
        assert_eq!(transfer.limit_kind, 32);

        let recovered = render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("relation and transfer failures must release all state");
        assert_eq!(recovered.sources.len(), 2);
    }

    #[test]
    fn source_map_builder_and_transfer_limits_report_exact_stages() {
        // The exact metadata-only input was checked against the registered
        // oracle. These assertions freeze collector accounting, not roff
        // interpretation.
        let mut bundle = SourceBundle::new();
        bundle
            .insert("root.1", b".TH LIMITS 1 \"2026-09-20\"\n".to_vec())
            .unwrap();

        let source_map_limits = Limits {
            max_source_map_entries: 1,
            ..Limits::default()
        };
        let source_map =
            render_prelude("root.1", &bundle, InputFormat::Man, 78, &source_map_limits)
                .expect_err("the normalized line map must be charged separately");
        assert_eq!(source_map.status, STATUS_BUDGET);
        assert_eq!(source_map.stage, 3);
        assert_eq!(source_map.limit_kind, 6);

        let operation_limits = Limits {
            max_builder_operations: 1,
            ..Limits::default()
        };
        let operations = render_prelude("root.1", &bundle, InputFormat::Man, 78, &operation_limits)
            .expect_err("the first committed source exceeds one builder operation");
        assert_eq!(operations.status, STATUS_BUDGET);
        assert_eq!(operations.stage, 2);
        assert_eq!(operations.limit_kind, 8);

        let baseline =
            render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default()).unwrap();
        let transfer_objects = 2_u64
            + baseline.sources.len() as u64
            + baseline.spans.len() as u64
            + baseline.provenances.len() as u64
            + baseline.owners.len() as u64
            + baseline.content_roots.len() as u64
            + baseline.content_atoms.len() as u64
            + baseline.blocks.len() as u64
            + baseline.diagnostics.len() as u64;
        let too_small = Limits {
            max_transfer_objects: transfer_objects - 1,
            ..Limits::default()
        };
        let transfer = render_prelude("root.1", &bundle, InputFormat::Man, 78, &too_small)
            .expect_err("every transferred record is counted");
        assert_eq!(transfer.status, STATUS_BUDGET);
        assert_eq!(transfer.stage, 6);
        assert_eq!(transfer.limit_kind, 32);
        assert_eq!(transfer.observed, transfer_objects);

        let exact = Limits {
            max_transfer_objects: transfer_objects,
            ..Limits::default()
        };
        render_prelude("root.1", &bundle, InputFormat::Man, 78, &exact)
            .expect("the exact transfer object boundary succeeds");
    }

    #[test]
    fn native_source_map_checks_both_span_endpoints() {
        // The exact UTF-8/78 body input was checked against the registered
        // oracle.  Pinned `read.c::mparse_readmem` reports each normalized
        // line length; both ends of a source-qualified span must address that
        // same source's observed line map.  Native-normalized coordinates do
        // not claim exact authored byte offsets.
        let mut bundle = SourceBundle::new();
        bundle
            .insert("span.1", b".TH SPAN 1\n.SH TEST\nbody\n".to_vec())
            .unwrap();
        for invalid_kind in 0..3 {
            let limits = Limits::default();
            let storage = InputStorage::new("span.1", &bundle, InputFormat::Man, &limits).unwrap();
            let (status, pointer, failure) = raw_render(&storage.view(78, PROFILE_UTF8), &limits);
            assert_eq!(status, STATUS_OK, "{failure:?}");
            let handle = ResultHandle(NonNull::new(pointer).unwrap());
            let mut view = ResultView::default();
            assert_eq!(
                unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
                STATUS_OK
            );
            let spans = unsafe {
                std::slice::from_raw_parts_mut(
                    view.spans.ptr.cast::<SpanView>().cast_mut(),
                    view.spans.count as usize,
                )
            };
            let span = spans.first_mut().expect("body has a source span");
            match invalid_kind {
                0 => {
                    span.line_end = u32::MAX;
                    span.column_end = 1;
                }
                1 => {
                    span.line_end = span.line_start;
                    span.column_end = u32::MAX;
                }
                _ => {
                    span.byte_range_present = 1;
                    span.byte_end = 1;
                }
            }
            let mut failure = FailureView::default();
            assert_eq!(
                unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
                STATUS_RELATION
            );
        }
    }

    #[test]
    fn all_source_identity_kinds_round_trip_without_path_guessing() {
        let mut bundle = SourceBundle::new();
        bundle
            .insert("root.1", b".TH IDENTITY 1 \"2026-09-20\"\n".to_vec())
            .unwrap();
        for (kind, identity) in [
            (1, "/usr/share/man/man1/identity.1"),
            (IDENTITY_BUNDLE_MEMBER, "root.1"),
            (3, "standard input"),
        ] {
            let mut storage =
                InputStorage::new("root.1", &bundle, InputFormat::Man, &Limits::default()).unwrap();
            storage.descriptors[0].identity_kind = kind;
            storage.descriptors[0].logical_name = BytesView {
                ptr: identity.as_ptr(),
                len: u64::try_from(identity.len()).unwrap(),
            };
            if kind == IDENTITY_BUNDLE_MEMBER {
                storage.descriptors[0].resolver_name = storage.descriptors[0].logical_name;
            }
            let (status, pointer, failure) =
                raw_render(&storage.view(78, PROFILE_UTF8), &Limits::default());
            assert_eq!(status, STATUS_OK, "{failure:?}");
            let handle = ResultHandle(NonNull::new(pointer).unwrap());
            let mut view = ResultView::default();
            assert_eq!(
                unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
                STATUS_OK
            );
            let sources = checked_slice::<SourceView>(view.sources, &handle).unwrap();
            assert_eq!(sources[0].identity_kind, kind);
            assert_eq!(copy_string(sources[0].logical_name).unwrap(), identity);
        }
    }

    #[test]
    fn malformed_views_and_resolver_statuses_are_controlled() {
        // Pinned read.c::mparse_readmem() turns the unresolved exact input
        // `.so missing.1` into visible "See the file" content. The structured
        // resolver must preserve its own denial/panic/invalid failure instead
        // of misreporting that native fallback as a complete result.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "root.1",
                b".TH CALLBACK 1 \"2026-09-20\"\n.so missing.1\n".to_vec(),
            )
            .unwrap();
        let storage =
            InputStorage::new("root.1", &bundle, InputFormat::Man, &Limits::default()).unwrap();

        let mut malformed = storage.view(78, PROFILE_UTF8);
        malformed.sources.stride = 0;
        let (status, pointer, failure) = raw_render(&malformed, &Limits::default());
        assert_eq!(status, STATUS_INVALID_INPUT);
        assert!(pointer.is_null());
        assert_eq!(failure.status, status);

        for (resolver_status, out_slot, expected) in [
            (RESOLVE_DENIED, 0, STATUS_NATIVE),
            (RESOLVE_PANIC, 0, STATUS_NATIVE),
            (RESOLVE_INVALID, 0, STATUS_INVALID_INPUT),
            (RESOLVE_DENIED, 1, STATUS_INVALID_INPUT),
        ] {
            let mut context = ResolverContext {
                status: resolver_status,
                out_slot,
                seen_current: 0,
            };
            let mut input = storage.view(78, PROFILE_UTF8);
            input.resolve = Some(controlled_resolver);
            input.resolve_context = (&raw mut context).cast();
            let (status, pointer, failure) = raw_render(&input, &Limits::default());
            assert_eq!(status, expected);
            assert!(pointer.is_null());
            assert_eq!(failure.status, expected);
            assert_eq!(failure.stage, 2);
            assert_eq!(context.seen_current, storage.root_input);
        }

        unsafe { mant_structured_result_free(std::ptr::null_mut()) };
        let mut failure = FailureView::default();
        assert_eq!(
            unsafe { mant_structured_result_check(std::ptr::null(), &raw mut failure) },
            STATUS_RELATION
        );
        assert_eq!(failure.status, STATUS_RELATION);
    }

    #[test]
    fn callback_reentry_rejects_inner_call_and_outer_state_recovers() {
        // The same exact unresolved `.so` input was checked against the
        // registered oracle before this callback-state assertion.
        let mut outer = SourceBundle::new();
        outer
            .insert("root.1", b".TH OUTER 1\n.so missing.1\n".to_vec())
            .unwrap();
        let storage =
            InputStorage::new("root.1", &outer, InputFormat::Man, &Limits::default()).unwrap();
        let mut context = ReentryContext { observed_status: 0 };
        let mut input = storage.view(78, PROFILE_UTF8);
        input.resolve = Some(reentering_resolver);
        input.resolve_context = (&raw mut context).cast();
        let (_, pointer, _) = raw_render(&input, &Limits::default());
        unsafe { mant_structured_result_free(pointer) };
        assert_eq!(context.observed_status, STATUS_REENTRANT);

        let mut recovered_bundle = SourceBundle::new();
        recovered_bundle
            .insert("recovered.1", b".TH RECOVERED 1\n".to_vec())
            .unwrap();
        let recovered = render_prelude(
            "recovered.1",
            &recovered_bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("outer callback and native TLS must unwind cleanly");
        assert_eq!(recovered.metadata.title.as_deref(), Some("RECOVERED"));
    }

    #[test]
    #[ignore = "measurement probe: run in release mode under /usr/bin/time -v"]
    fn probe_gcc_sidecar_workset() {
        run_real_fixture_probe("gcc.1.gz", "gcc.1");
    }

    #[test]
    #[ignore = "measurement probe: run in release mode under /usr/bin/time -v"]
    fn probe_git_sidecar_workset() {
        run_real_fixture_probe("git.1.gz", "git.1");
    }

    fn run_real_fixture_probe(fixture: &str, logical_name: &str) {
        use flate2::read::MultiGzDecoder;
        use std::{fs, io::Read, path::Path};

        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/roff/real/archlinux")
            .join(fixture);
        let mut bytes = Vec::new();
        MultiGzDecoder::new(fs::File::open(path).expect("open fixed probe fixture"))
            .read_to_end(&mut bytes)
            .expect("decode fixed probe fixture before measured calls");
        let mut bundle = SourceBundle::new();
        bundle.insert(logical_name, bytes).unwrap();

        for sample in 0..=10 {
            let metrics = probe_structured(
                logical_name,
                &bundle,
                InputFormat::Man,
                78,
                &Limits::default(),
            )
            .expect("probe must traverse the full native renderer and discard its document");
            assert!(metrics.collector_events > 0);
            assert!(metrics.logical_events > 0);
            assert!(metrics.buffer_writes > 0);
            assert!(metrics.token_count > 0);
            assert!(metrics.slot_capacity > 0);
            assert!(metrics.sidecar_allocated_bytes > 0);
            assert!(metrics.rendered_bytes > 0);
            if sample != 0 {
                eprintln!("{logical_name} sample={sample} {metrics:?}");
            }
        }
    }
}
