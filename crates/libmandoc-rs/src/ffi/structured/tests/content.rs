//! Prose, connection, width, and collector behavior checks.

use super::*;

#[test]
fn equation_words_follow_native_inline_execution() {
    // Exact source was checked with fixed CVS UTF-8/78 before this assertion.
    // man_term.c::print_man_node passes ROFFT_EQN to eqn_term.c::term_eqn;
    // its generated words remain inline between the neighboring prose.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "eqn.1",
            b".TH T 1\n.SH DESCRIPTION\nbefore\n.EQ\nx + y\n.EN\nafter\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude("eqn.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("native eqn execution produces structured inline content");
    let text = document
        .content_atoms
        .iter()
        .map(|atom| atom.text.as_str())
        .collect::<String>();
    assert!(text.contains("before"));
    assert!(text.contains('x'));
    assert!(text.contains('y'));
    assert!(text.contains("after"));
}

#[test]
fn nofill_lines_follow_native_line_boundaries() {
    // Both exact sources were checked with fixed CVS UTF-8/78.
    // man_term.c::print_man_node and mdoc_term.c::print_mdoc_node call
    // term_newln at NODE_LINE while TERMP_BRNEVER suppresses wrapping.
    let cases = [
        (
            "nf.1",
            InputFormat::Man,
            ".TH T 1\n.SH DESCRIPTION\nbefore\n.nf\nalpha  beta\n  gamma\n.fi\nafter\n",
        ),
        (
            "bd.1",
            InputFormat::Mdoc,
            ".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Bd -literal\nalpha  beta\n  gamma\n.Ed\nafter\n",
        ),
    ];
    for (name, format, source) in cases {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = render_prelude(name, &bundle, format, 78, &Limits::default())
            .expect("no-fill source has a structured native result");
        assert!(
            document
                .content_atoms
                .iter()
                .any(|atom| atom.text.contains("alpha"))
        );
    }
}

#[test]
fn nofill_paragraph_boundary_does_not_reuse_the_old_fixed_view() {
    // Exact sources were run through fixed CVS UTF-8/78.  man_term.c's
    // paragraph pre handler and mdoc_term.c::termp_pp_pre flush before the
    // next body; a new logical root cannot stay in the old physical view.
    for (name, format, source) in [
        (
            "pp.1",
            InputFormat::Man,
            ".TH T 1\n.SH D\n.nf\nbefore\n.PP\nafter\n.fi\n",
        ),
        (
            "pp-mdoc.1",
            InputFormat::Mdoc,
            ".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Bd -literal\nbefore\n.Pp\nafter\n.Ed\n",
        ),
    ] {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = render_prelude(name, &bundle, format, 78, &Limits::default())
            .expect("paragraph transition preserves checked fixed views");
        let after = document
            .content_atoms
            .iter()
            .filter(|atom| atom.text == "after")
            .collect::<Vec<_>>();
        assert_eq!(after.len(), 1, "{name}: after is one logical atom");
        let old_view = document.fixed_views[0].key;
        let old_lines = document
            .fixed_lines
            .iter()
            .filter(|line| line.view == old_view)
            .map(|line| line.key)
            .collect::<Vec<_>>();
        assert!(
            document.placements.iter().all(|placement| {
                placement.atom != Some(after[0].key) || !old_lines.contains(&placement.line)
            }),
            "{name}: the old fixed surface cannot also display the new paragraph"
        );
    }
}

#[test]
fn nofill_adjacent_structural_boundaries_keep_checked_views() {
    // Each exact source was run with fixed CVS UTF-8/78.  man_term.c
    // flushes the old line before rendering the next structure;
    // the structured observer must not carry a fixed root across that edge.
    for (name, format, source) in [
        (
            "section.1",
            InputFormat::Man,
            ".TH T 1\n.SH D\n.nf\nbefore\n.SH NEXT\nafter\n.fi\n",
        ),
        (
            "term.1",
            InputFormat::Man,
            ".TH T 1\n.SH D\n.nf\nbefore\n.TP\nterm\nbody\n.fi\n",
        ),
        (
            "item.1",
            InputFormat::Man,
            ".TH T 1\n.SH D\n.nf\nbefore\n.IP label\nbody\n.fi\n",
        ),
        (
            "equation.1",
            InputFormat::Man,
            ".TH T 1\n.SH D\n.nf\nbefore\n.EQ\nx + y\n.EN\nafter\n.fi\n",
        ),
    ] {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.as_bytes().to_vec()).unwrap();
        let document = render_prelude(name, &bundle, format, 78, &Limits::default())
            .unwrap_or_else(|error| panic!("{name}: adjacent native scope: {error:?}"));
        assert!(
            document
                .content_atoms
                .iter()
                .any(|atom| atom.text.contains("before")),
            "{name}"
        );
    }
}

#[test]
fn native_overstrike_marks_the_following_physical_glyph() {
    // Each exact source was checked with fixed CVS UTF-8/78 before these
    // assertions. term.c::term_field emits backspace before reducing viscol;
    // the following FIELD_PLACE, not the backspace slot's token, overlays the
    // prior glyph. The two wide cases retain different overlapping widths.
    for (name, body, prior_width, overlay_width) in [
        ("skip.1", "\\zAB", 1, 1),
        ("strike.1", "\\o'ab'", 1, 1),
        ("wide-first.1", "\\z界B", 2, 1),
        ("wide-second.1", "\\zA界", 1, 2),
    ] {
        let source = format!(".TH T 1\n.SH D\n.nf\n{body}\n.fi\n");
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.into_bytes()).unwrap();
        let document = render_prelude(name, &bundle, InputFormat::Man, 78, &Limits::default())
            .expect("native overstrike reaches the checked transfer boundary");
        let placements = document
            .placements
            .iter()
            .filter(|placement| placement.target_kind == 1)
            .collect::<Vec<_>>();
        assert!(placements.len() >= 2, "{body}: {placements:?}");
        let prior = placements[placements.len() - 2];
        let over = placements[placements.len() - 1];
        assert_eq!(over.cell_map_kind, 3, "{body}: following glyph is overlay");
        assert_eq!(over.columns.start, prior.columns.start, "{body}");
        assert_eq!(
            prior.columns.end - prior.columns.start,
            prior_width,
            "{body}"
        );
        assert_eq!(
            over.columns.end - over.columns.start,
            overlay_width,
            "{body}"
        );
    }
}

#[test]
fn multi_glyph_and_cross_font_overstrike_keep_checked_geometry() {
    // Both exact sources were run through fixed CVS UTF-8/78.  term.c's
    // backspace/placement order is unchanged across font switches and the
    // three successive positions generated by \o.
    for (name, body, minimum_overlays) in
        [("triple.1", "\\o'abc'", 2), ("font.1", "\\zA\\fBB\\fP", 1)]
    {
        let source = format!(".TH T 1\n.SH D\n.nf\n{body}\n.fi\n");
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.into_bytes()).unwrap();
        let document = render_prelude(name, &bundle, InputFormat::Man, 78, &Limits::default())
            .unwrap_or_else(|error| panic!("{name}: {error:?}"));
        assert!(
            document
                .placements
                .iter()
                .filter(|placement| placement.cell_map_kind == 3)
                .count()
                >= minimum_overlays,
            "{name}"
        );
    }
}

#[test]
fn combining_mark_does_not_consume_pending_overstrike() {
    // Exact input was checked with fixed CVS UTF-8/78: term.c::term_field
    // emits A, backspace, a zero-column combining mark, then B.  Only the
    // final visible placement consumes the pending overlay relationship.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "combining.1",
            ".TH T 1\n.SH D\n.nf\n\\zA\u{301}B\n.fi\n"
                .as_bytes()
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "combining.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("combining mark between backspace and glyph remains valid");
    assert!(
        document
            .placements
            .iter()
            .any(|placement| placement.cell_map_kind == 3)
    );
}

#[test]
fn transfer_rejects_an_unmarked_glyph_after_an_overlay() {
    // The source itself was checked with fixed CVS UTF-8/78.  This test
    // mutates only the borrowed ABI view: a prior Overlay cannot authorize
    // a later overlapping Affine placement without its own overstrike mark.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("overlay.1", b".TH T 1\n.SH D\n.nf\n\\zAB\n.fi\n".to_vec())
        .unwrap();
    let limits = Limits::default();
    let storage = InputStorage::new("overlay.1", &bundle, InputFormat::Man, &limits).unwrap();
    let (status, pointer, failure) = raw_render(&storage.view(78, PROFILE_UTF8), &limits);
    assert_eq!(status, STATUS_OK, "{failure:?}");
    let handle = ResultHandle(NonNull::new(pointer).unwrap());
    let mut view = ResultView::default();
    assert_eq!(
        unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
        STATUS_OK
    );
    let placements = unsafe {
        std::slice::from_raw_parts_mut(
            view.placements.ptr.cast::<PlacementView>().cast_mut(),
            view.placements.count as usize,
        )
    };
    assert_eq!(placements.len(), 2);
    placements[0].cell_map_kind = 3;
    placements[0].cell_map_value = 0;
    placements[1].cell_map_kind = 1;
    placements[1].cell_map_value = 1;
    assert!(copy_structured_document(&handle, &view, &limits).is_err());
}

#[test]
fn long_nofill_line_transfers_one_affine_placement() {
    // Exact 10,000-scalar source was checked with fixed CVS UTF-8/78.
    // man_term.c::print_man_node keeps the full NODE_NOFILL line unwrapped;
    // adjacent one-column terminal observations are one affine slice of its
    // logical atom, while cumulative builder work is still charged per glyph.
    let source = format!(".TH T 1\n.SH D\n.nf\n{}\n.fi\n", "a".repeat(10_000));
    let mut bundle = SourceBundle::new();
    bundle.insert("long-nofill.1", source.into_bytes()).unwrap();
    let document = render_prelude(
        "long-nofill.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("long fixed line transfers across the checked FFI boundary");
    assert_eq!(document.fixed_lines.len(), 1);
    assert_eq!(document.placements.len(), 1);
    assert_eq!(document.placements[0].scalars, 0..10_000);
    assert_eq!(document.placements[0].columns, 5..10_005);
    assert_eq!(
        (
            document.placements[0].cell_map_kind,
            document.placements[0].cell_map_value
        ),
        (1, 1)
    );
}

#[test]
fn mixed_width_long_atom_validates_each_scalar_boundary_once() {
    // This exact (a界)*2,900 no-fill source was accepted by fixed CVS UTF-8/78.
    // term.c::term_field alternates one- and two-column glyphs, so affine
    // placements cannot merge. Validation uses bounded per-atom checkpoints
    // instead of rescanning the growing UTF-8 prefix for every placement.
    let source = format!(".TH T 1\n.SH D\n.nf\n{}\n.fi\n", "a界".repeat(2_900));
    let mut bundle = SourceBundle::new();
    bundle.insert("mixed.1", source.into_bytes()).unwrap();
    let document = render_prelude("mixed.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("valid mixed-width line stays within actual result budgets");
    assert_eq!(document.fixed_lines.len(), 1);
    assert_eq!(document.fixed_lines[0].terminal_columns, 8_705);
    assert_eq!(document.placements.len(), 5_800);
    assert_eq!(document.placements.last().unwrap().scalars.end, 5_800);
}

#[test]
fn affine_line_charges_actual_edges_but_each_scalar_operation() {
    // Exact 10,000-scalar source was checked with fixed CVS UTF-8/78.
    // term.c::term_field emits each glyph; the retained affine placement
    // has one graph relation, while the executed work remains cumulative.
    let source = format!(".TH T 1\n.SH D\n.nf\n{}\n.fi\n", "a".repeat(10_000));
    let mut bundle = SourceBundle::new();
    bundle.insert("edge-line.1", source.into_bytes()).unwrap();
    let accepted = Limits {
        max_relation_edges: 200,
        ..Limits::default()
    };
    let document = render_prelude("edge-line.1", &bundle, InputFormat::Man, 78, &accepted)
        .expect("one affine placement must not spend ten thousand graph edges");
    assert_eq!(document.placements.len(), 1);

    let rejected = Limits {
        max_builder_operations: 200,
        ..Limits::default()
    };
    let error = render_prelude("edge-line.1", &bundle, InputFormat::Man, 78, &rejected)
        .expect_err("the terminal scalar work still exceeds the operation budget");
    assert_eq!((error.status, error.limit_kind), (STATUS_BUDGET, 8));
}

#[test]
fn inline_link_head_and_body_phases_do_not_split_the_surrounding_root() {
    // The exact source was run through the pinned reference first. In
    // `man_term.c::print_man_node`, UR head/body enter/leave phases surround
    // one inline formatter flow; they are not paragraph boundaries.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "inline-link.1",
            b".TH X 1\n.SH D\nBefore\n.UR https://example.test\nlabel\n.UE\nafter.\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "inline-link.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("inline link remains in one native prose root");
    let body_roots = document
        .content_roots
        .iter()
        .filter(|root| root.kind == ROOT_BODY)
        .collect::<Vec<_>>();
    assert_eq!(body_roots.len(), 1, "{document:#?}");
    let text = document
        .content_atoms
        .iter()
        .filter(|atom| atom.root == body_roots[0].key)
        .map(|atom| atom.text.as_str())
        .collect::<String>();
    assert_eq!(text, "Before label <https://example.test> after.");
}

#[test]
fn inline_mail_head_and_body_phases_do_not_split_the_surrounding_root() {
    // The exact source was run through the pinned reference first.
    // `man_term.c::print_man_node` traverses MT/ME in the surrounding flow,
    // with the same non-structural head/body boundary rule as UR/UE.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "inline-mail.1",
            b".TH X 1\n.SH D\nBefore\n.MT user@example.test\nlabel\n.ME\nafter.\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "inline-mail.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("inline mail remains in one native prose root");
    let body_roots = document
        .content_roots
        .iter()
        .filter(|root| root.kind == ROOT_BODY)
        .collect::<Vec<_>>();
    assert_eq!(body_roots.len(), 1, "{document:#?}");
    let text = document
        .content_atoms
        .iter()
        .filter(|atom| atom.root == body_roots[0].key)
        .map(|atom| atom.text.as_str())
        .collect::<String>();
    assert_eq!(text, "Before label <user@example.test> after.");
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
    let OwnedProvenance::Authored { span } =
        owned.provenances[owned.links[0].provenance as usize - 1]
    else {
        panic!("link destination must retain authored provenance: {owned:#?}");
    };
    assert_eq!(
        owned.spans[span as usize - 1]
            .line_columns
            .map(|coordinates| coordinates.0),
        Some(3),
        "the URL operand, not the first body label atom, owns the occurrence"
    );
    let label_start = owned.links[0].first_label_part as usize - 1;
    let label_end = label_start + owned.links[0].label_part_count as usize;
    assert!(!owned.link_label_parts[label_start..label_end].is_empty());
    assert!(
        owned.link_label_parts[label_start..label_end]
            .iter()
            .all(|part| {
                owned.content_atoms[part.atom as usize - 1].link == Some(owned.links[0].key)
            })
    );
    let label_styles = owned.link_label_parts[label_start..label_end]
        .iter()
        .map(|part| owned.content_atoms[part.atom as usize - 1].style_flags)
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
fn native_check_rejects_empty_partial_and_split_utf8_link_parts() {
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
    for mutation in ["empty", "partial", "split-utf8", "legacy-ref"] {
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
        let parts = unsafe {
            std::slice::from_raw_parts_mut(
                view.link_label_parts
                    .ptr
                    .cast::<LinkLabelPartView>()
                    .cast_mut(),
                view.link_label_parts.count as usize,
            )
        };
        let atoms = unsafe {
            std::slice::from_raw_parts(
                view.content_atoms.ptr.cast::<ContentAtomView>(),
                view.content_atoms.count as usize,
            )
        };
        if mutation == "legacy-ref" {
            let links = unsafe {
                std::slice::from_raw_parts_mut(
                    view.links.ptr.cast::<LinkView>().cast_mut(),
                    view.links.count as usize,
                )
            };
            links[0].first_label_ref = 1;
        } else {
            let part = parts
                .iter_mut()
                .find(|part| {
                    let atom = &atoms[part.atom as usize - 1];
                    let bytes = unsafe {
                        std::slice::from_raw_parts(
                            atom.text.ptr,
                            usize::try_from(atom.text.len).expect("test atom fits this platform"),
                        )
                    };
                    part.kind == LINK_LABEL_CONTENT
                        && std::str::from_utf8(bytes).is_ok_and(|text| match mutation {
                            "split-utf8" => text.contains('é'),
                            "partial" => text.len() > 1,
                            _ => true,
                        })
                })
                .expect("matching authored label part");
            match mutation {
                "empty" => part.byte_start = part.byte_end,
                "partial" => part.byte_start = 1,
                "split-utf8" => part.byte_start = part.byte_end - 1,
                _ => unreachable!(),
            }
        }
        let mut failure = FailureView::default();
        assert_eq!(
            unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
            STATUS_RELATION
        );
        assert!(copy_structured_document(&handle, &view, &limits).is_err());
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
        for (profile, break_count) in [(PROFILE_UTF8, utf8_breaks), (PROFILE_ASCII, ascii_breaks)] {
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
fn adjacent_ascii_glyph_projections_keep_atomic_scalar_alignment() {
    // Pinned term.c::encode1() emits each logical scalar before the ASCII
    // projection writes. The exact input was first run through the fixed
    // reference as `-T ascii`: its body displays `left----right`.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "adjacent-projections.1",
            b".TH PROJECTION 1\n.SH TEST\nleft\\[em]\\[em]right\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude_profile(
        "adjacent-projections.1",
        &bundle,
        InputFormat::Man,
        PROFILE_ASCII,
        78,
        &Limits::default(),
    )
    .expect("adjacent glyph projections retain a valid structured result");
    let projected = document
        .content_atoms
        .iter()
        .filter(|atom| atom.text.contains('\u{2014}'))
        .collect::<Vec<_>>();
    assert_eq!(
        projected.len(),
        2,
        "each displayed glyph has one logical atom"
    );
    assert!(
        projected
            .iter()
            .all(|atom| atom.text == "\u{2014}" && atom.display_override.as_deref() == Some("--"))
    );
}

#[test]
fn ascii_overstrike_projection_degrades_to_the_safe_logical_scalar() {
    // The exact input was run through the fixed `-T ascii -Owidth=78`
    // reference: `\[ct]` emits `2f 08 63` (`/\bc`). Pinned
    // term_ascii.c::ascii_uc2str and term.c::encode project these bytes
    // without changing the native renderer's output. The structured store
    // cannot carry the backspace as an inline display override.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("cent.1", b".TH X 1\n.SH NAME\nX \\[ct] Y\n".to_vec())
        .unwrap();
    let document = render_prelude_profile(
        "cent.1",
        &bundle,
        InputFormat::Man,
        PROFILE_ASCII,
        78,
        &Limits::default(),
    )
    .expect("unsafe device projection cannot invalidate a legal manual");
    let cent = document
        .content_atoms
        .iter()
        .find(|atom| atom.text == "¢")
        .expect("cent sign remains one logical scalar");
    assert_eq!(cent.display_override, None);
    assert!(document.content_atoms.iter().any(|atom| atom.text == "X"));
    assert!(document.content_atoms.iter().any(|atom| atom.text == "Y"));
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
    // The exact `.SS` input was checked with fixed CVS UTF-8/78. Subsection
    // hierarchy still lacks a complete structured representation; no-fill
    // and eqn now have their own positive native-result tests above.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "subsection.1",
            b".TH UNSUP 1\n.SH TOP\n.SS CHILD\ntext\n".to_vec(),
        )
        .unwrap();
    let error = render_prelude(
        "subsection.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect_err("unsupported shape must not return a partial document");
    assert_eq!((error.status, error.stage), (STATUS_UNSUPPORTED, 4));
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
