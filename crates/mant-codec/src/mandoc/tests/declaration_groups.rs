use super::*;

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
