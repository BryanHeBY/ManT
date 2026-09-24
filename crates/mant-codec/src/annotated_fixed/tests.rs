use libmandoc_rs::annotated::{
    AnnotatedDocument, AnnotatedMark, AnnotatedMetadata, AnnotatedRenderer, AnnotationCoverage,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{DisplayRole, DocumentBody, DocumentIndex, LinkTarget, OwnerRole, validate_document};
use mant_protocol::{
    EvidenceClass, ExplanationOptions, ExplanationQuery, SearchCase, SearchQuery, SearchScope,
    SearchSyntax,
};

use super::{lower_annotated_document, project_annotated_manual};

fn bundle(input: &[u8]) -> SourceBundle {
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", input.to_vec()).unwrap();
    bundle
}

fn native_query(input: &[u8], width: u32) -> mant_ir::ResolvedContent {
    let page = AnnotatedRenderer::new(width)
        .unwrap()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(lower_annotated_document(page).unwrap()),
        tldr: None,
    }
}

fn visible_total(query: &mant_ir::ResolvedContent, pattern: &str) -> u32 {
    mant_query::search_query(
        query,
        &SearchQuery {
            pattern: pattern.to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap()
    .total
}

#[test]
fn native_word_spaces_reach_fixed_visible_search() {
    // The exact input first ran on the pinned CVS -Tutf8 reference.  In
    // term.c::term_field, deferred word spaces are emitted before the next
    // glyph; the fixed visible query must retain that proven text relation.
    let input = b".TH T 1\n.SH DESCRIPTION\nalpha beta gamma\n.TP\n.B --foo\nfoo body\n";
    let query = native_query(input, 78);
    for pattern in ["alpha", "alpha beta", "foo body"] {
        assert_eq!(visible_total(&query, pattern), 1, "missing {pattern:?}");
    }
}

#[test]
fn native_unicode_hits_use_scalar_public_coordinates_and_byte_sources() {
    // Exact source first ran pinned CVS -Tutf8 -O width=78: term.c::
    // term_field preserves the wide scalar followed by ASCII in one body.
    let query = native_query(".TH T 1\n.SH D\n中a\n".as_bytes(), 78);
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let result = mant_query::search_query(
            &query,
            &SearchQuery {
                pattern: "中a".to_owned(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope,
                word: false,
                context_lines: 0,
                limit: 10,
                offset: 0,
            },
        )
        .unwrap();
        assert_eq!(result.total, 1);
        let hit = &result.matches[0];
        let (mant_protocol::SearchLocation::VisibleFixed {
            start_scalar: start,
            end_scalar: end,
            ..
        }
        | mant_protocol::SearchLocation::MarkdownArtifact {
            start_scalar: start,
            end_scalar: end,
            ..
        }) = hit.location
        else {
            panic!("unexpected Fixed search location");
        };
        assert_eq!(end - start, 2);
        assert_eq!(hit.matched_text, "中a");
        if scope == SearchScope::Visible {
            let slice = &hit.display_slices[0];
            assert_eq!(slice.end_scalar - slice.start_scalar, 2);
            let source = &result.content_projection.as_ref().unwrap().fragments[0].source;
            let mant_protocol::SearchFragmentSource::Fixed(source) = source else {
                panic!("Fixed fragment lost its byte source");
            };
            assert_eq!(source.end_byte - source.start_byte, 4);
        }
    }
}

#[test]
fn native_unsectioned_text_joins_styles_without_crossing_hard_lines() {
    // Both exact inputs first ran on pinned CVS -Tutf8 -O width=78.
    // man_term.c::print_man_nodelist() starts at ROOT's first child;
    // term.c::term_field/term_flushln supply the direct and hard joins.
    let styled = native_query(b".TH T 1\nalpha\\fBbeta\\fPgamma\n", 78);
    let DocumentBody::Fixed(fixed) = &styled.document.as_ref().unwrap().body else {
        panic!("native body is not Fixed");
    };
    assert!(fixed.regions.iter().any(|region| {
        region.kind == mant_ir::RegionKind::Unsectioned
            && region
                .selection
                .joins
                .contains(&mant_ir::TextJoin::DirectContact)
    }));
    assert_eq!(visible_total(&styled, "alphabeta"), 1);
    assert_eq!(visible_total(&styled, "alphabetagamma"), 1);

    let nofill = native_query(b".TH T 1\n.nf\nalpha\nbeta\n.fi\n", 78);
    assert_eq!(visible_total(&nofill, "alpha"), 1);
    assert_eq!(visible_total(&nofill, "beta"), 1);
    assert_eq!(visible_total(&nofill, "alphabeta"), 0);

    // These exact sources also ran on the same reference. The first ROOT
    // text may be nested under a style or link macro; a later SH owns its
    // own body and must not inherit the unsectioned region.
    let styled_first = native_query(b".TH T 1\n.B alpha\nbeta\n", 78);
    assert_eq!(visible_total(&styled_first, "alpha beta"), 1);
    let linked_first = native_query(b".TH T 1\n.UR https://example.test\nalpha\n.UE\nbeta\n", 78);
    assert_eq!(visible_total(&linked_first, "alpha"), 1);
    assert_eq!(visible_total(&linked_first, "beta"), 1);
    let section_after = native_query(b".TH T 1\n.B alpha\n.SH D\nbeta\n", 78);
    assert_eq!(visible_total(&section_after, "alpha beta"), 0);
}

#[test]
fn native_joins_preserve_cross_style_link_cell_and_wrap_search_boundaries() {
    // Each exact source first ran on pinned CVS -Tutf8 at its stated width.
    // term.c::term_field emits pending spaces before letters, while
    // term_flushln distinguishes a soft wrap from a literal hard line.
    let styled = native_query(b".TH T 1\n.SH DESCRIPTION\nalpha\\fBbeta\\fP gamma\n", 78);
    assert_eq!(visible_total(&styled, "alphabeta"), 1);
    assert_eq!(visible_total(&styled, "beta gamma"), 1);

    let linked = native_query(
        b".TH T 1\n.SH DESCRIPTION\n.UR https://example.test\nalpha beta\n.UE\n",
        78,
    );
    assert_eq!(visible_total(&linked, "alpha beta"), 1);

    let nofill = native_query(
        b".TH T 1\n.SH DESCRIPTION\n.nf\nalpha beta\ngamma delta\n.fi\n",
        78,
    );
    assert_eq!(visible_total(&nofill, "alpha beta"), 1);
    assert_eq!(visible_total(&nofill, "gamma delta"), 1);
    assert_eq!(visible_total(&nofill, "beta gamma"), 0);

    let table = native_query(
        b".TH T 1\n.SH DESCRIPTION\n.TS\ntab(;);\nl l.\nalpha beta;gamma delta\n.TE\n",
        78,
    );
    assert_eq!(visible_total(&table, "alpha beta"), 1);
    assert_eq!(visible_total(&table, "gamma delta"), 1);
    assert_eq!(visible_total(&table, "beta gamma"), 0);

    let wrapped = native_query(b".TH T 1\n.SH DESCRIPTION\nalpha beta gamma delta\n", 20);
    assert_eq!(visible_total(&wrapped, "beta gamma"), 1);
}

#[test]
fn native_multiword_definition_head_reaches_fixed_explain() {
    // The exact input first ran on pinned CVS -Tutf8.  term.c::term_field
    // flushes the head's word blank before the next glyph; a verified form
    // must retain both words rather than downgrade their join to Unknown.
    let input = b".TH T 1\n.SH DESCRIPTION\n.TP\n.B alpha beta\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let result = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "alpha beta".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert!(
        result
            .evidence
            .iter()
            .any(|item| item.class == EvidenceClass::DirectEntry),
        "multiword native head was not indexed: {result:?}"
    );
}

fn malformed_marks(marks: Vec<AnnotatedMark>) -> AnnotatedDocument {
    AnnotatedDocument {
        root_source: 0,
        profile: 0,
        width: 78,
        metadata: AnnotatedMetadata {
            macroset: 0,
            title: None,
            section: None,
            volume: None,
            operating_system: None,
            architecture: None,
            name: None,
            date: None,
            alias_target: None,
            has_body: false,
        },
        sources: Vec::new(),
        spans: Vec::new(),
        provenances: Vec::new(),
        diagnostics: Vec::new(),
        text: String::new(),
        rows: Vec::new(),
        runs: Vec::new(),
        marks,
        selection_parts: Vec::new(),
        join_text: String::new(),
        coverage: AnnotationCoverage {
            checks: Vec::new(),
            issues: Vec::new(),
        },
    }
}

fn malformed_anchor(key: u32, parent: u32) -> AnnotatedMark {
    AnnotatedMark {
        key,
        kind: 4,
        parent,
        owner: 0,
        source: 0,
        line: 0,
        column: 0,
        token: 0,
        region_kind: 0,
        title_region: 0,
        body_region: 0,
        flags: 0,
        selection_first: 0,
        selection_count: 0,
        point: None,
        native_table_position: None,
        name: Some("bad".to_owned()),
        link_target: None,
    }
}

#[test]
fn malformed_public_mark_keys_and_parent_cycles_return_errors() {
    // Pure defensive input, not a roff behavior assertion: the owned native
    // result is public and callers can construct values bypassing FFI checks.
    assert!(
        lower_annotated_document(malformed_marks(vec![malformed_anchor(u32::MAX, 0),])).is_err()
    );
    assert!(lower_annotated_document(malformed_marks(vec![malformed_anchor(1, 1),])).is_err());
    assert!(
        lower_annotated_document(malformed_marks(vec![
            malformed_anchor(1, 2),
            malformed_anchor(2, 1),
        ]))
        .is_err()
    );
}

#[test]
fn malformed_public_mark_role_flags_are_rejected_before_projection() {
    // Defensive owned-object boundary: no roff expectation is asserted here.
    let mut anchor = malformed_anchor(1, 0);
    anchor.flags = 16; // Definition evidence is valid only on owner marks.
    assert!(lower_annotated_document(malformed_marks(vec![anchor])).is_err());
    let mut owner = malformed_anchor(1, 0);
    owner.kind = 2;
    owner.flags = 8; // A subsection bit cannot turn an owner into a heading.
    assert!(lower_annotated_document(malformed_marks(vec![owner])).is_err());
}

#[test]
fn real_man_body_enters_one_fixed_surface_with_dense_typed_keys() {
    // Exact bytes first ran with pinned CVS -Tutf8 -O width=78.
    // man_term.c::print_man_node traverses SH HEAD/BODY and TP HEAD/BODY;
    // term.c::term_flushln is the sole device placement path.
    let input = b".TH T 1\n.SH D\n.TP\nterm\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert!(fixed.surface.text.contains("term"));
    assert!(fixed.surface.text.contains("body"));
    assert_eq!(fixed.headings.len(), 1);
    assert_eq!(fixed.owners.len(), 1);
    assert!(!fixed.owners[0].head.parts.is_empty());
    assert!(!fixed.owners[0].direct_body.parts.is_empty());
    assert!(fixed.owners[0].empty_point.is_none());
    assert_eq!(fixed.headings[0].key.get(), 1);
    assert_eq!(fixed.owners[0].key.get(), 1);
    assert_eq!(fixed.owners[0].role, OwnerRole::Definition);
    assert_eq!(
        fixed.owner_complete_form(&fixed.owners[0]),
        Some("term".to_owned())
    );
    assert!(
        DocumentIndex::build(&document)
            .get(fixed.owners[0].id.as_str())
            .is_some_and(|node| node.has_role(mant_ir::IndexedRole::Entry))
    );
}

#[test]
fn native_owner_role_distinguishes_definition_from_bullet_item() {
    // Exact input first ran pinned CVS -Tutf8 -O width=78. mdoc_term.c::
    // termp_it_pre reads the validated Bl type: tag heads are term labels,
    // while bullet glyphs are formatter markers, not declarations.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a\nbody\n.El\n.Bl -bullet\n.It\nother\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.owners.len(), 2);
    assert_eq!(fixed.owners[0].role, OwnerRole::Definition);
    assert_eq!(fixed.owners[1].role, OwnerRole::Other);
    assert_eq!(
        fixed.owner_complete_form(&fixed.owners[0]),
        Some("-a".to_owned())
    );
    assert!(fixed.owner_complete_form(&fixed.owners[1]).is_none());
    let index = DocumentIndex::build(&document);
    assert!(index.get(fixed.owners[0].id.as_str()).is_some());
    assert!(index.get(fixed.owners[1].id.as_str()).is_none());
}

#[test]
fn superseded_private_projection_display_inputs_reach_fixed_consumers() {
    // These exact four sources were rerun with the pinned CVS UTF-8/78
    // reference before these assertions. man_term.c::print_man_node flushes
    // no-fill before PP and tbl_term.c::tbl_word emits the l0 cells without
    // invented spacing. term.c::term_field folds the real \z backspace;
    // eqn_term.c::eqn_box emits the fraction in the surrounding body.
    for (source, expected, once) in [
        (
            ".TH T 1\n.SH D\n.nf\nbefore\n.PP\nafter\n.fi\n",
            "     after",
            true,
        ),
        (
            ".TH T 1\n.SH D\n.TS\ntab(;);\nl0 l.\na;b\n.TE\n",
            "     ab",
            false,
        ),
        (".TH T 1\n.SH D\n.nf\n\\zAB\n.fi\n", "     B", false),
        (
            ".TH T 1\n.SH D\nbefore\n.EQ\nx over y\n.EN\nafter\n",
            "     before x/y after",
            false,
        ),
    ] {
        let document =
            project_annotated_manual("t.1", &bundle(source.as_bytes()), InputFormat::Man)
                .expect("private-projection input reaches Fixed IR");
        assert!(validate_document(&document).is_empty(), "{source}");
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("private-projection input did not reach Fixed IR");
        };
        let query = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".to_owned(),
            document: Some(document.clone()),
            tldr: None,
        };
        let rows = mant_ui::DocumentView::new(&query)
            .render(78)
            .text
            .lines
            .into_iter()
            .map(|line| line.to_string().trim_end().to_owned())
            .collect::<Vec<_>>();
        assert!(
            rows.iter().any(|row| row == expected),
            "missing {expected:?} in {rows:?}"
        );
        if once {
            assert_eq!(rows.iter().filter(|row| row.contains(expected)).count(), 1);
        }
        assert!(!fixed.surface.rows.is_empty());
    }
}

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
