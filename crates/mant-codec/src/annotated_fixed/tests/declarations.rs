use super::*;

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
    let expected = untrusted.text.clone();
    let degraded = lower_annotated_document(untrusted).unwrap();
    assert!(validate_document(&degraded).is_empty());
    assert!(!mant_ir::semantics_complete(&degraded.diagnostics));
    let DocumentBody::Fixed(degraded_fixed) = &degraded.body else {
        unreachable!()
    };
    assert_eq!(degraded_fixed.surface.text, expected);
    assert!(degraded_fixed.owners.is_empty());
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
    // A complete bold `-1` HEAD is an independently witnessed short option;
    // a signed number inside an existing argument remains an argument.
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
    for owner in &fixed.owners[1..3] {
        assert_eq!(owner.entry.as_ref().unwrap().kind, EntryKind::Term);
    }
    assert_eq!(fixed.owners[3].entry.as_ref().unwrap().names, ["-1"]);
    assert_eq!(
        fixed.owners[3].entry.as_ref().unwrap().kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
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
    for (label, names) in [
        (r"\-Y, \-\-yay", vec!["-Y", "--yay"]),
        (r"\-p|\-\-parents", vec!["-p", "--parents"]),
        (r"\-a \-\-ascii", vec!["-a", "--ascii"]),
        (
            r"\-c \-\-stdout \-\-to-stdout",
            vec!["-c", "--stdout", "--to-stdout"],
        ),
        (r"\-\-builddir <dir>", vec!["--builddir"]),
        ("--builddir <dir>", vec!["--builddir"]),
        (r"\-p", vec!["-p"]),
    ] {
        let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nbody\n");
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!();
        };
        let owner = &fixed.owners[0];
        assert_eq!(owner.head_role, Some(OwnerHeadRole::Lexical), "{label}");
        assert_eq!(owner.head_role_prefix, None, "{label}");
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
fn escaped_dash_man_hint_uses_final_glyphs_and_style() {
    // Each exact input ran pinned CVS -Ttree/-Tascii first. term.c::term_word
    // executes \& without a glyph, but \[hy] is not an ASCII option dash.
    // pre_B() only chooses an initial font: term_word() preserves the same
    // visible spelling across an inline roman switch, so -B\fRn is -Bn.
    // One bold digit after the executed ASCII option dash is a native short
    // option candidate, not the same as a signed numeric argument.
    for (label, expected) in [
        (
            r"\-B\fRn",
            Some((
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                "-Bn",
            )),
        ),
        (r"\[hy]x", Some((EntryKind::Term, "‐x"))),
        (
            r"\-x\&foo",
            Some((
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                "-xfoo",
            )),
        ),
        (
            r"\-1",
            Some((
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                "-1",
            )),
        ),
        (r"\-x.", Some((EntryKind::Term, "-x."))),
        (
            r"\-a, text",
            Some((
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                "-a",
            )),
        ),
    ] {
        let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B {label}\nbody\n");
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
        let DocumentBody::Fixed(fixed) = &document.body else {
            unreachable!();
        };
        let owner = &fixed.owners[0];
        assert_eq!(owner.head_role, Some(OwnerHeadRole::Lexical), "{label}");
        assert_eq!(
            owner
                .entry
                .as_ref()
                .map(|entry| (entry.kind, entry.names[0].as_str())),
            expected,
            "{label}"
        );
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
        assert_eq!(owner.head_role_prefix, None);
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
        assert!(owner.entry.is_none());
    }
    assert!(validate_document(&document).is_empty());
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    for (requested, expected_sources, ordinary_head) in [
        ("-x", vec![3, 5], Some("-x")),
        ("--help", vec![7], None),
        ("-Wformat", vec![9], None),
        ("-xfoo", vec![], Some("-xfoo")),
    ] {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: requested.to_owned(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        // Unaccepted but visible IP heads are literal context, not another
        // direct option. The exact input was rerun through pinned CVS -Tutf8
        // and -Ttree before changing these behavioral assertions.
        assert_eq!(
            result.counts.direct_entry.total as usize,
            expected_sources.len(),
            "{requested}"
        );
        assert_eq!(
            result.counts.context_mention.total,
            u32::from(ordinary_head.is_some()),
            "{requested}"
        );
        assert_eq!(result.counts.entry_mention.total, 0, "{requested}");
        assert_eq!(
            result.total as usize,
            expected_sources.len() + usize::from(ordinary_head.is_some()),
            "{requested}"
        );
        assert_eq!(
            result
                .evidence
                .iter()
                .filter(|item| item.class == EvidenceClass::DirectEntry)
                .map(|item| item.source.unwrap().line)
                .collect::<Vec<_>>(),
            expected_sources
        );
        if let Some(head) = ordinary_head {
            let mention = result
                .evidence
                .iter()
                .find(|item| item.class == EvidenceClass::ContextMention)
                .unwrap();
            assert!(mention.entry.is_none(), "{requested}");
            assert_eq!(
                mention.fixed_previews[0]
                    .selection
                    .complete_text()
                    .as_deref(),
                Some(head),
                "{requested}"
            );
            if requested == "-x" {
                // GDB inspection of the current response showed the extra
                // two-scalar hit in the underlined IP head, not GLUED_BODY.
                assert!(mention.fixed_previews[0].selection.parts[0].style.underline);
            }
        }
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
    assert_eq!(fixed.owners[0].head_role_prefix, None);
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
fn native_man_declaration_segments_keep_arguments_out_and_later_names_in() {
    // Every exact input ran pinned CVS -Tutf8 first. man_term.c::pre_IP and
    // pre_alternate select the native HEAD; term.c::term_word applies inline
    // font overrides and prints \(dq as visible quotes. The final-display
    // declaration intervals, not raw punctuation, determine name boundaries.
    // A single IP operand with an italic argument has no native child
    // boundary proving that a later bold run starts a new declaration;
    // retain its text but conservatively omit that ambiguous name.
    for (label, input, names, rejected) in [
        (
            "quoted-argument",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--pattern\\fR \\(dqone,--fake,two\\(dq\" 4\nDescription.\n"
                .as_slice(),
            vec!["--pattern"],
            "--fake",
        ),
        (
            "quoted-argument-then-name",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--pattern\\fR \\(dqone,--fake,two\\(dq, \\fB--all\\fR\" 4\nDescription.\n"
                .as_slice(),
            vec!["--pattern", "--all"],
            "--fake",
        ),
        (
            "single-quoted-argument-then-name",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--pattern\\fR 'one,--fake,two', \\fB--all\\fR\" 4\nDescription.\n"
                .as_slice(),
            vec!["--pattern", "--all"],
            "--fake",
        ),
        (
            "adjacent-single-quoted-argument",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--pattern\\fR,'one,--fake,two'\" 4\nDescription.\n"
                .as_slice(),
            vec!["--pattern"],
            "--fake",
        ),
        (
            "italic-middle",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-a\\fR, \\fI--operand\\fR, \\fB--all\\fR\" 4\nDescription.\n"
                .as_slice(),
            vec!["-a"],
            "--all",
        ),
        (
            "component-inline-italic",
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR \"\\fI--operand\\fR\" \", \" --all\nDescription.\n"
                .as_slice(),
            vec!["--all"],
            "--operand",
        ),
        (
            "component-italic-middle",
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR -a \", \" \"\\fI--operand\\fR\" \", \" --all\nDescription.\n"
                .as_slice(),
            vec!["-a", "--all"],
            "--operand",
        ),
    ] {
        assert_fixed_segment_case(label, input, &names, rejected);
    }
}

#[test]
fn native_man_terminal_nonprinting_escapes_keep_complete_name() {
    // This exact standalone BR input also ran pinned CVS -Tutf8. Its inline
    // italic escape overrides pre_alternate's initial bold font. It is not
    // an option name, but man_term.c::pre_TP still prints a readable term.
    let italic_only = b".TH T 1\n.SH OPTIONS\n.TP\n.BR \"\\fI--operand\\fR\"\nDescription.\n";
    let document = project_annotated_manual("t.1", &bundle(italic_only), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let entry = fixed.owners[0].entry.as_ref().expect("readable TP term");
    assert_eq!(entry.kind, EntryKind::Term);
    assert!(entry.names.is_empty());
    assert!(validate_document(&document).is_empty());
}

#[test]
fn native_man_plain_argument_does_not_promote_embedded_option() {
    // Each exact .IP input ran pinned CVS -Tutf8 before these assertions.
    // man_term.c::pre_IP prints one label operand; term.c::term_word prints
    // its plain parameter punctuation without creating another HEAD.
    for (label, input, names) in [
        (
            "plain-argument-commas",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--list\\fR first,--fake,last\" 4\nDescription.\n"
                .as_slice(),
            vec!["--list"],
        ),
        (
            "plain-argument-after-comma",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--list\\fR, first,--fake,last\" 4\nDescription.\n"
                .as_slice(),
            vec!["--list"],
        ),
        (
            "plain-argument-pipes",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--list\\fR first|--fake|last\" 4\nDescription.\n"
                .as_slice(),
            vec!["--list"],
        ),
        (
            "plain-argument-then-name",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--list\\fR first,--fake,last, \\fB--all\\fR\" 4\nDescription.\n"
                .as_slice(),
            vec!["--list", "--all"],
        ),
        (
            "negative-number-argument",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--number\\fR -10,--fake,20\" 4\nDescription.\n"
                .as_slice(),
            vec!["--number"],
        ),
        (
            "negative-number-after-comma",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--number\\fR, -10,--fake,20\" 4\nDescription.\n"
                .as_slice(),
            vec!["--number"],
        ),
        (
            "negative-number-then-name",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--number\\fR -10,--fake,20, \\fB--all\\fR\" 4\nDescription.\n"
                .as_slice(),
            vec!["--number", "--all"],
        ),
    ] {
        assert_fixed_segment_case(label, input, &names, "--fake");
    }
}

#[test]
fn native_man_complete_heads_and_styled_argument_boundaries_bind_all_names() {
    // Each exact input ran pinned CVS -Ttree before these assertions.
    // man_term.c::pre_B/pre_IP/pre_TP preserve the complete HEAD while
    // term.c::term_word emits no glyph for trailing \& and applies inline
    // italic font changes before the adjacent argument's first glyph.
    for (label, input, names, rejected) in [
        (
            "bold-head-with-later-declaration",
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \"-a ARG, --all\"\nDescription.\n".as_slice(),
            vec!["-a", "--all"],
            "ARG",
        ),
        (
            "tp-glued-italic-argument",
            b".TH T 1\n.SH OPTIONS\n.TP\n\\fB-L\\fR\\fIdir\\fR\nDirectory.\n".as_slice(),
            vec!["-L"],
            "dir",
        ),
        (
            "ip-glued-italic-argument",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\-O\\fIlevel\\fP \\&\"\nEnables query optimisation.\n"
                .as_slice(),
            vec!["-O"],
            "level",
        ),
        (
            "ip-bold-glued-italic-argument",
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-L\\fR\\fIdir\\fR\" 4\nDirectory.\n".as_slice(),
            vec!["-L"],
            "dir",
        ),
    ] {
        assert_fixed_segment_case(label, input, &names, rejected);
    }

    // Every exact suffix spelling ran pinned CVS -Ttree/-Tutf8.  Consecutive
    // zero-glyph escapes after the last visible character cannot extend the
    // option spelling, irrespective of the final font mode they select.
    for input in [
        b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--help\\&\"\nDescription.\n".as_slice(),
        b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--help\\&\\fR\"\nDescription.\n",
        b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--help\\fR\\&\"\nDescription.\n",
        b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--help\\fI\"\nDescription.\n",
        b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--help\\fB\"\nDescription.\n",
        b".TH T 1\n.SH OPTIONS\n.IP \"\\fB--help\\&\\fR\" 4\nDescription.\n",
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        let entry = fixed.owners[0]
            .entry
            .as_ref()
            .expect("terminal zero-width escapes keep the name");
        assert_eq!(
            entry.names,
            ["--help"],
            "{}",
            String::from_utf8_lossy(input)
        );
        assert_eq!(
            entry.kind,
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            }
        );
        assert_eq!(
            fixed.selection_text(&entry.name_bindings[0].occurrences[0]),
            Some("--help".to_owned())
        );
    }
}

#[test]
fn executed_man_heads_share_complete_declaration_names() {
    // Each exact input ran pinned CVS -Tutf8 first. man_macro.c::blk_imp
    // establishes the HEAD; man_term.c::pre_B/pre_alternate and
    // term.c::term_word execute fonts and zero-width escapes before binding.
    for (input, names) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.PD\n.B \\-\\-exclude\\ \\fRfiles\nDescription.\n"
                .as_slice(),
            vec!["--exclude"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-L\\fIdir\\fP\" 4\nDescription.\n".as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \"\\&--help\"\nDescription.\n".as_slice(),
            vec!["--help"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--he\\&lp\"\nDescription.\n".as_slice(),
            vec!["--help"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \"\\fB--help\\fP\"\nDescription.\n".as_slice(),
            vec!["--help"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.SB --save\nDescription.\n".as_slice(),
            vec!["--save"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR --opt \" ARG, --all\"\nDescription.\n".as_slice(),
            vec!["--opt", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-c \\-\\-stdout \\-\\-to-stdout\nDescription.\n"
                .as_slice(),
            vec!["-c", "--stdout", "--to-stdout"],
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        assert_eq!(
            fixed.owners[0].entry.as_ref().map(|entry| &entry.names),
            Some(&names.iter().map(|name| (*name).to_owned()).collect()),
            "{}",
            String::from_utf8_lossy(input)
        );
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for name in names {
            let result = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, 1, "{name}");
            result.validate_references().unwrap();
        }
    }
}

#[test]
fn flow_and_fixed_share_styled_boundary_and_three_name_heads() {
    // Both exact inputs ran pinned CVS -Tutf8 first. man_term.c::pre_alternate
    // prints the italic operand (including its terminal comma) before the
    // next bold operand; man_term.c::pre_B retains all three gzip spellings.
    for (input, names) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir, \" \"--all\"\nDescription.\n".as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-c \\-\\-stdout \\-\\-to-stdout\nDescription.\n"
                .as_slice(),
            vec!["-c", "--stdout", "--to-stdout"],
        ),
    ] {
        let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
        let fixed = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        for document in [&flow, &fixed] {
            assert!(validate_document(document).is_empty());
            let index = mant_ir::SemanticIndex::build(document);
            let entries = index.section("options");
            assert_eq!(entries.len(), 1, "{}", String::from_utf8_lossy(input));
            assert_eq!(
                entries[0].names,
                names,
                "{}",
                String::from_utf8_lossy(input)
            );
        }
    }
}

#[test]
fn native_man_split_names_and_styled_arguments_share_one_checked_head() {
    // Each exact input ran pinned CVS -Tutf8 before these assertions.
    // man_macro.c::blk_imp preserves the TP HEAD; man_term.c::pre_B chooses
    // only the initial font, pre_alternate joins operands without a space,
    // and term.c::term_word executes inline font changes before display.
    for (input, names) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n\\fR--help\nDescription.\n".as_slice(),
            vec!["--help"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \"--he\\fRlp\"\nDescription.\n".as_slice(),
            vec!["--help"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR --he \"\\fBlp\"\nDescription.\n".as_slice(),
            vec!["--help"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR --he \"\\fBlp ARG\"\nDescription.\n".as_slice(),
            vec!["--help"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR -- o pt \" ARG\"\nDescription.\n".as_slice(),
            vec!["--opt"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR --help ARG\nDescription.\n".as_slice(),
            vec!["--helpARG"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR -o FILE\nDescription.\n".as_slice(),
            vec!["-oFILE"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-o --output \" FILE\nDescription.\n".as_slice(),
            vec!["-o", "--output"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-o or --output \" FILE\nDescription.\n".as_slice(),
            vec!["-o", "--output"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-### --long \" FILE\nDescription.\n".as_slice(),
            vec!["--long"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir, \" \"-### --long \" FILE\nDescription.\n"
                .as_slice(),
            vec!["-L", "--long"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir, \" \"--output=\" FILE\nDescription.\n"
                .as_slice(),
            vec!["-L", "--output"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir, \" \"--output[=FILE]\"\nDescription.\n"
                .as_slice(),
            vec!["-L", "--output"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir, \" \"--output=FILE, --all\"\nDescription.\n"
                .as_slice(),
            vec!["-L", "--output", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir, \" \"-10,--fake\"\nDescription.\n"
                .as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir, \" \"\\(dq--fake\\(dq\"\nDescription.\n"
                .as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"dir,--fake,tail, \" \"--all\"\nDescription.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"first,--fake,last,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
    ] {
        assert_flow_fixed_checked_names(input, &names);
    }
}

#[test]
fn native_operand_boundary_cannot_end_an_unclosed_argument() {
    // Each exact input ran pinned CVS -Tutf8 before this assertion. CVS
    // man_term.c::pre_alternate() switches font between BI operands, but
    // neither that boundary nor term.c::term_word()'s inline font escapes
    // closes an unmatched quote or bracket in the complete visible head.
    for (input, expected) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"(first,--fake,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"\\(dqfirst,--fake,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"(first,\\fB--fake,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"(first,\\fB--fake,\"\nBody.\n"
                .as_slice(),
            vec!["-L"],
        ),
    ] {
        let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
        let flow_index = mant_ir::SemanticIndex::build(&flow);
        let flow_entries = flow_index.section("options");
        assert_eq!(flow_entries.len(), 1);
        assert_eq!(flow_entries[0].names, ["-L"]);

        let fixed = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        assert!(validate_document(&fixed).is_empty());
        let DocumentBody::Fixed(body) = &fixed.body else {
            panic!("not Fixed")
        };
        assert_eq!(
            body.owners[0].entry.as_ref().map(|entry| &entry.names),
            Some(&expected.iter().map(|name| (*name).to_owned()).collect()),
            "{}",
            String::from_utf8_lossy(input)
        );
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(fixed),
            tldr: None,
        };
        for (name, total) in [("--fake", 0), ("--all", u32::from(expected.len() == 2))] {
            let result = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, total, "{name}");
            result.validate_references().unwrap();
        }
    }
}

#[test]
fn native_styled_argument_state_survives_font_operands_and_inline_escapes() {
    // Both exact inputs ran pinned CVS -Tutf8 before this assertion.
    // man_term.c::pre_alternate() joins BI operands without spaces, while
    // term.c::term_word() may change font within one parameter operand.
    // Neither event ends a quote or converts a parameter-internal comma
    // into a new declaration; the later independent --all follows a
    // terminal comma after the quote has closed or the parameter has ended.
    for (input, names) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"--pattern \" \"\\(dqfirst,\" \"--fake\" \",last\\(dq,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["--pattern", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"first,\\fB--fake\\fI,last,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
    ] {
        assert_flow_fixed_checked_names(input, &names);
        let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
        let fixed = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
        for document in [flow, fixed] {
            let resolved = mant_ir::ResolvedContent {
                address: None,
                label: "T(1)".into(),
                document: Some(document),
                tldr: None,
            };
            let result = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: "--fake".into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, 0);
            result.validate_references().unwrap();
        }
    }
}

#[test]
fn display_quote_wrappers_do_not_confuse_native_name_and_argument_ranges() {
    // Both exact inputs ran pinned CVS -Tutf8 first. man_term.c::pre_B
    // selects bold and term.c::term_word prints the display quotes; the
    // opening quote before --foo wraps a name, while the one after it wraps
    // an argument whose internal comma cannot declare --fake.
    for (input, names) in [
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.B “--foo”\nBody.\n",
            vec!["--foo"],
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.B --foo “one,--fake,two”\nBody.\n",
            vec!["--foo"],
        ),
    ] {
        assert_flow_fixed_checked_names(input.as_bytes(), &names);
    }
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "one Flow and Fixed operand boundary matrix"
)]
fn native_operand_intervals_bound_styled_declarations_across_all_fonts() {
    // Each exact input ran the pinned CVS reference with -Tutf8 before these
    // assertions. man_macro.c::in_line_eoln retains separate BI/BR operands;
    // man_term.c::pre_alternate() joins them, and term.c::term_word() executes
    // inline font escapes within an operand. CVS proves the visible text and
    // operand boundaries, not ManT's option semantics: punctuation within a
    // parameter must not promote --fake, while an independently executed
    // operand after a terminal separator can establish --all. pre_IP has one
    // label operand, so its inline font switch is deliberately not a restart.
    for (input, names) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"first\\fB, --fake\\fI,last,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"first\\fR, --fake\\fI,last,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-o \" FILE \", --all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-o", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-o \" FILE \",--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-o", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"arg,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"arg\" \",\" \"\\fB--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"arg\" \"\\fI,\\fB--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"arg\" \",\\fI--fake\"\nBody.\n"
                .as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"arg,\" \" --all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"arg,\" \"\\~--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR \"--opt \" \"arg,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["--opt", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"first|--fake|last|\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"first/--fake/last/\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            // Slash splits validated option aliases such as -h/--help,
            // not a styled parameter's path-like argument.
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"-10,--fake,20,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["-L", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"--pattern \" \"\\(dqfirst,\" \"--fake\" \",last\\(dq,\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["--pattern", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"--pattern \" \"[first,\" \"--fake\" \",last],\" \"--all \" FILE\nBody.\n"
                .as_slice(),
            vec!["--pattern", "--all"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-L\\fI first\\fB, --fake\\fI,last,\" 4\nBody.\n"
                .as_slice(),
            vec!["-L"],
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.de ZZ\n.BI \"-o \" FILE \", --all \" FILE\n..\n.TP\n.ZZ\nBody.\n"
                .as_slice(),
            vec!["-o", "--all"],
        ),
    ] {
        assert_flow_fixed_checked_names(input, &names);
        for document in [
            crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap(),
            project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap(),
        ] {
            let resolved = mant_ir::ResolvedContent {
                address: None,
                label: "T(1)".into(),
                document: Some(document),
                tldr: None,
            };
            let fake = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: "--fake".into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(fake.counts.direct_entry.total, 0, "{}", String::from_utf8_lossy(input));
            fake.validate_references().unwrap();
        }
    }
}

#[test]
fn native_declaration_name_limit_cannot_be_bypassed_by_first_operand_hint() {
    // Both generated BR heads (64 and 65 names) ran pinned CVS -Tutf8 first.
    // man_term.c::pre_alternate() executes each name and comma as a separate
    // child; its layout is unchanged when the semantic name cap is reached.
    for (count, expected) in [(64, 1), (65, 0)] {
        let mut operands = Vec::with_capacity(count * 2);
        for index in 1..=count {
            operands.push(format!("-a{index}"));
            if index != count {
                operands.push(",".to_owned());
            }
        }
        let input = format!(
            ".TH T 1\n.SH OPTIONS\n.TP\n.BR {}\nBody.\n",
            operands.join(" ")
        );
        for document in [
            crate::parse_roff_bytes(std::path::Path::new("t.1"), input.as_bytes()).unwrap(),
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap(),
        ] {
            assert!(validate_document(&document).is_empty());
            let resolved = mant_ir::ResolvedContent {
                address: None,
                label: "T(1)".into(),
                document: Some(document),
                tldr: None,
            };
            let result = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: "-a1".into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, expected, "{count} names");
        }
    }
}

#[test]
fn native_ip_font_run_cannot_reset_an_unclosed_argument() {
    // Exact bytes ran pinned CVS -Thtml first. man_term.c::pre_IP prints one
    // HEAD text operand; term.c::term_word changes its font for --fake, but
    // that new bold run is not a new native declaration component.
    let input =
        b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-L\\fR \\fI\\(dqfirst,\\fR\\fB--fake\\fR\" 4\nBody.\n";
    let fixed = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&fixed).is_empty());
    let DocumentBody::Fixed(body) = &fixed.body else {
        panic!("not Fixed")
    };
    assert_eq!(
        body.owners[0]
            .entry
            .as_ref()
            .map(|entry| entry.names.as_slice()),
        Some(["-L".to_owned()].as_slice())
    );
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(fixed),
        tldr: None,
    };
    let fake = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "--fake".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(fake.counts.direct_entry.total, 0);
    fake.validate_references().unwrap();
}

#[test]
fn native_ip_font_run_does_not_reopen_a_styled_parameter_after_whitespace() {
    // Exact bytes ran pinned CVS -Tutf8 first. man_term.c::pre_IP executes
    // one visible HEAD operand; term.c::term_word changes font inside that
    // operand, so even comma + space before a new bold run does not prove a
    // second native declaration. The body remains readable either way.
    let input = b".TH T 1\n.SH OPTIONS\n.IP \"\\fB-L\\fI first, \\fB--fake\\fI,last,\" 4\nBody.\n";
    assert_flow_fixed_checked_names(input, &["-L"]);
    for document in [
        crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap(),
        project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap(),
    ] {
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        let fake = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: "--fake".into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(fake.counts.direct_entry.total, 0);
        fake.validate_references().unwrap();
    }
}

#[test]
fn styled_terminal_delimiter_restarts_only_the_independent_name() {
    // Exact bytes ran pinned CVS -Tutf8 first. man_term.c::pre_alternate()
    // keeps the italic parameter's terminal comma and joins the next bold
    // operand without inserting a space; its internal commas stay arguments.
    let input =
        b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"first,--fake,last,\" \"--all \" FILE\nBody.\n";
    let fixed = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&fixed).is_empty());
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(fixed),
        tldr: None,
    };
    for (name, expected) in [("-L", 1), ("--all", 1), ("--fake", 0)] {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, expected, "{name}");
        result.validate_references().unwrap();
    }
}

#[test]
fn complete_head_scan_is_the_only_flow_and_fixed_option_name_decision() {
    // Each exact input ran pinned CVS -Tutf8 before these assertions.
    // man_term.c::pre_B/pre_alternate select initial font, but term.c::
    // term_word() may switch font within one operand. Only the independent
    // BI child after the terminal comma proves a fresh declaration.
    for (input, names, rejected) in [
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.B \"-L\\fIfirst\\fB, --fake\\fI,last\"\nBody.\n",
            vec!["-L"],
            "--fake",
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n-L\\fIfirst\\fB, --fake\\fI,last\nBody.\n",
            vec!["-L"],
            "--fake",
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.BI \"--opt \" \"arg,\" \"\\f[BI]--all \" FILE\nBody.\n",
            vec!["--opt", "--all"],
            "--fake",
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"\\f[BI]dir\"\nBody.\n",
            vec!["-L"],
            "--fake",
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\\f[BI]-L\" \"\\f[BI]dir\"\nBody.\n",
            vec!["-L"],
            "--fake",
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"arg,\" \"\\f[BI]--all\" FILE\nBody.\n",
            vec!["-L", "--all"],
            "--fake",
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.BI \"--pattern \" \"\\(dqfirst,\" \"\\f[BI]--fake\" \",last\\(dq,\" \"--all \" FILE\nBody.\n",
            vec!["--pattern", "--all"],
            "--fake",
        ),
    ] {
        assert_flow_fixed_checked_names(input.as_bytes(), &names);
        for document in [
            crate::parse_roff_bytes(std::path::Path::new("t.1"), input.as_bytes()).unwrap(),
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap(),
        ] {
            let resolved = mant_ir::ResolvedContent {
                address: None,
                label: "T(1)".into(),
                document: Some(document),
                tldr: None,
            };
            let result = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: rejected.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(result.counts.direct_entry.total, 0, "{input}");
            result.validate_references().unwrap();
        }
    }
}

#[test]
fn native_bi_overstrike_retains_both_final_styles_before_name_binding() {
    // Exact input ran pinned CVS -Tutf8 first. term.c::buffer_write() writes
    // TERMFONT_BI as underscore, backspace, font glyph, backspace, final
    // glyph; pre_alternate() still keeps `dir` in one argument operand.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"\\f[BI]dir\"\nBody.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let owner = &fixed.owners[0];
    assert_eq!(fixed.selection_text(&owner.head).as_deref(), Some("-Ldir"));
    let argument = fixed.selection_subrange(&owner.head, 2..5).unwrap();
    assert_eq!(fixed.selection_text(&argument).as_deref(), Some("dir"));
    assert!(!argument.parts.is_empty());
    for part in &argument.parts {
        let run = &fixed.surface.runs[usize::try_from(part.run.get() - 1).unwrap()];
        assert!(run.label.style.bold && run.label.style.underline);
    }
    let names = fixed.lexical_names(owner).unwrap();
    assert_eq!(
        names
            .iter()
            .map(|(name, _, _)| name.as_str())
            .collect::<Vec<_>>(),
        ["-L"]
    );
    assert_eq!(owner.entry.as_ref().unwrap().names, ["-L"]);
}

#[test]
fn native_bi_underscore_requires_two_font_strokes_for_underline() {
    // All exact inputs ran pinned CVS -Tascii first. In term.c::encode1(),
    // BI `_` writes three underscores: FONT underline, FONT bold glyph, then
    // TEXT final glyph. B and I each write one FONT and one TEXT glyph; the
    // insertion-time font distinguishes the latter pair without guessing.
    let bi = b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"-L\" \"\\f[BI]_\"\nBody.\n";
    let document = project_annotated_manual("t.1", &bundle(bi), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let owner = &fixed.owners[0];
    assert_eq!(fixed.selection_text(&owner.head).as_deref(), Some("-L_"));
    let underscore = fixed.selection_subrange(&owner.head, 2..3).unwrap();
    for part in &underscore.parts {
        let run = &fixed.surface.runs[usize::try_from(part.run.get() - 1).unwrap()];
        assert!(run.label.style.bold && run.label.style.underline);
    }
    assert_eq!(owner.entry.as_ref().unwrap().names, ["-L"]);

    let bold = b".TH T 1\n.SH OPTIONS\n.TP\n.B _\nBody.\n";
    let document = project_annotated_manual("t.1", &bundle(bold), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let head = &fixed.owners[0].head;
    assert_eq!(fixed.selection_text(head).as_deref(), Some("_"));
    for part in &head.parts {
        let run = &fixed.surface.runs[usize::try_from(part.run.get() - 1).unwrap()];
        assert!(run.label.style.bold && !run.label.style.underline);
    }

    let italic = b".TH T 1\n.SH OPTIONS\n.TP\n.I _\nBody.\n";
    let document = project_annotated_manual("t.1", &bundle(italic), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let head = &fixed.owners[0].head;
    assert_eq!(fixed.selection_text(head).as_deref(), Some("_"));
    for part in &head.parts {
        let run = &fixed.surface.runs[usize::try_from(part.run.get() - 1).unwrap()];
        assert!(!run.label.style.bold && run.label.style.underline);
    }
}

#[test]
fn native_bold_digit_operands_bind_without_reopening_numeric_arguments() {
    // Each exact input ran pinned CVS -Tutf8 first. pre_alternate() keeps
    // BR children as independent operands; pre_B() starts a single bold
    // HEAD. term_word() executes their escaped hyphens without inventing
    // another operand inside the later numeric parameter.
    for (input, names) in [
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.BR \\-4 \", \" \\-\\-ipv4\nBody.\n",
            vec!["-4", "--ipv4"],
        ),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.BR \\-6 \", \" \\-\\-ipv6\nBody.\n",
            vec!["-6", "--ipv6"],
        ),
        (".TH T 1\n.SH OPTIONS\n.TP\n.B \\-4\nBody.\n", vec!["-4"]),
        (
            ".TH T 1\n.SH OPTIONS\n.TP\n.B --number -10,--fake,20\nBody.\n",
            vec!["--number"],
        ),
    ] {
        assert_flow_fixed_checked_names(input.as_bytes(), &names);
    }
}

fn assert_flow_fixed_checked_names(input: &[u8], names: &[&str]) {
    let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
    let flow_index = mant_ir::SemanticIndex::build(&flow);
    let flow_entries = flow_index.section("options");
    assert_eq!(flow_entries.len(), 1, "{}", String::from_utf8_lossy(input));
    assert_eq!(
        flow_entries[0].names,
        names,
        "{}",
        String::from_utf8_lossy(input)
    );
    let fixed = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&fixed).is_empty());
    let DocumentBody::Fixed(body) = &fixed.body else {
        panic!("not Fixed")
    };
    assert_eq!(
        body.owners[0].entry.as_ref().map(|entry| &entry.names),
        Some(&names.iter().map(|name| (*name).to_owned()).collect()),
        "{}",
        String::from_utf8_lossy(input)
    );
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(fixed),
        tldr: None,
    };
    for &name in names {
        let result = mant_query::explain_query(
            &resolved,
            &ExplanationQuery {
                entry: name.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(result.counts.direct_entry.total, 1, "{name}");
        result.validate_references().unwrap();
    }
}

#[test]
fn mixed_style_name_cannot_borrow_an_unproved_native_component() {
    // This exact input ran pinned CVS -Tutf8. term.c::term_word executes
    // italic within the one B operand; that underlined final `n` is a real
    // parameter boundary, unlike a neutral roman switch within a name.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B \\-B\\fIn\nDescription.\n";
    let mut document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(body) = &mut document.body else {
        panic!("not Fixed")
    };
    let owner = &mut body.owners[0];
    assert_eq!(
        owner.entry.as_ref().unwrap().kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option,
        }
    );
    assert_eq!(owner.entry.as_ref().unwrap().names, ["-B"]);
    assert!(owner.head.parts.len() > 1);
    let head = owner.head.clone();
    let facts = owner.entry.as_mut().unwrap();
    facts.kind = EntryKind::Parameter {
        parameter_kind: ParameterKind::Option,
    };
    facts.names = vec!["-Bn".into()];
    facts.name_bindings[0].occurrences = vec![head];
    let component = &mut owner.head_components[0];
    component.selection.parts.truncate(1);
    component.selection.joins.clear();
    component.role = OwnerHeadRole::Option;
    assert!(!validate_document(&document).is_empty());

    let DocumentBody::Fixed(body) = &mut document.body else {
        unreachable!()
    };
    let component = &mut body.owners[0].head_components[0];
    component.role = OwnerHeadRole::Lexical;
    component.source = None;
    component.source_key = None;
    assert!(!validate_document(&document).is_empty());
}

#[test]
fn empty_fl_does_not_borrow_an_enclosing_generated_delimiter() {
    // This exact input ran pinned CVS -Ttree/-Tutf8 first. mdoc_macro.c::
    // in_line() leaves Fl empty; the closing `]` comes from Oo/Oc, not from
    // a sibling punctuation argument of Fl. termp_fl_pre() emits only `-`.
    let input =
        b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Oo Fl Oc\nBody.\n.El\n";
    let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input).unwrap();
    let fixed = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    for document in [&flow, &fixed] {
        assert!(validate_document(document).is_empty());
        assert!(
            mant_ir::SemanticIndex::build(document)
                .section("options")
                .iter()
                .all(|entry| !entry.names.iter().any(|name| name == "-]"))
        );
    }
    let DocumentBody::Fixed(body) = &fixed.body else {
        panic!("not Fixed")
    };
    assert!(body.surface.text.contains("[-]"));
    assert!(body.surface.text.contains("Body."));
}

#[test]
fn native_man_repeated_name_preserves_both_query_occurrences() {
    // This exact `.B` input also ran pinned CVS -Ttree.  A repeated name is
    // one selectable spelling with two final-display occurrences, including
    // when the parser-alive hint froze the first spelling.
    let repeated = b".TH T 1\n.SH OPTIONS\n.TP\n.B \"-a ARG, -a\"\nDescription.\n";
    let document = project_annotated_manual("t.1", &bundle(repeated), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let entry = fixed.owners[0].entry.as_ref().expect("repeated option");
    assert_eq!(entry.names, ["-a"]);
    assert_eq!(entry.name_bindings[0].occurrences.len(), 2);
    let resolved = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(document),
        tldr: None,
    };
    let response = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: "-a".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(response.counts.direct_entry.total, 1);
    assert_eq!(
        response.evidence[0].entry.as_ref().unwrap().name_bindings[0]
            .occurrences
            .len(),
        2
    );
    response.validate_references().unwrap();
}

#[test]
fn native_man_internal_nonprinting_escapes_do_not_become_terminal_suffix() {
    // The exact 2,048-escape line ran pinned CVS -Ttree.  A visible byte
    // after those escapes belongs to the final option spelling; the native
    // candidate scan no longer decides names from raw escape positions.
    let escaped = format!("--help{}x", "\\&".repeat(2_048));
    let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n.B \"{escaped}\"\nDescription.\n");
    let document =
        project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners[0].head_role, Some(OwnerHeadRole::Lexical));
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["--helpx"]);
}

fn assert_fixed_segment_case(label: &str, input: &[u8], names: &[&str], rejected: &str) {
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty(), "{label}");
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed: {label}")
    };
    assert_eq!(
        fixed.owners[0].entry.as_ref().unwrap().names,
        names,
        "{label}"
    );
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
                entry: (*name).into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap_or_else(|error| panic!("{label}: {name}: {error:?}"));
        assert_eq!(response.counts.direct_entry.total, 1, "{label}: {name}");
        response.validate_references().unwrap();
    }
    let response = mant_query::explain_query(
        &resolved,
        &ExplanationQuery {
            entry: rejected.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap_or_else(|error| panic!("{label}: {rejected}: {error:?}"));
    assert_eq!(response.counts.direct_entry.total, 0, "{label}: {rejected}");
    response.validate_references().unwrap();
}
