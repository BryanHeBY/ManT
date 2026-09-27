use super::*;

#[test]
fn invalid_optional_entry_and_link_facts_leave_native_body_and_siblings() {
    // Exact input ran pinned CVS -Tutf8 first. man_term.c::pre_B owns each
    // printed TP head; term.c::term_word prints the UR label regardless of
    // whether a downstream consumer can safely activate its target.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --good\nGood description.\n.TP\n.B --bad\nBad description.\n.UR https://example.test\nlink label\n.UE\n";
    let mut document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &mut document.body else {
        panic!("not Fixed")
    };
    let original_text = fixed.surface.text.clone();
    assert_eq!(fixed.owners.len(), 2);
    assert_eq!(fixed.links.len(), 1);
    fixed.owners[0].entry.as_mut().unwrap().names[0] = "--forged".to_owned();
    fixed.links[0].target = Some(LinkTarget::External {
        uri: "https://bad host".to_owned(),
    });
    assert!(fixed.validate().is_err());
    let diagnostics = super::isolation::isolate_optional_facts(fixed);
    assert_eq!(diagnostics.len(), 2);
    assert_eq!(fixed.surface.text, original_text);
    assert!(fixed.owners[0].entry.is_none());
    assert_eq!(fixed.owners[1].entry.as_ref().unwrap().names, ["--bad"]);
    assert!(fixed.links[0].target.is_none());
    assert!(fixed.validate().is_ok());
    document.diagnostics.extend(diagnostics);
    assert!(validate_document(&document).is_empty());
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
}

#[test]
fn native_fl_parser_depth_boundary_is_consistent_in_flow_and_fixed() {
    // All four exact inputs ran pinned CVS -Tutf8 before these assertions.
    // The separate vendor safety patch bounds recursive mdoc_macro_call()
    // dispatch at 64 levels. With It occupying one level, the 64th Fl is
    // retained as literal text and diagnosed; it cannot become a proved
    // option occurrence in either projection. A synthetic IR test exercises
    // the independent 64/65 semantic occurrence ceiling.
    for (repeated, count, expected_names) in [
        (false, 63, 63),
        (false, 64, 63),
        (true, 63, 1),
        (true, 64, 1),
    ] {
        let head = (0..count)
            .map(|index| {
                if repeated {
                    "Fl x".to_owned()
                } else {
                    format!("Fl {index}")
                }
            })
            .collect::<Vec<_>>()
            .join(" , ");
        let input = format!(
            ".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It {head}\nBody.\n.El\n"
        );
        let flow = crate::parse_roff_bytes(std::path::Path::new("t.1"), input.as_bytes())
            .expect("Flow keeps the complete native Fl head");
        assert!(validate_document(&flow).is_empty());
        let [mant_ir::Block::DefinitionList { items, .. }] =
            flow.flow().expect("Flow document").sections[0]
                .blocks
                .as_slice()
        else {
            panic!("expected one Fl list");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].entry.as_ref().map_or(0, |entry| entry.names.len()),
            expected_names,
            "Flow: repeated={repeated}, count={count}, names={:?}",
            items[0].entry.as_ref().map(|entry| &entry.names)
        );

        let fixed = project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Mdoc)
            .expect("Fixed keeps the complete native Fl head");
        assert!(validate_document(&fixed).is_empty());
        let DocumentBody::Fixed(body) = &fixed.body else {
            panic!("not Fixed")
        };
        assert!(body.surface.text.contains("Body."));
        assert_eq!(
            body.owners[0]
                .entry
                .as_ref()
                .map_or(0, |entry| entry.names.len()),
            expected_names,
            "Fixed: repeated={repeated}, count={count}"
        );
        assert!(!body.option_component_over_limit(&body.owners[0]));
    }
}

#[test]
fn rejected_native_link_target_keeps_other_fixed_facts_and_reports_gap() {
    // Exact bytes ran pinned CVS -Tutf8 first. man_term.c prints the TP
    // descriptions and UR label independently of this test's injected
    // target-rejection state; no production parser fault hook is installed.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --good\nGood description.\n.TP\n.B --bad\nBad description.\n.UR https://example.test\nlink label\n.UE\n";
    let mut page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    let expected = page.text.clone();
    let link = page.marks.iter_mut().find(|mark| mark.kind == 3).unwrap();
    let source_region = link.owner;
    assert_ne!(source_region, 0);
    link.link_target = None;
    page.coverage.issues.push(AnnotationCoverageIssue {
        producer: AnnotationProducer::Native,
        dimension: AnnotationDimension::Link,
        reason: AnnotationIssueReason::Rejected,
        scope: AnnotationScope::Region(source_region),
        source: None,
    });
    let document = lower_annotated_document(page).unwrap();
    assert!(validate_document(&document).is_empty());
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("annotated.coverage.link.rejected")
            && matches!(
                diagnostic.coverage_scope,
                Some(mant_ir::CoverageScope::Region { .. })
            )
    }));
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.surface.text, expected);
    assert_eq!(fixed.owners.len(), 2);
    assert!(fixed.owners.iter().all(|owner| owner.entry.is_some()));
    assert_eq!(fixed.links.len(), 1);
    assert!(fixed.links[0].target.is_none());
    let outline = mant_query::build_outline(&mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(document),
        tldr: None,
    })
    .unwrap();
    assert!(!outline.semantics_complete);
    let wire = serde_json::to_value(&outline).unwrap();
    assert_eq!(
        wire["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|value| value["code"] == "annotated.coverage.link.rejected")
            .count(),
        1
    );
}

#[test]
fn ambiguous_compatible_link_keeps_text_and_other_clickable_references() {
    // Exact input first ran pinned CVS -Tutf8 -O width=78. The first
    // pre_alternate() phrase and later pre_MR() phrase remain distinct.
    // Inject only the optional final-glyph proof failure after native render.
    let input = b".TH T 1\n.SH SEE ALSO\n.BR printf (3)\n.MR good 2\n";
    let mut page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    let original = page.text.clone();
    let failed = page.marks.iter_mut().find(|mark| mark.kind == 3).unwrap();
    assert_ne!(failed.owner, 0);
    let region = failed.owner;
    failed.link_target = None;
    page.coverage.issues.push(AnnotationCoverageIssue {
        producer: AnnotationProducer::Native,
        dimension: AnnotationDimension::Link,
        reason: AnnotationIssueReason::AmbiguousSurvival,
        scope: AnnotationScope::Region(region),
        source: None,
    });
    let document = lower_annotated_document(page).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed");
    };
    assert_eq!(fixed.surface.text, original);
    assert_eq!(fixed.links.len(), 1);
    assert_eq!(
        fixed.selection_text(&fixed.links[0].label).as_deref(),
        Some("good(2)")
    );
    assert!(document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code.as_deref() == Some("annotated.coverage.link.ambiguous-survival")
            && matches!(
                diagnostic.coverage_scope,
                Some(mant_ir::CoverageScope::Region { .. })
            )
    }));
}

#[test]
fn semantic_downgrade_does_not_admit_damaged_native_display() {
    // Exact input ran pinned CVS -Tutf8 first. The mutations below are
    // defensive transfer faults, not authored roff expectations.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --good\nDescription.\n";
    let page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    let mut truncated = page.clone();
    truncated.text.pop();
    truncated.marks[0].key = u32::MAX;
    assert!(lower_annotated_document(truncated).is_err());
    let mut out_of_range = page;
    out_of_range.runs[0].byte_count = u64::MAX;
    out_of_range.marks[0].key = u32::MAX;
    assert!(lower_annotated_document(out_of_range).is_err());
    let mut unknown_style = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle(input), InputFormat::Man)
        .unwrap();
    unknown_style.runs[0].label.style |= 2;
    assert!(lower_annotated_document(unknown_style).is_err());
}

#[test]
fn zero_glyph_native_components_do_not_block_fixed_body_or_invent_names() {
    // Each exact input ran pinned CVS -Tutf8 first. man_term.c::pre_B and
    // pre_alternate() retain macro instances, but term.c::term_word() emits
    // no glyph for `\&`; mdoc_term.c likewise keeps an empty Ev/Cm label.
    for (input, format) in [
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.B \\&\nDescription.\n".as_slice(),
            InputFormat::Man,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.SB \\&\nDescription.\n".as_slice(),
            InputFormat::Man,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BR \\& \\&\nDescription.\n".as_slice(),
            InputFormat::Man,
        ),
        (
            b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\" \"\"\nDescription.\n".as_slice(),
            InputFormat::Man,
        ),
        (
            b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Ev \\&\nDescription.\n.El\n"
                .as_slice(),
            InputFormat::Mdoc,
        ),
        (
            b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Cm \\&\nDescription.\n.El\n"
                .as_slice(),
            InputFormat::Mdoc,
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), format).unwrap();
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        assert!(fixed.surface.text.contains("Description."));
        assert_eq!(fixed.owners.len(), 1);
        assert!(fixed.owners[0].head_components.is_empty());
        assert!(fixed.owners[0].entry.is_none());
        assert!(!document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code.as_deref() == Some("annotated.internal-annotation-rejected")
        }));
    }
}

#[test]
fn empty_initial_bi_operands_do_not_hide_later_bold_name() {
    // Exact inputs ran pinned CVS -Ttree/-Tutf8/-Thtml first. The third
    // pre_alternate() operand is bold; the italic-only counterexample is not
    // a declaration even though both share the BI macro instance.
    let good = b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\" \"\" \"--help \" FILE\nDescription.\n";
    let document = project_annotated_manual("t.1", &bundle(good), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners[0].entry.as_ref().unwrap().names, ["--help"]);
    // Every visible pre_alternate() child now retains its operand interval;
    // only the final bold spelling becomes a checked name.
    assert_eq!(fixed.owners[0].head_components.len(), 2);
    assert!(
        fixed.owners[0]
            .head_components
            .iter()
            .all(|component| !component.selection.parts.is_empty())
    );

    let italic_only = b".TH T 1\n.SH OPTIONS\n.TP\n.BI \"\" \"--fake\"\nDescription.\n";
    let document = project_annotated_manual("t.1", &bundle(italic_only), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let term = fixed.owners[0].entry.as_ref().unwrap();
    assert_eq!(term.kind, EntryKind::Term);
    assert!(term.names.is_empty());
    assert!(fixed.surface.text.contains("--fake"));
}

#[test]
fn italic_ip_candidate_keeps_body_without_a_false_option_name() {
    // This exact IP input ran pinned CVS -Tutf8 first. pre_IP prints the
    // first operand, and term.c::term_word makes every name glyph underlined.
    // A broad native candidate is not final declaration evidence.
    let input = b".TH T 1\n.SH OPTIONS\n.IP \"\\fI--italic\\fR\" 4\nDescription.\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert!(fixed.surface.text.contains("--italic"));
    assert!(fixed.surface.text.contains("Description."));
    assert!(
        fixed.owners[0]
            .entry
            .as_ref()
            .is_none_or(|entry| entry.names.is_empty())
    );
    let result = mant_query::explain_query(
        &mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        },
        &ExplanationQuery {
            entry: "--italic".into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(result.counts.direct_entry.total, 0);
}

#[test]
fn zero_width_fl_operand_keeps_its_generated_visible_dash() {
    // Exact input ran pinned CVS -Ttree/-Tutf8 first. Unlike Ev/Cm,
    // mdoc_term.c::termp_fl_pre emits a dash before the zero-width operand;
    // that surviving glyph still belongs to the native Fl instance.
    let input = b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl \\&\nDescription.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    assert_eq!(fixed.owners[0].head_components.len(), 1);
    assert_eq!(
        fixed
            .selection_text(&fixed.owners[0].head_components[0].selection)
            .as_deref(),
        Some("-")
    );
    assert!(fixed.surface.text.contains("Description."));
}

#[test]
fn repeated_mdoc_fl_keeps_one_name_and_both_native_occurrences() {
    // Exact input ran pinned CVS -Tutf8 first. mdoc_macro.c::blk_full()
    // retains both Fl macro instances in the It HEAD; mdoc_term.c::
    // termp_fl_pre() prints a distinct dash for each invocation.
    let input = b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Fl a\nDescription.\n.El\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("not Fixed")
    };
    let entry = fixed.owners[0].entry.as_ref().expect("native Fl entry");
    assert_eq!(entry.names, ["-a"]);
    assert_eq!(entry.name_bindings.len(), 1);
    assert_eq!(entry.name_bindings[0].occurrences.len(), 2);
}

#[test]
fn unnameable_fl_instance_does_not_erase_sibling_native_names() {
    // Each exact input ran pinned CVS -Tutf8 first. mdoc_term.c::termp_fl_pre()
    // prints the dash even for a zero-width operand, while mdoc_macro.c::
    // blk_full() retains each Fl invocation as a distinct It HEAD child.
    for (input, expected) in [
        (
            b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl \\& , Fl a\nDescription.\n.El\n".as_slice(),
            vec!["-a"],
        ),
        (
            b".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Fl \\& , Fl b\nDescription.\n.El\n".as_slice(),
            vec!["-a", "-b"],
        ),
    ] {
        let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Mdoc).unwrap();
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        let entry = fixed.owners[0].entry.as_ref().expect("valid sibling Fl names");
        assert_eq!(entry.names, expected);
        assert_eq!(entry.name_bindings.len(), expected.len());
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for name in expected {
            let response = mant_query::explain_query(
                &resolved,
                &ExplanationQuery {
                    entry: name.into(),
                    options: ExplanationOptions::default(),
                },
            )
            .unwrap();
            assert_eq!(response.counts.direct_entry.total, 1, "{name}");
            response.validate_references().unwrap();
        }
    }
}

#[test]
fn mixed_mdoc_head_components_keep_only_fl_name_evidence() {
    // Both exact inputs ran pinned CVS -Tutf8 first. mdoc_macro.c::blk_full()
    // retains Fl/Cm/Fl in one It HEAD, while mdoc_term.c::termp_fl_pre()
    // executes the two independent Fl instances around the Cm literal.
    for middle in ["mode", "--fake"] {
        let input = format!(
            ".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Cm {middle} , Fl b\nDescription.\n.El\n"
        );
        let document =
            project_annotated_manual("t.1", &bundle(input.as_bytes()), InputFormat::Mdoc).unwrap();
        assert!(validate_document(&document).is_empty());
        let DocumentBody::Fixed(fixed) = &document.body else {
            panic!("not Fixed")
        };
        let entry = fixed.owners[0].entry.as_ref().expect("native Fl entry");
        assert_eq!(entry.names, ["-a", "-b"]);
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for (name, expected) in [("-a", 1), ("-b", 1), ("--fake", 0)] {
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
                "{middle}: {name}"
            );
            response.validate_references().unwrap();
        }
    }
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
