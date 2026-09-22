use super::*;

fn fixture(name: &str, source: &[u8], format: InputFormat) -> OwnedStructuredDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert(name, source.to_vec()).unwrap();
    render_prelude(name, &bundle, format, 18, &Limits::default())
        .expect("C04 address fixture renders")
}

#[test]
fn mdoc_heading_identity_uses_authored_phrase_and_sx_target() {
    // Oracle: pinned `mdoc_validate.c::post_section` applies `deroff()` to
    // `White Space` before terminal `.Sm off` joins the displayed heading.
    // `mdoc_html.c::mdoc_sx_pre` uses the same authored identity domain.
    let document = fixture(
        "links.1",
        b".Dd September 21, 2026\n.Dt LINKS 1\n.Os\n.Sm off\n.Sh White Space\n.Sm on\n.Pp\n.Sx White Space\n.Pp\n.Lk https://example.test/very/long/target visible-label\n.Pp\n.Xr printf 3\n",
        InputFormat::Mdoc,
    );
    assert_eq!(document.heading_evidence.len(), 1, "{document:#?}");
    assert_eq!(
        document.heading_evidence[0].authored_phrase.as_deref(),
        Some("White Space")
    );
    let section = document
        .links
        .iter()
        .find(|link| link.target_kind == 5)
        .expect("Sx becomes one section link occurrence");
    assert_eq!(section.target_a, "White Space");
}

#[test]
fn zero_width_targets_use_points_and_leave_legacy_item_fields_zero() {
    // Oracle: pinned `mdoc_validate.c::post_tg` moves Root.Target to Pp,
    // while Empty.Item stays on its own zero-width Tg before the empty It.
    let document = fixture(
        "anchors.1",
        b".Dd September 21, 2026\n.Dt LINKS 1\n.Os\n.Sh TARGETS\n.Tg Root.Target\n.Pp\nbody\n.Bl -bullet\n.Tg Empty.Item\n.It\n.El\n",
        InputFormat::Mdoc,
    );
    for target in ["Root.Target", "Empty.Item"] {
        let matches = document
            .anchors
            .iter()
            .filter(|anchor| anchor.target == target)
            .collect::<Vec<_>>();
        assert_eq!(
            matches.len(),
            1,
            "duplicate or missing {target}: {document:#?}"
        );
        let anchor = matches[0];
        let point = &document.content_points[anchor.point as usize - 1];
        assert_eq!(point.owner, anchor.owner);
    }
    let empty_item = &document.items[0];
    let empty_anchor = document
        .anchors
        .iter()
        .find(|anchor| anchor.target == "Empty.Item")
        .unwrap();
    assert_eq!(empty_anchor.owner, empty_item.owner);

    let generated = document
        .anchors
        .iter()
        .find(|anchor| anchor.target == "TARGETS")
        .expect("automatic heading target is retained");
    assert!(matches!(
        document.provenances[generated.provenance as usize - 1],
        OwnedProvenance::Generated {
            trigger_span: Some(_)
        }
    ));
    let authored = document
        .anchors
        .iter()
        .find(|anchor| anchor.target == "Root.Target")
        .unwrap();
    assert!(matches!(
        document.provenances[authored.provenance as usize - 1],
        OwnedProvenance::Authored { .. }
    ));
}

#[test]
fn one_link_occurrence_spans_roots_and_retains_hard_break_parts() {
    // Oracle: pinned `man_term.c::pre_UR/post_UR` keeps one UR block across
    // PP; pinned `roff_term.c::roff_term_pre_br` invokes `term_newln()`.
    let cross_root = fixture(
        "cross-root.1",
        b".TH LINKS 1\n.SH TARGET\n.UR https://example.test/cross-root\nbefore\n.PP\nafter\n.UE\n",
        InputFormat::Man,
    );
    assert_eq!(cross_root.links.len(), 1, "{cross_root:#?}");
    let link = &cross_root.links[0];
    let roots = cross_root.link_label_parts[link.first_label_part as usize - 1
        ..link.first_label_part as usize - 1 + link.label_part_count as usize]
        .iter()
        .map(|part| cross_root.content_atoms[part.atom as usize - 1].root)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(roots.len() > 1, "{cross_root:#?}");

    let hard_break = fixture(
        "hard-break.1",
        b".TH LINKS 1\n.SH TARGET\n.UR https://example.test/hard-break\nbefore\n.br\n.B after\n.UE\n",
        InputFormat::Man,
    );
    let link = &hard_break.links[0];
    let parts = &hard_break.link_label_parts[link.first_label_part as usize - 1
        ..link.first_label_part as usize - 1 + link.label_part_count as usize];
    assert!(
        parts.iter().any(|part| {
            part.kind == LINK_LABEL_HARD_BREAK
                && hard_break.content_atoms[part.atom as usize - 1].link == Some(link.key)
        }),
        "{hard_break:#?}"
    );
}

#[test]
fn sectionless_xr_and_mr_keep_document_targets() {
    // These exact sources were run through the pinned reference first.
    // `mdoc_term.c::termp_xr_pre` accepts a name without section, and
    // `man_term.c::pre_MR` prints the same case as `printf()`.
    let xr = fixture(
        "xr.1",
        b".Dd September 23, 2026\n.Dt X 1\n.Os\n.Sh SEE ALSO\n.Xr printf\n",
        InputFormat::Mdoc,
    );
    assert_eq!(xr.links.len(), 1, "{xr:#?}");
    assert_eq!(xr.links[0].target_kind, 3);
    assert_eq!(xr.links[0].target_a, "printf");
    assert_eq!(xr.links[0].target_b, None);

    let mr = fixture(
        "mr.1",
        b".TH X 1\n.SH SEE ALSO\n.MR printf\n",
        InputFormat::Man,
    );
    assert_eq!(mr.links.len(), 1, "{mr:#?}");
    assert_eq!(mr.links[0].target_kind, 3);
    assert_eq!(mr.links[0].target_a, "printf");
    assert_eq!(mr.links[0].target_b, None);
}

#[test]
fn multiple_mdoc_mail_addresses_have_independent_links_at_narrow_width() {
    // This exact 18-column source was run through the pinned reference first.
    // `mdoc_html.c::mdoc_mt_pre` gives each child its own mailto target;
    // `term.c::term_word_node` keeps the child identity while the narrow
    // terminal places each overlong address on its own physical line.
    let document = fixture(
        "multiple-mail.1",
        b".Dd September 23, 2026\n.Dt X 1\n.Os\n.Sh AUTHORS\n.Mt first.long.address@example.test second.long.address@example.test\n",
        InputFormat::Mdoc,
    );
    assert_eq!(document.links.len(), 2, "{document:#?}");
    for (index, address) in [
        "first.long.address@example.test",
        "second.long.address@example.test",
    ]
    .iter()
    .enumerate()
    {
        let link = &document.links[index];
        assert_eq!(link.target_kind, 2);
        assert_eq!(&link.target_a, address);
        assert!(matches!(
            document.provenances[link.provenance as usize - 1],
            OwnedProvenance::Authored { .. }
        ));
        let parts = &document.link_label_parts[link.first_label_part as usize - 1
            ..link.first_label_part as usize - 1 + link.label_part_count as usize];
        let label = parts
            .iter()
            .map(|part| &document.content_atoms[part.atom as usize - 1])
            .map(|atom| {
                assert_eq!(atom.link, Some(link.key));
                atom.text.as_str()
            })
            .collect::<String>();
        assert_eq!(label, *address);
    }
}

#[test]
fn default_mdoc_mail_target_has_generated_provenance() {
    // This exact source was run through the pinned reference first.
    // `mdoc_validate.c::post_defaults` inserts `~` with NODE_NOSRC, so the
    // visible mailto target is generated by `.Mt`, not authored source text.
    let document = fixture(
        "default-mail.1",
        b".Dd September 23, 2026\n.Dt X 1\n.Os\n.Sh AUTHORS\n.Mt\n",
        InputFormat::Mdoc,
    );
    assert_eq!(document.links.len(), 1, "{document:#?}");
    let link = &document.links[0];
    assert_eq!(link.target_kind, 2);
    assert_eq!(link.target_a, "~");
    assert!(matches!(
        document.provenances[link.provenance as usize - 1],
        OwnedProvenance::Generated {
            trigger_span: Some(_)
        }
    ));
    let parts = &document.link_label_parts[link.first_label_part as usize - 1
        ..link.first_label_part as usize - 1 + link.label_part_count as usize];
    assert_eq!(parts.len(), 1);
    let label_atom = &document.content_atoms[parts[0].atom as usize - 1];
    assert_eq!(label_atom.text, "~");
    assert!(matches!(
        document.provenances[label_atom.provenance as usize - 1],
        OwnedProvenance::Generated {
            trigger_span: Some(_)
        }
    ));
}

#[test]
fn included_address_evidence_keeps_the_emitting_source() {
    // Oracle: the exact two-file input was run through the fixed reference.
    // Pinned `read.c::mparse_readmem` restores the include source before
    // `mdoc_validate.c::{post_section,post_tg,post_sx}` records evidence.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "root.1",
            b".Dd September 22, 2026\n.Dt INCLUDE-ADDRESS 1\n.Os\n.Sh ROOT\n.Sx INCLUDED\n.so member.1\n"
                .to_vec(),
        )
        .unwrap();
    bundle
        .insert(
            "member.1",
            b".Sh INCLUDED\n.Tg Include.Target\n.Pp\n.Lk https://example.test/include included-link\n"
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude("root.1", &bundle, InputFormat::Mdoc, 78, &Limits::default())
        .expect("included address evidence renders");
    let source_name = |provenance: u32| {
        let span = match document.provenances[provenance as usize - 1] {
            OwnedProvenance::Authored { span }
            | OwnedProvenance::Generated {
                trigger_span: Some(span),
            } => span,
            ref other => panic!("address evidence lacks a source: {other:?}"),
        };
        document.sources[document.spans[span as usize - 1].source as usize - 1]
            .logical_name
            .as_str()
    };

    let heading = document
        .heading_evidence
        .iter()
        .find(|evidence| evidence.authored_phrase.as_deref() == Some("INCLUDED"))
        .expect("included heading evidence");
    assert_eq!(source_name(heading.provenance), "member.1");
    let generated_heading_anchor = document
        .anchors
        .iter()
        .find(|anchor| anchor.target == "INCLUDED")
        .expect("included generated heading target");
    assert_eq!(source_name(generated_heading_anchor.provenance), "member.1");
    let anchor = document
        .anchors
        .iter()
        .find(|anchor| anchor.target == "Include.Target")
        .expect("included authored target");
    assert_eq!(source_name(anchor.provenance), "member.1");
    let external = document
        .links
        .iter()
        .find(|link| link.target_a == "https://example.test/include")
        .expect("included external link");
    assert_eq!(source_name(external.provenance), "member.1");
    let section = document
        .links
        .iter()
        .find(|link| link.target_kind == 5 && link.target_a == "INCLUDED")
        .expect("root section cross-reference");
    assert_eq!(source_name(section.provenance), "root.1");
}

#[test]
fn trailing_owners_without_address_queues_finalize_safely() {
    // Oracle: the exact ten-empty-item input was run through the fixed
    // reference; pinned `mdoc_term.c::termp_it_pre` renders each bullet even
    // though no item creates address evidence. Address queues are lazy, so
    // finalization must not index them by the larger owner table length.
    let document = fixture(
        "empty-owners.1",
        b".Dd September 22, 2026\n.Dt EMPTY-OWNERS 1\n.Os\n.Sh ITEMS\n.Bl -bullet\n.It\n.It\n.It\n.It\n.It\n.It\n.It\n.It\n.It\n.It\n.El\n",
        InputFormat::Mdoc,
    );
    assert_eq!(document.items.len(), 10, "{document:#?}");
    assert!(document.owners.len() > 8, "{document:#?}");
}

#[test]
fn native_and_rust_checks_reject_corrupt_heading_and_point_relations() {
    // The exact two-heading input was first run through the fixed reference;
    // pinned man_validate.c:post_SH supplies one authored heading domain per
    // SH head. Repointing the second row must not leave one duplicated row and
    // one heading without evidence.
    let mut headings_bundle = SourceBundle::new();
    headings_bundle
        .insert(
            "heading-relations.1",
            b".Dd September 21, 2026\n.Dt LINKS 1\n.Os\n.Sh ONE\nfirst\n.Sh TWO\nsecond\n".to_vec(),
        )
        .unwrap();
    let limits = Limits::default();
    let headings_storage = InputStorage::new(
        "heading-relations.1",
        &headings_bundle,
        InputFormat::Mdoc,
        &limits,
    )
    .unwrap();
    let (status, pointer, failure) = raw_render(&headings_storage.view(78, PROFILE_UTF8), &limits);
    assert_eq!(status, STATUS_OK, "{failure:?}");
    let headings_handle = ResultHandle(NonNull::new(pointer).unwrap());
    let mut headings_view = ResultView::default();
    assert_eq!(
        unsafe { mant_structured_result_view(headings_handle.0.as_ptr(), &raw mut headings_view) },
        STATUS_OK
    );
    let headings = unsafe {
        std::slice::from_raw_parts_mut(
            headings_view
                .heading_evidence
                .ptr
                .cast::<HeadingEvidenceView>()
                .cast_mut(),
            headings_view.heading_evidence.count as usize,
        )
    };
    assert_eq!(headings.len(), 2);
    headings[1].block = headings[0].block;
    let mut failure = FailureView::default();
    assert_eq!(
        unsafe { mant_structured_result_check(headings_handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION
    );
    assert!(copy_structured_document(&headings_handle, &headings_view, &limits).is_err());

    // Pinned tag.c:tag_move_id()/tag_postprocess() lands this Tg at a
    // zero-width root boundary. The scalar coordinate is derived, not an
    // independent producer assertion.
    let mut point_bundle = SourceBundle::new();
    point_bundle
        .insert(
            "point-relations.1",
            b".Dd September 21, 2026\n.Dt LINKS 1\n.Os\n.Sh TARGETS\n.Tg Root.Target\n.Pp\nbody\n"
                .to_vec(),
        )
        .unwrap();
    let point_storage = InputStorage::new(
        "point-relations.1",
        &point_bundle,
        InputFormat::Mdoc,
        &limits,
    )
    .unwrap();
    let (status, pointer, failure) = raw_render(&point_storage.view(78, PROFILE_UTF8), &limits);
    assert_eq!(status, STATUS_OK, "{failure:?}");
    let point_handle = ResultHandle(NonNull::new(pointer).unwrap());
    let mut point_view = ResultView::default();
    assert_eq!(
        unsafe { mant_structured_result_view(point_handle.0.as_ptr(), &raw mut point_view) },
        STATUS_OK
    );
    let points = unsafe {
        std::slice::from_raw_parts_mut(
            point_view
                .content_points
                .ptr
                .cast::<ContentPointView>()
                .cast_mut(),
            point_view.content_points.count as usize,
        )
    };
    assert!(!points.is_empty());
    points[0].scalar_boundary = points[0].scalar_boundary.saturating_add(1);
    let mut failure = FailureView::default();
    assert_eq!(
        unsafe { mant_structured_result_check(point_handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION
    );
    assert!(copy_structured_document(&point_handle, &point_view, &limits).is_err());
}
