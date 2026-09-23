use super::*;
use mant_ir::{
    Document,
    visit::{self, Visit},
};
use std::ops::ControlFlow;

#[test]
fn literal_display_link_origin_resolves_to_native_inline() {
    // Exact input was checked with fixed CVS UTF-8/78. Pinned
    // mdoc_term.c::termp_bd_pre/post keeps the literal display fixed, while
    // termp_lk_pre emits one linked label and its target in that body.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "literal-link.1",
            b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Bd -literal\n.Lk https://example.test label\n.Ed\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("literal-link.1", &bundle, InputFormat::Mdoc)
        .expect("literal link lowers to final IR");
    let mut found = 0;
    let report = mant_ir::scan_references(
        &document,
        mant_ir::ReferenceScanLimits::default(),
        |occurrence| {
            let origin = occurrence.location.to_owned().expect("bounded origin");
            assert!(std::ptr::eq(
                origin.resolve_link(&document).expect("fixed link resolves"),
                occurrence.link,
            ));
            found += 1;
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete(), "{report:?}");
    assert_eq!(found, 1);
}

#[test]
fn terminal_tg_after_lists_stays_after_them_in_ir() {
    // Both exact inputs were checked with fixed CVS -T tree.  Pinned
    // mdoc_validate.c::post_tg retains the terminal Tg as its own carrier;
    // mdoc_html.c::mdoc_tg_pre emits its mark after the preceding list.
    for (name, source, nested) in [
        (
            "list-tail.1",
            b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Bl -bullet\n.It\nbody\n.El\n.Tg Tail\n".as_slice(),
            false,
        ),
        (
            "nested-tail.1",
            b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -bullet\n.It\nouter\n.Bl -bullet\n.It\ninner\n.El\n.Tg Tail\n.El\n".as_slice(),
            true,
        ),
    ] {
        let mut bundle = SourceBundle::new();
        bundle.insert(name, source.to_vec()).unwrap();
        let document = project_native_manual(name, &bundle, InputFormat::Mdoc)
            .expect("terminal target lowers to IR");
        let blocks = if nested {
            let Block::List { items, .. } = &document.flow().expect("Flow fixture").sections[0].blocks[0] else {
                panic!("{name}: outer list retained")
            };
            &items[0].blocks
        } else {
            &document.flow().expect("Flow fixture").sections[0].blocks
        };
        let list_index = blocks
            .iter()
            .position(|block| matches!(block, Block::List { .. }))
            .expect("preceding list retained");
        let anchor_index = blocks
            .iter()
            .position(|block| {
                matches!(block, Block::Paragraph { children, .. }
                    if children.iter().any(|inline| matches!(inline,
                        Inline::Anchor { id, fragment_aliases, .. }
                            if id.as_str() == "tail" && fragment_aliases.iter()
                                .any(|alias| alias.as_str() == "Tail"))))
            })
            .unwrap_or_else(|| panic!("{name}: authored Tail anchor retained: {blocks:#?}"));
        assert_eq!(anchor_index, list_index + 1, "{name}: {blocks:#?}");
    }
}

#[test]
fn nested_native_links_remain_two_resolvable_ir_occurrences() {
    // Exact input was run through the fixed reference first. Pinned
    // `man_term.c::pre_UR/post_UR` keeps the parent instance around the
    // nested `pre_MR` instance and its separate punctuation suffix.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "nested.1",
            b".TH TEST 1\n.SH DESCRIPTION\n.UR https://outer.test\nbefore\n.MR printf 3 ,\nafter\n.UE\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("nested.1", &bundle, InputFormat::Man)
        .expect("nested links lower to IR");
    let mut targets = Vec::new();
    let report = mant_ir::scan_references(
        &document,
        mant_ir::ReferenceScanLimits::default(),
        |occurrence| {
            let position = occurrence.location.to_owned().expect("bounded location");
            assert!(std::ptr::eq(
                position.resolve_link(&document).expect("original link"),
                occurrence.link,
            ));
            targets.push(occurrence.target.clone());
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete(), "{report:?}");
    assert_eq!(report.occurrences, 2, "{targets:#?}");
    assert!(targets.contains(&mant_ir::LinkTarget::External {
        uri: "https://outer.test".to_owned(),
    }));
    assert!(targets.contains(&mant_ir::LinkTarget::Manual {
        name: "printf".to_owned(),
        manual_section: Some("3".to_owned()),
    }));
}

#[test]
fn native_link_across_definition_owner_keeps_one_ir_reference() {
    // These exact TP/IP inputs were checked with the fixed reference.
    // `man_term.c::pre_UR/post_UR` encloses formatter owner changes.
    for item in [
        b".TP\nterm\nbody\n".as_slice(),
        b".IP label\nbody\n".as_slice(),
    ] {
        let mut source =
            b".TH TEST 1\n.SH DESCRIPTION\n.UR https://example.test\nbefore\n".to_vec();
        source.extend_from_slice(item);
        source.extend_from_slice(b".UE\n");
        let mut bundle = SourceBundle::new();
        bundle.insert("cross-owner.1", source).unwrap();
        let document = project_native_manual("cross-owner.1", &bundle, InputFormat::Man)
            .expect("cross-owner link lowers to IR");
        let report =
            mant_ir::scan_references(&document, mant_ir::ReferenceScanLimits::default(), |_| {
                ControlFlow::Continue(())
            });
        assert!(report.complete(), "{report:?}");
        assert_eq!(report.occurrences, 1, "{document:#?}");
    }
}

#[test]
fn one_native_link_remains_one_final_ir_occurrence_across_style_and_wrap() {
    // This exact source was run through the pinned reference first.
    // `man_term.c::pre_UR/post_UR` keeps the UR body inside one link while
    // `term.c::term_flushln` may partially consume the field at 18 columns;
    // `man_term.c::pre_MR` keeps its trailing comma outside the reference.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "links.1",
            b".TH LINKS 1\n.SH TARGET\n.ll 18n\n.UR https://example.test/very/long/target\nplain\n.B bold\n.I italic\n.UE\n.MR printf 3 ,\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("links.1", &bundle, InputFormat::Man)
        .expect("native links lower to semantic IR");

    let mut links = Vec::new();
    let report = mant_ir::scan_references(
        &document,
        mant_ir::ReferenceScanLimits::default(),
        |occurrence| {
            let location = occurrence
                .location
                .to_owned()
                .expect("link location is bounded");
            assert!(std::ptr::eq(
                location
                    .resolve_link(&document)
                    .expect("link location resolves"),
                occurrence.link,
            ));
            links.push((
                occurrence.target.clone(),
                mant_ir::inline_plain_text(document.content(), occurrence.label),
            ));
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete(), "{report:?}");
    assert_eq!(report.occurrences, 2, "{links:#?}");
    assert_eq!(
        links[0].0,
        mant_ir::LinkTarget::External {
            uri: "https://example.test/very/long/target".to_owned(),
        }
    );
    assert_eq!(
        links[0].1,
        "plain bold italic <https://example.test/very/long/target>"
    );
    assert_eq!(
        links[1].0,
        mant_ir::LinkTarget::Manual {
            name: "printf".to_owned(),
            manual_section: Some("3".to_owned()),
        }
    );
    assert_eq!(links[1].1.trim(), "printf(3)");
}

#[test]
fn repeated_native_target_remains_two_authored_occurrences() {
    // This exact source was run through the pinned reference first.
    // Each `UR` block is a distinct execution of `man_term.c::pre_UR` and
    // `post_UR`, even though both destinations have the same spelling.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "repeated-links.1",
            b".TH LINKS 1\n.SH TARGET\n.UR https://example.test/same\nfirst\n.UE\n.UR https://example.test/same\nsecond\n.UE\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("repeated-links.1", &bundle, InputFormat::Man)
        .expect("repeated native links lower independently");
    let mut labels = Vec::new();
    let report = mant_ir::scan_references(
        &document,
        mant_ir::ReferenceScanLimits::default(),
        |occurrence| {
            labels.push(mant_ir::inline_plain_text(
                document.content(),
                occurrence.label,
            ));
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete(), "{report:?}");
    assert_eq!(report.occurrences, 2, "{labels:#?}");
    assert!(labels[0].trim_start().starts_with("first "), "{labels:#?}");
    assert!(labels[1].trim_start().starts_with("second "), "{labels:#?}");
}

#[test]
fn hard_break_inside_native_link_keeps_one_occurrence() {
    // This exact source was run through the pinned reference first.
    // `roff_term.c::roff_term_pre_br` calls `term_newln` while the surrounding
    // `man_term.c::pre_UR/post_UR` block remains the same logical link.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "hard-break-link.1",
            b".TH LINKS 1\n.SH TARGET\n.UR https://example.test/hard-break\nbefore\n.br\n.B after\n.UE\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("hard-break-link.1", &bundle, InputFormat::Man)
        .expect("hard break inside native link lowers");
    let mut labels = Vec::new();
    let report = mant_ir::scan_references(
        &document,
        mant_ir::ReferenceScanLimits::default(),
        |occurrence| {
            labels.push(mant_ir::inline_plain_text(
                document.content(),
                occurrence.label,
            ));
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete(), "{report:?}");
    assert_eq!(report.occurrences, 1, "{labels:#?}");
    assert_eq!(labels[0], "before\nafter <https://example.test/hard-break>");
}

#[test]
fn authored_heading_phrase_drives_section_identity_and_sx_resolution() {
    // This exact source was run through the pinned reference first.
    // `mdoc_validate.c::post_section` derives `White Space` before terminal
    // `.Sm off` joins the displayed heading, and `mdoc_html.c::mdoc_sx_pre`
    // links `.Sx White Space` through that authored identity domain.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "heading-sx.1",
            b".Dd September 21, 2026\n.Dt LINKS 1\n.Os\n.Sm off\n.Sh White Space\n.Sm on\n.Pp\n.Sx White Space\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("heading-sx.1", &bundle, InputFormat::Mdoc)
        .expect("authored heading evidence lowers");

    assert_eq!(
        document.flow().expect("Flow fixture").sections[0]
            .id
            .as_str(),
        "white-space"
    );
    assert_eq!(
        document.flow().expect("Flow fixture").sections[0]
            .heading
            .plain_text(document.content()),
        "WhiteSpace"
    );
    let mut links = Vec::new();
    let report = mant_ir::scan_references(
        &document,
        mant_ir::ReferenceScanLimits::default(),
        |occurrence| {
            links.push((
                occurrence.target.clone(),
                mant_ir::inline_plain_text(document.content(), occurrence.label),
            ));
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete(), "{report:?}");
    assert_eq!(
        links,
        [(
            mant_ir::LinkTarget::Section {
                id: "white-space".into(),
            },
            "White Space".to_owned(),
        )]
    );
}

#[test]
fn ambiguous_and_missing_sx_occurrences_downgrade_without_dangling_links() {
    // This exact source was run through the pinned reference first.
    // Both headings receive native section tags, while the two Sx labels stay
    // visible. Codec must not guess which duplicate owns the first reference.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "ambiguous-sx.1",
            b".Dd September 21, 2026\n.Dt LINKS 1\n.Os\n.Sh DUPLICATE\nfirst\n.Sh DUPLICATE\n.Sx DUPLICATE\n.Sx MISSING\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("ambiguous-sx.1", &bundle, InputFormat::Mdoc)
        .expect("unresolved section occurrences downgrade atomically");

    assert_eq!(
        document.flow().expect("Flow fixture").sections[0]
            .id
            .as_str(),
        "duplicate"
    );
    assert_eq!(
        document.flow().expect("Flow fixture").sections[1]
            .id
            .as_str(),
        "duplicate-2"
    );
    let report =
        mant_ir::scan_references(&document, mant_ir::ReferenceScanLimits::default(), |_| {
            panic!("an ambiguous or missing Sx must not remain a link")
        });
    assert_eq!(report.occurrences, 0, "{report:?}");
    assert_eq!(
        document
            .diagnostics
            .iter()
            .filter(|diagnostic| {
                diagnostic.code.as_deref() == Some("unresolved-section-reference")
            })
            .count(),
        2,
        "{:#?}",
        document.diagnostics
    );
    let visible = document.flow().expect("Flow fixture").sections[1]
        .blocks
        .iter()
        .map(|block| match block {
            Block::Paragraph { children, .. } => {
                mant_ir::inline_plain_text(document.content(), children)
            }
            _ => String::new(),
        })
        .collect::<String>();
    assert!(visible.contains("DUPLICATE"), "{visible:?}");
    assert!(visible.contains("MISSING"), "{visible:?}");
}

#[test]
fn break_opportunity_does_not_split_native_link_occurrence() {
    // This exact source was run through the pinned reference first.
    // `term.c::term_fill` treats ASCII_HYPH as visible `-` followed by a
    // zero-width break opportunity, not as a link wrapper boundary.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "break-opportunity-link.1",
            b".TH LINKS 1\n.SH TARGET\n.UR https://example.test/break\na-b\n.UE\n".to_vec(),
        )
        .unwrap();
    let projection = super::super::super::projection::project_native_prose(
        "break-opportunity-link.1",
        &bundle,
        InputFormat::Man,
    )
    .expect("native break opportunity projects");
    assert!(projection.document().content_atoms().iter().any(|atom| {
        matches!(
            atom.kind(),
            libmandoc_rs::structured::ContentAtomKind::BreakOpportunity
        )
    }));
    let document = lower_projection(projection).expect("break opportunity link lowers");
    let mut labels = Vec::new();
    let report = mant_ir::scan_references(
        &document,
        mant_ir::ReferenceScanLimits::default(),
        |occurrence| {
            labels.push(mant_ir::inline_plain_text(
                document.content(),
                occurrence.label,
            ));
            ControlFlow::Continue(())
        },
    );
    assert!(report.complete(), "{report:?}");
    assert_eq!(report.occurrences, 1, "{labels:#?}");
    assert_eq!(labels[0], "a-b <https://example.test/break>");
}

#[test]
fn native_targets_use_normalized_ids_and_only_authored_aliases() {
    // These exact mdoc and man sources were run through the pinned reference
    // first. `tag_put(TAG_MANUAL)` retains Mixed.Target as an authored target;
    // Ev and TP contribute formatter-generated targets.
    let mut mdoc = SourceBundle::new();
    mdoc.insert(
        "target-identities.1",
        b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag\n.It Ev DEMO_HOME\nBODY\n.El\n.Sh OPTIONS\n.Bl -tag\n.Tg Mixed.Target\n.It Fl mixed\nBODY\n.El\n"
            .to_vec(),
    )
    .unwrap();
    let document = project_native_manual("target-identities.1", &mdoc, InputFormat::Mdoc)
        .expect("native targets lower to valid document identities");
    let anchors = anchor_identities(&document);
    assert!(
        anchors.contains(&("demo-home".to_owned(), Vec::new())),
        "{anchors:#?}"
    );
    assert!(
        anchors.contains(&("mixed-target".to_owned(), vec!["Mixed.Target".to_owned()])),
        "{anchors:#?}"
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.invalid-identity")),
        "{:#?}",
        document.diagnostics
    );

    let mut man = SourceBundle::new();
    man.insert(
        "target-identity.1",
        b".TH X 1\n.SH OPTIONS\n.TP\n--set=KEY\nBODY\n".to_vec(),
    )
    .unwrap();
    let document = project_native_manual("target-identity.1", &man, InputFormat::Man)
        .expect("man target lowers to a normalized generated identity");
    let anchors = anchor_identities(&document);
    assert!(
        anchors.contains(&("set-key".to_owned(), Vec::new())),
        "{anchors:#?}"
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.invalid-identity")),
        "{:#?}",
        document.diagnostics
    );
}

#[test]
fn authored_target_collision_keeps_alias_and_unique_internal_id() {
    // This exact source was run through the pinned reference first.
    // `tag.c::tag_put` retains both the Sh target and the manual Tg target;
    // their distinct authored fragment spellings must remain addressable.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "target-collision.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh Mixed Target\n.Bl -tag\n.Tg Mixed.Target\n.It Fl mixed\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("target-collision.1", &bundle, InputFormat::Mdoc)
        .expect("authored target collision receives a unique normalized identity");
    assert_eq!(
        document.flow().expect("Flow fixture").sections[0]
            .id
            .as_str(),
        "mixed-target"
    );
    let anchors = anchor_identities(&document);
    assert!(
        anchors.contains(&("mixed-target-2".to_owned(), vec!["Mixed.Target".to_owned()])),
        "{anchors:#?}"
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.invalid-identity")),
        "{:#?}",
        document.diagnostics
    );
}

fn anchor_identities(document: &Document) -> Vec<(String, Vec<String>)> {
    #[derive(Default)]
    struct Collector {
        identities: Vec<(String, Vec<String>)>,
    }

    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } = inline
            {
                self.identities.push((
                    id.to_string(),
                    fragment_aliases.iter().map(ToString::to_string).collect(),
                ));
            } else {
                visit::walk_inline(self, inline);
            }
        }
    }

    let mut collector = Collector::default();
    collector.visit_document(document);
    collector.identities
}

#[test]
fn nested_empty_mdoc_item_retains_its_explicit_target() {
    // This exact source was run through the pinned reference first.
    // `mdoc_validate.c::post_tg` keeps Tg as the target owner because the
    // following bullet item has no body child.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "nested-target.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl outer\n.Bl -bullet\n.Tg nested-target\n.It\n.El\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("nested-target.1", &bundle, InputFormat::Mdoc)
        .expect("nested empty target lowers through the native path");
    let Block::DefinitionList { items, .. } =
        &document.flow().expect("Flow fixture").sections[0].blocks[0]
    else {
        panic!("outer definition list retained")
    };
    let Block::List { items: nested, .. } = &items[0].description[0] else {
        panic!("nested bullet list retained")
    };
    let Block::Paragraph { children, .. } = &nested[0].blocks[0] else {
        panic!("target-only empty item receives an anchor paragraph")
    };
    assert!(matches!(
        children.as_slice(),
        [Inline::Anchor { id, .. }] if id.as_str() == "nested-target"
    ));
}
