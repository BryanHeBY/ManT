use super::*;

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
    assert_eq!(owner.head_role_prefix, None);
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
    assert_eq!(fixed.owners[0].head_role, Some(OwnerHeadRole::Lexical));
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
