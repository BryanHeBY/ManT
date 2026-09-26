use super::*;

fn fixed_document(input: &[u8], format: InputFormat) -> mant_ir::Document {
    let document = project_annotated_manual("t.1", &bundle(input), format).unwrap();
    assert!(validate_document(&document).is_empty());
    document
}

fn fixed_entries(
    document: &mant_ir::Document,
) -> Vec<(
    &mant_ir::OwnerMark,
    &mant_ir::EntryFacts<mant_ir::TextSelection>,
)> {
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    fixed
        .owners
        .iter()
        .filter_map(|owner| owner.entry.as_ref().map(|entry| (owner, entry)))
        .collect()
}

#[test]
fn non_declaration_subsections_stop_command_inference_without_leaking_to_siblings() {
    // This exact input ran pinned CVS -Tutf8 -Owidth=78 before the assertion.
    // man_macro.c::blk_imp/rew_scope retain each SS under COMMANDS;
    // man_term.c::pre_SH/pre_SS render the headings without assigning kinds.
    // Unknown TOPIC/MORE inherit ManT's command family, while these three
    // complete prose titles are explicit inheritance barriers.
    let input = b".TH T 1\n.SH COMMANDS\n.SS TOPIC\n.PP\n\\fBgit-add\\fR(1)\n.RS 4\nAdd files.\n.RE\n.SS DESCRIPTION\n.PP\n\\fBgit-help\\fR(1)\n.RS 4\nIntroduction.\n.RE\n.SS EXAMPLES\n.PP\n\\fBgit-show\\fR(1)\n.RS 4\nExample.\n.RE\n.SS SEE ALSO\n.PP\n\\fBgit-push\\fR(1)\n.RS 4\nReference.\n.RE\n.SS MORE\n.PP\n\\fBgit-status\\fR(1)\n.RS 4\nShow status.\n.RE\n";
    let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
    let fixed = fixed_document(input, InputFormat::Man);
    for document in [&flow, &fixed] {
        let index = mant_ir::SemanticIndex::build(document);
        for (subsection, name) in [(0, "git-add"), (4, "git-status")] {
            let entries = index.section_at(&[0, subsection]);
            assert_eq!(entries.len(), 1, "subsection {subsection}");
            assert_eq!(entries[0].kind, EntryKind::Command);
            assert_eq!(entries[0].names, [name]);
        }
        for subsection in 1..=3 {
            assert!(
                index.section_at(&[0, subsection]).is_empty(),
                "subsection {subsection} must stay readable prose"
            );
        }
    }
    let DocumentBody::Fixed(body) = &fixed.body else {
        unreachable!()
    };
    for text in ["git-help(1)", "git-show(1)", "git-push(1)"] {
        assert!(body.surface.text.contains(text), "{text}");
    }
}

#[test]
fn command_catalog_requires_a_complete_manual_call_not_bold_prose() {
    // Both exact E03/E03b inputs ran pinned CVS -Tutf8; E03b also ran
    // -Thtml. man_term.c::pre_PP/pre_RS and man_html.c::man_PP_pre/man_RS_pre
    // establish presentation and indentation, not a semantic command/link.
    let catalog = b".TH ENTRY-CATALOG 1\n.SH COMMANDS\n.PP\n\\fBgit-add\\fR(1)\n.RS 4\nAdd file contents to the index.\n.RE\n.SH SEE ALSO\n.MR git-add 1\n.SH DESCRIPTION\n.PP\n.B Important information follows.\n.RS 4\nThis is introductory prose, not a command declaration.\n.RE\n";
    let note = b".TH ENTRY-NOTE 1\n.SH COMMANDS\n.PP\n.B Note\n.RS 4\nRead this introduction before choosing a command.\n.RE\n.PP\n\\fBgit-add\\fR(1)\n.RS 4\nAdd file contents to the index.\n.RE\n";
    for input in [catalog.as_slice(), note.as_slice()] {
        let document = fixed_document(input, InputFormat::Man);
        let entries = fixed_entries(&document);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].1.kind, EntryKind::Command);
        assert_eq!(entries[0].1.names, ["git-add"]);
        assert_eq!(entries[0].1.forms.len(), 1);
        assert_eq!(entries[0].1.name_bindings.len(), 1);
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!()
        };
        assert_eq!(
            fixed.selection_text(&entries[0].1.forms[0]).as_deref(),
            Some("git-add(1)")
        );
        assert_eq!(
            fixed
                .selection_text(&entries[0].1.name_bindings[0].occurrences[0])
                .as_deref(),
            Some("git-add")
        );
        assert!(entries[0].0.hanging_continuation.is_some());
        let decoded: mant_ir::Document =
            serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
        assert!(validate_document(&decoded).is_empty());
        assert_eq!(fixed_entries(&decoded)[0].1.names, ["git-add"]);

        let mut forged = decoded;
        let DocumentBody::Fixed(forged_fixed) = &mut forged.body else {
            unreachable!()
        };
        let entry = forged_fixed
            .owners
            .iter_mut()
            .find_map(|owner| owner.entry.as_mut())
            .unwrap();
        entry.name_bindings[0].occurrences[0] = entry.forms[0].clone();
        assert!(!validate_document(&forged).is_empty());
    }
}

#[test]
fn true_tp_command_head_accepts_the_same_complete_manual_call() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78. man_macro.c::blk_imp
    // leaves TP HEAD open for the following line; man_term.c::pre_TP prints
    // that HEAD and the distinct BODY. Classification is ManT policy.
    let input = b".TH T 1\n.SH COMMANDS\n.TP\ngit-add(1)\nA command.\n";
    let fixed = fixed_document(input, InputFormat::Man);
    let entries = fixed_entries(&fixed);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1.kind, EntryKind::Command);
    assert_eq!(entries[0].1.names, ["git-add"]);
    let DocumentBody::Fixed(body) = &fixed.body else {
        unreachable!()
    };
    assert_eq!(
        body.selection_text(&entries[0].1.name_bindings[0].occurrences[0])
            .as_deref(),
        Some("git-add")
    );
    let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
    let indexed = mant_ir::SemanticIndex::build(&flow);
    let flow_entries = indexed.section("commands");
    assert_eq!(flow_entries.len(), 1);
    assert_eq!(flow_entries[0].kind, EntryKind::Command);
    assert_eq!(flow_entries[0].names, ["git-add"]);
}

#[test]
fn weak_manual_call_does_not_override_an_explicit_native_role() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78. mdoc_term.c::termp_it_pre
    // prints both true item HEADs; Cm and Ic use termp_bold_pre, while Ev is
    // a distinct authored macro. That role outranks ManT's weak COMMANDS
    // spelling inference; its own name is governed by the environment grammar.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n.It Ev git-add(1)\nEnvironment-looking label.\n.It Cm git-add(1)\nLiteral command label.\n.El\n";
    let fixed = fixed_document(input, InputFormat::Mdoc);
    let entries = fixed_entries(&fixed);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].1.kind, EntryKind::EnvironmentVariable);
    assert_eq!(entries[0].1.names, ["git-add(1)"]);
    assert_eq!(entries[1].1.kind, EntryKind::Command);
    assert_eq!(entries[1].1.names, ["git-add"]);
    let DocumentBody::Fixed(body) = &fixed.body else {
        unreachable!()
    };
    assert!(body.surface.text.contains("Environment-looking label."));
    assert!(body.surface.text.contains("Literal command label."));
}

#[test]
fn manual_call_negative_shapes_remain_readable_without_entries() {
    // This exact combined PP/RS input ran pinned CVS -Tutf8. Its URL, path,
    // mail-like spelling and section 0 are visible text, not inferred native
    // manual targets; unlike MAN_MR, man_html.c only emits paragraph/div here.
    let input = b".TH T 1\n.SH COMMANDS\n.PP\n\\fBhttps://example.test\\fR(1)\n.RS 4\nURL-like prose.\n.RE\n.PP\n\\fB/tmp/tool\\fR(1)\n.RS 4\nPath-like prose.\n.RE\n.PP\n\\fBuser@tool\\fR(1)\n.RS 4\nMail-like prose.\n.RE\n.PP\n\\fBtool\\fR(0)\n.RS 4\nAmbiguous section prose.\n.RE\n";
    let document = fixed_document(input, InputFormat::Man);
    assert!(fixed_entries(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(
        fixed
            .owners
            .iter()
            .filter(|owner| owner.hanging_candidate)
            .count(),
        4
    );
    for needle in ["https://example.test", "/tmp/tool", "user@tool", "tool(0)"] {
        assert!(fixed.surface.text.contains(needle), "{needle}");
    }
}

#[test]
fn complete_terms_and_environment_template_keep_physical_owners() {
    // Exact E04 ran pinned CVS -Tutf8. pre_TP/post_TP keep each physical
    // owner; only the complete visible subject of a genuine definition is a
    // Term name. The environment family label is not an exact selector.
    let input = b".TH ENTRY-TERMS 1\n.SH OPTIONS\n.TP\n.B --mode=MODE\nSelect a mode. The following values are supported.\n.RS\n.TP\n.B fast\nUse the fast mode.\n.TP\n.B --other\nAn independent option, not a value named --other.\n.RE\n.SH DIAGNOSTICS\n.TP\n.B file: not in gzip format\nThe input does not contain compressed data.\n.SH GLOSSARY\n.TP\n.B working tree\nThe checked out files.\n.SH ENVIRONMENT\n.TP\n.B FILE_TEMPLATE_*\nA family of names, not a literal environment variable named FILE_TEMPLATE_*.\n";
    let document = fixed_document(input, InputFormat::Man);
    let entries = fixed_entries(&document);
    for name in ["fast", "file: not in gzip format", "working tree"] {
        let (_, entry) = entries
            .iter()
            .find(|(_, entry)| entry.names.iter().any(|found| found == name))
            .unwrap_or_else(|| panic!("missing complete Term: {name}"));
        assert_eq!(entry.kind, EntryKind::Term);
        assert_eq!(entry.names.as_slice(), &[name]);
        assert_eq!(entry.name_bindings.len(), 1);
        assert_eq!(entry.alias_groups.len(), 0);
    }
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    let template = fixed
        .owners
        .iter()
        .find(|owner| fixed.selection_text(&owner.head).as_deref() == Some("FILE_TEMPLATE_*"))
        .unwrap();
    assert!(template.entry.is_none());
    assert!(!template.direct_body.parts.is_empty());
    let decoded: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&decoded).is_empty());
    assert!(
        fixed_entries(&decoded)
            .iter()
            .all(|(_, entry)| !entry.names.contains(&"FILE_TEMPLATE_*".to_owned()))
    );
}

#[test]
fn mdoc_templates_and_man_option_keep_distinct_environment_priority() {
    // Both exact sources ran pinned CVS -Tutf8. mdoc_term.c::termp_it_pre
    // preserves Sy/plain tag heads; man_term.c::pre_B emits the option glyphs.
    let mdoc = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Sy FILE_TEMPLATE_*\nStyled template body.\n.It FILE_TEMPLATE_*\nPlain template body.\n.El\n";
    let document = fixed_document(mdoc, InputFormat::Mdoc);
    assert!(fixed_entries(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        unreachable!()
    };
    assert_eq!(fixed.owners.len(), 2);
    assert!(
        fixed
            .owners
            .iter()
            .all(|owner| owner.entry.is_none() && !owner.direct_body.parts.is_empty())
    );

    let option = b".TH T 1\n.SH ENVIRONMENT\n.TP\n.B --foo\nDescription.\n";
    let document = fixed_document(option, InputFormat::Man);
    let entries = fixed_entries(&document);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].1.names, ["--foo"]);
    assert_eq!(
        entries[0].1.kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
}

#[test]
fn literal_role_does_not_turn_an_environment_template_into_a_term() {
    // Exact source ran pinned CVS -Tutf8 -Owidth=78. mdoc_term.c::termp_it_pre
    // preserves this real item HEAD, while Cm only changes its display font;
    // the template spelling is not an exact environment selector.
    let input = b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Cm FILE_TEMPLATE_*\nA family.\n.El\n";
    let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
    assert!(
        mant_ir::SemanticIndex::build(&flow)
            .section("environment")
            .is_empty()
    );
    let fixed = fixed_document(input, InputFormat::Mdoc);
    assert!(fixed_entries(&fixed).is_empty());
    let DocumentBody::Fixed(body) = &fixed.body else {
        unreachable!()
    };
    assert!(body.surface.text.contains("FILE_TEMPLATE_*"));
    assert!(body.surface.text.contains("A family."));
}

#[test]
fn split_style_and_punctuation_do_not_split_a_true_term() {
    // Both exact man inputs ran pinned CVS -Tutf8. pre_alternate concatenates
    // BR operands without semantic separators; pre_B prints the entire
    // punctuation-bearing label. Neither layout nor punctuation creates an
    // alias or a second name.
    let split = b".TH T 1\n.SH GLOSSARY\n.TP\n.B \"working tree\"\nFirst definition.\n.TP\n.BR \"working tree\" \"\"\nSecond definition.\n";
    let document = fixed_document(split, InputFormat::Man);
    let entries = fixed_entries(&document);
    assert_eq!(entries.len(), 2);
    assert!(
        entries
            .iter()
            .all(|(_, entry)| entry.kind == EntryKind::Term && entry.names == ["working tree"])
    );

    let punctuation = b".TH T 1\n.SH GLOSSARY\n.TP\n.B foo, bar\nComma subject.\n.TP\n.B foo|bar\nPipe subject.\n";
    let document = fixed_document(punctuation, InputFormat::Man);
    let entries = fixed_entries(&document);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].1.names, ["foo, bar"]);
    assert_eq!(entries[1].1.names, ["foo|bar"]);
    assert!(
        entries
            .iter()
            .all(|(_, entry)| entry.alias_groups.is_empty())
    );
}

#[test]
fn pure_command_markers_never_become_semantic_entries() {
    // Exact input ran pinned CVS -Tutf8. pre_TP/pre_B show both glyphs, but
    // a presentation mark in COMMANDS is not a callable declaration.
    let input = ".TH T 1\n.SH COMMANDS\n.TP\n.B •\nBullet body.\n.TP\n.B *\nStar body.\n";
    let fixed = fixed_document(input.as_bytes(), InputFormat::Man);
    assert!(fixed_entries(&fixed).is_empty());
    let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input.as_bytes()).unwrap();
    assert!(
        mant_ir::SemanticIndex::build(&flow)
            .section("commands")
            .is_empty()
    );
}

#[test]
fn plain_native_definition_punctuation_is_one_complete_term_in_both_paths() {
    // These exact man and mdoc files ran pinned CVS -Tutf8 -Owidth=78 before
    // this assertion. man_term.c::pre_TP and mdoc_term.c::termp_it_pre execute
    // each label as one native definition head; commas/pipes add no aliases.
    let cases: [(&[u8], InputFormat); 2] = [
        (
            b".TH T 1\n.SH GLOSSARY\n.TP\nfoo, bar\nPlain man subject.\n.TP\nfoo|bar\nPipe man subject.\n",
            InputFormat::Man,
        ),
        (
            b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh GLOSSARY\n.Bl -tag -width Ds\n.It foo, bar\nPlain mdoc subject.\n.It foo|bar\nPipe mdoc subject.\n.El\n",
            InputFormat::Mdoc,
        ),
    ];
    for (input, format) in cases {
        let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
        let flow_entries = mant_ir::SemanticIndex::build(&flow)
            .section("glossary")
            .iter()
            .map(|entry| (entry.kind, entry.names.clone(), entry.alias_groups.clone()))
            .collect::<Vec<_>>();
        let fixed = fixed_document(input, format);
        let fixed_entries = fixed_entries(&fixed)
            .iter()
            .map(|(_, entry)| (entry.kind, entry.names.clone(), entry.alias_groups.clone()))
            .collect::<Vec<_>>();
        for entries in [flow_entries, fixed_entries] {
            assert_eq!(entries.len(), 2);
            assert_eq!(
                entries[0],
                (EntryKind::Term, vec!["foo, bar".to_owned()], vec![])
            );
            assert_eq!(
                entries[1],
                (EntryKind::Term, vec!["foo|bar".to_owned()], vec![])
            );
        }
        let decoded: mant_ir::Document =
            serde_json::from_value(serde_json::to_value(&fixed).unwrap()).unwrap();
        assert!(validate_document(&decoded).is_empty());
    }
}

#[test]
fn explicitly_styled_ip_punctuation_stays_addressable_but_plain_marker_does_not() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78. man_macro.c::blk_imp
    // preserves each IP head; man_term.c::pre_IP/pre_B executes bold styling.
    // Native style distinguishes an authored key from the same plain marker.
    let input = b".TH T 1\n.SH OPTIONS\n.IP \"\\fB*\\fP\" 4\nStyled star key body.\n.IP \"\\fB-\\fP\" 4\nStyled dash key body.\n.IP * 4\nPlain star marker body.\n";
    let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
    let flow_entries = mant_ir::SemanticIndex::build(&flow)
        .section("options")
        .iter()
        .map(|entry| (entry.kind, entry.names.clone()))
        .collect::<Vec<_>>();
    let fixed = fixed_document(input, InputFormat::Man);
    let fixed_entries = fixed_entries(&fixed)
        .iter()
        .map(|(_, entry)| (entry.kind, entry.names.clone()))
        .collect::<Vec<_>>();
    for entries in [flow_entries, fixed_entries] {
        assert_eq!(
            entries,
            [
                (EntryKind::Term, vec!["*".to_owned()]),
                (EntryKind::Term, vec!["-".to_owned()])
            ]
        );
    }
    let decoded: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&fixed).unwrap()).unwrap();
    assert!(validate_document(&decoded).is_empty());
}
