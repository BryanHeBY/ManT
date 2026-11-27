//! Shared writer receipts must distinguish equal displayed declaration heads.
use super::{
    Block, EvidenceBasis, EvidenceClass, QueryExplanation, inline_plain_text, run_text_input,
};

fn public_output(source: &str, flag: &str) -> Vec<u8> {
    let output = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            "json",
            "--compact",
            flag,
        ],
        source,
    );
    assert!(output.status.success(), "{flag}");
    output.stdout
}

fn explain(source: &str, query: &str) -> QueryExplanation {
    serde_json::from_slice(&public_output(source, &format!("--explain={query}"))).unwrap()
}

#[test]
fn declaration_operator_rewind_distinguishes_separator_from_author_ink_in_both_orders() {
    // CVS term.c::term_word/encode1: the two displayed heads have the
    // same ink, but the first writes into a macro separator and the other
    // replaces an earlier No word. Keep the name and body proof for both.
    let form = "-a user=port / -v";
    for (source, correct, overwritten) in [
        (
            include_str!("ar/same-form-original-order.1"),
            "2/e1",
            "2/e2",
        ),
        (include_str!("ar/same-form-reverse-order.1"), "2/e2", "2/e1"),
    ] {
        let reply = explain(source, form);
        assert_eq!(reply.counts.direct_entry.total, 1);
        let direct = reply
            .evidence
            .iter()
            .find(|e| e.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(direct.outline.path().to_string(), correct);
        assert!(direct.bases.iter().any(|basis| matches!(basis, EvidenceBasis::Form { matches } if matches.iter().any(|entry| entry.text == form && !entry.occurrences.is_empty()))));
        assert_eq!(direct.entry.as_ref().unwrap().names, ["-a", "-v"]);
        for name in ["-a", "-v"] {
            let reply = explain(source, name);
            assert_eq!(reply.counts.direct_entry.total, 2, "{name}");
            for path in [correct, overwritten] {
                let evidence = reply
                    .evidence
                    .iter()
                    .find(|e| {
                        e.class == EvidenceClass::DirectEntry
                            && e.outline.path().to_string() == path
                    })
                    .unwrap();
                assert!(evidence.bases.iter().any(|basis| matches!(basis, EvidenceBasis::Name { matches } if matches.iter().any(|entry| entry.name == name && !entry.occurrences.is_empty()))));
            }
        }
        for (path, expected) in [
            (correct, "Correct owner."),
            (overwritten, "Overwritten owner."),
        ] {
            let reply: serde_json::Value =
                serde_json::from_slice(&public_output(source, &format!("--node={path}"))).unwrap();
            let block: Block =
                serde_json::from_value(reply["selections"][0]["entry"].clone()).unwrap();
            let Block::DefinitionList { items, .. } = block else {
                panic!("wrong node {path}")
            };
            assert_eq!(inline_plain_text(&items[0].terms[0]), form);
            let [Block::Paragraph { children, .. }] = items[0].description.as_slice() else {
                panic!("missing owner body {path}")
            };
            assert_eq!(inline_plain_text(children), expected);
        }
    }
}

#[test]
fn accepted_field_operators_bind_two_typed_fields_without_stealing_a_sibling_word() {
    // CVS mdoc_macro.c::in_line and mdoc_term.c::termp_fl/Ns expose the
    // two typed fields and separately executed operator. Fixed CVS ASCII
    // and UTF-8 show identical display for the borrowed-sibling control.
    for (source, form, count) in [
        (include_str!("ar/user-at-host.1"), "-a user@host / -v", 1),
        (include_str!("ar/user-at-host-no.1"), "-a user@host / -v", 1),
        (
            include_str!("ar/user-at-host-one.1"),
            "-a user@host / -v",
            1,
        ),
        (include_str!("ar/user-at-host-direct.1"), "-a user@host", 1),
        (include_str!("ar/encoded-0.1"), "-a user:port / -v", 1),
        (include_str!("ar/encoded-1.1"), "-v / -a user:port", 1),
        (include_str!("ar/encoded-2.1"), "-v / -a user@host", 1),
        (include_str!("ar/encoded-3.1"), "-a user@host / -v", 0),
        (include_str!("ar/encoded-4.1"), "-a user@host / -v", 1),
        (include_str!("ar/encoded-5.1"), "-a user:port / -v", 1),
    ] {
        let reply = explain(source, form);
        assert_eq!(reply.counts.direct_entry.total, count, "{source}");
        for name in ["-a", "-v"].into_iter().filter(|name| form.contains(name)) {
            assert_eq!(
                explain(source, name).counts.direct_entry.total,
                1,
                "{source}"
            );
        }
    }
}

#[test]
fn author_ink_loss_is_independent_of_grammar_role_and_font_copies() {
    // The CVS actual write targets a font-copy slot before the Sy payload;
    // the cover belongs to the canonical writer, not to the prior Ms.
    let styled = include_str!("ar/styled-writer.1");
    let exact = explain(styled, "-a user=port / -v");
    assert_eq!(exact.counts.direct_entry.total, 1);
    assert!(exact.evidence.iter().any(|e| {
        e.class == EvidenceClass::DirectEntry && e.outline.path().to_string() == "2/e1"
    }));

    // Native relative motion can revise previously written ink inside one
    // word without borrowing a sibling. It still is not a complete form.
    let revised = include_str!("ar/within-one-word.1");
    assert_eq!(explain(revised, "-a X / -v").counts.direct_entry.total, 0);
    for name in ["-a", "-v"] {
        assert_eq!(explain(revised, name).counts.direct_entry.total, 1);
    }
}
