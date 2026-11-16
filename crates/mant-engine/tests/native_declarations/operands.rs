use super::definitions;
use mant_ir::{EntryInlineRoot, EntryOwner, ResolvedContent};
use mant_protocol::{
    ContentSelector, EvidenceBasis, EvidenceClass, ExplanationOptions, ExplanationQuery,
};

#[path = "../support/json_roundtrip.rs"]
mod json_roundtrip;

const MAN_HEADER: &str = ".TH TEST 1 2026-11-17\n.SH DESCRIPTION\n";

pub(super) fn load(source: &str) -> ResolvedContent {
    let original = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let query = json_roundtrip::json_string_roundtrip(&original);
    assert_eq!(
        mant_ir::validate_document(query.document.as_ref().unwrap()),
        []
    );
    assert_eq!(query.document, original.document);
    query
}

pub(super) fn explain(query: &ResolvedContent, name: &str) -> mant_protocol::QueryExplanation {
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
            assert_eq!(facts.alias_groups.len(), 0);
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

#[test]
fn optional_man_keys_keep_arguments_out_of_names_across_fonts_and_sections() {
    // Pinned CVS pre_OP() executes up to two term_word() calls for OP,
    // including an empty first operand. The second is always the argument.
    let cases = [
        (".OP", "[]", None),
        (".OP color WHEN", "[color WHEN]", None),
        (r".OP \-\-color", "[--color]", Some("--color")),
        (r".OP \-\-color WHEN", "[--color WHEN]", Some("--color")),
        (r".OP \fI\-\-color WHEN", "[--color WHEN]", Some("--color")),
        (r".OP \z\-\-color WHEN", "[-color WHEN]", Some("-color")),
        (
            r#".OP \-\-color "\fB--fake, --phantom""#,
            "[--color --fake, --phantom]",
            Some("--color"),
        ),
        (
            r#".OP \-\-color "\fR--fake""#,
            "[--color --fake]",
            Some("--color"),
        ),
        (
            r#".OP \-\-color "\fB--fake" --ignored"#,
            "[--color --fake]",
            Some("--color"),
        ),
        (r#".OP "" "\fB--fake""#, "[ --fake]", None),
        (
            r#".OP "--color,--fake" WHEN"#,
            "[--color,--fake WHEN]",
            None,
        ),
        (r#".OP "\z\&" --fake"#, "[ -fake]", None),
        (
            r#".OP "\-\-color \-\-fake" WHEN"#,
            "[--color --fake WHEN]",
            None,
        ),
        (
            r#".OP "\-\-color\p \-\-fake" WHEN"#,
            "[--color\n--fake WHEN]",
            None,
        ),
    ];
    for section in ["DESCRIPTION", "OPTIONS"] {
        for (head, expected_form, name) in cases {
            let source = format!(".TH TEST 1 2026-11-17\n.SH {section}\n.TP\n{head}\nOP_BODY\n");
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            let [item] = items.as_slice() else {
                panic!("{source}")
            };
            let owner = EntryOwner::Definition(item);
            let facts = owner.facts().unwrap();
            assert_eq!(
                owner.validated_names().unwrap(),
                name.as_slice(),
                "{source}"
            );
            assert_eq!(
                owner
                    .forms()
                    .unwrap()
                    .iter()
                    .map(mant_ir::inline_plain_text)
                    .collect::<Vec<_>>(),
                [expected_form],
                "{source}"
            );
            let kind = if name.is_some() {
                mant_ir::EntryKind::Parameter {
                    parameter_kind: mant_ir::ParameterKind::Option,
                }
            } else {
                mant_ir::EntryKind::Term
            };
            assert_eq!(facts.kind, kind, "{source}");
            for fake in ["--fake", "-fake", "--phantom", "--ignored", "WHEN"] {
                assert_eq!(
                    explain(&query, fake).counts.direct_entry.total,
                    0,
                    "{source}"
                );
            }
            if let Some(name) = name {
                let result = explain(&query, name);
                assert_eq!(result.counts.direct_entry.total, 1, "{source}");
                let excerpt = mant_query::select_excerpt(
                    &query,
                    &[ContentSelector::path(result.evidence[0].outline.path())],
                )
                .unwrap();
                assert!(mant_render::render_excerpt_text(&excerpt).contains("OP_BODY"));
            }
        }
    }
}

#[test]
fn optional_man_keys_compose_with_tq_but_ordinary_op_use_never_admits_an_owner() {
    let optional = r#".OP \-\-color "\fB--fake""#;
    for section in ["DESCRIPTION", "OPTIONS"] {
        for (heads, names) in [
            ([optional, r".B \-R"], ["--color", "-R"]),
            ([r".B \-R", optional], ["-R", "--color"]),
        ] {
            let source = format!(
                ".TH TEST 1 2026-11-17\n.SH {section}\n.TP\n{}\n.TQ\n{}\nOP_BODY\n",
                heads[0], heads[1],
            );
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            let owner = EntryOwner::Definition(items[0]);
            assert_eq!(items.len(), 1);
            assert_eq!(owner.validated_names().unwrap(), names);
            assert_eq!(explain(&query, "--fake").counts.direct_entry.total, 0);
        }
    }
    for content in [
        ".SH SYNOPSIS\n.SY test\n.OP \\-\\-color WHEN\n.YS\n",
        ".SH DESCRIPTION\n.PP\n.OP \\-\\-color WHEN\n",
        ".SH DESCRIPTION\n.TP\n.B OWNER\n.OP \\-\\-color WHEN\n",
        ".SH DESCRIPTION\n.HP\n.OP \\-\\-color WHEN\n",
    ] {
        let query = load(&format!(".TH TEST 1 2026-11-17\n{content}"));
        assert_eq!(explain(&query, "--color").counts.direct_entry.total, 0);
        assert!(mant_render::render_query_text(&query).contains("[--color WHEN]"));
    }
}

#[test]
fn a_tq_optional_key_stays_addressable_after_an_unrelated_or_unrecognized_first_term() {
    // Pinned tree: the TQ owns an OP key and value independently of the
    // preceding TP's label macro. Each label has its own operand scope.
    for section in ["DESCRIPTION", "OPTIONS"] {
        for (first, names) in [
            (".OP color WHEN", &["--color"][..]),
            (".B normal", &["normal", "--color"][..]),
        ] {
            let source = format!(
                ".TH TEST 1 2026-11-17\n.SH {section}\n.TP\n{first}\n.TQ\n.OP \\-\\-color WHEN\nBODY\n"
            );
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            let [item] = items.as_slice() else {
                panic!("{source}")
            };
            let owner = EntryOwner::Definition(item);
            assert_eq!(owner.validated_names().unwrap(), names, "{source}");
            let facts = owner.facts().unwrap();
            let optional = facts.name_bindings.last().unwrap();
            assert!(
                optional
                    .occurrences
                    .iter()
                    .flat_map(|form| &form.parts)
                    .all(|part| { part.root == EntryInlineRoot::Term { index: 1 } })
            );
            assert_eq!(
                explain(&query, "WHEN").counts.direct_entry.total,
                0,
                "{source}"
            );
            assert_eq!(
                explain(&query, "--color").counts.direct_entry.total,
                1,
                "{source}"
            );
        }
    }
}

#[test]
fn a_qualified_optional_key_does_not_erase_a_later_independent_term_name() {
    // Both TP/TQ orders remain executed tagged paragraphs in the pinned
    // tree. The OP key and the ordinary bold label name their own forms.
    for section in ["DESCRIPTION", "OPTIONS"] {
        let source = format!(
            ".TH TEST 1 2026-11-17\n.SH {section}\n.TP\n.OP \\-\\-color WHEN\n.TQ\n.B normal\nBODY\n"
        );
        let query = load(&source);
        let items = definitions(query.document.as_ref().unwrap());
        let [item] = items.as_slice() else {
            panic!("{source}")
        };
        let owner = EntryOwner::Definition(item);
        assert_eq!(owner.facts().unwrap().kind, mant_ir::EntryKind::Term);
        assert_eq!(
            owner.validated_names().unwrap(),
            ["--color", "normal"],
            "{source}"
        );
        for name in ["--color", "normal"] {
            let result = explain(&query, name);
            assert_eq!(result.counts.direct_entry.total, 1);
            assert!(
                result.evidence[0]
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
            );
        }
    }
}
