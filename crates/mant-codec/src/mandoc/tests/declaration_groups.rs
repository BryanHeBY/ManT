use super::*;

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
    ] = body_closed.sections[0].blocks.as_slice()
    else {
        panic!(
            "expected one definition list: {:#?}",
            body_closed.sections[0].blocks
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
    let lists = split_macro.sections[0]
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
                .map(|item| inline_text(&item.terms[0]))
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
    let lists = repeated_tq.sections[0]
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
                    .map(|term| inline_text(term))
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
