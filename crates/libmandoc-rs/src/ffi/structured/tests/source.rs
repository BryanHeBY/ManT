//! Source identity, include provenance, and diagnostic checks.

use super::*;

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
        let OwnedProvenance::Authored { span } = owned.provenances[atom.provenance as usize - 1]
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
fn macro_reparse_diagnostic_keeps_source_without_claiming_authored_columns() {
    // Both exact inputs ran the pinned CVS reference with -Wstyle -Ttree.
    // read.c::mparse_buf_r reparses .EE at its call-site line, while
    // roff.c::roff_userdef substitutes its body: the 6:6 diagnostic
    // addresses expanded text even when the authored invocation is long
    // enough that a bounds-only heuristic would accept that column.
    for call in [".EE", ".EE ignored"] {
        let input = format!(".TH T 1\n.SH TEST\n.de EE\n.    fi\n..\n{call}\n.fi\ntext\n");
        let mut bundle = SourceBundle::new();
        bundle.insert("expanded.1", input.into_bytes()).unwrap();
        let owned = render_prelude(
            "expanded.1",
            &bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("a macro expansion diagnostic must not reject the whole page");
        let fill_warnings = owned
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.message.contains("fill mode already enabled"))
            .map(|diagnostic| diagnostic_span(&owned, diagnostic).expect("sourced diagnostic"))
            .collect::<Vec<_>>();
        assert_eq!(fill_warnings.len(), 2, "{call}");
        assert_eq!(fill_warnings[0].source, 1, "{call}");
        assert_eq!(fill_warnings[0].line_columns, None, "{call}");
        assert_eq!(fill_warnings[1].source, 1, "{call}");
        assert_eq!(fill_warnings[1].line_columns, Some((7, 2, 0, 0)), "{call}");
        assert!(owned.content_atoms.iter().any(|atom| atom.text == "text"));
    }
}

#[test]
fn macro_generated_content_keeps_source_identity_without_authored_positions() {
    // Both exact inputs ran the pinned CVS reference with -Thtml.  It emits
    // OPTIONS, --macro-generated, Description., and the linked label for
    // both invocation spellings.  read.c::mparse_buf_r reparses the macro
    // body at the call-site source key, but its node columns are expanded
    // coordinates even when the call is long enough to contain them.
    for call in [".EE", ".EE ignored-padding-for-coordinate-check"] {
        let input = format!(
            ".TH T 1\n.de EE\n.SH OPTIONS\n.TP\n.B --macro-generated\nDescription.\n.UR https://example.test/x\nlabel\n.UE\n..\n{call}\n"
        );
        let mut bundle = SourceBundle::new();
        bundle.insert("expanded.1", input.into_bytes()).unwrap();
        let owned = render_prelude(
            "expanded.1",
            &bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("macro-generated content retains source-only provenance");
        let visible = owned
            .content_atoms
            .iter()
            .map(|atom| atom.text.as_str())
            .collect::<String>();
        for marker in ["OPTIONS", "--macro-generated", "Description.", "label"] {
            assert!(visible.contains(marker), "missing {marker} for {call}");
        }
        assert!(
            owned
                .heading_evidence
                .iter()
                .any(|heading| heading.authored_phrase.as_deref() == Some("OPTIONS"))
        );
        for atom in &owned.content_atoms {
            let OwnedProvenance::Generated {
                trigger_span: Some(span),
            } = owned.provenances[atom.provenance as usize - 1]
            else {
                panic!("{} falsely has authored provenance for {call}", atom.text);
            };
            assert_eq!(owned.spans[span as usize - 1].source, 1, "{call}");
            assert_eq!(owned.spans[span as usize - 1].line_columns, None, "{call}");
        }
        let link = owned
            .links
            .iter()
            .find(|link| link.target_a == "https://example.test/x")
            .expect("generated UR occurrence");
        let OwnedProvenance::Generated {
            trigger_span: Some(span),
        } = owned.provenances[link.provenance as usize - 1]
        else {
            panic!("generated link falsely has authored provenance for {call}");
        };
        assert_eq!(owned.spans[span as usize - 1].source, 1, "{call}");
        assert_eq!(owned.spans[span as usize - 1].line_columns, None, "{call}");
    }
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
