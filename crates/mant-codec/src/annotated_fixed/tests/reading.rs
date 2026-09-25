use super::*;

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
    // man_term.c::pre_PP/pre_alternate retains the head and prints BI
    // operands without inserted space. term.c::term_word closes the italic
    // metavariable before the next comma-separated bold declaration.
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
        (
            "italic-template-and-parameter",
            b".TH T 1\n.SH OPTIONS\n.PP\n\\fI-<number>\\fR, \\fB-n\\fR \\fI<number>\\fR, \\fB--max-count\\fR=\\fI<number>\\fR\n.RS 4\nCOUNT_BODY\n.RE\n"
                .as_slice(),
            vec!["-n", "--max-count"],
            "COUNT_BODY",
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
fn retained_space_hanging_declarations_bind_each_direct_owner() {
    // This exact input ran pinned CVS -Ttree/-Tutf8 first. roff.c::
    // roff_node_alloc() stamps the retained .sp and its following TEXT with
    // one epoch; man_macro.c::blk_exp() creates the direct RS continuation.
    // Native paragraph discovery, the shared full-head name scan, and explain
    // must agree without borrowing either previous owner's description.
    let input = b".TH T 1\n.SH OPTIONS\n\\fB\\-a\\fP\n.RS 4\nFirst description.\n.RE\n.sp\n\\fB\\-g\\fP \\fIGLOB\\fP, \\fB\\-\\-glob\\fP=\\fIGLOB\\fP\n.RS 4\nGlob description.\n.RE\n";
    let resolved = native_query(input, 78);
    let document = resolved.document.as_ref().unwrap();
    assert!(validate_document(document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let entries = fixed
        .owners
        .iter()
        .filter_map(|owner| owner.entry.as_ref().map(|entry| entry.names.as_slice()))
        .collect::<Vec<_>>();
    assert_eq!(entries, [&["-a"][..], &["-g", "--glob"][..]]);
    for (name, expected_body, excluded_body) in [
        ("-a", "First description.", "Glob description."),
        ("-g", "Glob description.", "First description."),
        ("--glob", "Glob description.", "First description."),
    ] {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, 1, "{name}");
        let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
            result.evidence[0].content.as_ref().unwrap()
        else {
            panic!("not Fixed owner: {name}")
        };
        let body = reading_body
            .parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>();
        assert!(body.contains(expected_body), "{name}: {body}");
        assert!(!body.contains(excluded_body), "{name}: {body}");
        result.validate_references().unwrap();
    }
    assert_eq!(visible_total(&resolved, "--glob"), 1);
}

#[test]
fn retained_space_hanging_ignores_zero_width_origin() {
    // Each exact input ran pinned CVS -Ttree/-Tutf8 first. term.c::term_word()
    // emits no glyph for \&; roff.c nevertheless keeps its TEXT or B node.
    // The first visible declaration, not the zero-width instance, supplies
    // the authored source for the direct RS continuation.
    for (input, source_line) in [
        (
            b".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n\\&\n.B --foo\n.RS 4\nBody.\n.RE\n".as_slice(),
            6,
        ),
        (
            b".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n.B \\&\n--foo\n.RS 4\nBody.\n.RE\n".as_slice(),
            6,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n\\&\n.B --foo\n.RS 4\nBody.\n.RE\n".as_slice(),
            4,
        ),
    ] {
        let resolved = native_query(input, 78);
        assert!(validate_document(resolved.document.as_ref().unwrap()).is_empty());
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: "--foo".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, 1, "{input:?}");
        assert_eq!(
            result.evidence[0].source.unwrap().line,
            source_line,
            "{input:?}"
        );
        let mant_protocol::ExplanationContent::FixedOwner { reading_body, .. } =
            result.evidence[0].content.as_ref().unwrap()
        else {
            panic!("not Fixed owner")
        };
        let body = reading_body
            .parts
            .iter()
            .map(|part| part.text.as_str())
            .collect::<String>();
        assert!(body.contains("Body."), "{body}");
        result.validate_references().unwrap();
    }
}

#[test]
fn retained_space_does_not_turn_other_content_into_definitions() {
    // Each exact input ran pinned CVS -Ttree before assertions.
    // man_validate.c::post_SH keeps only later .sp nodes; roff.c increments
    // the epoch for a deleted .br too. The direct RS parent and its visible
    // body remain necessary, while complete grammar rejects ordinary prose.
    for (label, input, needle) in [
        (
            "ordinary-sentence",
            b".TH T 1\n.SH OPTIONS\nIntro.\n.sp\nCompare --foo with --bar.\n.RS 4\nAdditional details.\n.RE\n".as_slice(),
            "--foo",
        ),
        (
            "deleted-br",
            b".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n.br\n\\fB--foo\\fP\n.RS 4\nBody.\n.RE\n".as_slice(),
            "--foo",
        ),
        (
            "cross-section",
            b".TH T 1\n.SH OPTIONS\n.sp\n\\fB--foo\\fP\n.SH NEXT\n.RS 4\nEXAMPLE\n.RE\n".as_slice(),
            "--foo",
        ),
        (
            "nested-rs",
            b".TH T 1\n.SH OPTIONS\n.RS 4\n.sp\n\\fB--inner\\fP\n.RS 4\nINNER\n.RE\n.RE\n".as_slice(),
            "--inner",
        ),
        (
            "empty-rs",
            b".TH T 1\n.SH OPTIONS\n.sp\n\\fB--foo\\fP\n.RS 4\n.RE\n".as_slice(),
            "--foo",
        ),
        (
            "layout-only-rs",
            b".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n\\fB--foo\\fP\n.RS 4\n.sp\n.RE\n".as_slice(),
            "--foo",
        ),
        (
            "no-fill",
            b".TH T 1\n.SH OPTIONS\n.nf\n.sp\n\\fB--foo\\fP\n.RS 4\nEXAMPLE\n.RE\n.fi\n".as_slice(),
            "--foo",
        ),
    ] {
        let resolved = native_query(input, 78);
        assert!(validate_document(resolved.document.as_ref().unwrap()).is_empty(), "{label}");
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
        result.validate_references().unwrap();
    }
}

#[test]
fn hanging_short_prefix_keeps_only_proved_long_names() {
    // Each exact input ran pinned CVS -Tutf8 first. term.c::term_word()
    // executes the final bold/italic style; the short prefix is only
    // presentation evidence, while the long name and direct RS body remain
    // checked independently.
    for (input, name, expected) in [
        (
            b".TH T 1\n.SH OPTIONS\n\\fB\\-.\\fP, \\fB\\-\\-hidden\\fP\n.RS 4\nHidden files.\n.RE\n".as_slice(),
            "--hidden",
            1,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n\\fB\\-0\\fP, \\fB\\-\\-null\\fP\n.RS 4\nNUL output.\n.RE\n".as_slice(),
            "--null",
            1,
        ),
        (
            b".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n\\fB\\-0\\fP, \\fB\\-\\-he\\fP\\fBlp\\fP\n.RS 4\nBody.\n.RE\n".as_slice(),
            "--help",
            1,
        ),
        (
            b".TH T 1\n.SH OPTIONS\nIntro.\n.sp\n\\fB\\-\\fP\\fB0\\fP, \\fB\\-\\-null\\fP\n.RS 4\nBody.\n.RE\n".as_slice(),
            "--null",
            1,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n\\fB\\-10\\fP, \\fB\\-\\-fake\\fP\n.RS 4\nBody.\n.RE\n".as_slice(),
            "--fake",
            0,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n\\fB\\-0\\fP,\\fB\\-\\-fake\\fP\n.RS 4\nBody.\n.RE\n".as_slice(),
            "--fake",
            0,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n\\fI\\-0\\fP, \\fB\\-\\-fake\\fP\n.RS 4\nBody.\n.RE\n".as_slice(),
            "--fake",
            0,
        ),
        (
            b".TH T 1\n.SH DESCRIPTION\nA sample numeric argument follows.\n.sp\n\\fB\\-0\\fP, \\-\\-fake\n.RS 4\nThis is an input example, not an option declaration.\n.RE\n".as_slice(),
            "--fake",
            0,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n\\fB\\-0\\fP, \\fI\\-\\-fake\\fP\n.RS 4\nBody.\n.RE\n".as_slice(),
            "--fake",
            0,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n\\fB\\-0\\fP, \\fB\\-\\-fake\\fP\n.RS 4\n.RE\n".as_slice(),
            "--fake",
            0,
        ),
    ] {
        let resolved = native_query(input, 78);
        assert!(validate_document(resolved.document.as_ref().unwrap()).is_empty());
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, expected, "{input:?}");
        assert!(visible_total(&resolved, name) > 0, "{input:?}");
        result.validate_references().unwrap();
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
    let expected = page.text.clone();
    let document = lower_annotated_document(page).unwrap();
    assert!(validate_document(&document).is_empty());
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(fixed.surface.text, expected);
    assert!(fixed.regions.is_empty());
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
