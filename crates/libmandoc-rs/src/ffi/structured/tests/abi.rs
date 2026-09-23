//! C/Rust ABI layout checks.

use super::*;

#[test]
#[allow(clippy::too_many_lines)] // One exhaustive native ABI fingerprint table.
fn all_frozen_view_sizes_and_alignments_match() {
    assert_eq!(unsafe { mant_structured_abi_version() }, 5);
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
        (28, size_of::<ListView>(), align_of::<ListView>()),
        (29, size_of::<ItemView>(), align_of::<ItemView>()),
        (30, size_of::<AnchorView>(), align_of::<AnchorView>()),
        (
            31,
            size_of::<HeadingEvidenceView>(),
            align_of::<HeadingEvidenceView>(),
        ),
        (
            32,
            size_of::<LinkLabelPartView>(),
            align_of::<LinkLabelPartView>(),
        ),
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
        max_nesting_depth, max_include_depth, max_anchor_evidence,
        max_heading_evidence, max_link_label_parts, reserved);
    offsets!(5, ResultView; root_source, profile, width, metadata, sources, spans,
        provenances, owners, content_roots, content_atoms, content_refs,
        content_points, links, blocks, lists, items, tables, table_rows, table_cells,
        fixed_views, fixed_lines, placements, decorations, forms, name_hints,
        relations, diagnostics, anchors, heading_evidence, link_label_parts, reserved);
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
        target_b_reserved_bytes, target_b, title_present, title_reserved_bytes, title,
        first_label_ref, label_ref_count, provenance, first_label_part, label_part_count, reserved);
    offsets!(15, BlockView; key, owner, kind, parent, ordinal, provenance,
        root, table, fixed_view, reserved);
    offsets!(16, TableView; key, block, fixed_view, provenance, reserved);
    offsets!(17, TableRowView; key, table, ordinal, kind, point, provenance, reserved);
    offsets!(18, TableCellView; key, row, column, owner, kind, alignment,
        row_span, column_span, point, provenance, reserved);
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
    offsets!(28, ListView; key, block, kind, compact, start, provenance, reserved);
    offsets!(29, ItemView; key, list, owner, ordinal, first_form, form_count,
        target_present, target_origin, target_reserved_bytes, target, provenance, reserved);
    offsets!(30, AnchorView; key, owner, point, origin, target, provenance, reserved);
    offsets!(31, HeadingEvidenceView; key, block, owner, authored_phrase_present,
        authored_phrase_reserved_bytes, authored_phrase, provenance, reserved);
    offsets!(32, LinkLabelPartView; kind, atom, byte_start, byte_end, reserved);

    let discriminants: &[u32] = &[
        0, 1, 2, 3, 4, 5, 6, 7, // status
        0, 1, 2, 3, 4, 5, 6, // stage
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
        25, 26, 27, 28, 29, 30, 31, 32, // view
        0, 1, 2, 3, // identity
        0, 1, 2, 3, // format
        0, 1, 2, // profile
        0, 1, 2, // coordinate
        0, 1, 2, 3, 4, 1, 210, // diagnostic level and native range
        0, 1, 2, 3, // provenance
        0, 1, 2, 3, 4, // atom
        0, 1, 2, 4, 8, // style bits
        0, 1, 2, 3, 4, 5, // role
        0, 1, 2, // target origin
        0, 1, 2, 3, 4, 5, 6, 7, // owner
        0, 1, 2, 3, 4, 5, // root
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, // block
        0, 1, 2, 3, 4, 5, // list
        0, 1, 2, 3, 4, 5, // link target
        0, 1, 2, // link-label part
        0, 1, 2, // point boundary
        0, 1, 2, // placement target
        0, 1, 2, 3, // cell map
        0, 1, 2, 3, 4, 5, // table cell
        0, 1, 2, 3, 4, // table row
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
