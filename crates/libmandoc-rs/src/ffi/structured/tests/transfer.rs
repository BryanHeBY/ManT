//! Handle-bound view and owned transfer checks.

use super::*;

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
    let empty_lists: [ListView; 0] = [];
    let empty_items: [ItemView; 0] = [];
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
        lists: abi_slice(&empty_lists),
        items: abi_slice(&empty_items),
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
    let expected_bytes =
        u64::try_from(source_name.len() + atom_text.len() + diagnostic_text.len() + table_bytes)
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
