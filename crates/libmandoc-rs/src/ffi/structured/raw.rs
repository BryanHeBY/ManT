//! Private C ABI layouts, discriminants, and foreign functions.

use std::ffi::c_void;

pub(super) const STATUS_OK: u32 = 0;
pub(super) const STATUS_INVALID_INPUT: u32 = 1;
pub(super) const STATUS_REENTRANT: u32 = 2;
pub(super) const STATUS_BUDGET: u32 = 3;
pub(super) const STATUS_BUILDER_ALLOC: u32 = 4;
pub(super) const STATUS_NATIVE: u32 = 5;
pub(super) const STATUS_RELATION: u32 = 6;
pub(super) const STATUS_UNSUPPORTED: u32 = 7;

pub(super) const IDENTITY_BUNDLE_MEMBER: u32 = 2;
pub(super) const FORMAT_MAN: u32 = 1;
pub(super) const FORMAT_MDOC: u32 = 2;
pub(super) const PROFILE_UTF8: u32 = 1;
pub(super) const PROFILE_ASCII: u32 = 2;
pub(super) const COORD_NATIVE_NORMALIZED_BYTES: u32 = 2;
pub(super) const DIAGNOSTIC_STYLE: u32 = 1;
pub(super) const DIAGNOSTIC_UNSUPPORTED: u32 = 4;
pub(super) const DIAGNOSTIC_CODE_NATIVE_LAST: u32 = 210;
pub(super) const PROVENANCE_AUTHORED: u32 = 1;
pub(super) const PROVENANCE_GENERATED: u32 = 2;
pub(super) const PROVENANCE_UNKNOWN: u32 = 3;
pub(super) const ATOM_TEXT: u32 = 1;
pub(super) const ATOM_WHITESPACE: u32 = 2;
pub(super) const ATOM_BREAK_OPPORTUNITY: u32 = 3;
pub(super) const ATOM_HARD_BREAK: u32 = 4;
pub(super) const STYLE_MASK: u32 = 1 | 2 | 4 | 8;
pub(super) const OWNER_KIND_LAST: u32 = 7;
pub(super) const OWNER_LIST_ITEM: u32 = 4;
pub(super) const OWNER_DEFINITION_ITEM: u32 = 5;
pub(super) const ROOT_HEADING: u32 = 1;
pub(super) const ROOT_TERM: u32 = 2;
pub(super) const ROOT_BODY: u32 = 3;
pub(super) const ROOT_KIND_LAST: u32 = 5;
pub(super) const BLOCK_HEADING: u32 = 1;
pub(super) const BLOCK_PARAGRAPH: u32 = 2;
pub(super) const BLOCK_LIST: u32 = 3;
pub(super) const BLOCK_DEFINITION_LIST: u32 = 4;
pub(super) const BLOCK_TABLE: u32 = 5;
pub(super) const BLOCK_INDENTED: u32 = 6;
pub(super) const BLOCK_FIXED_DISPLAY: u32 = 7;
pub(super) const BLOCK_VERTICAL_SPACE: u32 = 8;
pub(super) const BLOCK_THEMATIC_BREAK: u32 = 9;
pub(super) const LIST_BULLET: u32 = 1;
pub(super) const LIST_ORDERED: u32 = 2;
pub(super) const LIST_PLAIN: u32 = 3;
pub(super) const LIST_DEFINITION: u32 = 4;
pub(super) const LIST_NATIVE_MARKER: u32 = 5;
pub(super) const RESOLVE_NOT_FOUND: u32 = 1;
pub(super) const RESOLVE_DENIED: u32 = 2;
pub(super) const RESOLVE_PANIC: u32 = 4;
pub(super) const RESOLVE_INVALID: u32 = 5;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct BytesView {
    pub(super) ptr: *const u8,
    pub(super) len: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct SliceView {
    pub(super) ptr: *const c_void,
    pub(super) count: u32,
    pub(super) stride: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct InputSourceView {
    pub(super) identity_kind: u32,
    pub(super) format: u32,
    pub(super) logical_name: BytesView,
    pub(super) resolver_name: BytesView,
    pub(super) source_bytes: BytesView,
    pub(super) reserved: u32,
}

pub(super) type ResolveFn = unsafe extern "C" fn(*mut c_void, u32, BytesView, *mut u32) -> u32;

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct InputView {
    pub(super) sources: SliceView,
    pub(super) root_input: u32,
    pub(super) profile: u32,
    pub(super) width: u32,
    pub(super) resolve: Option<ResolveFn>,
    pub(super) resolve_context: *mut c_void,
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct FailureView {
    pub(super) status: u32,
    pub(super) stage: u32,
    pub(super) limit_kind: u32,
    pub(super) observed: u64,
    pub(super) allowed: u64,
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub(super) struct ProbeMetrics {
    pub(super) collector_events: u64,
    pub(super) logical_events: u64,
    pub(super) buffer_writes: u64,
    pub(super) cursor_moves: u64,
    pub(super) truncates: u64,
    pub(super) consumes: u64,
    pub(super) partial_consumes: u64,
    pub(super) continued_consumes: u64,
    pub(super) resets: u64,
    pub(super) peak_columns: u64,
    pub(super) peak_slots: u64,
    pub(super) rendered_bytes: u64,
    pub(super) builder_allocated_bytes: u64,
    pub(super) content_bytes: u64,
    pub(super) source_count: u64,
    pub(super) token_count: u64,
    pub(super) slot_capacity: u64,
    pub(super) sidecar_allocated_bytes: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Limits {
    pub(super) max_input_sources: u64,
    pub(super) max_sources: u64,
    pub(super) max_source_path_bytes: u64,
    pub(super) max_decoded_source_bytes_per_source: u64,
    pub(super) max_decoded_source_bytes_total: u64,
    pub(super) max_source_map_entries: u64,
    pub(super) max_source_map_bytes: u64,
    pub(super) max_builder_operations: u64,
    pub(super) max_builder_allocated_bytes: u64,
    pub(super) max_content_bytes: u64,
    pub(super) max_owners: u64,
    pub(super) max_blocks: u64,
    pub(super) max_content_atoms: u64,
    pub(super) max_content_refs: u64,
    pub(super) max_content_points: u64,
    pub(super) max_links: u64,
    pub(super) max_tables: u64,
    pub(super) max_table_rows: u64,
    pub(super) max_table_cells: u64,
    pub(super) max_fixed_views: u64,
    pub(super) max_fixed_lines: u64,
    pub(super) max_placements: u64,
    pub(super) max_decorations: u64,
    pub(super) max_forms: u64,
    pub(super) max_name_hints: u64,
    pub(super) max_relations: u64,
    pub(super) max_connection_atoms: u64,
    pub(super) max_annotation_runs: u64,
    pub(super) max_annotation_mutations: u64,
    pub(super) max_relation_edges: u64,
    pub(super) max_diagnostics: u64,
    pub(super) max_transfer_objects: u64,
    pub(super) max_transfer_edges: u64,
    pub(super) max_transfer_bytes: u64,
    pub(super) max_nesting_depth: u64,
    pub(super) max_include_depth: u64,
    pub(super) reserved: u32,
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
    pub(super) fn is_valid(&self) -> bool {
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
pub(super) struct MetadataView {
    pub(super) macroset: u32,
    pub(super) presence_flags: u32,
    pub(super) title: BytesView,
    pub(super) section: BytesView,
    pub(super) volume: BytesView,
    pub(super) operating_system: BytesView,
    pub(super) architecture: BytesView,
    pub(super) name: BytesView,
    pub(super) date: BytesView,
    pub(super) alias_target: BytesView,
    pub(super) has_body: u8,
    pub(super) reserved_bytes: [u8; 3],
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct SourceView {
    pub(super) key: u32,
    pub(super) identity_kind: u32,
    pub(super) format: u32,
    pub(super) coordinate_kind: u32,
    pub(super) logical_name: BytesView,
    pub(super) decoded_length: u64,
    pub(super) hash_present: u8,
    pub(super) hash: [u8; 32],
    pub(super) reserved_bytes: [u8; 7],
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct SpanView {
    pub(super) line_column_present: u8,
    pub(super) byte_range_present: u8,
    pub(super) reserved_bytes: [u8; 2],
    pub(super) source: u32,
    pub(super) line_start: u32,
    pub(super) column_start: u32,
    pub(super) line_end: u32,
    pub(super) column_end: u32,
    pub(super) byte_start: u64,
    pub(super) byte_end: u64,
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ProvenanceView {
    pub(super) kind: u32,
    pub(super) authored_span: u32,
    pub(super) generated_trigger_span: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct OwnerView {
    pub(super) key: u32,
    pub(super) kind: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ContentRootView {
    pub(super) key: u32,
    pub(super) owner: u32,
    pub(super) ordinal: u32,
    pub(super) kind: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ContentAtomView {
    pub(super) key: u32,
    pub(super) root: u32,
    pub(super) ordinal: u32,
    pub(super) owner: u32,
    pub(super) kind: u32,
    pub(super) style_flags: u32,
    pub(super) role: u32,
    pub(super) link: u32,
    pub(super) text: BytesView,
    pub(super) display_override_present: u8,
    pub(super) display_reserved_bytes: [u8; 7],
    pub(super) display_override: BytesView,
    pub(super) whitespace_breakable: u8,
    pub(super) reserved_bytes: [u8; 3],
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ContentRefView {
    pub(super) atom: u32,
    pub(super) byte_start: u32,
    pub(super) byte_end: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ContentPointView {
    pub(super) key: u32,
    pub(super) root: u32,
    pub(super) ordinal: u32,
    pub(super) owner: u32,
    pub(super) boundary_kind: u32,
    pub(super) atom_boundary: u32,
    pub(super) atom: u32,
    pub(super) byte_offset: u32,
    pub(super) scalar_boundary: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct LinkView {
    pub(super) key: u32,
    pub(super) owner: u32,
    pub(super) target_kind: u32,
    pub(super) target_a: BytesView,
    pub(super) target_b_present: u8,
    pub(super) target_b_reserved_bytes: [u8; 7],
    pub(super) target_b: BytesView,
    pub(super) title_present: u8,
    pub(super) title_reserved_bytes: [u8; 7],
    pub(super) title: BytesView,
    pub(super) first_label_ref: u32,
    pub(super) label_ref_count: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct BlockView {
    pub(super) key: u32,
    pub(super) owner: u32,
    pub(super) kind: u32,
    pub(super) parent: u32,
    pub(super) ordinal: u32,
    pub(super) provenance: u32,
    pub(super) root: u32,
    pub(super) table: u32,
    pub(super) fixed_view: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ListView {
    pub(super) key: u32,
    pub(super) block: u32,
    pub(super) kind: u32,
    pub(super) compact: u32,
    pub(super) start: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ItemView {
    pub(super) key: u32,
    pub(super) list: u32,
    pub(super) owner: u32,
    pub(super) ordinal: u32,
    pub(super) first_form: u32,
    pub(super) form_count: u32,
    pub(super) target_present: u8,
    pub(super) target_reserved_bytes: [u8; 7],
    pub(super) target: BytesView,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct TableView {
    pub(super) key: u32,
    pub(super) block: u32,
    pub(super) fixed_view: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct TableRowView {
    pub(super) key: u32,
    pub(super) table: u32,
    pub(super) ordinal: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct TableCellView {
    pub(super) key: u32,
    pub(super) row: u32,
    pub(super) column: u32,
    pub(super) owner: u32,
    pub(super) kind: u32,
    pub(super) alignment: u32,
    pub(super) row_span: u32,
    pub(super) column_span: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct FixedView {
    pub(super) key: u32,
    pub(super) owner: u32,
    pub(super) block: u32,
    pub(super) table: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct FixedLineView {
    pub(super) key: u32,
    pub(super) view: u32,
    pub(super) ordinal: u32,
    pub(super) total_columns: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct PlacementView {
    pub(super) key: u32,
    pub(super) line: u32,
    pub(super) ordinal: u32,
    pub(super) target_kind: u32,
    pub(super) atom: u32,
    pub(super) byte_start: u32,
    pub(super) byte_end: u32,
    pub(super) point: u32,
    pub(super) scalar_start: u32,
    pub(super) scalar_end: u32,
    pub(super) column_start: u32,
    pub(super) column_end: u32,
    pub(super) cell_map_kind: u32,
    pub(super) cell_map_value: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DecorationView {
    pub(super) key: u32,
    pub(super) line: u32,
    pub(super) ordinal: u32,
    pub(super) kind: u32,
    pub(super) text: BytesView,
    pub(super) column_start: u32,
    pub(super) column_end: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct FormView {
    pub(super) key: u32,
    pub(super) owner: u32,
    pub(super) role: u32,
    pub(super) first_ref: u32,
    pub(super) ref_count: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct NameHintView {
    pub(super) key: u32,
    pub(super) form: u32,
    pub(super) first_ref: u32,
    pub(super) ref_count: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct RelationView {
    pub(super) key: u32,
    pub(super) owner: u32,
    pub(super) kind: u32,
    pub(super) target_owner: u32,
    pub(super) provenance: u32,
    pub(super) reserved: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DiagnosticView {
    pub(super) level: u32,
    pub(super) code: u32,
    pub(super) message: BytesView,
    pub(super) span: u32,
    pub(super) owner: u32,
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ResultView {
    pub(super) root_source: u32,
    pub(super) profile: u32,
    pub(super) width: u32,
    pub(super) metadata: MetadataView,
    pub(super) sources: SliceView,
    pub(super) spans: SliceView,
    pub(super) provenances: SliceView,
    pub(super) owners: SliceView,
    pub(super) content_roots: SliceView,
    pub(super) content_atoms: SliceView,
    pub(super) content_refs: SliceView,
    pub(super) content_points: SliceView,
    pub(super) links: SliceView,
    pub(super) blocks: SliceView,
    pub(super) lists: SliceView,
    pub(super) items: SliceView,
    pub(super) tables: SliceView,
    pub(super) table_rows: SliceView,
    pub(super) table_cells: SliceView,
    pub(super) fixed_views: SliceView,
    pub(super) fixed_lines: SliceView,
    pub(super) placements: SliceView,
    pub(super) decorations: SliceView,
    pub(super) forms: SliceView,
    pub(super) name_hints: SliceView,
    pub(super) relations: SliceView,
    pub(super) diagnostics: SliceView,
    pub(super) reserved: u32,
}

#[repr(C)]
pub(super) struct ResultHandleRaw {
    pub(super) _private: [u8; 0],
}

unsafe extern "C" {
    pub(super) fn mant_structured_abi_version() -> u32;
    pub(super) fn mant_structured_discriminant_fingerprint() -> u64;
    pub(super) fn mant_structured_render(
        input: *const InputView,
        limits: *const Limits,
        result: *mut *mut ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    pub(super) fn mant_structured_result_check(
        result: *const ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    pub(super) fn mant_structured_result_view(
        result: *const ResultHandleRaw,
        view: *mut ResultView,
    ) -> u32;
    pub(super) fn mant_structured_result_free(result: *mut ResultHandleRaw);
    pub(super) fn mant_structured_view_size(kind: u32) -> usize;
    pub(super) fn mant_structured_view_align(kind: u32) -> usize;
    pub(super) fn mant_structured_view_offset(kind: u32, field: u32) -> usize;
    pub(super) fn mant_structured_test_fail_after(successful_allocations: u64);
    pub(super) fn mant_structured_probe(
        input: *const InputView,
        limits: *const Limits,
        metrics: *mut ProbeMetrics,
        failure: *mut FailureView,
    ) -> u32;
}
