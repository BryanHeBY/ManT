use super::*;

#[test]
fn thousand_row_allbox_table_keeps_two_thousand_unique_cell_regions() {
    use std::{collections::HashSet, fmt::Write as _};

    // This exact generated 1,000-row source was rerun with the pinned CVS
    // UTF-8/78 reference before this assertion (source SHA-256
    // 1018a216cc9c1150f350d935bf54d43d062128104e4ccf1993318f6d166cba84).
    // tbl_term.c::term_tbl draws allbox rules; tbl_word emits each data cell
    // once. Fixed region selections point into one final visible byte arena.
    let mut source = ".TH T 1\n.SH DATA\n.TS\nallbox tab(;);\nl l.\n".to_owned();
    for index in 0..1_000 {
        writeln!(source, "left_{index};right_{index}").unwrap();
    }
    source.push_str(".TE\n");
    let document = project_annotated_manual("t.1", &bundle(source.as_bytes()), InputFormat::Man)
        .expect("large table reaches Fixed IR");
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("large table did not produce Fixed IR");
    };
    let mut cells = HashSet::new();
    let mut count = 0;
    for region in &fixed.regions {
        if region.kind != mant_ir::RegionKind::TableCell {
            continue;
        }
        count += 1;
        let mut text = String::new();
        for part in &region.selection.parts {
            let run = fixed.surface.run_text(part.run).expect("valid run");
            let start = usize::try_from(part.start_byte).expect("start fits usize");
            let end = usize::try_from(part.end_byte).expect("end fits usize");
            text.push_str(run.get(start..end).expect("valid UTF-8 part"));
        }
        assert!(cells.insert(text), "duplicated cell region");
    }
    assert_eq!(count, 2_000);
    assert!(cells.contains("left_0"));
    assert!(cells.contains("right_0"));
    assert!(cells.contains("left_999"));
    assert!(cells.contains("right_999"));
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let rendered = mant_ui::DocumentView::new(&query).render(78);
    assert!(rendered.row_count > 2_000);
    assert!(
        rendered
            .text
            .lines
            .iter()
            .any(|row| row.to_string().contains("left_999"))
    );
}

#[test]
fn one_native_result_keeps_fixed_rows_across_real_tui_buffer_widths() {
    use ratatui::{
        buffer::Buffer,
        layout::Rect,
        widgets::{Paragraph, Widget},
    };

    // Exact bytes first ran pinned CVS -Tutf8 -O width=78. term.c::
    // term_flushln emits one no-fill row; viewport width must only crop it.
    let input = b".TH T 1\n.SH D\n.nf\nabcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789\n.fi\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let query = mant_ir::ResolvedContent {
        label: "T(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let view = mant_ui::DocumentView::new(&query);
    assert!(view.max_fixed_columns() >= 62);
    let mut expected_rows = None;
    for width in [20, 40, 78, 120] {
        let rendered = view.render(width);
        assert_eq!(
            rendered
                .text
                .lines
                .iter()
                .filter(|line| { line.to_string().contains("abc") })
                .count(),
            1
        );
        if let Some(expected_rows) = expected_rows {
            assert_eq!(rendered.row_count, expected_rows);
        } else {
            expected_rows = Some(rendered.row_count);
        }
        let height = u16::try_from(rendered.row_count).unwrap();
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        Paragraph::new(rendered.text).render(area, &mut buffer);
        let first = buffer
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect::<String>();
        assert!(
            first.contains("abc"),
            "viewport {width} lost the fixed line"
        );
    }
    let shifted = view.render_with_horizontal_offset(20, 30);
    assert!(
        shifted
            .text
            .lines
            .iter()
            .any(|line| line.to_string().contains("KLM"))
    );
}

#[test]
fn real_mdoc_body_and_manual_link_retain_distinct_typed_domains() {
    // Both exact inputs first ran with pinned CVS -Tutf8 -O width=78.
    // mdoc_term.c traverses Sh HEAD/BODY; man_term.c::pre_MR emits the
    // generated parentheses inside one HTML-equivalent macro occurrence.
    let mdoc = b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(mdoc), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert!(fixed.surface.text.contains("body"));
    assert_eq!(fixed.headings.len(), 1);

    let man = b".TH T 1\n.SH D\n.MR printf 3\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(man), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.links.len(), 1);
    assert_eq!(fixed.links[0].key.get(), 1);
    assert!(matches!(
        fixed.links[0].target,
        Some(LinkTarget::Manual { ref name, ref manual_section })
            if name == "printf" && manual_section.as_deref() == Some("3")
    ));
    assert!(!fixed.links[0].label.parts.is_empty());
    assert!(fixed.surface.text.contains("body"));
}

#[test]
fn section_links_resolve_to_fixed_heading_identity_without_guessing_missing_targets() {
    // Exact bytes first ran with pinned CVS -Tutf8 -O width=78.
    // mdoc_html.c::mdoc_sx_pre creates a same-page link from the operand;
    // the native label remains visible even for a missing local target.
    let input = b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh SEE ALSO\n.Sx SEE ALSO\n.Sx MISSING\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.links.len(), 2);
    assert_eq!(fixed.headings[0].id.as_str(), "see-also");
    assert!(
        fixed.anchors.is_empty(),
        "implicit heading tag must not claim a second identity"
    );
    assert!(
        matches!(
            &fixed.links[0].target,
            Some(LinkTarget::Section { id }) if id == &fixed.headings[0].id
        ),
        "first target {:?}, heading ID {:?}, title selection {:?}, display {:?}",
        fixed.links[0].target,
        fixed.headings[0].id,
        fixed.headings[0].title,
        fixed.surface.text
    );
    assert!(fixed.links[1].target.is_none());
    assert!(!fixed.links[1].label.parts.is_empty());
    assert!(
        document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("unresolved-section-reference")
        })
    );
}

#[test]
fn empty_section_link_operand_does_not_reject_native_body() {
    // Exact bytes ran pinned CVS -Tutf8 first. roff.c::deroff() skips \\&;
    // the missing destination does not prevent the following terminal body
    // from rendering. This test makes no claim about CVS's HTML output.
    let input = b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh NAME\n.Nm t\n.Nd test\n.Sh DESCRIPTION\nBefore.\n.Sx \\&\nAfter.\n";
    let page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Mdoc)
        .unwrap();
    let expected = page.text.clone();
    assert!(expected.contains("Before."));
    assert!(expected.contains("After."));
    let document = lower_annotated_document(page).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.surface.text, expected);
    assert!(fixed.links.iter().all(|link| link.target.is_none()));
}

#[test]
fn empty_section_link_keeps_independent_entry_and_link_annotations() {
    // Exact bytes ran pinned CVS -Tutf8 -O width=78 first. mdoc_term.c
    // emits the Fl entry and the later Sx OPTIONS label; roff.c::deroff()
    // yields no destination only for the intervening zero-width Sx.
    let input = b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh NAME\n.Nm t\n.Nd test\n.Sh OPTIONS\n.Bl -tag\n.It Fl good\nGood body.\n.El\n.Sh DESCRIPTION\nBefore.\n.Sx \\&\n.Sx OPTIONS\nAfter.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(fixed.surface.text.contains("Before."));
    assert!(fixed.surface.text.contains("After."));
    assert!(fixed.owners.iter().any(|owner| {
        owner
            .entry
            .as_ref()
            .is_some_and(|entry| entry.names == ["-good"])
    }));
    assert!(fixed.links.iter().any(|link| {
        matches!(&link.target, Some(LinkTarget::Section { id }) if id.as_str() == "options")
    }));
    assert!(fixed.links.iter().all(|link| {
        !matches!(&link.target, Some(LinkTarget::Section { id }) if id.as_str().is_empty())
    }));
}

#[test]
fn moved_manual_tag_is_an_exact_heading_alias_not_an_independent_anchor() {
    // Exact bytes first ran with pinned CVS -Tutf8 and -Thtml.  The HTML
    // `<h2 id="Named.Target">OPTIONS</h2>` comes from tag.c::tag_move_id();
    // roff.c::deroff on the Sh HEAD remains the authored phrase OPTIONS.
    let input = b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\nbody\n.Tg Named.Target\n.Sh OPTIONS\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.headings[1].id.as_str(), "options");
    assert!(
        fixed.headings[1]
            .fragment_aliases
            .iter()
            .any(|alias| alias.as_str() == "Named.Target")
    );
    assert!(
        !fixed
            .anchors
            .iter()
            .any(|anchor| anchor.name == "Named.Target")
    );
    let index = DocumentIndex::build(&document);
    assert_eq!(
        index.fragment_target("Named.Target"),
        Some(&fixed.headings[1].id)
    );
    assert!(
        index
            .authored_fragments()
            .any(|alias| alias.as_str() == "Named.Target")
    );
}

#[test]
fn heading_id_does_not_claim_another_headings_authored_alias() {
    // Exact bytes first ran with pinned CVS -Thtml: the first heading owns
    // id="x", while the second rendered heading is distinct.  The IR must
    // not let the latter normalized ID steal the former exact .Tg alias.
    let input = b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Tg x\n.Sh FIRST\nbody\n.Sh X\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.headings[0].id.as_str(), "first");
    assert_eq!(fixed.headings[1].id.as_str(), "x-2");
    let index = DocumentIndex::build(&document);
    assert_eq!(index.fragment_target("x"), Some(&fixed.headings[0].id));
}

#[test]
fn generated_heading_target_remains_addressable_without_becoming_authored() {
    // Exact input first ran with pinned CVS -Thtml: man_validate.c::post_SH
    // deroffs a multiword HEAD and tag_put retains id="FOO_BAR".
    let input = b".TH T 1\n.SH FOO BAR\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert!(
        fixed.headings[0]
            .generated_fragment_aliases
            .iter()
            .any(|alias| alias.as_str() == "FOO_BAR")
    );
    let index = DocumentIndex::build(&document);
    assert_eq!(
        index.fragment_target("FOO_BAR"),
        Some(&fixed.headings[0].id)
    );
    assert!(
        !index
            .authored_fragments()
            .any(|alias| alias.as_str() == "FOO_BAR")
    );
}

#[test]
fn generated_anchor_targets_reserve_each_others_exact_spelling() {
    // Exact input first ran with pinned CVS -Thtml: tag.c retains distinct
    // id="foo_bar" and id="foo-bar" on the two Fl terms.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl \\-foo_bar\nfirst\n.It Fl \\-foo-bar\nsecond\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let index = DocumentIndex::build(&document);
    let first = index.fragment_target("foo_bar").expect("first native tag");
    let second = index.fragment_target("foo-bar").expect("second native tag");
    assert_ne!(first, second);
    assert!(
        !index
            .authored_fragments()
            .any(|alias| matches!(alias.as_str(), "foo_bar" | "foo-bar"))
    );
}

#[test]
fn repeated_manual_targets_keep_each_declaration_and_resolve_native_html_ids() {
    // Exact bytes first ran with pinned CVS -Thtml: tag.c::tag_put retains
    // all three same-priority .Tg declarations; html.c::html_make_id emits
    // id="C", id="C~2", and id="C~3" in that order.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh FIRST\n.Tg C\nfirst\n.Tg C\nsecond\n.Tg C\nthird\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.anchors.len(), 3);
    let index = DocumentIndex::build(&document);
    for (anchor, fragment) in fixed.anchors.iter().zip(["C", "C~2", "C~3"]) {
        assert_eq!(anchor.name, "C");
        assert!(anchor.authored);
        assert_eq!(anchor.rendered_fragment.as_str(), fragment);
        assert_eq!(index.fragment_target(fragment), Some(&anchor.id));
    }
    assert!(
        index
            .authored_fragments()
            .any(|alias| alias.as_str() == "C")
    );
}

#[test]
fn repeated_target_ordinals_cross_heading_and_independent_anchor() {
    // Both exact inputs first ran with pinned CVS -Thtml. The first emits
    // C on a heading, C~2 on an independent anchor; the second reverses
    // those roles. tag.c::tag_move_id() selects the heading carrier.
    let cases: &[(&[u8], &str, &str)] = &[
        (
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Tg C\n.Sh HEADING\nbody\n.Tg C\nsecond\n",
            "C",
            "C~2",
        ),
        (
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh FIRST\n.Tg C\nfirst\n.Tg C\n.Sh SECOND\nbody\n",
            "C~2",
            "C",
        ),
    ];
    for (input, heading_fragment, anchor_fragment) in cases {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("native output did not become Fixed");
        };
        let heading = fixed
            .headings
            .iter()
            .find(|heading| {
                heading
                    .fragment_aliases
                    .iter()
                    .any(|alias| alias.as_str() == "C")
            })
            .expect("tagged heading");
        let anchor = fixed
            .anchors
            .iter()
            .find(|anchor| anchor.name == "C")
            .expect("anchor");
        assert_eq!(
            heading.rendered_fragment_aliases[0].as_str(),
            *heading_fragment
        );
        assert_eq!(anchor.rendered_fragment.as_str(), *anchor_fragment);
        let index = DocumentIndex::build(&document);
        assert_eq!(index.fragment_target(heading_fragment), Some(&heading.id));
        assert_eq!(index.fragment_target(anchor_fragment), Some(&anchor.id));
    }
}

#[test]
fn html_id_normalization_and_canonical_id_reservation_stay_disjoint() {
    // Both exact inputs first ran with pinned CVS -Thtml. html_make_id
    // converts '~' to '_' before counting, so C~x and C_x become C_x and
    // C_x~2. A manual target c prevents another heading from claiming c.
    let normalized =
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh FIRST\n.Tg C~x\nfirst\n.Tg C_x\nsecond\n";
    let document = project_annotated_manual("t.1", &bundle(normalized), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.anchors[0].name, "C~x");
    assert_eq!(fixed.anchors[1].name, "C_x");
    let index = DocumentIndex::build(&document);
    assert_eq!(index.fragment_target("C_x"), Some(&fixed.anchors[0].id));
    assert_eq!(index.fragment_target("C_x~2"), Some(&fixed.anchors[1].id));

    let reserved = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Tg c\n.Sh OTHER\nbody\n.Sh c\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(reserved), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.headings[1].id.as_str(), "c-2");
    assert_eq!(
        DocumentIndex::build(&document).fragment_target("c"),
        Some(&fixed.headings[0].id)
    );
}

#[test]
fn subsection_before_any_top_heading_keeps_its_native_level() {
    // Exact input first ran with pinned CVS -Thtml: man_term.c::pre_SS
    // still emits an Ss h3 even though no SH parent has been seen.
    let input = b".TH T 1\n.SS orphan\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.headings[0].parent, None);
    assert_eq!(fixed.headings[0].level_hint, 2);
}

#[test]
fn formatter_bullet_gap_remains_layout_after_cursor_traversal() {
    // Exact input ran through pinned CVS -Tutf8 first. term.c::bufferc()
    // traverses HORIZ blanks between the bullet and bold body without
    // replacing those cells; term_field() later prints them as indentation.
    let input = b".TH T 1\n.SH D\n.IP \\(bu 4\n.B git-revert\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man)
        .expect("native bullet display projects without a false visible join gap");
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert!(fixed.surface.runs.iter().any(|run| {
        run.label.role == DisplayRole::Layout && fixed.surface.run_text(run.key) == Some("   ")
    }));
}

#[test]
fn source_less_field_spacing_does_not_break_a_native_direct_join() {
    // Exact input ran through pinned CVS -Tutf8 first. term.c::term_field()
    // prints the three blank cells generated by the nroff bullet arm; they
    // are visible layout between one logical bullet/body selection.
    let input = b".TH T 1\n.SH D\n.RS 4\n.ie n \\{\\\n\\h'-04'\\(bu\\h'+03'\\c\n.\\}\n.el \\{\\\n.sp -1\n.IP \\(bu 2.3\n.\\}\n\\fBgit-revert\\fR(1)\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man)
        .expect("source-less formatter spacing is layout, not missing logical text");
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert!(fixed.surface.runs.iter().any(|run| {
        run.label.role == DisplayRole::Layout && fixed.surface.run_text(run.key) == Some("   ")
    }));
}
