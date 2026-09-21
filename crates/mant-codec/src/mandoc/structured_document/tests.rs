use super::*;
use mant_ir::{
    EntryKind, EntryNameEvidence, EntryOwner, ParameterKind, ResolvedContent,
    visit::{self, Visit},
};
use mant_protocol::{EntryProjection, EvidenceBasis, ExplanationOptions, ExplanationQuery};

#[test]
fn cross_wrapper_terms_reach_real_entry_facts_without_body_borrowing() {
    // The same source was run through the pinned reference before this
    // assertion. `man_macro.c::blk_imp` binds TQ to the preceding TP body;
    // `man_term.c::pre_TP` executes BR/B wrappers in formatter order.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "c03.1",
            br#".TH C03 1
.SH OPTIONS
.TP
.BR --output , " -o=" FILE
.TQ
.B -O
Write file.
.TP
.B --empty
.TP
.B --same
Body A.
.TP
.B --same
Body B.
"#
            .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("c03.1", &bundle, InputFormat::Man)
        .expect("native structure lowers to semantic IR");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("first structured block is a definition list")
    };
    assert_eq!(items.len(), 1, "each man TP owns its native list block");
    assert_eq!(items[0].terms.len(), 2);
    let facts = items[0].entry.as_ref().expect("option facts discovered");
    assert_eq!(
        facts.kind,
        EntryKind::Parameter {
            parameter_kind: ParameterKind::Option
        }
    );
    assert_eq!(facts.names, ["--output", "-o", "-O"]);
    assert_eq!(facts.forms.len(), 3);
    assert_eq!(
        facts
            .forms
            .iter()
            .map(|form| {
                EntryOwner::Definition(&items[0])
                    .form(form)
                    .map(|form| mant_ir::inline_plain_text(&form))
                    .unwrap()
            })
            .collect::<Vec<_>>(),
        ["--output", "-o=FILE", "-O"]
    );
    assert!(
        facts
            .name_bindings
            .iter()
            .all(|binding| binding.evidence == EntryNameEvidence::Lexical)
    );
    assert_eq!(items[0].description.len(), 1);

    let definition_items = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => Some(items.as_slice()),
            _ => None,
        })
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(definition_items.len(), 4);
    assert!(definition_items[1].description.is_empty());
    assert_eq!(definition_items[2].description.len(), 1);
    assert_eq!(definition_items[3].description.len(), 1);
    assert_ne!(definition_items[2].source, definition_items[3].source);

    assert_real_query_consumers(document);
}

fn assert_real_query_consumers(document: Document) {
    let query = ResolvedContent {
        label: "c03(1)".to_owned(),
        address: None,
        document: Some(document),
        tldr: None,
    };
    let outline = mant_query::build_outline_projection(&query, EntryProjection::All, None)
        .expect("the actual outline consumer accepts native EntryFacts");
    let outline_json = serde_json::to_string(&outline).unwrap();
    assert!(outline_json.contains("--output"));
    assert!(outline_json.contains("-o=FILE"));

    let explanation = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "-o".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .expect("the actual explanation consumer accepts native EntryFacts");
    assert_eq!(explanation.total, 1);
    assert_eq!(explanation.returned, 1);
    let evidence = &explanation.evidence[0];
    let entry = evidence.entry.as_ref().expect("semantic facts retained");
    assert_eq!(entry.names, ["--output", "-o", "-O"]);
    assert_eq!(
        entry
            .forms
            .iter()
            .map(|form| mant_ir::inline_plain_text(form))
            .collect::<Vec<_>>(),
        ["--output", "-o=FILE", "-O"]
    );
    assert!(evidence.bases.iter().any(|basis| matches!(
        basis,
        EvidenceBasis::Name { matches }
            if matches.iter().any(|matched| matched.name == "-o")
    )));
    assert_eq!(entry.name_bindings.len(), 3);

    let empty = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "--empty".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .expect("the empty native owner remains independently queryable");
    assert_eq!(empty.total, 1);
    let duplicate = mant_query::explain_query(
        &query,
        &ExplanationQuery {
            entry: "--same".to_owned(),
            options: ExplanationOptions::default(),
        },
    )
    .expect("duplicate native owners remain independently queryable");
    assert_eq!(duplicate.total, 2);
}

#[test]
fn mdoc_multiple_labels_bind_native_markup_to_exact_forms() {
    // The exact source was run through the pinned reference before this
    // assertion. `mdoc_term.c::termp_it_pre` preserves both Fl occurrences
    // around the authored separator.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "labels.1",
            b".Dd September 21, 2026\n.Dt LABELS 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl a , Fl b\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("labels.1", &bundle, InputFormat::Mdoc)
        .expect("native label evidence lowers to semantic IR");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("tag list retained")
    };
    let facts = items[0].entry.as_ref().expect("option facts discovered");
    assert_eq!(facts.names, ["-a", "-b"]);
    assert_eq!(facts.forms.len(), 2);
    assert_eq!(
        facts
            .forms
            .iter()
            .map(|form| {
                EntryOwner::Definition(&items[0])
                    .form(form)
                    .map(|form| mant_ir::inline_plain_text(&form))
                    .unwrap()
            })
            .collect::<Vec<_>>(),
        ["-a", "-b"]
    );
    assert_eq!(facts.name_bindings.len(), 2);
    assert!(
        facts.name_bindings.iter().all(|binding| {
            binding.evidence == EntryNameEvidence::NativeMarkup && binding.occurrences.len() == 1
        }),
        "{facts:#?}"
    );
}

#[test]
fn native_targets_use_normalized_ids_and_only_authored_aliases() {
    // These exact mdoc and man sources were run through the pinned reference
    // first. `tag_put(TAG_MANUAL)` retains Mixed.Target as an authored target;
    // Ev and TP contribute formatter-generated targets.
    let mut mdoc = SourceBundle::new();
    mdoc.insert(
        "target-identities.1",
        b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh ENVIRONMENT\n.Bl -tag\n.It Ev DEMO_HOME\nBODY\n.El\n.Sh OPTIONS\n.Bl -tag\n.Tg Mixed.Target\n.It Fl mixed\nBODY\n.El\n"
            .to_vec(),
    )
    .unwrap();
    let document = project_native_manual("target-identities.1", &mdoc, InputFormat::Mdoc)
        .expect("native targets lower to valid document identities");
    let anchors = anchor_identities(&document);
    assert!(
        anchors.contains(&("demo-home".to_owned(), Vec::new())),
        "{anchors:#?}"
    );
    assert!(
        anchors.contains(&("mixed-target".to_owned(), vec!["Mixed.Target".to_owned()])),
        "{anchors:#?}"
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.invalid-identity")),
        "{:#?}",
        document.diagnostics
    );

    let mut man = SourceBundle::new();
    man.insert(
        "target-identity.1",
        b".TH X 1\n.SH OPTIONS\n.TP\n--set=KEY\nBODY\n".to_vec(),
    )
    .unwrap();
    let document = project_native_manual("target-identity.1", &man, InputFormat::Man)
        .expect("man target lowers to a normalized generated identity");
    let anchors = anchor_identities(&document);
    assert!(
        anchors.contains(&("set-key".to_owned(), Vec::new())),
        "{anchors:#?}"
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.invalid-identity")),
        "{:#?}",
        document.diagnostics
    );
}

#[test]
fn authored_target_collision_keeps_alias_and_unique_internal_id() {
    // This exact source was run through the pinned reference first.
    // `tag.c::tag_put` retains both the Sh target and the manual Tg target;
    // their distinct authored fragment spellings must remain addressable.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "target-collision.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh Mixed Target\n.Bl -tag\n.Tg Mixed.Target\n.It Fl mixed\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("target-collision.1", &bundle, InputFormat::Mdoc)
        .expect("authored target collision receives a unique normalized identity");
    assert_eq!(document.sections[0].id.as_str(), "mixed-target");
    let anchors = anchor_identities(&document);
    assert!(
        anchors.contains(&("mixed-target-2".to_owned(), vec!["Mixed.Target".to_owned()])),
        "{anchors:#?}"
    );
    assert!(
        document
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.code.as_deref() != Some("ir.invalid-identity")),
        "{:#?}",
        document.diagnostics
    );
}

fn anchor_identities(document: &Document) -> Vec<(String, Vec<String>)> {
    #[derive(Default)]
    struct Collector {
        identities: Vec<(String, Vec<String>)>,
    }

    impl<'ir> Visit<'ir> for Collector {
        fn visit_inline(&mut self, inline: &'ir Inline) {
            if let Inline::Anchor {
                id,
                fragment_aliases,
                ..
            } = inline
            {
                self.identities.push((
                    id.to_string(),
                    fragment_aliases.iter().map(ToString::to_string).collect(),
                ));
            } else {
                visit::walk_inline(self, inline);
            }
        }
    }

    let mut collector = Collector::default();
    collector.visit_document(document);
    collector.identities
}

#[test]
fn man_marker_source_evidence_controls_list_kind_and_sequence_merging() {
    // The exact input was run through the pinned reference first. Pinned
    // `man_term.c::pre_IP` renders each authored marker; only the named roff
    // bullet is structural evidence, while consecutive ordinals form one run.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "markers.1",
            b".TH MARKERS 1\n.SH STEPS\n.IP \\(bu\nBullet.\n.IP \"*\"\nStar.\n.IP 3.\nThird.\n.IP 4.\nFourth.\n.IP 9.\nNinth.\n.IP \"(1)\"\nParenthesized.\n.IP 1.\nDot.\n.IP 2)\nParen.\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("markers.1", &bundle, InputFormat::Man)
        .expect("man marker evidence lowers without text guessing");
    assert!(matches!(
        &document.sections[0].blocks[0],
        Block::List {
            kind: ListKind::Bullet,
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[1],
        Block::DefinitionList { items, .. } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[2],
        Block::List {
            kind: ListKind::Ordered { start: Some(3) },
            items,
            ..
        } if items.len() == 2
    ));
    assert!(matches!(
        &document.sections[0].blocks[3],
        Block::List {
            kind: ListKind::Ordered { start: Some(9) },
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[4],
        Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[5],
        Block::List {
            kind: ListKind::Ordered { start: Some(1) },
            items,
            ..
        } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[6],
        Block::List {
            kind: ListKind::Ordered { start: Some(2) },
            items,
            ..
        } if items.len() == 1
    ));
}

#[test]
fn tp_width_tq_boundary_and_rs_continuation_survive_lowering() {
    // Each fragment was run through the pinned reference first. The native
    // path follows `pre_TP` for width/tag separation, keeps TQ independent
    // from marker lists, and treats the sibling RS produced by `blk_exp` as
    // content of the preceding ordered item.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "boundaries.1",
            b".TH X 1\n.SH D\n.TP 4\n\\(bu\nBULLET\n.TP\n\\(bu\n.TQ\nALIAS\nBODY\n.IP 1.\nONE\n.RS\ncontinuation\n.RE\n.IP 2.\nTWO\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("boundaries.1", &bundle, InputFormat::Man)
        .expect("native marker boundaries lower to semantic IR");
    let blocks = &document.sections[0].blocks;
    assert!(matches!(
        &blocks[0],
        Block::List {
            kind: ListKind::Bullet,
            items,
            ..
        } if items.len() == 2
    ));
    assert!(matches!(
        &blocks[1],
        Block::DefinitionList { items, .. }
            if mant_ir::inline_plain_text(&items[0].terms[0]) == "ALIAS"
    ));
    let Block::List {
        kind: ListKind::Ordered { start: Some(1) },
        items,
        ..
    } = &blocks[2]
    else {
        panic!("RS-separated ordinals remain one ordered list: {blocks:#?}")
    };
    assert_eq!(items.len(), 2);
    let first_text = items[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } => Some(mant_ir::inline_plain_text(children)),
            _ => None,
        })
        .collect::<String>();
    assert!(first_text.contains("ONE"), "{first_text:?}");
    assert!(first_text.contains("continuation"), "{first_text:?}");
}

#[test]
fn nested_rs_list_keeps_outer_ordinal_state_and_item_ownership() {
    // The exact source was run through the pinned reference first. The RS
    // subtree owns its local bullet state, while the outer 1./2. sequence and
    // the nested list's attachment to item 1 remain intact.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "nested-rs.1",
            b".TH X 1\n.SH D\n.IP 1.\nONE\n.RS\n.IP \\(bu\nNESTED\n.RE\n.IP 2.\nTWO\n".to_vec(),
        )
        .unwrap();
    let document = project_native_manual("nested-rs.1", &bundle, InputFormat::Man)
        .expect("nested RS list lowers with scoped marker state");
    let Block::List {
        kind: ListKind::Ordered { start: Some(1) },
        items,
        ..
    } = &document.sections[0].blocks[0]
    else {
        panic!("outer ordered list retained: {:#?}", document.sections)
    };
    assert_eq!(items.len(), 2);
    assert!(items[0].blocks.iter().any(|block| matches!(
        block,
        Block::List {
            kind: ListKind::Bullet,
            items,
            ..
        } if items.len() == 1
    )));
}

#[test]
fn independent_mdoc_lists_keep_their_container_boundaries() {
    // This exact source was run through the pinned reference first. The two
    // Bl/El containers remain independent even though their visible bullets
    // are adjacent in terminal output.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "separate-lists.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh D\n.Bl -bullet\n.It\nONE\n.El\n.Bl -bullet -compact\n.It\nTWO\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("separate-lists.1", &bundle, InputFormat::Mdoc)
        .expect("authored mdoc containers remain distinct");
    assert_eq!(document.sections[0].blocks.len(), 2, "{document:#?}");
    assert!(matches!(
        &document.sections[0].blocks[0],
        Block::List { compact: false, items, .. } if items.len() == 1
    ));
    assert!(matches!(
        &document.sections[0].blocks[1],
        Block::List { compact: true, items, .. } if items.len() == 1
    ));
}

#[test]
fn one_native_form_keeps_hint_evidence_without_widening_names() {
    // The exact source was run through the pinned reference first. Native Fl
    // evidence on both sides of an authored slash remains two occurrences
    // inside one form.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "hint-runs.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh D\n.Bl -tag\n.It Fl a No / Fl b\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("hint-runs.1", &bundle, InputFormat::Mdoc)
        .expect("multiple hint runs reach EntryFacts");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("tag list retained")
    };
    let facts = items[0].entry.as_ref().expect("option facts discovered");
    assert_eq!(facts.names, ["-a"]);
    assert_eq!(facts.forms.len(), 1);
    assert!(facts.name_bindings.iter().all(|binding| {
        binding.evidence == EntryNameEvidence::NativeMarkup && binding.occurrences.len() == 1
    }));
}

#[test]
fn literal_separator_definition_keeps_its_term() {
    // The exact source was run through the pinned reference first; the pipe
    // is a literal label rather than a declaration separator without sides.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("operator.1", b".TH X 1\n.SH D\n.TP\n|\nBODY\n".to_vec())
        .unwrap();
    let document = project_native_manual("operator.1", &bundle, InputFormat::Man)
        .expect("literal operator lowers as a definition term");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("definition list retained")
    };
    assert_eq!(mant_ir::inline_plain_text(&items[0].terms[0]), "|");
}

#[test]
fn shared_declaration_grammar_distinguishes_arguments_from_aliases() {
    // This exact source was run through the pinned UTF-8/78 reference first.
    // `man_term.c::pre_alternate` can put a comma in its own styled atom, but
    // that formatter boundary does not decide whether it separates aliases.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "form-grammar.1",
            b".TH X 1\n.SH OPTIONS\n.TP\n.BR --set=KEY , VALUE\nBODY\n.TP\n.B --set=KEY,VALUE\nSECOND\n.TP\n.BR -a , --all\nTHIRD\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("form-grammar.1", &bundle, InputFormat::Man)
        .expect("native structural forms use the shared declaration grammar");
    let items = document.sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList { items, .. } => items.first(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(items.len(), 3, "{document:#?}");
    let forms = items
        .iter()
        .map(|item| {
            let facts = item.entry.as_ref().expect("option facts");
            facts
                .forms
                .iter()
                .map(|form| {
                    EntryOwner::Definition(item)
                        .form(form)
                        .map(|form| mant_ir::inline_plain_text(&form))
                        .expect("form resolves")
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        forms,
        [
            vec!["--set=KEY,VALUE".to_owned()],
            vec!["--set=KEY,VALUE".to_owned()],
            vec!["-a".to_owned(), "--all".to_owned()],
        ]
    );
}

#[test]
fn mdoc_native_kinds_nesting_and_targets_survive_ir_lowering() {
    // Reference output and the corresponding
    // `mdoc_term.c::termp_bl_pre/termp_it_pre` path were inspected before
    // adding this assertion; `tag.c::tag_move_id` owns Tg attachment.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "c03.1",
            br".Dd September 21, 2026
.Dt C03 1
.Os
.Sh OPTIONS
.Bl -tag -compact
.It Fl o Ar file
Write file.
.Tg item-target
.It Fl q
.Bl -enum
.It
Nested one.
.It
Nested two.
.El
.El
"
            .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("c03.1", &bundle, InputFormat::Mdoc)
        .expect("mdoc structure lowers to semantic IR");
    let Block::DefinitionList { items, compact, .. } = &document.sections[0].blocks[0] else {
        panic!("tag list retained")
    };
    assert!(*compact);
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].entry.as_ref().unwrap().names, ["-o"]);
    assert_eq!(items[1].entry.as_ref().unwrap().names, ["-q"]);
    let Block::List {
        kind,
        items: nested,
        ..
    } = &items[1].description[0]
    else {
        panic!("nested enum retained inside its owning item")
    };
    assert_eq!(*kind, ListKind::Ordered { start: Some(1) });
    assert_eq!(nested.len(), 2);
    assert!(
        items[1].terms[0].iter().any(
            |inline| matches!(inline, Inline::Anchor { id, .. } if id.as_str() == "item-target")
        )
    );
}

#[test]
fn nested_empty_mdoc_item_retains_its_explicit_target() {
    // This exact source was run through the pinned reference first.
    // `mdoc_validate.c::post_tg` keeps Tg as the target owner because the
    // following bullet item has no body child.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "nested-target.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It Fl outer\n.Bl -bullet\n.Tg nested-target\n.It\n.El\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = project_native_manual("nested-target.1", &bundle, InputFormat::Mdoc)
        .expect("nested empty target lowers through the native path");
    let Block::DefinitionList { items, .. } = &document.sections[0].blocks[0] else {
        panic!("outer definition list retained")
    };
    let Block::List { items: nested, .. } = &items[0].description[0] else {
        panic!("nested bullet list retained")
    };
    let Block::Paragraph { children, .. } = &nested[0].blocks[0] else {
        panic!("target-only empty item receives an anchor paragraph")
    };
    assert!(matches!(
        children.as_slice(),
        [Inline::Anchor { id, .. }] if id.as_str() == "nested-target"
    ));
}
