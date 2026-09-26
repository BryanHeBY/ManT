use super::*;

#[test]
fn flow_option_fact_roundtrip_keeps_binding_without_serializing_native_hint() {
    // Exact input ran pinned CVS -Tutf8 -Owidth=78 before this assertion.
    // man_term.c::pre_TP prints the real HEAD after layout operands and
    // pre_B selects the initial font; term.c::term_word prints its glyphs.
    let input = b".TH T 1\n.SH OPTIONS\n.TP\n.B --save\nBody.\n";
    let mut document = parse_manual_bytes(std::path::Path::new("save.1"), input)
        .expect("parse complete Flow definition");
    document.diagnostics.push(mant_ir::Diagnostic {
        level: mant_ir::DiagnosticLevel::Unsupported,
        impact: mant_ir::DiagnosticImpact::SemanticCoverage,
        code: Some("test.producer-coverage-gap".to_owned()),
        message: "producer reported incomplete optional evidence".to_owned(),
        source: None,
        source_key: None,
        coverage_scope: Some(mant_ir::CoverageScope::Document),
    });
    assert!(!mant_ir::semantics_complete(&document.diagnostics));
    let before = mant_ir::SemanticIndex::build(&document);
    assert_eq!(before.section("options")[0].names, ["--save"]);
    let wire = serde_json::to_value(&document).expect("serialize Flow document");
    let decoded: mant_ir::Document =
        serde_json::from_value(wire).expect("decode validated Flow facts");
    assert_eq!(mant_ir::SemanticIndex::build(&decoded), before);
    assert_eq!(decoded.diagnostics, document.diagnostics);
    assert!(!mant_ir::semantics_complete(&decoded.diagnostics));
    assert_eq!(
        visible_document_text(&decoded),
        visible_document_text(&document)
    );

    // The serialized category is a producer fact, not a replayable proof of
    // the original B macro. A structurally valid Term edit can retain the
    // exact name; read-time checking must not claim native authentication.
    let mut recategorized = decoded.clone();
    let [Block::DefinitionList { items, .. }] =
        recategorized.flow_mut().expect("Flow document").sections[0]
            .blocks
            .as_mut_slice()
    else {
        panic!("expected one definition list");
    };
    items[0].entry.as_mut().expect("retained facts").kind = mant_ir::EntryKind::Term;
    let recategorized_index = mant_ir::SemanticIndex::build(&recategorized);
    let recategorized_entry = &recategorized_index.section("options")[0];
    assert_eq!(recategorized_entry.kind, mant_ir::EntryKind::Term);
    assert_eq!(recategorized_entry.names, ["--save"]);

    // A parse-local NativeHeadEvidence witness is absent after the round
    // trip. Read-time validation checks the retained name against original
    // content instead of fabricating another macro role from its font.
    let mut stale = decoded;
    let [Block::DefinitionList { items, .. }] = stale.flow_mut().expect("Flow document").sections
        [0]
    .blocks
    .as_mut_slice() else {
        panic!("expected one definition list");
    };
    items[0].entry.as_mut().expect("retained facts").names[0] = "--other".to_owned();
    assert!(
        mant_ir::SemanticIndex::build(&stale).section("options")[0]
            .names
            .is_empty()
    );
    assert_eq!(
        visible_document_text(&stale),
        visible_document_text(&document)
    );
}

#[test]
fn flow_complete_head_keeps_negative_and_quoted_arguments_out_of_names() {
    // Exact inputs ran the pinned CVS -Tutf8 reference first. man_term.c::
    // pre_TP/pre_IP retain one label; term.c::term_word executes the quoted
    // and styled argument text without making a second declaration macro.
    for (label, head) in [
        ("same-bold-negative", ".TP\n.B --number -10,--fake,20"),
        ("same-bold-unit", ".TP\n.B --number -10%,--fake,20"),
        (
            "quoted-argument",
            ".IP \"\\fB--pattern\\fR \\(dqone, --fake,two\\(dq\" 4",
        ),
    ] {
        let input = format!(".TH T 1\n.SH OPTIONS\n{head}\nDescription.\n");
        let document =
            parse_manual_bytes(std::path::Path::new("argument-head.1"), input.as_bytes())
                .expect("parse complete man declaration head");
        let [Block::DefinitionList { items, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!("expected a definition list: {label}");
        };
        assert_eq!(
            items[0].entry.as_ref().expect("entry").names,
            [if label == "quoted-argument" {
                "--pattern"
            } else {
                "--number"
            }],
            "{label}"
        );
    }
}

#[test]
#[expect(clippy::too_many_lines, reason = "one cross-operand boundary matrix")]
fn alternating_operands_do_not_promote_inline_font_changes_to_names() {
    // Each exact input below ran the pinned CVS -Tutf8 reference first.
    // man_term.c::pre_alternate() concatenates child operands, whereas
    // term.c::term_word() executes \fB/\fI inside *one* child. A font run is
    // therefore not an independent declaration boundary; a later operand
    // may start one only after the quoted/bracketed parameter has closed.
    let cases = [
        (
            "quoted-operands",
            ".BI \"--pattern \" \"\\(dqfirst,\" \"--fake\" \",last\\(dq,\" \"--all \" FILE",
            vec!["--pattern", "--all"],
        ),
        (
            "bracketed-operands",
            ".BI \"-L\" \"(first,\" \"--fake\" \",last),\" \"--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "inline-font-in-parameter",
            ".BI \"-L\" \"first,\\fB--fake\\fI,last,\" \"--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "pipe-in-parameter",
            ".BI \"-L\" \"first|\\fB--fake\\fI|last|\" \"--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "negative-in-parameter",
            ".BI \"-L\" \"-10,\\fB--fake\\fI,20,\" \"--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "slash-in-parameter",
            ".BI \"-L\" \"first/\\fB--fake\\fI/last,\" \"--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "missing-terminal-separator",
            ".BI \"-L\" \"first,\\fB--fake\\fI,last\" \"--all \" FILE",
            vec!["-L"],
        ),
        (
            "independent-bold-operand",
            ".BI \"-L\" \"first,\" \"--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "styled-comma-inside-argument",
            ".BI \"-L\" \"first\\fB, --fake\\fI,last,\" \"--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "next-operand-starts-with-comma",
            ".BI \"-o \" FILE \", --all \" FILE",
            vec!["-o", "--all"],
        ),
        (
            "roman-argument-ends-in-comma",
            ".BR \"--opt \" \"arg,\" \"--all \" FILE",
            vec!["--opt", "--all"],
        ),
        (
            "next-operand-starts-with-blank",
            ".BI \"-L\" \"arg,\" \" --all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "next-operand-starts-with-nbsp",
            ".BI \"-L\" \"arg,\" \"\\~--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "italic-comma-bold-name",
            ".BI \"-L\" \"arg\" \"\\fI,\\fB--all \" FILE",
            vec!["-L", "--all"],
        ),
        (
            "bold-comma-italic-parameter",
            ".BI \"-L\" \"arg\" \",\\fI--fake\"",
            vec!["-L"],
        ),
        (
            "roman-argument-internal-fake",
            ".BR \"--opt \" \"arg, --fake,last\" \"--all \" FILE",
            vec!["--opt"],
        ),
    ];
    for (label, head, expected) in cases {
        let input = format!(".TH T 1\n.SH OPTIONS\n.TP\n{head}\nBody.\n");
        let document =
            parse_manual_bytes(std::path::Path::new("alternating-head.1"), input.as_bytes())
                .expect("parse alternating man declaration head");
        let [Block::DefinitionList { items, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!("expected one definition list: {label}");
        };
        assert_eq!(
            items[0].entry.as_ref().expect("entry").names,
            expected,
            "{label}"
        );
        assert!(
            visible_document_text(&document).contains("Body."),
            "{label}"
        );
        assert!(
            !format!("{document:?}").contains("mant-native-operand-"),
            "private operand marks escaped into IR: {label}"
        );
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for name in ["--fake", "--all"] {
            let result = mant_query::explain_query(
                &resolved,
                &mant_protocol::ExplanationQuery {
                    entry: name.into(),
                    options: mant_protocol::ExplanationOptions::default(),
                },
            )
            .expect("query completed Flow entry");
            assert_eq!(
                result.counts.direct_entry.total,
                u32::from(name == "--all" && expected.contains(&"--all")),
                "{label}: {name}"
            );
        }
    }
}

#[test]
fn ip_parameter_font_changes_do_not_create_native_declarations() {
    // Both exact labels ran pinned CVS -Tutf8 first. man_term.c::pre_IP
    // executes one label child; term.c::term_word changes its font inside
    // that child, so comma/pipe and later bold text cannot prove another
    // independent declaration operand.
    for head in [
        ".IP \"\\fB-L\\fI first\\fR, --fake\\fI,last\" 4",
        ".IP \"\\fB-L\\fI first\\fR| --fake\\fI|last\" 4",
    ] {
        let input = format!(".TH T 1\n.SH OPTIONS\n{head}\nBody.\n");
        let document =
            parse_manual_bytes(std::path::Path::new("ip-font-argument.1"), input.as_bytes())
                .expect("parse styled IP argument");
        let [Block::DefinitionList { items, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!("expected one definition list: {head}");
        };
        assert_eq!(
            items[0].entry.as_ref().expect("option entry").names,
            ["-L"],
            "{head}"
        );
        assert!(visible_document_text(&document).contains("Body."), "{head}");
    }
}

#[test]
fn each_native_fl_proves_its_own_numeric_name_without_licensing_bold_text() {
    // Both inputs ran the pinned CVS reference first. mdoc_macro.c::in_line()
    // keeps the second Fl as its own macro; mdoc_term.c::termp_fl_pre() emits
    // that macro's dash. Sy -6 looks identical in bold but is not an Fl.
    for (label, head, expected) in [
        ("second-fl", "Fl 4 , Fl 6 Ar file", vec!["-4", "-6"]),
        (
            "interleaved-fl",
            "Fl 4 , Fl 6 , Fl alpha",
            vec!["-4", "-6", "-alpha"],
        ),
        ("bold-negative", "Fl 4 , Sy -6", vec!["-4"]),
    ] {
        let input = format!(
            ".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It {head}\nProtocol options.\n.El\n"
        );
        let document = parse_manual_bytes(std::path::Path::new("numeric-fl.1"), input.as_bytes())
            .expect("parse mdoc numeric option head");
        let [Block::DefinitionList { items, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!("expected one definition list: {label}");
        };
        assert_eq!(
            items[0].entry.as_ref().expect("option identity").names,
            expected,
            "{label}"
        );
        assert!(
            !format!("{document:?}").contains("mant-native-option-"),
            "parse-local Fl witnesses must not enter public IR: {label}"
        );
    }
}

#[test]
fn native_fl_name_limit_does_not_publish_a_partial_head() {
    // The exact 63- and 64-Fl inputs ran pinned CVS -Tutf8 first. Its
    // mdoc_macro.c::in_line() executes every instance, but our bounded parser
    // patch stops recursive inline dispatch at 64 levels and retains the
    // rejected 64th Fl as literal text with a diagnostic. This integration
    // test checks the safe limit; the independent semantic 64/65 ceiling is
    // exercised with constructed, fully checked components in mant-ir.
    for count in [63, 64] {
        let head = (0..count)
            .map(|index| format!("Fl {index}"))
            .collect::<Vec<_>>()
            .join(" , ");
        let input = format!(
            ".Dd September 25, 2026\n.Dt T 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It {head}\nBody.\n.El\n"
        );
        let document = parse_manual_bytes(std::path::Path::new("bounded-fl.1"), input.as_bytes())
            .expect("parse complete native Fl head");
        let [Block::DefinitionList { items, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!("expected one definition list with {count} Fl instances");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].entry.as_ref().map_or(0, |entry| entry.names.len()),
            63,
            "{count} Fl instances"
        );
        assert!(visible_document_text(&document).contains("Body."));
        if count == 64 {
            assert!(inline_text(document.content(), &items[0].terms[0]).contains("Fl 63"));
            let report = libmandoc_rs::Parser::default()
                .parse_bytes("bounded-fl.1", input.as_bytes())
                .expect("bounded parser retains literal suffix");
            assert!(!report.diagnostics.is_empty());
        }
    }
}

#[test]
fn generic_section_uses_checked_native_numeric_head_evidence_for_option_role() {
    // Each exact input ran pinned CVS -Tutf8 first. man_term.c::pre_TP
    // executes a detached HEAD; pre_B/pre_alternate set the initial bold
    // font, and term.c::term_word executes the final dash and digits. A bare
    // TP label has no such style/operand proof, while -10 inside an existing
    // --number argument is not a new declaration.
    for (head, body, expected) in [
        (
            ".BR \\-4 \", \" \\-\\-ipv4",
            "Search only for IPv4 sockets.",
            vec!["-4", "--ipv4"],
        ),
        (".B \\-6", "Search only for IPv6 sockets.", vec!["-6"]),
        (
            ".B --number -10,--fake,20",
            "Description.",
            vec!["--number"],
        ),
    ] {
        let input = format!(".TH T 1\n.SH DESCRIPTION\n.TP\n{head}\n{body}\n");
        let document =
            parse_manual_bytes(std::path::Path::new("numeric-generic.1"), input.as_bytes())
                .expect("parse generic section numeric head");
        let [Block::DefinitionList { items, .. }] = document.flow().expect("Flow fixture").sections
            [0]
        .blocks
        .as_slice() else {
            panic!("expected one definition list: {head}");
        };
        let entry = items[0].entry.as_ref().expect("definition entry");
        assert_eq!(entry.names, expected, "{head}");
        assert_eq!(
            entry.kind,
            mant_ir::EntryKind::Parameter {
                parameter_kind: mant_ir::ParameterKind::Option,
            },
            "{head}"
        );
        let resolved = mant_ir::ResolvedContent {
            address: None,
            label: "T(1)".into(),
            document: Some(document),
            tldr: None,
        };
        for name in expected {
            let explanation = mant_query::explain_query(
                &resolved,
                &mant_protocol::ExplanationQuery {
                    entry: name.into(),
                    options: mant_protocol::ExplanationOptions::default(),
                },
            )
            .expect("query checked native numeric option");
            assert_eq!(explanation.counts.direct_entry.total, 1, "{head}: {name}");
        }
    }
    let bare = b".TH T 1\n.SH DESCRIPTION\n.TP\n\\-4\nBare numeric label.\n";
    let document = parse_manual_bytes(std::path::Path::new("numeric-bare.1"), bare)
        .expect("parse bare numeric label");
    let [Block::DefinitionList { items, .. }] = document.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!("expected one definition list");
    };
    assert_ne!(
        items[0].entry.as_ref().expect("definition entry").kind,
        mant_ir::EntryKind::Parameter {
            parameter_kind: mant_ir::ParameterKind::Option,
        }
    );
}

#[test]
fn declaration_witnesses_close_on_unclassified_bodies_and_survive_split_macro_lists() {
    let body_closed = parse_manual_bytes(
        std::path::Path::new("declaration-body-closure.1"),
        b".TH PROBE 1\n.SH OPTIONS\n.TP\n.B \"This is explanatory prose.\"\nOWN DESCRIPTION.\n.TP\n.B --alpha\n.TP\n.B --beta\nSHARED DESCRIPTION.\n",
    )
    .expect("parse an unclassified definition with its own body");
    let [
        Block::DefinitionList {
            items,
            declaration_groups,
            ..
        },
    ] = body_closed.flow().expect("Flow fixture").sections[0]
        .blocks
        .as_slice()
    else {
        panic!(
            "expected one definition list: {:#?}",
            body_closed.flow().expect("Flow fixture").sections[0].blocks
        );
    };
    assert_eq!(items.len(), 3);
    assert_eq!(
        declaration_groups,
        &[mant_ir::DeclarationGroup {
            start_item: 1,
            end_item: 3,
        }],
        "the prose owner's own body closes its physical run"
    );

    let split_macro = parse_manual_bytes(
        std::path::Path::new("declaration-split-macro.1"),
        b".TH PROBE 1\n.SH OPTIONS\n.de XX\n.TP\n.B --alpha\n.TP\n.B --beta\nSHARED DESCRIPTION.\n..\n.XX\n.PP\nSEPARATOR.\n.XX\n",
    )
    .expect("parse two macro-expanded declaration lists");
    let lists = split_macro.flow().expect("Flow fixture").sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } => Some((items, declaration_groups)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2, "the paragraph splits physical lists");
    for (items, declaration_groups) in lists {
        assert_eq!(
            items
                .iter()
                .map(|item| inline_text(split_macro.content(), &item.terms[0]))
                .collect::<Vec<_>>(),
            ["--alpha", "--beta"]
        );
        assert_eq!(
            declaration_groups,
            &[mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }],
            "each complete expansion retains its own shared description"
        );
    }
    assert!(
        !format!("{split_macro:?}").contains("mant-native-definition-owner"),
        "parse-local owner markers must be removed before public IR escapes"
    );
}

#[test]
fn declaration_witnesses_keep_tq_groups_across_repeated_macro_expansions() {
    let repeated_tq = parse_manual_bytes(
        std::path::Path::new("declaration-repeated-tq-macro.1"),
        b".TH PROBE 1\n.SH OPTIONS\n.de XX\n.TP\n.B -a\n.TQ\n.B --alpha\n.TP\n.B --beta\nSHARED BODY.\n.PP\nSEPARATOR.\n.TP\n.B -a\n.TQ\n.B --alpha\n.TP\n.B --beta\nSHARED BODY.\n..\n.XX\n",
    )
    .expect("parse repeated TQ macro expansion");
    let lists = repeated_tq.flow().expect("Flow fixture").sections[0]
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::DefinitionList {
                items,
                declaration_groups,
                ..
            } => Some((items, declaration_groups)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(lists.len(), 2, "the macro contains two physical runs");
    for (items, declaration_groups) in lists {
        assert_eq!(
            items
                .iter()
                .map(|item| item
                    .terms
                    .iter()
                    .map(|term| inline_text(repeated_tq.content(), term))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>(),
            vec![
                vec!["-a".to_owned(), "--alpha".to_owned()],
                vec!["--beta".to_owned()],
            ]
        );
        assert_eq!(
            declaration_groups,
            &[mant_ir::DeclarationGroup {
                start_item: 0,
                end_item: 2,
            }],
            "each repeated TQ expansion retains its own shared declaration body"
        );
    }
    assert_eq!(
        visible_document_text(&repeated_tq)
            .matches("SHARED BODY.")
            .count(),
        2,
        "the trailing description remains with each repeated declaration run"
    );
}
