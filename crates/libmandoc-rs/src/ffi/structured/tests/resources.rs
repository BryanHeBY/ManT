//! Failure, budget, malformed input, and re-entry checks.

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
    let allocation = render_prelude("root.1", &bundle, InputFormat::Man, 78, &Limits::default())
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
fn fixed_table_allocation_failures_release_active_token_sidecars() {
    // Exact source checked with fixed CVS UTF-8/78. tbl_term.c::term_tbl
    // flushes buffered authored cells and direct borders; injected failures
    // can interrupt before the current token retires. Run this matrix under
    // a leak sanitizer to audit the native cleanup path as well as recovery.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "box.1",
            b".TH T 1\n.SH DATA\n.TS\nbox tab(;);\nl l.\nleft;right\nempty;\n.TE\n".to_vec(),
        )
        .unwrap();
    let mut failures = 0;
    for successful_allocations in 0..128 {
        unsafe { mant_structured_test_fail_after(successful_allocations) };
        match render_prelude("box.1", &bundle, InputFormat::Man, 78, &Limits::default()) {
            Ok(document) => assert_eq!(document.table_cells.len(), 4),
            Err(error) => {
                assert_eq!(error.status, STATUS_BUILDER_ALLOC, "{error:?}");
                failures += 1;
            }
        }
    }
    assert!(failures > 0);
    let recovered = render_prelude("box.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("the next complete fixed-table call recovers");
    assert_eq!(recovered.table_cells.len(), 4);
}

#[test]
fn c04_evidence_budget_kind_survives_failure_mapping_and_recovers() {
    // The exact two-target input was run through the fixed reference first;
    // pinned tag.c moves each ID to its following paragraph. The generated
    // heading target and the two explicit targets all count as AnchorEvidence
    // rows. The dedicated budget reports limit kind 37, and the failure cannot
    // poison the next same-thread session.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "anchors.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh TARGETS\n.Tg first\n.Pp\none\n.Tg second\n.Pp\ntwo\n"
                .to_vec(),
        )
        .unwrap();
    let limits = Limits {
        max_anchor_evidence: 1,
        ..Limits::default()
    };
    let error = render_prelude("anchors.1", &bundle, InputFormat::Mdoc, 78, &limits)
        .expect_err("second anchor exceeds its dedicated evidence budget");
    assert_eq!(error.status, STATUS_BUDGET, "{error:?}");
    assert_eq!(error.limit_kind, 37, "{error:?}");
    assert!(error.observed > error.allowed, "{error:?}");

    let recovered = render_prelude(
        "anchors.1",
        &bundle,
        InputFormat::Mdoc,
        78,
        &Limits::default(),
    )
    .expect("a following session recovers after the evidence budget failure");
    assert_eq!(recovered.anchors.len(), 3, "{recovered:#?}");
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
    let source_map = render_prelude("root.1", &bundle, InputFormat::Man, 78, &source_map_limits)
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
