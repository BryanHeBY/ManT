use super::*;

fn malformed_marks(marks: Vec<AnnotatedMark>) -> AnnotatedDocument {
    AnnotatedDocument {
        root_source: 0,
        profile: 0,
        width: 78,
        annotation_degraded: false,
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
    assert_eq!(fixed.headings.len(), 1, "{:?}", document.diagnostics);
    assert_eq!(fixed.owners.len(), 1);
    assert!(!fixed.owners[0].head.parts.is_empty());
    assert!(!fixed.owners[0].direct_body.parts.is_empty());
    assert!(fixed.owners[0].empty_point.is_none());
    assert_eq!(fixed.headings[0].key.get(), 1);
    assert_eq!(fixed.owners[0].key.get(), 1);
    assert_eq!(fixed.owners[0].role, OwnerRole::Definition);
    assert!(fixed.owners[0].lexical_term_witness);
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
