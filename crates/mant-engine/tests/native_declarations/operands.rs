use super::definitions;
use mant_ir::{EntryInlineRoot, EntryOwner, ResolvedContent};
use mant_protocol::{
    ContentSelector, EvidenceBasis, EvidenceClass, ExplanationOptions, ExplanationQuery,
};

#[path = "../support/json_roundtrip.rs"]
mod json_roundtrip;

const MAN_HEADER: &str = ".TH TEST 1 2026-11-17\n.SH DESCRIPTION\n";

fn load(source: &str) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let query = json_roundtrip::json_string_roundtrip(&original);
    assert!(mant_ir::validate_document(query.document.as_ref().unwrap()).is_empty());
    assert_eq!(query.document, original.document);
    query
}

fn explain(query: &ResolvedContent, name: &str) -> mant_protocol::QueryExplanation {
    mant_query::explain_query(
        query,
        &ExplanationQuery {
            entry: name.into(),
            options: ExplanationOptions::default(),
        },
    )
    .unwrap()
}

#[test]
fn tq_keeps_each_heads_executed_argument_ranges_in_source_order() {
    // Fixed CVS man_term.c::pre_alternate() executes a single word for the
    // second BI operand, independently of its font escape. pre_TP/post_TP
    // execute the same HEAD shape for TP and TQ before the final BODY.
    let names = ["-L", "-R", "-S"];
    let forms = ["-Ldir,--fake", "-R", "-S"];
    for font in ['B', 'R'] {
        let heads = [
            format!(r#".BI \-L "dir,\f{font}--fake""#),
            r".B \-R".into(),
            r".B \-S".into(),
        ];
        for order in [&[0][..], &[0, 1], &[1, 0], &[1, 0, 2], &[1, 2, 0]] {
            let mut source = MAN_HEADER.to_owned();
            for (index, &term) in order.iter().enumerate() {
                source.push_str(if index == 0 { ".TP\n" } else { ".TQ\n" });
                source.push_str(&heads[term]);
                source.push('\n');
            }
            source.push_str("UNIQUE_BODY\n");
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            let [item] = items.as_slice() else {
                panic!("{source}")
            };
            let owner = EntryOwner::Definition(item);
            let facts = owner.facts().unwrap();
            assert_eq!(
                facts.names,
                order.iter().map(|&term| names[term]).collect::<Vec<_>>(),
                "{source}"
            );
            assert_eq!(
                owner
                    .forms()
                    .unwrap()
                    .iter()
                    .map(mant_ir::inline_plain_text)
                    .collect::<Vec<_>>(),
                order.iter().map(|&term| forms[term]).collect::<Vec<_>>(),
                "{source}"
            );
            assert!(facts.alias_groups.is_empty());
            assert_eq!(owner.validated_names(), Some(facts.names.as_slice()));
            for (index, binding) in facts.name_bindings.iter().enumerate() {
                assert!(
                    binding
                        .occurrences
                        .iter()
                        .flat_map(|occurrence| &occurrence.parts)
                        .all(|part| part.root == EntryInlineRoot::Term { index })
                );
                let result = explain(&query, &facts.names[binding.name]);
                assert_eq!(result.counts.direct_entry.total, 1, "{source}");
                let hit = &result.evidence[0];
                assert!(
                    hit.bases
                        .iter()
                        .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
                );
                let excerpt = mant_query::select_excerpt(
                    &query,
                    &[ContentSelector::path(hit.outline.path())],
                )
                .unwrap();
                assert!(mant_render::render_excerpt_text(&excerpt).contains("UNIQUE_BODY"));
            }
            assert_eq!(
                explain(&query, "--fake").counts.direct_entry.total,
                0,
                "{source}"
            );
        }
    }
}

#[test]
fn an_inherited_name_grammar_cannot_select_a_proved_argument() {
    for section in ["COMMANDS", "DESCRIPTION"] {
        // The fixed tree has an empty first BI operand and one parameter.
        // pre_alternate() still executes the second operand in that position.
        let source = format!(
            ".TH TEST 1 2026-11-17\n.SH {section}\n.TP\n.B actual\n.TQ\n.BI \"\" \"\\fBargument\"\nBODY\n",
        );
        let query = load(&source);
        let items = definitions(query.document.as_ref().unwrap());
        let [item] = items.as_slice() else {
            panic!("{source}")
        };
        let owner = EntryOwner::Definition(item);
        assert_eq!(owner.validated_names().unwrap(), ["actual"]);
        assert_eq!(
            owner
                .forms()
                .unwrap()
                .iter()
                .map(mant_ir::inline_plain_text)
                .collect::<Vec<_>>(),
            ["actual", "argument"]
        );
        let result = explain(&query, "argument");
        // The second complete authored form is legal evidence. The tested
        // distinction is the absence of a name selector/binding for it.
        assert!(result.evidence.iter().all(|entry| {
            entry
                .bases
                .iter()
                .all(|basis| !matches!(basis, EvidenceBasis::Name { .. }))
        }));
    }
}

#[test]
fn continued_heads_with_equal_source_coordinates_keep_independent_owners() {
    // CVS -Ttree reports both TP/TQ pairs on the PAIRS invocation line.
    let query = load(concat!(
        ".TH TEST 1 2026-11-17\n.SH DESCRIPTION\n",
        ".de PAIR\n.TP\n.BI \\-L \"dir,\\fB--fake\"\n.TQ\n.B \\-R\n\\\\$1\n..\n",
        ".de PAIRS\n.PAIR FIRST_BODY\n.PAIR SECOND_BODY\n..\n.PAIRS\n",
    ));
    let items = definitions(query.document.as_ref().unwrap());
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].source, items[1].source);
    assert_ne!(
        items[0].entry.as_ref().unwrap().id,
        items[1].entry.as_ref().unwrap().id
    );
    let result = explain(&query, "-L");
    assert_eq!(result.counts.direct_entry.total, 2);
    for (hit, body) in result
        .evidence
        .iter()
        .filter(|hit| hit.class == EvidenceClass::DirectEntry)
        .zip(["FIRST_BODY", "SECOND_BODY"])
    {
        let excerpt =
            mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        let visible = mant_render::render_excerpt_text(&excerpt);
        assert!(visible.contains(body), "{visible}");
        assert_eq!(visible.matches("_BODY").count(), 1);
    }
    assert_eq!(explain(&query, "--fake").counts.direct_entry.total, 0);
}
