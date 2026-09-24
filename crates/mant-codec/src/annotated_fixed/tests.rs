use libmandoc_rs::annotated::{
    AnnotatedDocument, AnnotatedMark, AnnotatedMetadata, AnnotatedRenderer, AnnotationCoverage,
};
use libmandoc_rs::{InputFormat, SourceBundle};
use mant_ir::{
    DisplayRole, DocumentAddress, DocumentBody, DocumentIndex, EntryKind, LinkTarget,
    MarkdownOrigin, OwnerHeadRole, OwnerRole, ParameterKind, SourceKey, validate_document,
};
use mant_protocol::{
    DocumentScope, EvidenceBasis, EvidenceClass, ExplanationOptions, ExplanationQuery,
    ResolvedDocumentScope, ScopedDocument, SearchCase, SearchQuery, SearchScope, SearchSyntax,
};

use super::{lower_annotated_document, project_annotated_manual};

fn bundle(input: &[u8]) -> SourceBundle {
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", input.to_vec()).unwrap();
    bundle
}

#[test]
fn macro_generated_marks_keep_source_identity_without_authored_coordinates() {
    // Exact bytes ran pinned CVS -Ttree/-Tutf8 before these assertions.
    // read.c::mparse_buf_r reparses the EE body at the invocation source key;
    // its displayed line 13 is an expansion coordinate, not an authored
    // location for SH, TP, UR or the empty EQ region.
    let input = b".TH T 1 2026-09-24\n.de EE\n.SH OPTIONS\n.TP\n.B --macro-generated\nDescription.\n.UR https://example.test/x\nlabel\n.UE\n.EQ\n.EN\n..\n.EE\n";
    let page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    for kind in [1, 2, 3, 6] {
        assert!(page.marks.iter().any(|mark| {
            mark.kind == kind && mark.source == 1 && mark.line == 0 && mark.column == 0
        }));
    }
    assert!(page.marks.iter().any(|mark| {
        mark.kind == 5
            && mark.region_kind == 8
            && mark.source == 1
            && mark.line == 0
            && mark.column == 0
            && mark.selection_count == 0
    }));
    let document = lower_annotated_document(page).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.headings[0].source, None);
    assert_eq!(fixed.headings[0].source_key, Some(SourceKey::FIRST));
    assert_eq!(fixed.owners[0].source, None);
    assert_eq!(fixed.owners[0].source_key, Some(SourceKey::FIRST));
    assert!(fixed.owners[0].head_components.iter().any(|component| {
        component.source.is_none() && component.source_key == Some(SourceKey::FIRST)
    }));
    assert_eq!(fixed.links[0].source, None);
    assert_eq!(fixed.links[0].source_key, Some(SourceKey::FIRST));
    let equation = fixed
        .regions
        .iter()
        .find(|region| region.kind == mant_ir::RegionKind::Equation)
        .expect("empty equation region");
    assert!(equation.selection.parts.is_empty());
    assert_eq!(equation.source, None);
    assert_eq!(equation.source_key, Some(SourceKey::FIRST));

    // The exact direct SH input also ran pinned CVS -Ttree: its line 2 is an
    // authored coordinate, so no source-only companion may be populated.
    let authored = project_annotated_manual(
        "t.1",
        &bundle(b".TH T 1 2026-09-24\n.SH AUTHORED\nbody\n"),
        InputFormat::Man,
    )
    .unwrap();
    let DocumentBody::Fixed(authored) = &authored.body else {
        panic!("not Fixed")
    };
    assert_eq!(
        authored.headings[0].source.as_ref().map(|span| span.line),
        Some(2)
    );
    assert_eq!(authored.headings[0].source_key, None);
}

#[test]
fn macro_diagnostic_retains_only_its_source_identity() {
    // Exact bytes ran pinned CVS -Ttree -Wwarning first: the generated UR
    // reports a missing resource identifier at expansion line 8. Its source
    // key is known, but that line is not an authored position in the file.
    let input = b".TH T 1 2026-09-24\n.de EE\n.SH D\n.UR\nlabel\n.UE\n..\n.EE\n";
    let page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    assert!(page.diagnostics.iter().any(|diagnostic| {
        diagnostic.span != 0
            && page
                .spans
                .get((diagnostic.span - 1) as usize)
                .is_some_and(|span| span.source == 1 && span.line_column.is_none())
    }));
    let document = lower_annotated_document(page).unwrap();
    assert!(validate_document(&document).is_empty());
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.source.is_none() && diagnostic.source_key == Some(SourceKey::FIRST)
    }));
}

#[test]
fn expanded_mdoc_head_components_keep_all_native_option_names() {
    // Each exact input ran pinned CVS -Tutf8 first. read.c::mparse_buf_r
    // reparses macro and string expansions without authored coordinates;
    // mdoc_term.c::termp_fl_pre still executes each Fl and emits its dash.
    for (label, input) in [
        (
            "macro-arguments",
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.de Opt\n.It Fl a , Fl b Ar file\nOption description.\n..\n.Bl -tag -width xxx\n.Opt\n.El\n".as_slice(),
        ),
        (
            "macro-no-argument",
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.de Opt\n.It Fl a , Fl b\nOption description.\n..\n.Bl -tag -width xxx\n.Opt\n.El\n".as_slice(),
        ),
        (
            "string-expansion",
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.ds X Fl a , Fl b Ar file\n.Bl -tag -width xxx\n.It \\*[X]\nOption description.\n.El\n".as_slice(),
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc)
            .unwrap();
        assert!(validate_document(&document).is_empty(), "{label}");
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        let owner = &fixed.owners[0];
        assert_eq!(owner.entry.as_ref().unwrap().names, ["-a", "-b"], "{label}");
        assert!(owner.head_components.iter().all(|component| {
            component.source.is_some() || component.source_key == Some(SourceKey::FIRST)
        }));
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for name in ["-a", "-b"] {
            let response = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(response.counts.direct_entry.total, 1, "{label}: {name}");
            response.validate_references().unwrap();
        }
    }
}

#[test]
fn expanded_man_alternating_font_components_keep_name_boundaries() {
    // Both exact inputs ran pinned CVS -Tutf8 first. read.c::mparse_buf_r
    // removes authored coordinates from expansion nodes, while man_term.c::
    // pre_alternate preserves which operand received bold font.
    for (label, input, names) in [
        (
            "br-aliases",
            b".TH T 1\n.SH OPTIONS\n.de Opt\n.TP\n.BR -a , --all\nShared description.\n..\n.Opt\n"
                .as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "bi-argument",
            b".TH T 1\n.SH OPTIONS\n.de Opt\n.TP\n.BI --output= FILE\nDescription.\n..\n.Opt\n"
                .as_slice(),
            vec!["--output"],
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        assert!(validate_document(&document).is_empty(), "{label}");
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        let owner = &fixed.owners[0];
        assert_eq!(owner.entry.as_ref().unwrap().names, names, "{label}");
        assert!(owner.head_components.iter().all(|component| {
            component.source.is_none() && component.source_key == Some(SourceKey::FIRST)
        }));
    }
}

#[test]
fn rejected_hanging_candidates_remain_ordinary_mentions() {
    // Each exact input ran pinned CVS -Tutf8 first. man_term.c::pre_PP and
    // pre_RS display the original paragraph independently of our optional
    // PP/RS declaration evidence; a rejected head is still readable ink.
    for (label, input, needle) in [
        (
            "prose",
            b".TH T 1\n.SH DESCRIPTION\n.PP\nCompare --foo with --bar.\n.RS 4\nAdditional details.\n.RE\n".as_slice(),
            "--foo",
        ),
        (
            "zero-indent",
            b".TH T 1\n.SH DESCRIPTION\n.PP\nCompare --foo with --bar.\n.RS 0\nAdditional details.\n.RE\n".as_slice(),
            "--foo",
        ),
        (
            "intervening-prose",
            b".TH T 1\n.SH DESCRIPTION\n.PP\n.B --git-dir\nintervening text\n.RS 4\nAdditional details.\n.RE\n".as_slice(),
            "--git-dir",
        ),
    ] {
        let resolved = native_query(input, 78);
        let document = resolved.document.as_ref().unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        assert!(fixed.owners.iter().any(|owner| owner.hanging_candidate && owner.entry.is_none()), "{label}");
        assert!(visible_total(&resolved, needle) > 0, "{label}");
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: needle.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, 0, "{label}");
        assert!(result.counts.context_mention.total > 0, "{label}");
        result.validate_references().unwrap();
    }
}

#[test]
fn native_man_declarations_keep_legacy_option_names() {
    for (label, input) in [
        ("width", b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--width=NUMBER\"\nWidth description.\n".as_slice()),
        ("output", b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"--output=\" FILE\nOutput description.\n".as_slice()),
        ("set", b".TH T 1\n.SH OPTIONS\n.TP\n.B --set=KEY,VALUE\nSet description.\n".as_slice()),
        ("ip", b".TH T 1\n.SH OPTIONS\n.IP --plain 4\nPlain description.\n".as_slice()),
        ("tar", b".TH T 1\n.SH OPTIONS\n.TP\n\\fB\\-f\\fR, \\fB\\-\\-file\\fR=\\fIARCHIVE\\fR\nArchive description.\n".as_slice()),
        ("bi-operand", b".TH T 1\n.SH OPTIONS\n.TP\n.BI --foo FILE\nbody\n".as_slice()),
        ("bi-option-operand", b".TH T 1\n.SH OPTIONS\n.TP\n.BI --foo -x\nbody\n".as_slice()),
        ("br-alias", b".TH T 1\n.SH OPTIONS\n.TP\n.BR -a , --all\nbody\n".as_slice()),
        ("duplicate", b".TH T 1\n.SH OPTIONS\n.TP\n.B \"-a, -a\"\nbody\n".as_slice()),
        ("spaced-slash", b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-a / \\-\\-all\nBODY\n".as_slice()),
        ("plain-term", b".TH T 1\n.SH OPTIONS\n.TP\n.B FILE\nbody\n".as_slice()),
        ("italic-term", b".TH T 1\n.SH OPTIONS\n.TP\n.I --save\nitalic body\n".as_slice()),
        ("empty-bold", b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\" --foo\nDescription.\n".as_slice()),
        ("empty-bold-roman", b".TH T 1\n.SH OPTIONS\n.TP\n.BR \"\" --foo\nDescription.\n".as_slice()),
    ] {
        let old = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
        let new = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        if label == "duplicate" {
            let query = ExplanationQuery {
                entry: "-a".into(),
                options: ExplanationOptions::default(),
            };
            let old_result = mant_query::explain_query(&mant_ir::ResolvedContent {
                address: None, label: "T(1)".into(), document: Some(old.clone()), tldr: None,
            }, &query).unwrap();
            let new_result = mant_query::explain_query(&mant_ir::ResolvedContent {
                address: None, label: "T(1)".into(), document: Some(new.clone()), tldr: None,
            }, &query).unwrap();
            assert_eq!(
                new_result.evidence[0].entry.as_ref().unwrap().name_bindings[0].occurrences.len(),
                old_result.evidence[0].entry.as_ref().unwrap().name_bindings[0].occurrences.len(),
            );
        }
        let old = mant_ir::SemanticIndex::build(&old)
            .section("options")
            .iter()
            .map(|entry| (entry.names.clone(), entry.forms.clone()))
            .collect::<Vec<_>>();
        let new = mant_ir::SemanticIndex::build(&new)
            .section("options")
            .iter()
            .map(|entry| (entry.names.clone(), entry.forms.clone()))
            .collect::<Vec<_>>();
        assert_eq!(new, old, "{label}");
    }
}

#[test]
fn tp_layout_width_keeps_the_next_line_label_and_tq_reading_group() {
    // Exact input ran pinned CVS -Ttree and -Tutf8 first. man_term.c::pre_TP
    // skips the same-line width operand and prints the NODE_LINE label;
    // TQ extends the TP head without replacing its shared body.
    let input = b".TH T 1\n.SH OPTIONS\n.TP 4\n.B -a\n.TQ\n.B --all\nShared description.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(fixed.owners.len(), 2);
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["-a"]);
    assert_eq!(fixed.owners[1].entry.as_ref().unwrap().names, ["--all"]);
    assert_eq!(fixed.owners[1].preceding_owner, Some(fixed.owners[0].key));
    let index = mant_ir::SemanticIndex::build(&document);
    assert!(index.fixed_reading_group(fixed.owners[0].key).is_some());
    assert!(validate_document(&document).is_empty());
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

fn assert_single_section_covers_each_visible_byte_once(fixed: &mant_ir::FixedBody) {
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    assert_eq!(reader.roots().len(), 1);
    let parts = reader.subtree_parts(reader.roots()[0]).unwrap();
    let mut counts = fixed
        .surface
        .runs
        .iter()
        .map(|run| vec![0_u8; usize::try_from(run.byte_count).unwrap()])
        .collect::<Vec<_>>();
    for part in parts {
        let slots = &mut counts[(part.slice.run.get() - 1) as usize];
        for count in &mut slots[usize::try_from(part.slice.start_byte).unwrap()
            ..usize::try_from(part.slice.end_byte).unwrap()]
        {
            *count += 1;
        }
    }
    for (run, counts) in fixed.surface.runs.iter().zip(counts) {
        if run.label.role != DisplayRole::Layout {
            assert!(
                counts.iter().all(|&count| count == 1),
                "run {run:?}: {counts:?}"
            );
        }
    }
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

#[test]
fn native_section_reader_includes_entries_and_transparent_regions_once() {
    // These exact inputs ran on pinned CVS -Tutf8 -O width=78 before this
    // assertion. man_term.c::print_man_node() enters ROFFT_TBL separately;
    // tbl_term.c::term_tbl() emits its cell text outside the heading's direct
    // body, while MAN_TP has its own HEAD/BODY scopes.
    for input in [
        b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\nfoo body\n".as_slice(),
        b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l.\nkey;value\n.TE\n".as_slice(),
        b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.TS\ntab(;);\nl l.\nkey;value\n.TE\n".as_slice(),
    ] {
        let query = native_query(input, 78);
        let DocumentBody::Fixed(fixed) = &query.document.as_ref().unwrap().body else {
            panic!("native body is not Fixed");
        };
        let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
        let parts = reader.subtree_parts(reader.roots()[0]).unwrap();
        let text = parts.iter().map(|part| part.text).collect::<String>();
        if input.windows(3).any(|window| window == b".TP") {
            assert!(text.contains("--foo"), "{text}");
        }
        if input.windows(3).any(|window| window == b".TS") {
            assert!(text.contains("key"), "{text}");
            assert!(text.contains("value"), "{text}");
        }
        if input.windows(8).any(|window| window == b"foo body") {
            assert!(text.contains("foo body"), "{text}");
        }
    }
}

#[test]
fn native_owner_reading_includes_table_and_visible_search_uses_its_section() {
    // Exact input ran pinned CVS -Tutf8 -O width=78. The table's final
    // display cells belong under OPTIONS even though the native table span
    // is not the owner's direct BODY selection.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.TS\ntab(;);\nl l.\nkey;value\n.TE\n";
    let query = native_query(input, 78);
    let result = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "--foo".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
        result.evidence[0].content.as_ref().unwrap()
    else {
        panic!("not Fixed owner")
    };
    assert!(reading_body.parts.iter().any(|part| part.text == "key"));
    assert!(reading_body.parts.iter().any(|part| part.text == "value"));
    let table_only = native_query(
        b".TH T 1\n.SH DATA\n.TS\ntab(;);\nl l.\nkey;value\n.TE\n",
        78,
    );
    let found = mant_query::search_query(
        &table_only,
        &SearchQuery {
            pattern: "value".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(found.total, 1);
    assert_eq!(found.matches[0].outline.node.title(), "DATA");
}

#[test]
fn fixed_explain_keeps_native_cells_distinct_from_unicode_scalars_and_bytes() {
    // Exact input ran pinned CVS -Tutf8 -O width=78. term.c::term_field
    // advances the terminal by three cells for the two-scalar "中a" body.
    let query = native_query(".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n中a\n".as_bytes(), 78);
    let result = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "--foo".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
        result.evidence[0].content.as_ref().unwrap()
    else {
        panic!("not Fixed owner")
    };
    let part = reading_body
        .parts
        .iter()
        .find(|part| part.text == "中a")
        .unwrap();
    assert_eq!(part.text.chars().count(), 2);
    assert_eq!(part.text.len(), 4);
    assert_eq!(part.width, 3);
    assert!(part.column > 0);
}

#[test]
fn native_mdoc_nested_owner_and_literal_are_in_complete_reading_view() {
    // Both exact inputs ran pinned CVS -Tutf8 -O width=78. mdoc_term.c::
    // termp_it_pre/post and termp_bd_pre/post keep nested .It and literal
    // output inside their enclosing native section and owner boundaries.
    let nested = b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl outer\nouter body\n.Bl -tag\n.It Fl inner\ninner body\n.El\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(nested), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_single_section_covers_each_visible_byte_once(fixed);
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let section = reader.subtree_parts(reader.roots()[0]).unwrap();
    let text = section.iter().map(|part| part.text).collect::<String>();
    for expected in ["outer", "outer body", "inner", "inner body"] {
        assert!(text.contains(expected), "{expected}: {text}");
    }
    let outer = fixed
        .owners
        .iter()
        .find(|owner| owner.parent.is_none())
        .unwrap();
    let body = reader.owner_body_parts(outer.key).unwrap();
    let text = body.iter().map(|part| part.text).collect::<String>();
    assert!(text.contains("inner body"), "{text}");

    let literal = b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl foo\n.Bd -literal\ncode one\n.Ed\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(literal), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let body = reader.owner_body_parts(fixed.owners[0].key).unwrap();
    assert!(body.iter().any(|part| part.text.contains("code one")));
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(document),
        tldr: None,
    };
    let explained = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "-foo".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
        explained.evidence[0].content.as_ref().unwrap()
    else {
        panic!("not Fixed owner")
    };
    assert!(
        reading_body
            .parts
            .iter()
            .any(|part| part.text.contains("code one"))
    );
}

#[test]
fn native_man_pp_rs_continuation_is_read_once_under_its_definition() {
    // This exact input ran pinned CVS -Ttree/-Tutf8 before this assertion.
    // man_validate.c removes the leading PP but records its execution on B;
    // man_macro.c::blk_exp keeps a separate RS, and man_term.c::pre_RS
    // offsets its body without moving the text into that B declaration.
    let input = b".TH T 1\n.SH OPTIONS\n.PP\n.B --git-dir\n.RS 4\nCONTINUATION\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(validate_document(&document).is_empty());
    let owner = fixed
        .owners
        .iter()
        .find(|owner| {
            owner
                .entry
                .as_ref()
                .is_some_and(|entry| entry.names == ["--git-dir"])
        })
        .expect("native PP declaration");
    let continuation = fixed
        .regions
        .iter()
        .find(|region| region.kind == mant_ir::RegionKind::HangingContinuation)
        .expect("native PP/RS relation");
    assert_eq!(continuation.owner, None);
    assert_eq!(continuation.continuation_of, Some(owner.key));
    assert_eq!(continuation.section, owner.section);
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let body = reader.owner_body_parts(owner.key).unwrap();
    assert_eq!(
        body.iter()
            .map(|part| part.text)
            .collect::<String>()
            .matches("CONTINUATION")
            .count(),
        1
    );
    let section = reader.subtree_parts(reader.roots()[0]).unwrap();
    assert_eq!(
        section
            .iter()
            .map(|part| part.text)
            .collect::<String>()
            .matches("CONTINUATION")
            .count(),
        1
    );
    let result = mant_query::explain_query(
        &mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        },
        &ExplanationQuery {
            entry: "--git-dir".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
        result.evidence[0].content.as_ref().unwrap()
    else {
        panic!("not Fixed owner")
    };
    assert_eq!(
        reading_body
            .parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>()
            .matches("CONTINUATION")
            .count(),
        1
    );
    result.validate_references().unwrap();
}

#[test]
fn man_hanging_declaration_requires_a_complete_head_and_positive_rs_indent() {
    // Every exact source ran pinned CVS -Ttree/-Tutf8 before assertions.
    // man_validate.c marks each elided PP child;
    // man_term.c::pre_RS records the effective offset, not source adjacency.
    // Intervening text remains in the same PP, so native continuation can
    // survive while the complete HEAD grammar rejects a semantic name.
    for (label, input, expect_relation, name) in [
        (
            "zero-indent",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --git-dir\n.RS 0\nzero body\n.RE\n".as_slice(),
            false,
            "--git-dir",
        ),
        (
            "intervening-text",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --git-dir\nintervening text\n.RS 4\ngap body\n.RE\n"
                .as_slice(),
            true,
            "--git-dir",
        ),
        (
            "br-boundary",
            b".TH T 1\n.SH OPTIONS\n.br\n.B --git-dir\n.RS 4\nbr body\n.RE\n".as_slice(),
            false,
            "--git-dir",
        ),
        (
            "empty-rs",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\n\\&\n.RE\n".as_slice(),
            true,
            "--foo",
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        assert!(validate_document(&document).is_empty(), "{label}");
        assert_eq!(
            fixed
                .regions
                .iter()
                .any(|region| region.kind == mant_ir::RegionKind::HangingContinuation),
            expect_relation,
            "{label} native relation"
        );
        let result = mant_query::explain_query(
            &mant_ir::ResolvedContent {
                address: None,
                label: "T(1)".into(),
                document: Some(document),
                tldr: None,
            },
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, 0, "{label}");
        result.validate_references().unwrap();
    }
}

#[test]
fn native_hanging_option_forms_reuse_checked_name_boundaries() {
    // Each exact input ran pinned CVS -Tutf8 before these assertions.
    // man_validate.c keeps the PP or first-section execution boundary;
    // man_term.c::pre_alternate prints BI operands without an inserted space.
    // The whole head stays one form while native bold components can prove a
    // shorter name than the visibly glued operand.
    for (label, input, expected_names, expected_body) in [
        (
            "first-section",
            b".TH T 1\n.SH OPTIONS\n.B --git-dir=path\n.RS 4\nfirst body\n.RE\n".as_slice(),
            vec!["--git-dir"],
            "first body",
        ),
        (
            "argument",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --output FILE\n.RS 4\noutput body\n.RE\n".as_slice(),
            vec!["--output"],
            "output body",
        ),
        (
            "two-children",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --output\n.I FILE\n.RS 4\nstyled body\n.RE\n".as_slice(),
            vec!["--output"],
            "styled body",
        ),
        (
            "inline-font",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B \"\\fB--git-dir=\\fIpath\\fR\"\n.RS 4\ninline body\n.RE\n".as_slice(),
            vec!["--git-dir"],
            "inline body",
        ),
        (
            "aliases",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B \"-a, --all\"\n.RS 4\nalias body\n.RE\n".as_slice(),
            vec!["-a", "--all"],
            "alias body",
        ),
        (
            "alternating-font",
            b".TH T 1\n.SH OPTIONS\n.BI --foo FILE\n.RS 4\nBI body\n.RE\n".as_slice(),
            vec!["--foo"],
            "BI body",
        ),
        (
            "lp-alias",
            b".TH T 1\n.SH OPTIONS\n.LP\n.B --foo\n.RS 4\nLP body\n.RE\n".as_slice(),
            vec!["--foo"],
            "LP body",
        ),
        (
            "p-alias",
            b".TH T 1\n.SH OPTIONS\n.P\n.B --foo\n.RS 4\nP body\n.RE\n".as_slice(),
            vec!["--foo"],
            "P body",
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        assert!(validate_document(&document).is_empty(), "{label}");
        let owner = fixed
            .owners
            .iter()
            .find(|owner| owner.hanging_candidate)
            .expect("native hanging candidate");
        let entry = owner.entry.as_ref().expect("checked semantic declaration");
        assert_eq!(entry.names, expected_names, "{label}");
        assert_eq!(entry.forms.len(), 1, "{label}");
        for name in expected_names {
            let result = mant_query::explain_query(
                &mant_ir::ResolvedContent {
                    address: None,
                    label: "T(1)".into(),
                    document: Some(document.clone()),
                    tldr: None,
                },
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, 1, "{label}: {name}");
            let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
                result.evidence[0].content.as_ref().unwrap()
            else {
                panic!("not Fixed owner: {label}")
            };
            let body = reading_body
                .parts
                .iter()
                .map(|part| part.text.as_str())
                .collect::<String>();
            assert!(body.contains(expected_body), "{label}: {body}");
            result.validate_references().unwrap();
        }
    }
}

#[test]
fn hanging_continuation_reads_native_descendants_once_without_inventing_a_new_owner() {
    // Each exact input ran pinned CVS -Ttree/-Tutf8 before these assertions.
    // man_macro.c::blk_exp keeps the RS BODY as one scope, while its tbl and
    // nested TP nodes create independent descendants. man_term.c::pre_RS
    // indents that scope; it does not move its children into the B node.
    for (label, input, expected) in [
        (
            "table",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\nbefore\n.TS\ntab(;);\nl l.\nkey;value\n.TE\nafter\n.RE\n".as_slice(),
            &["before", "key", "value", "after"][..],
        ),
        (
            "literal",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\nbefore\n.nf\n  code one\n  code two\n.fi\nafter\n.RE\n".as_slice(),
            &["before", "code one", "code two", "after"][..],
        ),
        (
            "literal-only",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\n.nf\n  code one\n  code two\n.fi\n.RE\n".as_slice(),
            &["code one", "code two"][..],
        ),
        (
            "nested-definition",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\nbefore\n.TP\n.B --inner\ninner body\nafter\n.RE\n".as_slice(),
            &["before", "--inner", "inner body", "after"][..],
        ),
        (
            "nested-only",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\n.TP\n.B --inner\ninner body\n.RE\n".as_slice(),
            &["--inner", "inner body"][..],
        ),
        (
            "nested-empty-body",
            b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\n.TP\n.B --inner\n.RE\n".as_slice(),
            &["--inner"][..],
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        assert!(validate_document(&document).is_empty(), "{label}");
        assert_single_section_covers_each_visible_byte_once(fixed);
        if label == "literal-only" {
            assert!(
                fixed
                    .regions
                    .iter()
                    .find(|region| region.kind == mant_ir::RegionKind::HangingContinuation)
                    .is_some_and(|region| !region.selection.parts.is_empty()),
                "man .nf glyphs must belong to RS direct selection"
            );
        }
        let owner = fixed
            .owners
            .iter()
            .find(|owner| owner.entry.as_ref().is_some_and(|entry| entry.names == ["--foo"]))
            .expect("hanging definition");
        let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
        let body = reader
            .owner_body_parts(owner.key)
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>();
        let section = reader
            .subtree_parts(reader.roots()[0])
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>();
        let result = mant_query::explain_query(
            &mant_ir::ResolvedContent {
                address: None,
                label: "T(1)".into(),
                document: Some(document.clone()),
                tldr: None,
            },
            &ExplanationQuery {
                entry: "--foo".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, 1, "{label}");
        let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
            result.evidence[0].content.as_ref().unwrap()
        else {
            panic!("not Fixed owner: {label}")
        };
        let projected = reading_body
            .parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>();
        for token in expected {
            assert_eq!(body.matches(token).count(), 1, "{label} reader: {body}");
            assert_eq!(section.matches(token).count(), 1, "{label} section: {section}");
            assert_eq!(projected.matches(token).count(), 1, "{label} explain: {projected}");
        }
        result.validate_references().unwrap();
    }
}

#[test]
fn table_only_hanging_region_remains_presentation_without_an_entry() {
    // Exact input ran pinned CVS -Ttree before this assertion. The RS BODY
    // contains only a tbl node; it supplies no direct lexical description.
    let input =
        b".TH T 1\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\n.TS\ntab(;);\nl l.\nkey;value\n.TE\n.RE\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(validate_document(&document).is_empty());
    assert!(fixed.owners.iter().any(|owner| owner.hanging_candidate));
    assert!(fixed.owners.iter().all(|owner| owner.entry.is_none()));
    assert_single_section_covers_each_visible_byte_once(fixed);
}

#[test]
fn hanging_rs_with_unlabeled_ip_is_readable_without_an_invalid_definition_proof() {
    // Exact input ran pinned CVS -Ttree before this assertion. man_macro.c::
    // blk_exp retains RS; man_validate.c::post_IP retains the label-less IP
    // BODY. man_term.c::pre_IP displays it, though the IP is not a named
    // Definition owner in the native collector.
    let input = b".TH T 1 2026-09-24\n.SH OPTIONS\n.PP\n.B --foo\n.RS 4\n.IP\ninside\n.RE\n";
    let page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    let continuation = page
        .marks
        .iter()
        .find(|mark| mark.region_kind == 12)
        .expect("RS continuation");
    let nested_owner = page
        .marks
        .iter()
        .find(|mark| mark.kind == 2 && mark.parent == continuation.key)
        .expect("nested IP owner");
    assert!(
        page.marks
            .iter()
            .any(|mark| mark.region_kind == 3 && mark.parent == nested_owner.key)
    );
    assert_eq!(nested_owner.flags & 16, 0, "IP is presentation-only");
    let document = lower_annotated_document(page).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let candidate = fixed
        .owners
        .iter()
        .find(|owner| owner.hanging_candidate)
        .expect("hanging candidate");
    assert_eq!(candidate.hanging_nested_head, None);
    let reading = mant_ir::FixedSectionReader::new(fixed)
        .unwrap()
        .owner_body_parts(candidate.key)
        .unwrap()
        .iter()
        .map(|part| part.text)
        .collect::<String>();
    assert!(reading.contains("inside"), "{reading}");
}

#[test]
fn native_mdoc_column_bodies_remain_one_owner_and_one_reading() {
    // Exact input ran pinned CVS -Tutf8 -O width=78 before this assertion.
    // mdoc_macro.c::phrase_ta() creates a separate BODY for each .It column;
    // mdoc_term.c::termp_it_pre() places both in the same list item.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl foo\n.Bl -column one two\n.It alpha Ta beta\n.El\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_single_section_covers_each_visible_byte_once(fixed);
    let outer = fixed
        .owners
        .iter()
        .find(|owner| owner.parent.is_none())
        .unwrap();
    let inner = fixed
        .owners
        .iter()
        .find(|owner| owner.parent == Some(outer.key))
        .unwrap();
    let direct = inner
        .direct_body
        .parts
        .iter()
        .map(|part| {
            fixed
                .surface
                .run_text(part.run)
                .unwrap()
                .get(
                    usize::try_from(part.start_byte).unwrap()
                        ..usize::try_from(part.end_byte).unwrap(),
                )
                .unwrap()
        })
        .collect::<String>();
    assert_eq!(direct.matches("alpha").count(), 1);
    assert_eq!(direct.matches("beta").count(), 1);
    assert!(
        inner
            .direct_body
            .joins
            .contains(&mant_ir::TextJoin::Unknown)
    );
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    for parts in [
        reader.subtree_parts(reader.roots()[0]).unwrap(),
        reader.owner_body_parts(outer.key).unwrap(),
    ] {
        let text = parts.iter().map(|part| part.text).collect::<String>();
        assert_eq!(text.matches("alpha").count(), 1, "{text}");
        assert_eq!(text.matches("beta").count(), 1, "{text}");
    }
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(document),
        tldr: None,
    };
    let result = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "-foo".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
        result.evidence[0].content.as_ref().unwrap()
    else {
        panic!("not Fixed owner")
    };
    let text = reading_body
        .parts
        .iter()
        .map(|part| part.text.as_str())
        .collect::<String>();
    assert_eq!(text.matches("alpha").count(), 1);
    assert_eq!(text.matches("beta").count(), 1);
}

#[test]
fn native_margin_glyph_stays_with_its_flushed_owner_and_section() {
    // Exact input ran pinned CVS -Tutf8 -O width=78 before this assertion.
    // term.c::endline() emits .mc after the field as a separate direct write.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.mc |\nsome body\n.br\n.mc\n";
    let query = native_query(input, 78);
    let DocumentBody::Fixed(fixed) = &query.document.as_ref().unwrap().body else {
        panic!("not Fixed")
    };
    assert_single_section_covers_each_visible_byte_once(fixed);
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let owner = &fixed.owners[0];
    for parts in [
        reader.subtree_parts(reader.roots()[0]).unwrap(),
        reader.owner_body_parts(owner.key).unwrap(),
    ] {
        let text = parts.iter().map(|part| part.text).collect::<String>();
        assert_eq!(text.matches('|').count(), 1, "{text}");
    }
    let result = mant_query::search_query(
        &query,
        &SearchQuery {
            pattern: "|".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.matches[0].outline.node.title(), "OPTIONS");
}

#[test]
fn native_mdoc_columns_cover_multiple_and_empty_final_bodies() {
    // Each exact source ran pinned CVS -Tutf8 -O width=78 first.  The .Ta
    // and tab paths in mdoc_macro.c::phrase_ta() both create a separate BODY;
    // an empty final BODY does not erase the preceding visible column.
    for (input, expected) in [
        (
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl foo\n.Bl -column one two three\n.It alpha Ta beta Ta gamma\n.El\n.El\n".as_slice(),
            ["alpha", "beta", "gamma"].as_slice(),
        ),
        (
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl foo\n.Bl -column one two\n.It alpha\tbeta\n.El\n.El\n".as_slice(),
            ["alpha", "beta"].as_slice(),
        ),
        (
            b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It Fl foo\n.Bl -column one two\n.It alpha Ta\n.El\n.El\n".as_slice(),
            ["alpha"].as_slice(),
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        let outer = fixed.owners.iter().find(|owner| owner.parent.is_none()).unwrap();
        let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
        for parts in [
            reader.subtree_parts(reader.roots()[0]).unwrap(),
            reader.owner_body_parts(outer.key).unwrap(),
        ] {
            let text = parts.iter().map(|part| part.text).collect::<String>();
            for word in expected {
                assert_eq!(text.matches(word).count(), 1, "{text}");
            }
        }
        let query = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        let explained = mant_query::explain_query(
            &query,
            &ExplanationQuery {
                entry: "-foo".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
            explained.evidence[0].content.as_ref().unwrap()
        else {
            panic!("not Fixed owner")
        };
        let text = reading_body
            .parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>();
        for word in expected {
            assert_eq!(text.matches(word).count(), 1, "{text}");
        }
    }
}

#[test]
fn native_margin_follows_new_heading_and_unicode_owner() {
    // Both exact inputs ran pinned CVS -Tutf8 -O width=78 first.  In
    // term.c::endline(), the configured margin is emitted after the physical
    // field, including a later heading; encode1() handles Unicode .mc.
    let sections = native_query(
        b".TH T 1\n.SH FIRST\n.mc |\nfirst\n.SH SECOND\nsecond\n.mc\n",
        78,
    );
    let DocumentBody::Fixed(fixed) = &sections.document.as_ref().unwrap().body else {
        panic!("not Fixed")
    };
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    assert_eq!(reader.roots().len(), 2);
    for &root in reader.roots() {
        let text = reader
            .subtree_parts(root)
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>();
        assert_eq!(text.matches('|').count(), 1, "{text}");
    }
    let unicode = native_query(
        b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.mc \\[u263A]\nbody\n.br\n.mc\n",
        78,
    );
    let DocumentBody::Fixed(fixed) = &unicode.document.as_ref().unwrap().body else {
        panic!("not Fixed")
    };
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let body = reader.owner_body_parts(fixed.owners[0].key).unwrap();
    assert_eq!(
        body.iter()
            .map(|part| part.text)
            .collect::<String>()
            .matches('☺')
            .count(),
        1
    );
    let found = mant_query::search_query(
        &unicode,
        &SearchQuery {
            pattern: "☺".into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(found.total, 1);
    assert_eq!(found.matches[0].outline.node.title(), "OPTIONS");
}

#[test]
fn native_margin_is_readable_without_becoming_a_heading_or_definition_name() {
    // Both exact sources ran pinned CVS -Tutf8 -O width=78 first.  In
    // man_term.c::post_TP(HEAD), term_flushln() can end a separate label
    // line; term.c::endline() then appends .mc outside that semantic HEAD.
    let definition = native_query(
        b".TH T 1\n.SH OPTIONS\n.mc |\n.TP 4n\n.B --foo\nbody\n.br\n.mc\n",
        78,
    );
    let DocumentBody::Fixed(fixed) = &definition.document.as_ref().unwrap().body else {
        panic!("not Fixed")
    };
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let owner = &fixed.owners[0];
    let section_text = reader
        .subtree_parts(reader.roots()[0])
        .unwrap()
        .iter()
        .map(|part| part.text)
        .collect::<String>();
    assert_eq!(section_text.matches('|').count(), 2);
    let owner_body = reader.owner_body_parts(owner.key).unwrap();
    assert_eq!(
        owner_body
            .iter()
            .map(|part| part.text)
            .collect::<String>()
            .matches('|')
            .count(),
        1
    );
    assert!(
        owner
            .head
            .parts
            .iter()
            .all(|part| { !fixed.surface.run_text(part.run).unwrap().contains('|') })
    );
    let explained = mant_query::explain_query(
        &definition,
        &ExplanationQuery {
            entry: "--foo".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(explained.total, 1);

    let heading = native_query(b".TH T 1\n.mc |\n.SH OPTIONS\nbody\n.br\n.mc\n", 78);
    let DocumentBody::Fixed(fixed) = &heading.document.as_ref().unwrap().body else {
        panic!("not Fixed")
    };
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let root = reader.roots()[0];
    assert_eq!(reader.label(root).as_deref(), Some("OPTIONS"));
    assert_eq!(
        reader
            .subtree_parts(root)
            .unwrap()
            .iter()
            .map(|part| part.text)
            .collect::<String>()
            .matches('|')
            .count(),
        2
    );

    // The public owned native record can also be constructed without FFI;
    // it must not be able to forge a margin with authored provenance.
    let mut page = AnnotatedRenderer::default()
        .render_bundle(
            "t.1",
            &bundle(b".TH T 1\n.mc |\n.SH OPTIONS\nbody\n.br\n.mc\n"),
            InputFormat::Man,
        )
        .unwrap();
    let margin = page
        .marks
        .iter_mut()
        .find(|mark| mark.kind == 5 && mark.region_kind == 11)
        .unwrap();
    margin.source = 1;
    assert!(lower_annotated_document(page).is_err());
}

#[test]
fn native_margin_handles_overprint_and_a_zero_width_setting() {
    // Both exact sources ran pinned CVS -Tutf8 -O width=78 first.
    // term.c::endline() emits .mc after \\o's real backspaces; \\& emits
    // no glyph and cannot create a phantom margin region or description.
    let overprint = native_query(
        b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.mc |\n\\o'ab'\n.br\n.mc\n",
        78,
    );
    let DocumentBody::Fixed(fixed) = &overprint.document.as_ref().unwrap().body else {
        panic!("not Fixed")
    };
    let reader = mant_ir::FixedSectionReader::new(fixed).unwrap();
    let body = reader.owner_body_parts(fixed.owners[0].key).unwrap();
    assert_eq!(
        body.iter()
            .map(|part| part.text)
            .collect::<String>()
            .matches('|')
            .count(),
        1
    );

    let zero = native_query(
        b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.mc \\&\nbody\n.br\n.mc\n",
        78,
    );
    let DocumentBody::Fixed(fixed) = &zero.document.as_ref().unwrap().body else {
        panic!("not Fixed")
    };
    assert!(
        fixed
            .regions
            .iter()
            .all(|region| region.kind != mant_ir::RegionKind::Margin)
    );
    assert_eq!(visible_total(&zero, "body"), 1);
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
        preceding_owner: 0,
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
    let mut owner = malformed_anchor(1, 0);
    owner.kind = 2;
    owner.flags = 32; // Head evidence requires a definition owner.
    assert!(lower_annotated_document(malformed_marks(vec![owner])).is_err());
    let mut owner = malformed_anchor(1, 0);
    owner.kind = 2;
    owner.flags = 16 | 32 | 64; // One head cannot have two first roles.
    assert!(lower_annotated_document(malformed_marks(vec![owner])).is_err());
    let mut owner = malformed_anchor(1, 0);
    owner.kind = 2;
    owner.flags = 256; // Lexical eligibility still needs a definition owner.
    assert!(lower_annotated_document(malformed_marks(vec![owner])).is_err());
    let mut owner = malformed_anchor(1, 0);
    owner.kind = 2;
    owner.flags = 16 | 256 | 32; // A head cannot claim lexical and Fl roles.
    assert!(lower_annotated_document(malformed_marks(vec![owner])).is_err());
    let mut owner = malformed_anchor(1, 0);
    owner.kind = 2;
    owner.flags = 16; // An operand cannot exist without its native role.
    owner.name = Some("-a".to_owned());
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
    let facts = fixed.owners[0].entry.as_ref().expect("native owner facts");
    assert_eq!(facts.forms, [fixed.owners[0].head.clone()]);
    assert_eq!(facts.names, ["term"]);
    let rebuilt: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert_eq!(
        mant_ir::SemanticIndex::build(&rebuilt).section("d")[0].names,
        ["term"]
    );
    let mut forged_memory = document.clone();
    let DocumentBody::Fixed(forged_fixed) = &mut forged_memory.body else {
        unreachable!("cloned Fixed document changed body kind");
    };
    forged_fixed.owners[0].entry.as_mut().unwrap().names[0] = "unseen".into();
    assert!(
        mant_ir::SemanticIndex::build(&forged_memory)
            .section("d")
            .is_empty()
    );
    assert!(
        DocumentIndex::build(&forged_memory)
            .get(fixed.owners[0].id.as_str())
            .is_none()
    );
    let mut forged = serde_json::to_value(&document).unwrap();
    forged["body"]["owners"][0]["entry"]["names"][0] = serde_json::json!("unseen");
    assert!(serde_json::from_value::<mant_ir::Document>(forged).is_err());
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
fn native_head_roles_promote_only_complete_visible_names() {
    // Exact input first ran pinned CVS -Tutf8 -O width=78. mdoc_macro.c::
    // blk_full constructs each It HEAD; mdoc_term.c::termp_fl_pre adds the
    // visible dash. The collector freezes Fl/Ev/Ic before the AST dies.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a\nbody\n.It Ev DEMO_HOME\nenv body\n.It Ic run\ncommand body\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!("annotated output must use Fixed");
    };
    assert_eq!(fixed.owners.len(), 3);
    assert_eq!(fixed.owners[0].head_role, Some(OwnerHeadRole::Option));
    assert_eq!(fixed.owners[1].head_role, Some(OwnerHeadRole::Environment));
    assert_eq!(fixed.owners[2].head_role, Some(OwnerHeadRole::Literal));
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    assert_eq!(
        fixed.owners[1].entry.as_ref().unwrap().kind,
        EntryKind::EnvironmentVariable
    );
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["-a"]);
    assert_eq!(fixed.owners[1].entry.as_ref().unwrap().names, ["DEMO_HOME"]);
    assert_eq!(
        fixed.owners[2].entry.as_ref().unwrap().kind,
        EntryKind::Command
    );
    assert!(validate_document(&document).is_empty());
}

#[test]
fn mdoc_literal_head_component_binds_only_a_complete_command_word() {
    // Exact fixture first ran pinned CVS -Ttree/-Tutf8. mdoc_macro.c::blk_full
    // gives each It a distinct HEAD/BODY; mdoc_term.c::termp_bold_pre prints
    // Ic/Cm glyphs without changing their owner or adjacent Op/Fl children.
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/annotated-mdoc-command-heads.1"
    ));
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!();
    };
    let by_line = fixed
        .owners
        .iter()
        .map(|owner| (owner.source.unwrap().line, owner))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (line, name) in [(6, "run"), (8, "attach-session"), (16, "new-session")] {
        let owner = by_line[&line];
        assert_eq!(owner.head_role, Some(OwnerHeadRole::Literal));
        let entry = owner.entry.as_ref().unwrap();
        assert_eq!(entry.kind, EntryKind::Command);
        assert_eq!(entry.names, [name]);
        assert_eq!(
            entry.name_bindings[0].occurrences,
            [owner.head_components[0].selection.clone()]
        );
    }
    for line in [10, 12, 14] {
        assert_eq!(by_line[&line].entry.as_ref().unwrap().kind, EntryKind::Term);
    }
    assert!(validate_document(&document).is_empty());
    let mut untrusted = document.clone();
    let DocumentBody::Fixed(untrusted_fixed) = &mut untrusted.body else {
        unreachable!();
    };
    untrusted_fixed.owners[0].head_components[0].source = None;
    assert!(!validate_document(&untrusted).is_empty());
    let mut untrusted = document.clone();
    let DocumentBody::Fixed(untrusted_fixed) = &mut untrusted.body else {
        unreachable!();
    };
    untrusted_fixed.owners[0].head_components[0].selection.parts[0].end_byte = 2;
    assert!(
        untrusted_fixed
            .literal_command_component(&untrusted_fixed.owners[0])
            .is_none()
    );
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    for (name, source_line) in [("run", 6), ("attach-session", 8), ("new-session", 16)] {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.evidence[0].source.unwrap().line, source_line);
        assert_eq!(
            result.evidence[0].entry.as_ref().unwrap().kind,
            EntryKind::Command
        );
        let returned = result.evidence[0].entry.as_ref().unwrap();
        assert_eq!(
            returned.name_bindings[0].occurrences[0].fixed_forms[0]
                .resolve(&returned.fixed_forms)
                .as_deref(),
            Some(name)
        );
        assert!(result.evidence[0].bases.iter().any(|basis| match basis {
            EvidenceBasis::Name { matches } => matches.iter().any(|matched| {
                matched.occurrences[0].fixed_forms[0]
                    .resolve(&returned.fixed_forms)
                    .as_deref()
                    == Some(name)
            }),
            _ => false,
        }));
    }
    let body_only = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "body-only".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(body_only.counts.direct_entry.total, 0);
}

#[test]
fn fixed_mentions_keep_direct_entry_owner_and_plain_section_distinct() {
    // Exact fixture ran pinned CVS -Ttree/-Tutf8. man_macro.c::blk_imp keeps
    // each TP HEAD/BODY separate and SH closes the preceding section scope;
    // term.c::term_flushln consumes only the native-proven text joins.
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/annotated-fixed-mentions.1"
    ));
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let query = |options| {
        mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: "--foo".to_owned(),
                options,
            },
        )
        .unwrap()
    };
    let all = query(ExplanationOptions::default());
    assert_eq!(all.total, 3);
    assert_eq!(all.counts.direct_entry.total, 1);
    assert_eq!(all.counts.entry_mention.total, 1);
    assert_eq!(all.counts.context_mention.total, 1);
    assert_eq!(
        all.evidence
            .iter()
            .map(|item| item.class)
            .collect::<Vec<_>>(),
        [
            EvidenceClass::DirectEntry,
            EvidenceClass::EntryMention,
            EvidenceClass::ContextMention,
        ]
    );
    assert_eq!(all.evidence[0].source.unwrap().line, 3);
    assert_eq!(all.evidence[1].source.unwrap().line, 6);
    assert!(all.evidence[2].source.is_none());
    assert_eq!(all.evidence[2].outline.node.title(), "NOTES");
    for record in &all.evidence {
        assert_eq!(record.fixed_previews.len(), 1);
        let preview = &record.fixed_previews[0];
        assert_eq!(preview.selection.complete_text().as_deref(), Some("--foo"));
        assert_eq!(
            (preview.match_start_scalar, preview.match_end_scalar),
            (0, 5)
        );
        assert!(record.block_path.is_none());
    }
    all.validate_references().unwrap();
    for (offset, class) in [
        (0, EvidenceClass::DirectEntry),
        (1, EvidenceClass::EntryMention),
        (2, EvidenceClass::ContextMention),
    ] {
        let page = query(ExplanationOptions {
            limit: 1,
            offset,
            ..Default::default()
        });
        assert_eq!(page.total, 3);
        assert_eq!(page.evidence[0].class, class);
        page.validate_references().unwrap();
    }
    let small = query(ExplanationOptions {
        content_bytes: 1,
        ..Default::default()
    });
    assert_eq!(small.total, 3);
    assert!(
        small
            .evidence
            .iter()
            .all(|item| item.fixed_previews.is_empty())
    );
    small.validate_references().unwrap();
}

#[test]
fn fixed_mention_preview_clips_zwj_run_with_native_scalar_cell_width() {
    // Exact input first ran pinned CVS -Tutf8. term_ascii.c::utf8_getwidth
    // measures each scalar through mant_mandoc_utf8_width, including ZWJ;
    // grapheme-wide measurement cannot map this clipped native run.
    let query = native_query(".TH T 1\n.SH D\nemoji👩‍👩‍👧‍👧 --foo\n".as_bytes(), 78);
    let result = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "--foo".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.counts.context_mention.total, 1);
    let evidence = &result.evidence[0];
    assert!(!evidence.previews_omitted);
    assert_eq!(evidence.fixed_previews.len(), 1);
    assert_eq!(
        evidence.fixed_previews[0]
            .selection
            .complete_text()
            .as_deref(),
        Some("--foo")
    );
}

#[test]
fn fixed_mentions_preserve_table_literal_owner_and_exclude_margin_ink() {
    // Exact source first ran pinned CVS -Tutf8. man_term.c::pre_TP retains
    // the item body while tbl_term.c emits cells; term.c::term_flushln emits
    // the margin character at line end, not as authored body prose.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --foo\n.TS\ntab(;);\nl l.\nleft;needle\n.TE\n.nf\nneedle literal\n.fi\n.mc |\ntail\n.br\n.mc\n";
    let query = native_query(input, 78);
    let explain = |entry: &str| {
        mant_query::explain_query(
            &query,
            &ExplanationQuery {
                entry: entry.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap()
    };
    let needle = explain("needle");
    assert_eq!(needle.counts.entry_mention.total, 1);
    assert_eq!(needle.counts.context_mention.total, 0);
    assert_eq!(needle.evidence[0].fixed_previews.len(), 2);
    assert!(
        needle.evidence[0]
            .fixed_previews
            .iter()
            .all(|preview| preview.selection.complete_text().as_deref() == Some("needle"))
    );
    let margin = explain("|");
    assert_eq!(margin.total, 0);
}

#[test]
fn fixed_unsectioned_styled_body_is_one_context_mention() {
    // Exact source first ran pinned CVS -Ttree/-Tutf8. term.c::term_word
    // changes font in one text node; neither a section nor an entry is made.
    let query = native_query(b".TH T 1\nalpha\\fBbeta\\fP gamma\n", 78);
    let result = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "alphabeta".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.counts.context_mention.total, 1);
    assert_eq!(result.counts.entry_mention.total, 0);
    assert_eq!(
        result.evidence[0].fixed_previews[0]
            .selection
            .complete_text()
            .as_deref(),
        Some("alphabeta")
    );
}

#[test]
#[allow(clippy::too_many_lines)] // One self-contained mixed-body scope fixture and its pages.
fn scoped_fixed_mentions_rebuild_selected_units_after_flow_document() {
    // The Fixed source is the pinned-CVS-checked TP/SH fixture above. This
    // checks scope page scheduling, not a second roff formatting expectation.
    let fixed_source = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/annotated-fixed-mentions.1"
    ));
    let fixed = native_query(fixed_source, 78);
    let flow = crate::parse_markdown(
        "# Options\n\n<!-- mant:entries role=option case=sensitive -->\n- `--foo`: Flow body.\n",
        None,
    )
    .unwrap();
    let mut documents = vec![
        fixed,
        mant_ir::ResolvedContent {
            address: None,
            label: "flow".to_owned(),
            document: Some(flow.document),
            tldr: None,
        },
    ];
    let sources = ["fixed", "flow"]
        .into_iter()
        .map(|path| ScopedDocument {
            address: DocumentAddress::Markdown {
                path: path.into(),
                origin: MarkdownOrigin::Documents,
            },
            depth: 0,
            root_indices: vec![],
            reached_from: vec![],
        })
        .collect::<Vec<_>>();
    for (source, document) in sources.iter().zip(&mut documents) {
        document.address = Some(source.address.clone());
    }
    let graph = ResolvedDocumentScope {
        reference_limits: Vec::new(),
        query: DocumentScope {
            documents: vec![],
            traversal: mant_protocol::DocumentTraversal::default(),
        },
        documents: sources,
        edges: vec![],
        frontier: vec![],
        unresolved: vec![],
    };
    let input = mant_query::QueryScopeView::new(&graph, &documents).unwrap();
    let response = mant_query::explain_scope(
        input,
        &ExplanationQuery {
            entry: "--foo".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(response.total, 4);
    assert_eq!(
        response
            .evidence
            .iter()
            .map(|record| (record.document_index, record.evidence.class))
            .collect::<Vec<_>>(),
        [
            (0, EvidenceClass::DirectEntry),
            (1, EvidenceClass::DirectEntry),
            (0, EvidenceClass::EntryMention),
            (0, EvidenceClass::ContextMention),
        ]
    );
    for record in response
        .evidence
        .iter()
        .filter(|item| item.document_index == 0)
    {
        assert_eq!(
            record.evidence.fixed_previews[0]
                .selection
                .complete_text()
                .as_deref(),
            Some("--foo")
        );
    }
    response.validate_references().unwrap();
    for offset in 0..response.total {
        let page = mant_query::explain_scope(
            input,
            &ExplanationQuery {
                entry: "--foo".to_owned(),
                options: ExplanationOptions {
                    limit: 1,
                    offset,
                    ..Default::default()
                },
            },
        )
        .unwrap();
        assert_eq!(page.returned, 1);
        let expected = &response.evidence[offset as usize];
        assert_eq!(page.evidence[0].document_index, expected.document_index);
        assert_eq!(page.evidence[0].evidence.class, expected.evidence.class);
        page.validate_references().unwrap();
    }
}

#[test]
fn fixed_xo_wrap_keeps_generated_space_in_full_form_and_visible_match() {
    // Exact fixture first ran pinned CVS -Ttree/-Tutf8. mdoc_macro.c keeps
    // Xo children in one HEAD; term.c::term_word writes AUTO_SPACE and
    // term_flushln consumes the last option's separator at a soft wrap.
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/annotated-mdoc-xo-generated-space.1"
    ));
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let full = "run [-alpha] [-bravo] [-charlie] [-delta] [-echo] [-foxtrot] [-golf] [-hotel]";
    let fixed = match &document.body {
        DocumentBody::Fixed(fixed) => fixed,
        DocumentBody::Flow(_) => unreachable!(),
    };
    let owner = fixed
        .owners
        .iter()
        .find(|owner| owner.entry.is_some())
        .unwrap();
    let forms = &owner.entry.as_ref().unwrap().forms;
    assert_eq!(forms.len(), 1);
    assert_eq!(fixed.selection_text(&forms[0]).as_deref(), Some(full));
    assert!(
        owner
            .head
            .joins
            .iter()
            .any(|join| matches!(join, mant_ir::TextJoin::GeneratedSeparator(text) if text == " "))
    );
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let result = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: full.to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.counts.direct_entry.total, 1);
    let found = mant_query::search_query(
        &resolved,
        &SearchQuery {
            pattern: full.to_owned(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: false,
            context_lines: 0,
            limit: 10,
            offset: 0,
        },
    )
    .unwrap();
    assert_eq!(found.total, 1);
    let hit = &found.matches[0];
    assert_eq!(hit.matched_text, full);
    let mant_protocol::SearchLocation::VisibleFixed {
        unit,
        start_scalar,
        end_scalar,
    } = hit.location
    else {
        panic!("generated-space hit lost its Fixed coordinate");
    };
    assert_eq!(end_scalar - start_scalar, full.chars().count() as u64);
    let projection = found.content_projection.as_ref().unwrap();
    assert_eq!(projection.unit_text(unit).unwrap(), full);
    assert!(projection.units.iter().any(|unit| unit.joins.iter().any(
        |join| matches!(join, mant_protocol::SearchTextJoin::GeneratedSeparator { text } if text == " ")
    )));
    projection.validate_match(hit).unwrap();
}

#[test]
fn fixed_native_nospace_modes_do_not_forge_generated_word_join() {
    // Both exact inputs ran pinned CVS -Tutf8 first. mdoc_term.c sets
    // TERMP_NOSPACE for Ns/Sm off, so term.c::term_word emits no AUTO_SPACE.
    for input in [
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh COMMANDS\n.Bl -tag\n.It Ic foo Ns Ic bar\nbody\n.El\n".as_slice(),
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh COMMANDS\n.Bl -tag\n.Sm off\n.It Ic foo Ic bar\nbody\n.Sm on\n.El\n".as_slice(),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!()
        };
        let head = &fixed.owners[0].head;
        assert_eq!(fixed.selection_text(head).as_deref(), Some("foobar"));
        assert!(!head
            .joins
            .iter()
            .any(|join| matches!(join, mant_ir::TextJoin::GeneratedSeparator(_))));
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".to_owned(),
            document: Some(document),
            tldr: None,
        };
        assert_eq!(visible_total(&resolved, "foo bar"), 0);
    }
}

#[test]
fn native_head_components_index_distinct_mdoc_options_without_guessing_styled_terms() {
    // These exact inputs first ran pinned CVS -Ttree and -Tutf8. In
    // mdoc_macro.c::blk_full, each Fl is a distinct HEAD child; mdoc_term.c::
    // termp_fl_pre contributes its visible dash. Sy is not another Fl.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Fl b\nbody\n.El\n";
    let mut untrusted = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Mdoc)
        .unwrap();
    untrusted
        .marks
        .iter_mut()
        .find(|mark| mark.kind == 6)
        .unwrap()
        .source = 0;
    assert!(lower_annotated_document(untrusted).is_err());
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    let owner = &fixed.owners[0];
    assert_eq!(owner.head_components.len(), 2);
    let entry = owner.entry.as_ref().expect("native Fl declarations");
    assert_eq!(entry.names, ["-a", "-b"]);
    assert_eq!(entry.forms.len(), 2);
    assert!(validate_document(&document).is_empty());
    let index = mant_ir::SemanticIndex::build(&document);
    assert!(
        index
            .section("options")
            .iter()
            .any(|entry| entry.names == ["-a", "-b"])
    );
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let result = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "-b".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.total, 1);
    let details = result.evidence[0].entry.as_ref().unwrap();
    assert_eq!(details.names, ["-a", "-b"]);
    assert_eq!(
        details.fixed_forms[1].complete_text().as_deref(),
        Some("-b")
    );
    result.validate_references().unwrap();

    let styled = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Sy -b\nbody\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(styled), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(fixed.owners[0].head_components.len(), 1);
    assert_ne!(fixed.owners[0].entry.as_ref().unwrap().names, ["-a", "-b"]);
    assert!(validate_document(&document).is_empty());
}

#[test]
fn parameterized_mdoc_head_keeps_each_native_option_name_in_one_form() {
    // The exact source ran pinned CVS -Tutf8 first. mdoc_macro.c::blk_full
    // keeps both Fl nodes and Ar in one HEAD, and mdoc_term.c::termp_fl_pre
    // prints both option names before the styled operand.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Fl b Ar file\nShared description.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    let owner = &fixed.owners[0];
    let entry = owner.entry.as_ref().expect("source-backed Fl names");
    assert_eq!(entry.names, ["-a", "-b"]);
    assert_eq!(entry.forms.as_slice(), std::slice::from_ref(&owner.head));
    assert_eq!(
        fixed.selection_text(&entry.forms[0]).as_deref(),
        Some("-a, -b file")
    );
    assert!(entry.alias_groups.is_empty());
    assert_eq!(entry.name_bindings.len(), 2);
    for (name, binding) in entry.names.iter().zip(&entry.name_bindings) {
        assert_eq!(binding.evidence, mant_ir::EntryNameEvidence::NativeMarkup);
        assert_eq!(binding.occurrences.len(), 1);
        assert_eq!(
            fixed.selection_text(&binding.occurrences[0]).as_deref(),
            Some(name.as_str())
        );
    }
    assert!(validate_document(&document).is_empty());
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    for name in ["-a", "-b"] {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.total, 1, "{name}");
        let details = result.evidence[0].entry.as_ref().unwrap();
        assert_eq!(details.names, ["-a", "-b"]);
        assert_eq!(details.fixed_forms.len(), 1);
        assert_eq!(
            details.fixed_forms[0].complete_text().as_deref(),
            Some("-a, -b file")
        );
        let binding = &details.name_bindings[usize::from(name == "-b")];
        assert_eq!(
            binding.occurrences[0].fixed_forms[0]
                .resolve(&details.fixed_forms)
                .as_deref(),
            Some(name)
        );
        result.validate_references().unwrap();
    }
}

#[test]
fn repeated_mdoc_option_keeps_two_native_occurrences() {
    // Exact input ran pinned CVS -Tutf8 first. mdoc_macro.c::blk_full
    // retains both Fl nodes; mdoc_term.c::termp_fl_pre prints both names.
    let input = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Fl a Ar file\nShared description.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    let entry = fixed.owners[0].entry.as_ref().unwrap();
    assert_eq!(entry.names, ["-a"]);
    assert_eq!(entry.forms, [fixed.owners[0].head.clone()]);
    assert_eq!(entry.name_bindings[0].occurrences.len(), 2);
    assert!(
        entry.name_bindings[0]
            .occurrences
            .iter()
            .all(|selection| fixed.selection_text(selection).as_deref() == Some("-a"))
    );
    assert!(validate_document(&document).is_empty());
    let result = mant_query::explain_query(
        &mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        },
        &ExplanationQuery {
            entry: "-a".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(
        result.evidence[0].entry.as_ref().unwrap().name_bindings[0]
            .occurrences
            .len(),
        2
    );
    result.validate_references().unwrap();
}

#[test]
fn repeated_fixed_name_occurrences_obey_response_cap() {
    // Exact generated TP/B input ran pinned CVS -Tutf8 first. One native
    // HEAD prints all 33 occurrences; response policy retains at most 32.
    let label = std::iter::repeat_n("-a", 33).collect::<Vec<_>>().join(", ");
    let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nbody\n");
    let document =
        project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    let entry = fixed.owners[0].entry.as_ref().unwrap();
    assert_eq!(entry.names, ["-a"]);
    assert_eq!(entry.name_bindings[0].occurrences.len(), 33);
    let result = mant_query::explain_query(
        &mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        },
        &ExplanationQuery {
            entry: "-a".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let evidence = &result.evidence[0];
    assert!(evidence.name_bindings_omitted);
    assert!(evidence.match_details_omitted);
    assert_eq!(
        evidence.entry.as_ref().unwrap().name_bindings[0]
            .occurrences
            .len(),
        32
    );
    result.validate_references().unwrap();
}

#[test]
fn many_native_head_components_keep_bounded_explanation_bindings() {
    use std::fmt::Write as _;
    // This generated exact 33-Fl line ran pinned CVS -Tutf8 width=78 first. Each Fl
    // remains a separate mdoc_macro.c::blk_full HEAD child across soft wraps.
    let mut head = String::new();
    for number in 1..=33 {
        write!(&mut head, " Fl a{number} ,").unwrap();
    }
    let input = format!(
        ".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It{head}\nbody\n.El\n"
    );
    let document =
        project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names.len(), 33);
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let result = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "-a33".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    let evidence = &result.evidence[0];
    assert!(evidence.name_bindings_omitted);
    assert!(evidence.match_details_omitted);
    assert_eq!(evidence.entry.as_ref().unwrap().name_bindings.len(), 32);
    result.validate_references().unwrap();
}

#[test]
fn complete_man_tp_option_uses_shared_lexical_rule_without_promoting_other_terms() {
    // This exact input first ran pinned CVS -Tutf8. man_macro.c::blk_imp
    // creates distinct TP HEAD/BODY scopes; man_term.c::pre_TP/post_TP
    // prints the full head before the body, using term.c::term_word/flushln.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --save\nbody\n.TP\n.B {+\nbody\n.TP\n.B FILE\nbody\n.TP\n.B -1\nnumber body\n.TP\n.B --save=FILE\nassignment body\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!("annotated output must use Fixed");
    };
    assert_eq!(fixed.owners.len(), 5);
    assert!(
        fixed
            .owners
            .iter()
            .all(|owner| owner.head_role == Some(OwnerHeadRole::Lexical))
    );
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["--save"]);
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().forms,
        [fixed.owners[0].head.clone()]
    );
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    for owner in &fixed.owners[1..4] {
        assert_eq!(owner.entry.as_ref().unwrap().kind, EntryKind::Term);
    }
    assert_eq!(fixed.owners[4].entry.as_ref().unwrap().names, ["--save"]);
    assert_eq!(
        fixed.owners[4].entry.as_ref().unwrap().kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    assert!(validate_document(&document).is_empty());
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let result = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "--save".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.total, 2);
    assert!(
        result
            .evidence
            .iter()
            .all(|evidence| evidence.entry.as_ref().unwrap().names == ["--save"])
    );
    result.validate_references().unwrap();
}

#[test]
fn one_native_man_head_keeps_alias_names_bound_to_one_complete_form() {
    // Each exact input first ran pinned CVS -Ttree. man_macro.c::blk_imp
    // retains one TP HEAD and one BODY; man_term.c::pre_B/term.c::term_word
    // emit the single literal operand without creating separate owners.
    for (label, names) in [
        ("-a, --all", ["-a", "--all"]),
        ("-a --all", ["-a", "--all"]),
        ("-q or --quiet", ["-q", "--quiet"]),
    ] {
        let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nBODY\n");
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!();
        };
        let owner = &fixed.owners[0];
        let facts = owner.entry.as_ref().unwrap();
        assert_eq!(
            facts.kind,
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option
            }
        );
        assert_eq!(facts.names, names);
        assert_eq!(facts.forms.as_slice(), std::slice::from_ref(&owner.head));
        assert_eq!(
            fixed.selection_text(&facts.forms[0]).as_deref(),
            Some(label)
        );
        for (binding, name) in facts.name_bindings.iter().zip(&facts.names) {
            assert_eq!(
                fixed.selection_text(&binding.occurrences[0]).as_deref(),
                Some(name.as_str())
            );
        }
        assert!(validate_document(&document).is_empty());
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".to_owned(),
            document: Some(document),
            tldr: None,
        };
        for requested in names {
            let result = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: requested.to_owned(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.total, 1, "{label}: {requested}");
            let entry = result.evidence[0].entry.as_ref().unwrap();
            assert_eq!(entry.fixed_forms.len(), 1);
            assert_eq!(entry.fixed_forms[0].complete_text().as_deref(), Some(label));
            let binding = &entry.name_bindings[usize::from(requested == names[1])];
            assert_eq!(
                binding.occurrences[0].fixed_forms[0]
                    .resolve(&entry.fixed_forms)
                    .as_deref(),
                Some(requested)
            );
            result.validate_references().unwrap();
        }
    }

    // The exact heads also ran pinned CVS. Their visible commas do not prove
    // another name, but the leading option remains a valid declaration.
    for (label, name) in [("--set=KEY,VALUE", "--set"), ("-a, text", "-a")] {
        let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nBODY\n");
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!();
        };
        let entry = fixed.owners[0].entry.as_ref().unwrap();
        assert_eq!(
            entry.kind,
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option
            }
        );
        assert_eq!(entry.names, [name]);
        assert_eq!(entry.forms, [fixed.owners[0].head.clone()]);
        assert!(validate_document(&document).is_empty());
    }
}

#[test]
fn man_bold_heads_bind_only_checked_option_names() {
    // Each exact input ran pinned CVS -Ttree/-Tascii before these assertions.
    // man_macro.c::blk_imp keeps one TP/TQ HEAD; man_term.c::pre_B and
    // term.c::term_word print each \- as a hyphen in that same HEAD.
    for (label, names, prefix) in [
        (r"\-Y, \-\-yay", vec!["-Y", "--yay"], None),
        (r"\-p|\-\-parents", vec!["-p", "--parents"], None),
        (r"\-a \-\-ascii", vec!["-a", "--ascii"], None),
        (
            r"\-c \-\-stdout \-\-to-stdout",
            vec!["-c", "--stdout", "--to-stdout"],
            None,
        ),
        (
            r"\-\-builddir <dir>",
            vec!["--builddir"],
            Some("--builddir"),
        ),
        ("--builddir <dir>", vec!["--builddir"], Some("--builddir")),
        (r"\-p", vec!["-p"], None),
    ] {
        let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nbody\n");
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!();
        };
        let owner = &fixed.owners[0];
        assert_eq!(owner.head_role, Some(OwnerHeadRole::Lexical), "{label}");
        assert_eq!(owner.head_role_prefix.as_deref(), prefix, "{label}");
        let facts = owner.entry.as_ref().unwrap();
        assert_eq!(facts.names, names, "{label}");
        assert_eq!(
            facts.kind,
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option
            },
            "{label}"
        );
        assert_eq!(facts.forms.as_slice(), std::slice::from_ref(&owner.head));
        for (name, binding) in facts.names.iter().zip(&facts.name_bindings) {
            assert_eq!(
                fixed.selection_text(&binding.occurrences[0]).as_deref(),
                Some(name.as_str()),
                "{label}"
            );
        }
        assert!(validate_document(&document).is_empty());
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".to_owned(),
            document: Some(document),
            tldr: None,
        };
        for name in names {
            let result = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: name.to_owned(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, 1, "{label}: {name}");
            result.validate_references().unwrap();
        }
    }
}

#[test]
fn escaped_dash_tq_keeps_its_own_checked_option_name() {
    // This exact input ran pinned CVS -Ttree. man_macro.c::blk_imp keeps the
    // TQ HEAD distinct; man_term.c::pre_TP traverses its sole B operand.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-a\nbody\n.TQ\n.B \\-\\-all\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!();
    };
    assert_eq!(fixed.owners.len(), 2);
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["-a"]);
    assert_eq!(fixed.owners[1].entry.as_ref().unwrap().names, ["--all"]);
    assert!(validate_document(&document).is_empty());
}

#[test]
fn escaped_dash_man_hint_rejects_unproved_escapes_and_names() {
    // Each exact input ran pinned CVS -Ttree/-Tascii first.  term.c::
    // term_word() changes font for \fR and prints \[hy] as a different
    // special character; neither is the literal \- option-head proof.
    for (label, role, kind) in [
        (r"\-B\fRn", None, EntryKind::Term),
        (r"\[hy]x", None, EntryKind::Term),
        (r"\-x\&foo", None, EntryKind::Term),
        (r"\-1", Some(OwnerHeadRole::Lexical), EntryKind::Term),
        (r"\-x.", Some(OwnerHeadRole::Lexical), EntryKind::Term),
        (
            r"\-a, text",
            Some(OwnerHeadRole::Lexical),
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
        ),
    ] {
        let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nbody\n");
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!();
        };
        let owner = &fixed.owners[0];
        assert_eq!(owner.head_role, role, "{label}");
        assert_eq!(owner.entry.as_ref().unwrap().kind, kind, "{label}");
        assert!(validate_document(&document).is_empty());
    }
}

#[test]
fn literal_alias_binding_cap_and_page_keep_one_owner() {
    // This exact generated TP/B input first ran pinned CVS -Ttree. Its 33
    // option spellings remain one literal HEAD and one BODY even when the
    // final terminal head wraps over multiple physical rows.
    let label = (0..33)
        .map(|index| format!("-o{index}"))
        .collect::<Vec<_>>()
        .join(", ");
    let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nBODY\n");
    let resolved = native_query(input.as_bytes(), 78);
    let query = |options| {
        mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: "-o32".to_owned(),
                options,
            },
        )
        .unwrap()
    };
    let first = query(ExplanationOptions {
        limit: 1,
        ..ExplanationOptions::default()
    });
    assert_eq!(first.total, 1);
    assert_eq!(first.evidence.len(), 1);
    assert_eq!(first.next_offset, None);
    assert!(first.evidence[0].name_bindings_omitted);
    assert!(first.evidence[0].match_details_omitted);
    assert_eq!(first.evidence[0].entry.as_ref().unwrap().names.len(), 33);
    let retained = &first.evidence[0].entry.as_ref().unwrap().name_bindings;
    assert_eq!(retained.len(), 32);
    assert_eq!(retained[31].name_index, 31);
    first.validate_references().unwrap();

    let second = query(ExplanationOptions {
        limit: 1,
        offset: 1,
        ..ExplanationOptions::default()
    });
    assert_eq!(second.total, 1);
    assert!(second.evidence.is_empty());
    assert_eq!(second.next_offset, None);
    second.validate_references().unwrap();

    let limited = query(ExplanationOptions {
        content_bytes: 128,
        ..ExplanationOptions::default()
    });
    assert_eq!(limited.total, 1);
    assert_eq!(limited.evidence.len(), 1);
    assert!(limited.evidence[0].details_omitted || limited.evidence[0].content_omitted);
    limited.validate_references().unwrap();
}

#[test]
fn authored_man_ip_bold_prefix_binds_options_without_promoting_other_labels() {
    // This exact input first ran pinned CVS -Ttree and -Tutf8.  man_macro.c::
    // blk_imp retains each IP HEAD/BODY; man_term.c::pre_IP prints only its
    // first HEAD argument and uses the second for width. term.c::term_word
    // renders \- as '-' and font changes without a visible glyph.
    let input = b".TH T 1\n.SH OPTIONS\n.IP \"\\fB\\-x\\fR \\fIlanguage\\fR\" 4\nLANGUAGE_BODY\n.IP \"\\fB\\-x none\\fR\" 4\nNONE_BODY\n.IP \"\\fB\\-\\-help\\fR\" 4\nHELP_BODY\n.IP \"\\fB\\-Wformat=2\\fR\" 4\nFORMAT_BODY\n.IP \"\\fB\\-x\\fRfoo\" 4\nGLUED_BODY\n.IP \"\\fI\\-x\\fR\" 4\nITALIC_BODY\n.IP \\(bu 4\nBULLET_BODY\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!();
    };
    assert_eq!(fixed.owners.len(), 7);
    for (owner, name) in fixed.owners[..4]
        .iter()
        .zip(["-x", "-x", "--help", "-Wformat"])
    {
        assert_eq!(owner.role, OwnerRole::Definition);
        assert_eq!(owner.head_role, Some(OwnerHeadRole::Lexical));
        assert_eq!(owner.head_role_prefix.as_deref(), Some(name));
        let facts = owner.entry.as_ref().unwrap();
        assert_eq!(facts.names, [name]);
        assert_eq!(
            fixed
                .selection_text(&facts.name_bindings[0].occurrences[0])
                .as_deref(),
            Some(name)
        );
    }
    for owner in &fixed.owners[4..] {
        assert_eq!(owner.role, OwnerRole::Other);
        assert!(owner.entry.is_none());
    }
    assert!(validate_document(&document).is_empty());
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    for (requested, expected_sources) in [
        ("-x", vec![3, 5]),
        ("--help", vec![7]),
        ("-Wformat", vec![9]),
        ("-xfoo", vec![]),
    ] {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: requested.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.total as usize, expected_sources.len(), "{requested}");
        assert_eq!(
            result
                .evidence
                .iter()
                .map(|item| item.source.unwrap().line)
                .collect::<Vec<_>>(),
            expected_sources
        );
        result.validate_references().unwrap();
    }

    // This exact standalone input first ran pinned CVS -Ttree.  Its bold
    // terminal punctuation is visible but not a complete option spelling;
    // the native candidate must not fall back into an invented Term entry.
    let punctuated = b".TH T 1\n.SH OPTIONS\n.IP \"\\fB\\-x.\\fR\" 4\nBODY\n";
    let document = project_annotated_manual("t.1", &bundle(punctuated), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!();
    };
    assert_eq!(fixed.owners[0].head_role_prefix.as_deref(), Some("-x."));
    assert!(fixed.owners[0].entry.is_none());
    assert!(validate_document(&document).is_empty());
}

#[test]
fn man_ip_equivalent_bold_aliases_use_one_checked_display_grammar() {
    // Each exact input ran pinned CVS -Tutf8 first. man_term.c::pre_IP prints
    // the first HEAD operand and term.c::term_word changes font without a
    // glyph. The C bold role is a candidate; visible spelling and exact
    // sub-selections decide names, irrespective of equivalent font syntax.
    for (label, input, names) in [
        (
            "short-font",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a, --all\\fR\" 4\nShared description.\n".as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "bracket-font",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\f[B]-a, --all\\f[R]\" 4\nShared description.\n"
                .as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "split-font",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fR, \\fB--all\\fR\" 4\nShared description.\n"
                .as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "value-and-alias",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a, --all=FILE\\fR\" 4\nShared description.\n"
                .as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "constant-width-bold",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\f[CB]-a, --all\\f[CR]\" 4\nBody.\n".as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "previous-font",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fP, \\fB--all\\fP\" 4\nShared description.\n"
                .as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "styled-or",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fR or \\fB--all\\fR\" 4\nShared description.\n"
                .as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "plain-or",
            b".TH T 1\n.SH OPTIONS\n.IP \"-a or --all\" 4\nShared description.\n".as_slice(),
            vec!["-a", "--all"],
        ),
        (
            "bold-space-pair",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fR \\fB--all\\fR\" 4\nBody.\n".as_slice(),
            vec!["-a", "--all"],
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        assert!(validate_document(&document).is_empty(), "{label}");
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        let entry = fixed.owners[0].entry.as_ref().expect("option entry");
        assert_eq!(entry.names, names, "{label}");
        assert!(entry.alias_groups.is_empty(), "{label}");
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for name in names {
            let response = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(response.counts.direct_entry.total, 1, "{label}: {name}");
            response.validate_references().unwrap();
        }
    }
}

#[test]
fn man_ip_styled_argument_and_list_labels_do_not_become_option_aliases() {
    // Each exact input ran pinned CVS -Tutf8 first. The same final-display
    // grammar must not infer names across a glued or underlined operand.
    for (label, input) in [
        (
            "glued",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fRfoo\" 4\nBody.\n".as_slice(),
        ),
        (
            "italic",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fI-a, --all\\fR\" 4\nBody.\n".as_slice(),
        ),
        (
            "bullet",
            b".TH T 1\n.SH OPTIONS\n.IP \\(bu 4\nBody.\n".as_slice(),
        ),
        (
            "constant-width-glued",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\f[CB]-a\\f[CR]foo\" 4\nBody.\n".as_slice(),
        ),
        (
            "previous-font-glued",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fPfoo\" 4\nBody.\n".as_slice(),
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        assert!(fixed.owners[0].entry.is_none(), "{label}");
    }

    for (label, input) in [
        (
            "italic-second-or",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fR or \\fI--all\\fR\" 4\nBody.\n".as_slice(),
        ),
        (
            "italic-second-argument",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fR \\fI--all\\fR\" 4\nBody.\n".as_slice(),
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed: {label}")
        };
        assert_eq!(
            fixed.owners[0].entry.as_ref().unwrap().names,
            ["-a"],
            "{label}"
        );
        assert!(validate_document(&document).is_empty(), "{label}");
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for (name, expected) in [("-a", 1), ("--all", 0)] {
            let response = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(
                response.counts.direct_entry.total, expected,
                "{label}: {name}"
            );
            response.validate_references().unwrap();
        }
    }
}

#[test]
fn man_ip_reading_groups_require_native_siblings_and_empty_prior_body() {
    // This exact fixture first ran pinned CVS -Ttree and -Tutf8.  man_macro.c::
    // blk_imp keeps distinct IP HEAD/BODY owners; .PD remains in the prior
    // BODY, while a deleted .PP changes roff.c's flow_epoch and bars a group.
    let input = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/roff/annotated-man-ip-reading-groups.1"
    ));
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!();
    };
    let by_line = fixed
        .owners
        .iter()
        .map(|owner| (owner.source.unwrap().line, owner))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(by_line[&3].preceding_owner, Some(by_line[&2].key));
    assert_eq!(by_line[&8].preceding_owner, Some(by_line[&6].key));
    assert_eq!(by_line[&13].preceding_owner, None);
    assert_eq!(by_line[&17].preceding_owner, Some(by_line[&15].key));
    assert_eq!(by_line[&22].preceding_owner, None);
    let index = mant_ir::SemanticIndex::build(&document);
    for (member, expected) in [(2, vec![2, 3]), (6, vec![6, 8])] {
        let group = index.fixed_reading_group(by_line[&member].key).unwrap();
        assert_eq!(
            group
                .members
                .iter()
                .map(|key| fixed.owners[(key.get() - 1) as usize].source.unwrap().line)
                .collect::<Vec<_>>(),
            expected
        );
    }
    for line in [11, 13, 15, 17, 20, 22] {
        assert!(index.fixed_reading_group(by_line[&line].key).is_none());
    }
    assert!(validate_document(&document).is_empty());
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    for (name, expected_heads, expected_body) in [
        ("-root", ["-root", "-root-long"], "ROOT_BODY"),
        ("-a", ["-a", "--all"], "SHARED_BODY"),
    ] {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        result.validate_references().unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.evidence[0].support, Some(0));
        let mant_protocol::ExplanationSupport::FixedDeclarationGroup {
            members,
            reading_body,
        } = &result.supports[0]
        else {
            unreachable!();
        };
        assert_eq!(
            members
                .iter()
                .map(|member| member.head.complete_text().unwrap())
                .collect::<Vec<_>>(),
            expected_heads
        );
        assert!(
            reading_body
                .parts
                .iter()
                .any(|part| part.text.contains(expected_body))
        );
    }
    let blocked = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "-b".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    blocked.validate_references().unwrap();
    assert_eq!(blocked.total, 1);
    assert!(blocked.supports.is_empty());
    assert!(blocked.evidence[0].support.is_none());
}

#[test]
fn man_tp_tq_reading_groups_preserve_distinct_heads_around_pd() {
    // Both exact inputs first ran pinned CVS -Ttree. man_macro.c::blk_imp
    // gives each TP/TQ its own HEAD/BODY; man_term.c::pre_TP executes a PD
    // preceding B inside HEAD without printing a label glyph. The later
    // owner's body is reading context, not body owned by the earlier head.
    for (input, requested, expected_heads, expected_lines) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.PD 0\n.B -A\n.TP\n.PD\n.B --adjust-sfx\nBODY\n"
                .as_slice(),
            "-A",
            ["-A", "--adjust-sfx"],
            [3, 6],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B -a\n.TQ\n.B --all\nBODY\n".as_slice(),
            "-a",
            ["-a", "--all"],
            [3, 5],
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!()
        };
        assert_eq!(fixed.owners.len(), 2);
        assert_eq!(fixed.owners[0].source.unwrap().line, expected_lines[0]);
        assert_eq!(fixed.owners[1].source.unwrap().line, expected_lines[1]);
        assert_eq!(fixed.owners[0].head_role, Some(OwnerHeadRole::Lexical));
        assert_eq!(fixed.owners[1].head_role, Some(OwnerHeadRole::Lexical));
        assert_eq!(fixed.owners[0].preceding_owner, None);
        assert_eq!(fixed.owners[1].preceding_owner, Some(fixed.owners[0].key));
        for (owner, expected) in fixed.owners.iter().zip(expected_heads) {
            assert_eq!(fixed.selection_text(&owner.head).as_deref(), Some(expected));
            assert_eq!(owner.entry.as_ref().unwrap().names, [expected]);
        }
        let index = mant_ir::SemanticIndex::build(&document);
        let group = index.fixed_reading_group(fixed.owners[0].key).unwrap();
        assert_eq!(group.members, [fixed.owners[0].key, fixed.owners[1].key]);
        assert!(validate_document(&document).is_empty());
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".to_owned(),
            document: Some(document),
            tldr: None,
        };
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: requested.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        result.validate_references().unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.evidence[0].support, Some(0));
        let mant_protocol::ExplanationSupport::FixedDeclarationGroup {
            members,
            reading_body,
        } = &result.supports[0]
        else {
            unreachable!()
        };
        assert_eq!(members.len(), 2);
        assert_eq!(
            members[0].head.complete_text().as_deref(),
            Some(expected_heads[0])
        );
        assert_eq!(
            members[1].head.complete_text().as_deref(),
            Some(expected_heads[1])
        );
        assert!(
            reading_body
                .parts
                .iter()
                .any(|part| part.text.contains("BODY"))
        );
    }
}

#[test]
fn man_tp_reading_context_stops_at_body_flow_and_nonlexical_heads() {
    // Every exact input first ran pinned CVS -Ttree. roff.c stamps executed
    // paragraph boundaries even if validation removes .PP; man_term.c still
    // prints an italic HEAD but it cannot certify a lexical option candidate.
    for (input, expected_predecessor) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B -a\nOWN\n.TP\n.B --all\nBODY\n".as_slice(),
            true,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B -a\n.PP\n.TP\n.B --all\nBODY\n".as_slice(),
            false,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.I -a\n.TP\n.B --all\nBODY\n".as_slice(),
            false,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB\\-a\\fR\" 4\n.TP\n.B --all\nBODY\n".as_slice(),
            false,
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!()
        };
        assert_eq!(fixed.owners.len(), 2);
        assert_eq!(
            fixed.owners[1].preceding_owner,
            expected_predecessor.then_some(fixed.owners[0].key)
        );
        assert!(validate_document(&document).is_empty());
        let index = mant_ir::SemanticIndex::build(&document);
        assert!(index.fixed_reading_group(fixed.owners[0].key).is_none());
        assert!(index.fixed_reading_group(fixed.owners[1].key).is_none());
    }
}

#[test]
fn man_tp_styled_operands_and_punctuation_keep_native_head_classification() {
    // Each exact input first ran pinned CVS -Ttree. term.c::term_word()
    // executes a font escape after the authored space, so the first option
    // remains a complete prefix; punctuation instead leaves alias splitting
    // to the source-neutral grammar over the full final HEAD.
    let styled = b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-x \\fIlang\nBODY\n";
    let document = project_annotated_manual("t.1", &bundle(styled), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    let owner = &fixed.owners[0];
    assert_eq!(owner.head_role_prefix.as_deref(), Some("-x"));
    assert_eq!(owner.entry.as_ref().unwrap().names, ["-x"]);
    assert_eq!(
        fixed.selection_text(&owner.head).as_deref(),
        Some("-x lang")
    );
    assert!(validate_document(&document).is_empty());

    for (input, expected_names) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-a , \\-\\-all\nBODY\n".as_slice(),
            Some(vec!["-a", "--all"]),
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-a | \\-\\-all\nBODY\n".as_slice(),
            Some(vec!["-a", "--all"]),
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-a / \\-\\-all\nBODY\n".as_slice(),
            Some(vec!["-a"]),
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-a , text\nBODY\n".as_slice(),
            Some(vec!["-a"]),
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!()
        };
        let owner = &fixed.owners[0];
        assert_eq!(owner.head_role_prefix, None);
        assert_eq!(owner.entry.as_ref().unwrap().names, expected_names.unwrap());
        assert!(validate_document(&document).is_empty());
    }
}

#[test]
fn emphasized_native_heads_do_not_gain_lexical_option_eligibility() {
    // Both exact inputs first ran pinned CVS -Tutf8. man_term.c::pre_I and
    // mdoc_term.c::termp_under_pre select underline; those presentation
    // choices are not the conservative sole-B hint on a man TP/TQ head.
    let man = b".TH T 1\n.SH OPTIONS\n.TP\n.B --save\nbody\n.TP\n.I --save\nitalic body\n";
    let document = project_annotated_manual("t.1", &bundle(man), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(fixed.owners[0].head_role, Some(OwnerHeadRole::Lexical));
    assert_eq!(fixed.owners[1].head_role, None);
    assert_eq!(
        fixed.owners[1].entry.as_ref().unwrap().kind,
        EntryKind::Term
    );
    assert!(validate_document(&document).is_empty());

    // This exact input also ran pinned CVS. term.c::term_word executes the
    // embedded font escape after man_term.c::pre_B, so the final head is
    // underlined rather than a plain bold declaration candidate.
    let escaped = b".TH T 1\n.SH OPTIONS\n.TP\n.B \\fI--save\\fP\nbody\n";
    let document = project_annotated_manual("t.1", &bundle(escaped), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(fixed.owners[0].head_role, None);
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().kind,
        EntryKind::Term
    );
    assert!(validate_document(&document).is_empty());

    let mdoc = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Ar -a\narg body\n.It Em --save\nem body\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(mdoc), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    for owner in &fixed.owners {
        assert_eq!(owner.head_role, None);
        assert_eq!(owner.entry.as_ref().unwrap().kind, EntryKind::Term);
    }
    assert!(validate_document(&document).is_empty());
}

#[test]
fn native_partial_head_names_resolve_to_display_and_explain_ranges() {
    // The exact input also ran pinned CVS. A styled argument and an
    // assignment retain full forms while binding only their proven names.
    let partial = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a Ar VALUE\nbody\n.It Ev DEMO_HOME=foo\nenv body\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(partial), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!("annotated output must use Fixed");
    };
    assert_eq!(fixed.owners[0].head_role, Some(OwnerHeadRole::Option));
    assert_eq!(fixed.owners[1].head_role, Some(OwnerHeadRole::Environment));
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    assert_eq!(
        fixed.owners[1].entry.as_ref().unwrap().kind,
        EntryKind::EnvironmentVariable
    );
    for (owner, name) in fixed.owners.iter().zip(["-a", "DEMO_HOME"]) {
        let entry = owner.entry.as_ref().unwrap();
        assert_eq!(entry.names, [name]);
        assert_eq!(
            fixed.selection_text(&entry.name_bindings[0].occurrences[0]),
            Some(name.to_owned())
        );
        assert_ne!(fixed.selection_text(&owner.head).as_deref(), Some(name));
    }
    assert!(validate_document(&document).is_empty());
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    for name in ["-a", "DEMO_HOME"] {
        let explanation = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(explanation.total, 1);
        let entry = explanation.evidence[0].entry.as_ref().unwrap();
        let name_range = &entry.name_bindings[0].occurrences[0].fixed_forms[0];
        assert_eq!(
            name_range.resolve(&entry.fixed_forms).as_deref(),
            Some(name)
        );
        let form = entry.fixed_forms[0].complete_text().unwrap();
        assert_ne!(form, name);
        let by_form = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: form.clone(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        let evidence = &by_form.evidence[0];
        let returned = evidence.entry.as_ref().unwrap();
        assert!(evidence.bases.iter().any(|basis| match basis {
            EvidenceBasis::Form { matches } => matches.iter().any(|matched| {
                matched.occurrences[0].fixed_forms[0]
                    .resolve(&returned.fixed_forms)
                    .as_deref()
                    == Some(form.as_str())
            }),
            _ => false,
        }));
    }

    // The exact input ran pinned CVS. mdoc_term.c::termp_ns_pre removes the
    // inter-macro blank, but the Ar glyphs do not become part of Fl's name.
    let glued = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a Ns Ar VALUE\nbody\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(glued), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!("annotated output must use Fixed");
    };
    assert_eq!(fixed.owners[0].head_role_prefix.as_deref(), Some("-a"));
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["-a"]);
    assert_eq!(
        fixed.selection_text(
            &fixed.owners[0].entry.as_ref().unwrap().name_bindings[0].occurrences[0]
        ),
        Some("-a".to_owned())
    );
}

#[test]
fn native_head_role_respects_visible_prefix_and_punctuation() {
    // This exact input ran pinned CVS too. Later Fl markup cannot
    // override an earlier visible word in the same It HEAD.
    let later = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It prefix Fl a\nbody\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(later), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!("annotated output must use Fixed");
    };
    assert_eq!(fixed.owners[0].head_role, None);

    // Both exact inputs first ran pinned CVS. term.c::term_word emits no
    // glyph for ESCAPE_IGNORE or font changes, so neither can hide the first
    // visible Fl declaration from native role capture.
    for leading in [
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It \\& Fl a\nbody\n.El\n"
            .as_slice(),
        b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It \\fB Fl a\nbody\n.El\n"
            .as_slice(),
    ] {
        let document =
            project_annotated_manual("t.1", &bundle(leading), InputFormat::Mdoc).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!("annotated output must use Fixed");
        };
        assert_eq!(fixed.owners[0].head_role, Some(OwnerHeadRole::Option));
        // The native formatter can retain a leading head space; it is not
        // included in the checked name binding.
        assert_eq!(
            fixed.owners[0].entry.as_ref().unwrap().kind,
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option
            }
        );
        assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["-a"]);
    }

    // mdoc_term.c::termp_fl_pre supplies the generated dash; the shared Fl
    // grammar admits complete two-character punctuation options too.
    let punctuation = b".Dd September 24, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl ,\nbody\n.It Fl -\nsecond\n.El\n";
    let document =
        project_annotated_manual("t.1", &bundle(punctuation), InputFormat::Mdoc).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!("annotated output must use Fixed");
    };
    for (owner, spelling) in fixed.owners.iter().zip(["-,", "--"]) {
        assert_eq!(owner.head_role, Some(OwnerHeadRole::Option));
        assert_eq!(owner.entry.as_ref().unwrap().names, [spelling]);
        assert_eq!(
            owner.entry.as_ref().unwrap().kind,
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option
            },
            "{spelling}: native role prefix: {:?}",
            owner.head_role_prefix
        );
    }
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
