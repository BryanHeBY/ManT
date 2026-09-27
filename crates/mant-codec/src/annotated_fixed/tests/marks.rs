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
    let mut forged = malformed_anchor(1, 0);
    forged.owner = 1;
    assert!(lower_annotated_document(malformed_marks(vec![forged])).is_err());
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
fn styled_native_reference_projects_one_valid_fixed_manual_link() {
    // Exact input first ran in pinned CVS -Ttree/-Tutf8. The two text
    // operands are executed by man_term.c::pre_alternate() without a space;
    // the following comma is a distinct operand outside the candidate.
    let input = b".TH T 1\n.SH SEE ALSO\n.BR printf (3) ,\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.links.len(), 1);
    assert_eq!(
        fixed.links[0].target,
        Some(mant_ir::LinkTarget::Manual {
            name: "printf".into(),
            manual_section: Some("3".into()),
        })
    );
    assert_eq!(
        fixed.selection_text(&fixed.links[0].label).as_deref(),
        Some("printf(3)")
    );
    let rebuilt: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&rebuilt).is_empty());
}

#[test]
fn two_same_node_sphinx_markers_roundtrip_with_distinct_fixed_keys() {
    // Exact bytes first ran pinned CVS -Ttree/-Tutf8. term.c::term_word()
    // executes two \% markers within one source node; only the immediately
    // preceding name(section) glyph ranges become Manual links.
    let input = b".TH LINKS 1\n.SH DESCRIPTION\na(1) \\%<> and b(2) \\%<>\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.links.len(), 2);
    for (index, (name, section)) in [("a", "1"), ("b", "2")].into_iter().enumerate() {
        let link = &fixed.links[index];
        assert_eq!(link.key.get(), u32::try_from(index).unwrap() + 1);
        assert_eq!(
            link.target,
            Some(mant_ir::LinkTarget::Manual {
                name: name.into(),
                manual_section: Some(section.into()),
            })
        );
        assert_eq!(
            fixed.selection_text(&link.label).as_deref(),
            Some(format!("{name}({section})").as_str())
        );
        assert!(link.source.is_none());
        assert!(link.source_key.is_some());
    }
    assert!(fixed.surface.text.contains("a(1) <> and b(2) <>"));
    let rebuilt: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&rebuilt).is_empty());
}

#[test]
fn unstyled_styled_candidate_disappears_from_fixed_links_without_losing_text() {
    // Exact input first ran pinned CVS -Ttree/-Tutf8. term_word() resets
    // BR's first operand to roman before any name glyph; its weak native
    // candidate must not become a no-href Fixed link or erase visible text.
    let input = b".TH T 1\n.SH D\n.BR \"\\fRprintf\" (3)\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert!(fixed.links.is_empty());
    assert!(fixed.surface.text.contains("printf(3)"));
}

#[test]
fn skipped_compatible_link_does_not_shift_surviving_typed_keys() {
    // Exact input first ran pinned CVS -Tutf8 -O width=78. The first BR
    // operand resets the initial font in term_word(); man_term.c::pre_MR()
    // and the later sourced Sphinx marker retain separate, valid labels.
    let input = b".TH T 1\n.SH D\n.BR \"\\fRbad\" (1)\n.MR good 2\na(1) \\%<>\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.links.len(), 2);
    assert_eq!(fixed.links[0].key.get(), 1);
    assert_eq!(fixed.links[1].key.get(), 2);
    assert_eq!(
        fixed.selection_text(&fixed.links[0].label).as_deref(),
        Some("good(2)")
    );
    assert_eq!(
        fixed.selection_text(&fixed.links[1].label).as_deref(),
        Some("a(1)")
    );
    assert!(fixed.surface.text.contains("bad(1) good(2) a(1) <>"));
    let rebuilt: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&rebuilt).is_empty());
}

#[test]
fn fixed_link_origin_context_follows_native_section_and_owner() {
    // Exact input first ran pinned CVS -Tutf8 -O width=78. man_term.c's
    // SH/TP traversal establishes the context in which each MR macro starts;
    // the later section does not borrow the preceding definition owner.
    let input = b".TH T 1\n.SH TOP\n.MR root 1\n.TP\n.B --foo\n.MR own 2\n.SH NEXT\n.MR next 3\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    assert!(validate_document(&document).is_empty());
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    assert_eq!(fixed.links.len(), 3);
    assert_eq!(fixed.links[0].section, Some(fixed.headings[0].key));
    assert_eq!(fixed.links[0].owner, None);
    assert_eq!(fixed.links[1].section, Some(fixed.headings[0].key));
    assert_eq!(fixed.links[1].owner, Some(fixed.owners[0].key));
    assert_eq!(fixed.links[2].section, Some(fixed.headings[1].key));
    assert_eq!(fixed.links[2].owner, None);
    let rebuilt: mant_ir::Document =
        serde_json::from_value(serde_json::to_value(&document).unwrap()).unwrap();
    assert!(validate_document(&rebuilt).is_empty());
    let mut forged = serde_json::to_value(&document).unwrap();
    forged["body"]["links"][1]["section"] = serde_json::json!(fixed.headings[1].key);
    assert!(serde_json::from_value::<mant_ir::Document>(forged).is_err());
}

#[test]
fn native_links_project_bounded_fixed_reference_inventory_and_section_read() {
    // Exact input first ran pinned CVS -Tutf8 -O width=78. term.c::term_word()
    // leaves the two source-marked labels and <> markers in one section;
    // man_term.c::pre_MR() emits an independent occurrence in the next.
    let input = b".TH T 1\n.SH SEE ALSO\na(1) \\%<> and b(2) \\%<>\n.SH OTHER\n.MR c 3\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("native output did not become Fixed");
    };
    let other = fixed.headings[1].id.clone();
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".to_owned(),
        document: Some(document),
        tldr: None,
    };
    let policy = mant_protocol::ReferenceProjection {
        mode: mant_protocol::ReferenceProjectionMode::All,
        target_types: vec![mant_ir::ReferenceTargetType::Manual],
        offset: 0,
        limit: 2,
    };
    let all = mant_query::build_outline_with_references(
        &query,
        mant_protocol::EntryProjection::None,
        None,
        &policy,
    )
    .unwrap();
    assert_eq!(
        all.references.occurrences,
        mant_protocol::ReferenceCount::Exact { value: 3 }
    );
    assert_eq!(all.references.page.returned, 2);
    assert_eq!(all.references.page.next_offset, Some(2));
    assert!(all.references.records.is_empty());
    assert!(all.references.content_projection.is_none());
    assert_eq!(all.references.fixed_records[0].label_preview, "a(1)");
    assert_eq!(all.references.fixed_records[1].label_preview, "b(2)");
    assert_eq!(all.references.fixed_records[0].origin.link.get(), 1);
    assert_eq!(all.references.fixed_records[1].origin.link.get(), 2);
    assert_eq!(
        all.references.fixed_records[0].source_read.value(),
        "see-also"
    );
    let rebuilt: mant_protocol::ReferenceInventory =
        serde_json::from_value(serde_json::to_value(&all.references).unwrap()).unwrap();
    assert_eq!(rebuilt.fixed_records, all.references.fixed_records);
    let selected = mant_query::build_outline_with_references(
        &query,
        mant_protocol::EntryProjection::None,
        Some(mant_protocol::ContentSelector::id(other)),
        &policy,
    )
    .unwrap();
    assert_eq!(
        selected.references.occurrences,
        mant_protocol::ReferenceCount::Exact { value: 1 }
    );
    assert_eq!(selected.references.fixed_records[0].label_preview, "c(3)");
}

#[test]
fn fixed_reference_inventory_can_select_the_native_entry_owner() {
    // Exact input first ran pinned CVS -Tutf8 -O width=78.
    // man_term.c::pre_TP() establishes the item owner before the body invokes
    // pre_MR(), whose emitted label belongs to that item's native surface.
    let input = b".TH T 1\n.SH SEE ALSO\n.TP\n.B --opt\n.MR printf 3\n";
    let document = project_annotated_manual("t.1", &bundle(input), InputFormat::Man).unwrap();
    let DocumentBody::Fixed(fixed) = &document.body else {
        panic!("expected Fixed");
    };
    let owner = fixed
        .owners
        .iter()
        .find(|owner| owner.entry.is_some())
        .expect("entry");
    let owner_id = owner.id.clone();
    let owner_key = owner.key;
    let query = mant_ir::ResolvedContent {
        address: None,
        label: "T(1)".into(),
        document: Some(document),
        tldr: None,
    };
    let policy = mant_protocol::ReferenceProjection {
        mode: mant_protocol::ReferenceProjectionMode::All,
        target_types: vec![mant_ir::ReferenceTargetType::Manual],
        offset: 0,
        limit: 10,
    };
    let selected = mant_query::build_outline_with_references(
        &query,
        mant_protocol::EntryProjection::All,
        Some(mant_protocol::ContentSelector::id(owner_id)),
        &policy,
    )
    .unwrap();
    assert_eq!(selected.references.fixed_records.len(), 1);
    assert_eq!(
        selected.references.fixed_records[0].origin.owner,
        Some(owner_key)
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
    // visible dash. The collector freezes Fl/Ev/Ic before the AST dies; Ic
    // alone in OPTIONS is a named Term, not independent Command evidence.
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
        EntryKind::Term
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
