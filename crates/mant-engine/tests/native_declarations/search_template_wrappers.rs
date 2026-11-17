//! Source-role recognition inside mdoc controls and wrappers.
use super::operands::{explain, load};
use super::search_templates::{check_three, list_groups, source};
use mant_ir::{Inline, inline_plain_text};
use mant_protocol::{EntryProjection, EvidenceBasis, OutlineNode, QueryOutline};

const COMMANDS: &str = ".Sh COMMANDS\n.Sm off\n";

fn probe(head: &str) -> String {
    source("", COMMANDS, &format!("{head}\n"))
}

#[test]
fn ns_does_not_disguise_search_arguments_inside_enclosures() {
    // Fixed CVS lint is empty for all 36 cases. mdoc_term.c::termp_ns_pre
    // does not write a word. Its quote handlers emit the enclosure and
    // term.c::encode1 replaces the zero-advance prefix with the first Ar
    // glyph. The remaining spelling comes from Ar in each input.
    for (enclosure, form) in [
        ("Bq", "[RE]"),
        ("Pq", "(RE)"),
        ("Brq", "{RE}"),
        ("Dq", "“RE”"),
        ("Qq", "\"RE\""),
        ("Aq", "⟨RE⟩"),
    ] {
        for sign in ['?', '/'] {
            for (before, after) in [("", ""), ("Ns ", ""), ("", " Ns")] {
                let input = probe(&format!(
                    r".It {before}{enclosure} Pf \z{sign} Ar RE{after}"
                ));
                let content = check_three(&input, form, &[], None);
                let unexplained = explain(&content, "RE");
                assert_eq!(unexplained.counts.direct_entry.total, 0, "{input}");
                assert!(unexplained.supports.is_empty(), "{input}");
            }
        }
    }
}

#[test]
fn nested_control_views_leave_enclosure_and_execution_evidence_intact() {
    for (head, form, group) in [
        (
            ".It Xo\n.Sm off\n.Xo\n.Pf ? Ar RE\n.Xc\n.Xc",
            "?RE",
            Some(1),
        ),
        (
            ".It Xo\n.Sm on\n.Sm\n.Xo\n.No ? Ar RE\n.Xc\n.Sm\n.Xc",
            "?RE",
            Some(1),
        ),
        (
            ".It Xo\n.Sm off\n.Ns Bq Pf \\z? Ar RE Ns\n.Xc",
            "[RE]",
            None,
        ),
        (".It Xo\n.Pf ? Xo Ar RE Xc\n.Xc", "?RE", Some(1)),
        (".It Bo\n.Ns\n.Xo\n.Pf \\z? Ar RE\n.Xc\n.Bc", "[RE]", None),
        (r".It Ns Bq Xo Pf \z? Ar RE Xc Ns", "[RE]", None),
        (r".It Ns Pq Bq Pf \z? Ar RE Ns", "([RE])", None),
        (r".It Ns Bq Pf ? Ar RE Ns", "[?RE]", None),
        (r".It Ns Pq Pf / Ar RE Ns", "(/RE)", None),
        (
            ".It Xo\n.Sm off\n.Xo\n.Pf ? Ar \"A\\p B\"\n.Xc\n.Xc",
            "?A\nB",
            None,
        ),
        (
            r".It Ns Pf ? Ar RE Ns ? Ar OFFSET Ns",
            "?RE?OFFSET",
            Some(1),
        ),
    ] {
        check_three(&probe(head), form, &[], group);
    }
}

#[test]
fn tag_and_font_nodes_keep_their_effects_inside_the_normalized_view() {
    // Pinned mdoc_validate.c::post_tg marks Tg NOPRT and transfers its tag
    // to It HEAD. roff_term.c::roff_term_pre_ft bolds the opening sign, and
    // the Ar handler still uses emphasis for its own word.
    let input = probe(".It Xo\n.ft B\n.Tg search-target\n.Xo\n.Pf ? Ar RE\n.Xc\n.ft R\n.Xc");
    let content = check_three(&input, "?RE", &[], Some(1));
    let document = content.document.as_ref().unwrap();
    let items = super::definitions(document);
    let styles = &items[0].terms[0].content;
    assert!(styles.iter().any(|inline| {
        matches!(inline, Inline::Strong { children } if inline_plain_text(children) == "?")
    }));
    assert!(styles.iter().any(|inline| {
        matches!(inline, Inline::Emphasis { children } if inline_plain_text(children) == "RE")
    }));
    for (index, item) in items.iter().enumerate() {
        let authored = item
            .terms
            .iter()
            .flat_map(|term| &term.content)
            .find_map(|inline| {
                if let Inline::Anchor {
                    id, owner_source, ..
                } = inline
                    && id.as_str() == "search-target"
                {
                    return *owner_source;
                }
                None
            });
        assert_eq!(authored, if index == 0 { item.source } else { None });
    }
    let outline =
        mant_query::build_outline_projection(&content, EntryProjection::All, None).unwrap();
    let outline: QueryOutline =
        serde_json::from_str(&serde_json::to_string(&outline).unwrap()).unwrap();
    let names: Vec<&[String]> = outline
        .nodes
        .last()
        .unwrap()
        .children()
        .iter()
        .filter_map(|node| {
            if let OutlineNode::DocumentEntry { names, .. } = node {
                Some(names.as_slice())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(names, [&[][..], &["n"][..], &["N"][..]]);
}

#[test]
fn real_formatter_words_cannot_make_an_entire_head_a_bare_template() {
    // The leading words, breaks and ESCAPE_IGNORE cells remain execution
    // input. They can coexist with a search expression whose Ar must not
    // become a name just because the operator was overstruck.
    for (head, form) in [
        (r".It \& Pf ? Ar RE", "?RE"),
        (r".It \& Pf \z? Ar RE", "RE"),
        (r".It \& Bq Pf \z? Ar RE", "[RE]"),
        (r#".It No "" Bq Pf \z? Ar RE"#, "[RE]"),
        (r".It \zX Bq Pf \z? Ar RE", "[RE]"),
        (r".It Pf \z? \& Ar RE", "RE"),
        (r".It Ns Bq Pf \z? No \& Ar RE Ns", "[RE]"),
        (r#".It Ns Bq Pf \z? Ar "" No \& Ar RE Ns"#, "[RE]"),
        (r".It Ns Bq Pf \z? Ar \zX No \& Ar RE Ns", "[RE]"),
        (r".It Pf ? Ar RE No \& Ar OFFSET", "?REOFFSET"),
        (".It Xo\n.br\n.Pf \\z? Ar RE\n.Xc", "RE"),
        (r".It No UNKNOWN Ns Ar RE", "UNKNOWNRE"),
        (r".It No \z? \& Ar RE", "RE"),
        (".It Xo\n.Pf \\z? Xo Ar RE Xc\n.Xc", "RE"),
        (".It Xo\n.Pf \\z? Bq Ar RE\n.Xc", "[RE]"),
    ] {
        let input = probe(head);
        let content = check_three(&input, form, &[], None);
        let explained = explain(&content, "RE");
        assert!(
            explained.evidence.iter().all(|hit| {
                hit.bases
                    .iter()
                    .all(|basis| !matches!(basis, EvidenceBasis::Name { .. }))
            }),
            "{input}"
        );
        assert!(explained.supports.is_empty(), "{input}");
    }
}

#[test]
fn standalone_arguments_and_later_declarations_keep_their_names() {
    for (head, form, name) in [
        (r".It Ns Bq Ar file Ns", "[file]", "file"),
        (r".It \& Bq Ar file", "[file]", "file"),
        (r".It Ns Pf \z? Ar RE , Cm later Ns", "RE,later", "later"),
        (
            r".It Ns Bq Pf \z? Ar RE , Cm later Ns",
            "[RE,later]",
            "later",
        ),
        (
            ".It Xo\n.Sm off\n.Bo Pf \\z? Ar RE\n.Bc\n.No , Cm later\n.Xc",
            "[RE],later",
            "later",
        ),
    ] {
        check_three(&probe(head), form, &[name], Some(0));
    }
    let option = source(
        "",
        ".Sh OPTIONS\n.Sm off\n",
        ".It Ns Pf \\z? Ar RE , Fl later Ns\n",
    );
    check_three(&option, "RE,-later", &["-later"], Some(0));
}

#[test]
fn a_template_with_or_without_a_body_keeps_only_its_source_ownership() {
    let head = ".It Xo\n.Sm off\n.Xo\n.Pf ? Ar RE\n.Xc\n.Xc";
    for (input, groups) in [
        (probe(&format!("{head}\nTEMPLATE_BODY")), vec![(1, 3)]),
        (probe(head).replace("GROUP_BODY\n", ""), vec![]),
    ] {
        let content = load(&input);
        assert_eq!(list_groups(content.document.as_ref().unwrap()), [groups]);
        let n = explain(&content, "n");
        let text = mant_render::render_explanation_text(&n);
        assert_eq!(text.contains("GROUP_BODY"), input.contains("GROUP_BODY"));
        assert!(!text.contains("TEMPLATE_BODY"));
        assert!(explain(&content, "?RE").supports.is_empty(), "{input}");
    }
}

#[test]
fn expanded_nested_wrappers_keep_their_argument_and_body_owners() {
    // The pinned tree gives all three It BLOCKs of each invocation the
    // same coordinate. The two source occurrences still own separate text.
    let input = probe(".de SHOW\n.It Xo\n.Sm off\n.Xo\n.Pf ? Ar RE\n.Xc\n.Xc\n.It Cm n\n.It Cm N\n\\\\$1\n..\n.SHOW FIRST_BODY\n.SHOW SECOND_BODY")
        .replace(".It Cm n\n.It Cm N\nGROUP_BODY\n.El\n", ".El\n");
    let content = load(&input);
    let document = content.document.as_ref().unwrap();
    let items = super::definitions(document);
    assert_eq!(items.len(), 6);
    for [template, n, upper_n] in items.as_chunks::<3>().0 {
        assert_eq!(template.source, n.source);
        assert_eq!(n.source, upper_n.source);
        assert!(
            mant_ir::EntryOwner::Definition(template)
                .validated_names()
                .unwrap()
                .is_empty(),
            "{template:?}"
        );
        assert!(n.description.is_empty(), "{n:?}");
        assert!(mant_ir::blocks_have_readable_content(&upper_n.description));
    }
    assert_eq!(list_groups(document), [vec![(1, 3), (4, 6)]]);
    let query = explain(&content, "n");
    assert_eq!(query.counts.direct_entry.total, 2);
    assert_eq!(query.supports.len(), 2);
    let reading = mant_render::render_explanation_text(&query);
    for body in ["FIRST_BODY", "SECOND_BODY"] {
        assert_eq!(reading.matches(body).count(), 1);
    }
}
