//! Handle-bound borrowed views and the private owned transfer model.

use super::{
    BlockView, BytesView, ContentAtomView, ContentPointView, ContentRefView, ContentRootView,
    DecorationView, DiagnosticView, FixedLineView, FixedView, FormView, ItemView, Limits, LinkView,
    ListView, MetadataView, NameHintView, NativeStructuredError, OwnerView, PROFILE_ASCII,
    PROFILE_UTF8, PROVENANCE_AUTHORED, PROVENANCE_GENERATED, PROVENANCE_UNKNOWN, PlacementView,
    ProvenanceView, RelationView, ResultHandleRaw, ResultView, SourceView, SpanView, TableCellView,
    TableRowView, TableView, alloc_error, checked_slice, mant_structured_result_free,
    relation_error, transfer_preflight, validate_metadata, validate_structured_relations,
};
use std::ptr::NonNull;

pub(super) struct ResultHandle(pub(super) NonNull<ResultHandleRaw>);

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
pub(crate) struct OwnedList {
    pub(crate) key: u32,
    pub(crate) block: u32,
    pub(crate) kind: u32,
    pub(crate) compact: bool,
    pub(crate) start: Option<u32>,
    pub(crate) provenance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedItem {
    pub(crate) key: u32,
    pub(crate) list: u32,
    pub(crate) owner: u32,
    pub(crate) ordinal: u32,
    pub(crate) first_form: Option<u32>,
    pub(crate) form_count: u32,
    pub(crate) target: Option<String>,
    pub(crate) provenance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedForm {
    pub(crate) key: u32,
    pub(crate) owner: u32,
    pub(crate) role: Option<u32>,
    pub(crate) first_ref: u32,
    pub(crate) ref_count: u32,
    pub(crate) provenance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnedNameHint {
    pub(crate) key: u32,
    pub(crate) form: u32,
    pub(crate) first_ref: u32,
    pub(crate) ref_count: u32,
    pub(crate) provenance: u32,
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
    pub(crate) lists: Vec<OwnedList>,
    pub(crate) items: Vec<OwnedItem>,
    pub(crate) forms: Vec<OwnedForm>,
    pub(crate) name_hints: Vec<OwnedNameHint>,
    pub(crate) diagnostics: Vec<OwnedDiagnostic>,
}

pub(super) struct StructuredSlices<'a> {
    pub(super) sources: &'a [SourceView],
    pub(super) spans: &'a [SpanView],
    pub(super) provenances: &'a [ProvenanceView],
    pub(super) owners: &'a [OwnerView],
    pub(super) content_roots: &'a [ContentRootView],
    pub(super) content_atoms: &'a [ContentAtomView],
    pub(super) content_refs: &'a [ContentRefView],
    pub(super) content_points: &'a [ContentPointView],
    pub(super) links: &'a [LinkView],
    pub(super) blocks: &'a [BlockView],
    pub(super) lists: &'a [ListView],
    pub(super) items: &'a [ItemView],
    pub(super) tables: &'a [TableView],
    pub(super) table_rows: &'a [TableRowView],
    pub(super) table_cells: &'a [TableCellView],
    pub(super) fixed_views: &'a [FixedView],
    pub(super) fixed_lines: &'a [FixedLineView],
    pub(super) placements: &'a [PlacementView],
    pub(super) decorations: &'a [DecorationView],
    pub(super) forms: &'a [FormView],
    pub(super) name_hints: &'a [NameHintView],
    pub(super) relations: &'a [RelationView],
    pub(super) diagnostics: &'a [DiagnosticView],
}
#[allow(clippy::too_many_lines)] // Keeps the frozen C-to-owned transfer audit in one sequence.
pub(super) fn copy_structured_document(
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
        lists: checked_slice::<ListView>(view.lists, handle)?,
        items: checked_slice::<ItemView>(view.items, handle)?,
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
    let mut owned_lists = Vec::new();
    let mut owned_items = Vec::new();
    let mut owned_forms = Vec::new();
    let mut owned_name_hints = Vec::new();
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
    owned_lists
        .try_reserve_exact(slices.lists.len())
        .map_err(alloc_error)?;
    owned_items
        .try_reserve_exact(slices.items.len())
        .map_err(alloc_error)?;
    owned_forms
        .try_reserve_exact(slices.forms.len())
        .map_err(alloc_error)?;
    owned_name_hints
        .try_reserve_exact(slices.name_hints.len())
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
    for list in slices.lists {
        owned_lists.push(OwnedList {
            key: list.key,
            block: list.block,
            kind: list.kind,
            compact: list.compact == 1,
            start: (list.start != 0).then_some(list.start),
            provenance: list.provenance,
        });
    }
    for item in slices.items {
        owned_items.push(OwnedItem {
            key: item.key,
            list: item.list,
            owner: item.owner,
            ordinal: item.ordinal,
            first_form: (item.first_form != 0).then_some(item.first_form),
            form_count: item.form_count,
            target: (item.target_present == 1)
                .then(|| copy_string(item.target))
                .transpose()?,
            provenance: item.provenance,
        });
    }
    for form in slices.forms {
        owned_forms.push(OwnedForm {
            key: form.key,
            owner: form.owner,
            role: (form.role != 0).then_some(form.role),
            first_ref: form.first_ref,
            ref_count: form.ref_count,
            provenance: form.provenance,
        });
    }
    for hint in slices.name_hints {
        owned_name_hints.push(OwnedNameHint {
            key: hint.key,
            form: hint.form,
            first_ref: hint.first_ref,
            ref_count: hint.ref_count,
            provenance: hint.provenance,
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
        lists: owned_lists,
        items: owned_items,
        forms: owned_forms,
        name_hints: owned_name_hints,
        diagnostics: owned_diagnostics,
    })
}
pub(super) fn copy_string(view: BytesView) -> Result<String, NativeStructuredError> {
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
