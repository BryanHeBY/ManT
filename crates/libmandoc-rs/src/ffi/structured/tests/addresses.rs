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
fn deleted_paragraph_does_not_leave_or_reassign_its_tg_target() {
    // Each exact input was run through the fixed reference before these
    // assertions. Pinned mdoc_validate.c::post_tg first tags Pp, then
    // post_section deletes a terminal Pp; tag.c::tag_postprocess only sees
    // the surviving NOPRT Tg, so End is not a native target.
    let terminal = fixture(
        "terminal-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbody\n.Tg End\n.Pp\n",
        InputFormat::Mdoc,
    );
    assert!(!terminal.anchors.iter().any(|anchor| anchor.target == "End"));

    let next_section = fixture(
        "next-section-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbody\n.Tg End\n.Pp\n.Sh NEXT\nnext\n",
        InputFormat::Mdoc,
    );
    assert!(
        !next_section
            .anchors
            .iter()
            .any(|anchor| anchor.target == "End")
    );
    assert!(
        next_section
            .anchors
            .iter()
            .any(|anchor| anchor.target == "NEXT")
    );

    let reused = fixture(
        "reused-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbody\n.Tg End\n.Pp\n.Sh NEXT\n.Tg End\nnext\n",
        InputFormat::Mdoc,
    );
    let matching = reused
        .anchors
        .iter()
        .filter(|anchor| anchor.target == "End")
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "{reused:#?}");
    let OwnedProvenance::Authored { span } =
        reused.provenances[matching[0].provenance as usize - 1]
    else {
        panic!("surviving target must retain its authored provenance");
    };
    assert_eq!(reused.spans[span as usize - 1].line_columns.unwrap().0, 9);
}

#[test]
fn surviving_tg_carriers_keep_their_zero_width_targets() {
    // These exact inputs were run through the fixed reference. Pinned
    // mdoc_validate.c::post_tg selects Pp or Sh head, while tag.c::tag_move_id
    // can move an inline target backwards to a preceding Pp.
    let live_paragraph = fixture(
        "live-paragraph-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Tg Live\n.Pp\nbody\n",
        InputFormat::Mdoc,
    );
    assert_eq!(
        live_paragraph
            .anchors
            .iter()
            .filter(|anchor| anchor.target == "Live")
            .count(),
        1
    );

    let next_heading = fixture(
        "next-heading-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbody\n.Tg Next.Target\n.Sh NEXT\nnext\n",
        InputFormat::Mdoc,
    );
    assert_eq!(
        next_heading
            .anchors
            .iter()
            .filter(|anchor| anchor.target == "Next.Target")
            .count(),
        1
    );

    let standalone = fixture(
        "standalone-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbody\n.Tg End\n",
        InputFormat::Mdoc,
    );
    assert_eq!(
        standalone
            .anchors
            .iter()
            .filter(|anchor| anchor.target == "End")
            .count(),
        1
    );

    let moved = fixture(
        "moved-inline-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Pp\n.Tg Moved\n.Cm cmd\nafter\n",
        InputFormat::Mdoc,
    );
    assert_eq!(
        moved
            .anchors
            .iter()
            .filter(|anchor| anchor.target == "Moved")
            .count(),
        1,
        "{moved:#?}"
    );
    let anchor = moved
        .anchors
        .iter()
        .find(|anchor| anchor.target == "Moved")
        .unwrap();
    let OwnedProvenance::Authored { span } = &moved.provenances[anchor.provenance as usize - 1]
    else {
        panic!("moved target must retain its authored Tg source: {moved:#?}");
    };
    assert_eq!(
        moved.spans[*span as usize - 1]
            .line_columns
            .expect("Tg source line is known")
            .0,
        7,
        "tag.c::tag_move_id moves the landing point, not the Tg source"
    );
}

#[test]
fn tg_self_target_moving_back_keeps_its_declaration_source() {
    // Both exact inputs were checked with the fixed CVS tree reference.
    // Pinned post_tg() first tags Tg itself; tag.c::tag_move_id() then moves
    // placement to the preceding Pp or It body without moving the request.
    for (name, source) in [
        (
            "back-to-paragraph.1",
            b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Pp\none\n.Tg Here\ntwo\n"
                .as_slice(),
        ),
        (
            "back-to-item.1",
            b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.It\none\n.Tg Here\ntwo\n.El\n"
                .as_slice(),
        ),
    ] {
        let document = fixture(name, source, InputFormat::Mdoc);
        let targets = document
            .anchors
            .iter()
            .filter(|anchor| anchor.target == "Here")
            .collect::<Vec<_>>();
        assert_eq!(targets.len(), 1, "{document:#?}");
        let anchor = targets[0];
        let OwnedProvenance::Authored { span } =
            document.provenances[anchor.provenance as usize - 1]
        else {
            panic!("Here must retain authored Tg provenance: {document:#?}");
        };
        assert_eq!(document.spans[span as usize - 1].line_columns.unwrap().0, 8);
        assert_ne!(anchor.point, 0, "{document:#?}");
    }
}

#[test]
fn repeated_manual_targets_keep_each_request_source() {
    // The exact source was checked with fixed CVS -T tree. Both Pp nodes
    // retain ID=Same, but their authored requests are distinct Tg nodes.
    let document = fixture(
        "repeated-manual-target.1",
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Tg Same\n.Pp\none\n.Tg Same\n.Pp\ntwo\n",
        InputFormat::Mdoc,
    );
    let mut lines = document
        .anchors
        .iter()
        .filter(|anchor| anchor.target == "Same")
        .map(|anchor| {
            let OwnedProvenance::Authored { span } =
                document.provenances[anchor.provenance as usize - 1]
            else {
                panic!("manual target must retain Tg provenance: {document:#?}");
            };
            document.spans[span as usize - 1]
                .line_columns
                .expect("Tg source line is known")
                .0
        })
        .collect::<Vec<_>>();
    lines.sort_unstable();
    assert_eq!(lines, [5, 8], "{document:#?}");
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
fn man_mr_generated_parentheses_remain_in_the_link_without_a_suffix() {
    // These exact 18-column sources were checked with the fixed reference.
    // Pinned `man_term.c::pre_MR` emits `(` and `)` with no source node;
    // only a real third child is a suffix outside the MR link.
    for (source, expected) in [
        (
            b".TH TEST 1\n.SH REFERENCES\n.MR printf 3\n".as_slice(),
            "printf(3)",
        ),
        (
            b".TH TEST 1\n.SH REFERENCES\n.MR printf 3 \"\"\n".as_slice(),
            "printf(3)",
        ),
        (
            b".TH TEST 1\n.SH REFERENCES\n.MR printf 3 ,\n".as_slice(),
            "printf(3)",
        ),
        (
            b".TH TEST 1\n.SH\n.MR printf 3\nbody\n".as_slice(),
            "printf(3)",
        ),
    ] {
        let document = fixture("mr.1", source, InputFormat::Man);
        assert_eq!(document.links.len(), 1, "{document:#?}");
        let link = &document.links[0];
        let label = document.link_label_parts[link.first_label_part as usize - 1
            ..link.first_label_part as usize - 1 + link.label_part_count as usize]
            .iter()
            .map(|part| document.content_atoms[part.atom as usize - 1].text.as_str())
            .collect::<String>();
        assert_eq!(label, expected, "{document:#?}");
    }
}

#[test]
fn escaped_link_destinations_use_upstream_scalar_decoding() {
    // These exact 18-column inputs were run with the fixed reference.
    // `term.c::term_word` and `html.c::print_encode` decode `\-` before
    // presenting the destination; the authored operand remains provenance.
    let mr = fixture(
        "mr-escape.1",
        b".TH TEST 1\n.SH REFERENCES\n.MR git\\-config 1 ,\n",
        InputFormat::Man,
    );
    assert_eq!(mr.links.len(), 1, "{mr:#?}");
    assert_eq!(mr.links[0].target_a, "git-config");
    let ur = fixture(
        "ur-escape.1",
        b".TH TEST 1\n.SH REFERENCES\n.UR https://example.test/a\\-b\nlabel\n.UE\n",
        InputFormat::Man,
    );
    assert_eq!(ur.links.len(), 1, "{ur:#?}");
    assert_eq!(ur.links[0].target_a, "https://example.test/a-b");
    // Pinned `term.c::term_word` expands the selected UTF-8 terminal device
    // in this exact source; the fixed 18-column reference prints `/utf8`.
    let device = fixture(
        "device-escape.1",
        b".TH TEST 1\n.SH REFERENCES\n.UR https://example.test/\\*(.T\nlabel\n.UE\n",
        InputFormat::Man,
    );
    assert_eq!(device.links[0].target_a, "https://example.test/utf8");

    // The same exact source was also run with the fixed ASCII reference;
    // `term.c::term_word` expands the selected device to `ascii` there.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "device-ascii.1",
            b".TH TEST 1\n.SH REFERENCES\n.UR https://example.test/\\*(.T\nlabel\n.UE\n".to_vec(),
        )
        .unwrap();
    let ascii = render_prelude_profile(
        "device-ascii.1",
        &bundle,
        InputFormat::Man,
        PROFILE_ASCII,
        18,
        &Limits::default(),
    )
    .unwrap();
    assert_eq!(ascii.links[0].target_a, "https://example.test/ascii");
}

#[test]
fn link_target_skip_escapes_follow_html_execution_order() {
    // Each exact input was run with the fixed CVS HTML reference first.
    // Pinned html.c::print_encode() processes font, repeated SKIPCHAR, and
    // malformed escapes before consuming the next printable character.
    for (name, target, expected) in [
        (
            "skip-font.1",
            "https://a.test/\\z\\fBX\\fPY",
            "https://a.test/Y",
        ),
        (
            "skip-repeat.1",
            "https://a.test/\\z\\zXY",
            "https://a.test/Y",
        ),
        (
            "unicode.1",
            "https://a.test/\\[u03B1]Y",
            "https://a.test/αY",
        ),
        (
            "skip-unicode.1",
            "https://a.test/\\z\\fB\\[u03B1]Y",
            "https://a.test/Y",
        ),
    ] {
        let source = format!(".TH TEST 1\n.SH DESCRIPTION\n.UR {target}\nlabel\n.UE\n");
        let document = fixture(name, source.as_bytes(), InputFormat::Man);
        assert_eq!(document.links.len(), 1, "{document:#?}");
        assert_eq!(document.links[0].target_a, expected, "{document:#?}");
    }
}

#[test]
fn nested_manual_link_reuses_the_outer_occurrence_after_the_child() {
    // Fixed CVS `man_term.c::pre_UR/post_UR` encloses the one `pre_MR`
    // execution; it does not create a second UR macro after the child.
    let document = fixture(
        "nested.1",
        b".TH TEST 1\n.SH DESCRIPTION\n.UR https://outer.test\nbefore\n.MR printf 3 ,\nafter\n.UE\n",
        InputFormat::Man,
    );
    assert_eq!(document.links.len(), 2, "{document:#?}");
    let outer = document
        .links
        .iter()
        .find(|link| link.target_a == "https://outer.test")
        .unwrap();
    let inner = document
        .links
        .iter()
        .find(|link| link.target_a == "printf")
        .unwrap();
    let outer_atoms = document.link_label_parts[outer.first_label_part as usize - 1
        ..outer.first_label_part as usize - 1 + outer.label_part_count as usize]
        .iter()
        .map(|part| part.atom)
        .collect::<Vec<_>>();
    let inner_atoms = document.link_label_parts[inner.first_label_part as usize - 1
        ..inner.first_label_part as usize - 1 + inner.label_part_count as usize]
        .iter()
        .map(|part| part.atom)
        .collect::<Vec<_>>();
    assert!(
        outer_atoms[0] < inner_atoms[0]
            && inner_atoms[inner_atoms.len() - 1] < outer_atoms[outer_atoms.len() - 1],
        "{document:#?}"
    );
    let inner_label = inner_atoms
        .iter()
        .map(|key| document.content_atoms[*key as usize - 1].text.as_str())
        .collect::<String>();
    assert_eq!(inner_label, "printf(3)", "{document:#?}");
    let outer_label = outer_atoms
        .iter()
        .map(|key| document.content_atoms[*key as usize - 1].text.as_str())
        .collect::<String>();
    assert!(outer_label.contains("before"));
    assert!(outer_label.contains(','));
    assert!(outer_label.contains("after"));
}

#[test]
fn multiple_nested_manual_links_keep_disjoint_grouped_label_parts() {
    // Exact 18-column source was checked against the fixed reference.
    // `man_term.c::pre_MR` executes twice inside one UR block; punctuation
    // after either nested MR remains inside the outer UR body.
    let document = fixture(
        "nested-pair.1",
        b".TH TEST 1\n.SH DESCRIPTION\n.UR https://outer.test\nbefore\n.MR printf 3 ,\nmiddle\n.MR scanf 3 ;\nafter\n.UE\n",
        InputFormat::Man,
    );
    assert_eq!(document.links.len(), 3, "{document:#?}");
    let parts = document
        .links
        .iter()
        .flat_map(|link| {
            &document.link_label_parts[link.first_label_part as usize - 1
                ..link.first_label_part as usize - 1 + link.label_part_count as usize]
        })
        .collect::<Vec<_>>();
    assert_eq!(parts.len(), document.link_label_parts.len());
    let distinct_atoms = parts
        .iter()
        .map(|part| part.atom)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(distinct_atoms.len(), parts.len(), "{document:#?}");
}

#[test]
fn man_ur_crosses_definition_owners_without_changing_occurrence() {
    // Fixed CVS `man_term.c::pre_UR/post_UR` keeps one block while
    // `man_term.c::pre_TP/pre_IP` changes the formatting owner inside it.
    for item in [
        b".TP\nterm\nbody\n".as_slice(),
        b".IP label\nbody\n".as_slice(),
    ] {
        let mut source =
            b".TH TEST 1\n.SH DESCRIPTION\n.UR https://example.test\nbefore\n".to_vec();
        source.extend_from_slice(item);
        source.extend_from_slice(b".UE\n");
        let document = fixture("cross-owner.1", &source, InputFormat::Man);
        assert_eq!(document.links.len(), 1, "{document:#?}");
        let link = &document.links[0];
        let owners = document.link_label_parts[link.first_label_part as usize - 1
            ..link.first_label_part as usize - 1 + link.label_part_count as usize]
            .iter()
            .map(|part| document.content_atoms[part.atom as usize - 1].owner)
            .collect::<std::collections::BTreeSet<_>>();
        assert!(owners.len() > 1, "{document:#?}");
    }
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
