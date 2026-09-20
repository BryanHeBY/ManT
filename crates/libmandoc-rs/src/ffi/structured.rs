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
pub(crate) struct OwnedDiagnostic {
    pub(crate) level: u32,
    pub(crate) code: u32,
    pub(crate) message: String,
    pub(crate) span: Option<OwnedSpan>,
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
pub(crate) struct OwnedPrelude {
    pub(crate) root_source: u32,
    pub(crate) profile: u32,
    pub(crate) width: u32,
    pub(crate) metadata: OwnedMetadata,
    pub(crate) sources: Vec<OwnedSource>,
    pub(crate) diagnostics: Vec<OwnedDiagnostic>,
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
    fn new(root: &str, bundle: &'a SourceBundle, format: InputFormat) -> Result<Self, String> {
        let format = match format {
            InputFormat::Man => FORMAT_MAN,
            InputFormat::Mdoc => FORMAT_MDOC,
            InputFormat::Auto => {
                return Err("structured rendering requires an explicit man or mdoc format".into());
            }
        };
        let names = bundle.sources().map(|(name, _)| name).collect::<Vec<_>>();
        let root_index = names
            .iter()
            .position(|name| *name == root)
            .ok_or_else(|| "source bundle does not contain the requested root".to_owned())?;
        let source_count =
            u32::try_from(names.len()).map_err(|_| "too many structured input sources")?;
        let descriptors = bundle
            .sources()
            .map(|(name, bytes)| InputSourceView {
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
            })
            .collect();
        Ok(Self {
            _bundle: bundle,
            descriptors,
            source_count,
            root_input: u32::try_from(root_index + 1).map_err(|_| "too many sources")?,
        })
    }

    fn view(&self, width: u32) -> InputView {
        InputView {
            sources: SliceView {
                ptr: self.descriptors.as_ptr().cast(),
                count: self.source_count,
                stride: u32::try_from(std::mem::size_of::<InputSourceView>())
                    .expect("input-source ABI size fits u32"),
            },
            root_input: self.root_input,
            profile: PROFILE_UTF8,
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
) -> Result<OwnedPrelude, NativeStructuredError> {
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
    let storage = InputStorage::new(root, bundle, format).map_err(|_| NativeStructuredError {
        status: STATUS_INVALID_INPUT,
        stage: 1,
        limit_kind: 0,
        observed: 0,
        allowed: 0,
    })?;
    let input = storage.view(width);
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
    copy_prelude(&handle, &view, limits)
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
fn copy_prelude(
    handle: &ResultHandle,
    view: &ResultView,
    limits: &Limits,
) -> Result<OwnedPrelude, NativeStructuredError> {
    if !(PROFILE_UTF8..=PROFILE_ASCII).contains(&view.profile)
        || view.width == 0
        || view.reserved != 0
    {
        return Err(relation_error());
    }
    let sources = checked_slice::<SourceView>(view.sources, handle)?;
    let spans = checked_slice::<SpanView>(view.spans, handle)?;
    let diagnostics = checked_slice::<DiagnosticView>(view.diagnostics, handle)?;
    validate_empty_body_tables(view, handle)?;
    validate_metadata(view.metadata)?;
    transfer_preflight(view, sources, spans, diagnostics, limits)?;
    let mut owned_sources = Vec::new();
    owned_sources
        .try_reserve_exact(sources.len())
        .map_err(alloc_error)?;
    for (index, source) in sources.iter().enumerate() {
        if source.key != u32::try_from(index).map_err(|_| relation_error())? + 1
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
    if view.root_source == 0 || view.root_source as usize > owned_sources.len() {
        return Err(relation_error());
    }
    if view.metadata.macroset != owned_sources[view.root_source as usize - 1].format {
        return Err(relation_error());
    }
    let mut owned_diagnostics = Vec::new();
    owned_diagnostics
        .try_reserve_exact(diagnostics.len())
        .map_err(alloc_error)?;
    for diagnostic in diagnostics {
        if diagnostic.reserved != 0
            || !(DIAGNOSTIC_STYLE..=DIAGNOSTIC_UNSUPPORTED).contains(&diagnostic.level)
            || !(1..=DIAGNOSTIC_CODE_NATIVE_LAST).contains(&diagnostic.code)
            || diagnostic.span as usize > spans.len()
            || diagnostic.owner != 0
        {
            return Err(relation_error());
        }
        let span = if diagnostic.span == 0 {
            None
        } else {
            let span = &spans[diagnostic.span as usize - 1];
            if span.reserved != 0
                || span.reserved_bytes != [0; 2]
                || span.line_column_present > 1
                || span.byte_range_present > 1
                || span.source == 0
                || span.source as usize > sources.len()
                || !valid_span(span, &sources[span.source as usize - 1])
            {
                return Err(relation_error());
            }
            Some(OwnedSpan {
                source: span.source,
                line_columns: (span.line_column_present == 1).then_some((
                    span.line_start,
                    span.column_start,
                    span.line_end,
                    span.column_end,
                )),
                byte_range: (span.byte_range_present == 1)
                    .then_some(span.byte_start..span.byte_end),
            })
        };
        owned_diagnostics.push(OwnedDiagnostic {
            level: diagnostic.level,
            code: diagnostic.code,
            message: copy_string(diagnostic.message)?,
            span,
        });
    }
    Ok(OwnedPrelude {
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

fn valid_span(span: &SpanView, source: &SourceView) -> bool {
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
    let bytes_valid = if span.byte_range_present == 0 {
        span.byte_start == 0 && span.byte_end == 0
    } else {
        span.byte_start <= span.byte_end && span.byte_end <= source.decoded_length
    };
    line_valid && bytes_valid
}

fn transfer_preflight(
    view: &ResultView,
    sources: &[SourceView],
    spans: &[SpanView],
    diagnostics: &[DiagnosticView],
    limits: &Limits,
) -> Result<(), NativeStructuredError> {
    // The top-level result view and metadata are records in addition to every
    // typed table row validated below.
    let mut objects = 2_u64;
    let mut edges = 0_u64;
    let mut bytes = 0_u64;
    for count in [sources.len(), spans.len(), diagnostics.len()] {
        objects = objects
            .checked_add(u64::try_from(count).map_err(|_| relation_error())?)
            .ok_or_else(relation_error)?;
    }
    edges = edges
        .checked_add(u64::try_from(sources.len()).map_err(|_| relation_error())?)
        .and_then(|value| {
            value.checked_add(diagnostics.iter().filter(|d| d.span != 0).count() as u64)
        })
        .ok_or_else(relation_error)?;
    for source in sources {
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
    for diagnostic in diagnostics {
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

fn validate_empty_body_tables(
    view: &ResultView,
    handle: &ResultHandle,
) -> Result<(), NativeStructuredError> {
    macro_rules! empty {
        ($field:ident, $ty:ty) => {
            if !checked_slice::<$ty>(view.$field, handle)?.is_empty() {
                return Err(relation_error());
            }
        };
    }
    empty!(provenances, ProvenanceView);
    empty!(owners, OwnerView);
    empty!(content_roots, ContentRootView);
    empty!(content_atoms, ContentAtomView);
    empty!(content_refs, ContentRefView);
    empty!(content_points, ContentPointView);
    empty!(links, LinkView);
    empty!(blocks, BlockView);
    empty!(tables, TableView);
    empty!(table_rows, TableRowView);
    empty!(table_cells, TableCellView);
    empty!(fixed_views, FixedView);
    empty!(fixed_lines, FixedLineView);
    empty!(placements, PlacementView);
    empty!(decorations, DecorationView);
    empty!(forms, FormView);
    empty!(name_hints, NameHintView);
    empty!(relations, RelationView);
    Ok(())
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
    fn body_is_not_reported_as_a_complete_empty_document() {
        // The registered oracle renders `body` from this exact input.
        // Until C02b collects term.c output, the structured entry must fail.
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "body.1",
                b".TH BODY 1 \"2026-09-20\"\n.SH NAME\nbody\n".to_vec(),
            )
            .unwrap();
        let error = render_prelude("body.1", &bundle, InputFormat::Man, 78, &Limits::default())
            .expect_err("C02a must reject uncollected body content");
        assert_eq!(error.status, STATUS_UNSUPPORTED);
        assert_eq!(error.stage, 4);
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
            .filter_map(|diagnostic| diagnostic.span.as_ref().map(|span| span.source))
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
                    .as_ref()
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
                    .as_ref()
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
        let mut storage = InputStorage::new("root.1", &bundle, InputFormat::Man).unwrap();
        let include = storage
            .descriptors
            .iter_mut()
            .find(|source| source.logical_name.len == "included.1".len() as u64)
            .unwrap();
        include.format = FORMAT_MDOC;
        let input = storage.view(78);
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
            + baseline.diagnostics.len() as u64
            + baseline
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.span.is_some())
                .count() as u64;
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
            let mut storage = InputStorage::new("root.1", &bundle, InputFormat::Man).unwrap();
            storage.descriptors[0].identity_kind = kind;
            storage.descriptors[0].logical_name = BytesView {
                ptr: identity.as_ptr(),
                len: u64::try_from(identity.len()).unwrap(),
            };
            if kind == IDENTITY_BUNDLE_MEMBER {
                storage.descriptors[0].resolver_name = storage.descriptors[0].logical_name;
            }
            let (status, pointer, failure) = raw_render(&storage.view(78), &Limits::default());
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
        let storage = InputStorage::new("root.1", &bundle, InputFormat::Man).unwrap();

        let mut malformed = storage.view(78);
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
            let mut input = storage.view(78);
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
        let storage = InputStorage::new("root.1", &outer, InputFormat::Man).unwrap();
        let mut context = ReentryContext { observed_status: 0 };
        let mut input = storage.view(78);
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
}
