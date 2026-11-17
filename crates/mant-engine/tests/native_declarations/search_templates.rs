//! Search-form presentation and the original owners of shared context.
use super::support::{
    COMMANDS, HEADER, LIST, assert_search_fixture, check_head, explain, list_groups, load, source,
};
use mant_protocol::ExplanationSupport;

#[test]
fn declared_search_parameters_follow_the_executed_word_boundary() {
    // Pinned mdoc_term.c::termp_sm_pre(), termp_ns_pre() and termp_pf_post()
    // alter term.c::term_word's inter-word blank. Filled Xo treats a new
    // source row as an input event, not a required separator.
    for sign in ['/', '?'] {
        let simple = format!("{sign}RE");
        for (before, section, head, form) in [
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Pf {sign} Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It No {sign} Ns Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It {sign} Ns Ar RE\n"),
                simple.clone(),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(".It No {sign} Ar RE\n"),
                simple.clone(),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n.Ss Search\n",
                format!(".It No {sign} Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".Sm\n.It No {sign} Ar RE\n.Sm\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.Sm off\n.No {sign}\n.Ar RE\n.Sm on\n.Xc\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.No {sign}\\c\n.Ar RE\n.Xc\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.No {sign} Ns\n.Ar RE\n.Xc\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It Pf \\fB\\^{sign}\\fP Ar RE\n"),
                simple.clone(),
            ),
            (
                "",
                ".Sh COMMANDS\n",
                format!(".It No \\z{sign} Ar RE\n"),
                simple,
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(".It No {sign} Ar RE {sign} Ar OFFSET\n"),
                format!("{sign}RE{sign}OFFSET"),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(r#".It No {sign} Ar "\fB語, (n) \(dq--fake\(dq\fP""#) + "\n",
                format!("{sign}語, (n) \"--fake\""),
            ),
            (
                ".Sm off\n",
                ".Sh COMMANDS\n",
                format!(".It Xo\n.No {sign}\n.Ar \"R\\p \\p DROPPED\"\n.Xc\n"),
                format!("{sign}R"),
            ),
        ] {
            assert_search_fixture(&source(before, section, &head), &form, &[], Some(1));
        }
    }
}

#[test]
fn a_search_form_requires_surviving_source_owned_signs_and_argument() {
    // Pinned term.c::encode1() lets later glyphs overstrike a pending \z
    // cell, and term_fill() consumes \p at the retained word boundary.
    for sign in ['/', '?'] {
        let spaced = format!("{sign} RE");
        for (before, head, form) in [
            ("", format!(".It No {sign} Ar RE\n"), spaced.clone()),
            ("", format!(".It Pf {sign} Ar \" RE\"\n"), spaced),
            (
                "",
                format!(".It Xo\n.No {sign}\n.Sm off\n.Ar RE\n.Xc\n.Sm on\n"),
                format!("{sign}{}RE", if sign == '?' { "  " } else { " " }),
            ),
            (
                ".Sm off\n",
                format!(".It No \\z{sign} Ar RE\n"),
                "RE".into(),
            ),
            (
                ".Sm off\n",
                format!(".It No {sign} Ar \\zR Ar E\n"),
                format!("{sign}E"),
            ),
            (
                ".Sm off\n",
                format!(".It No {sign} Ar RE Ns \\z{sign} Ar OFFSET\n"),
                format!("{sign}REOFFSET"),
            ),
            (
                ".Sm off\n",
                format!(".It No {sign} Ar \"A\\p B\"\n"),
                format!("{sign}A\nB"),
            ),
            (
                ".Sm off\n",
                format!(".It Bq No {sign} Ar RE\n"),
                format!("[{sign}RE]"),
            ),
            (
                ".Sm off\n",
                format!(".It Pq No {sign} Ar RE\n"),
                format!("({sign}RE)"),
            ),
        ] {
            assert_search_fixture(&source(before, ".Sh COMMANDS\n", &head), &form, &[], None);
        }
    }
    for (head, form) in [
        (".It Bq No \\z? Ar RE\n", "[RE]"),
        (".It Pq No \\z? Ar RE\n", "(RE)"),
        (".It Bo No \\z? Ar RE Bc\n", "[RE]"),
    ] {
        // The enclosure's own words remain, but the overwritten operator
        // cannot turn the Ar slot into a standalone operand definition.
        assert_search_fixture(
            &source(".Sm off\n", ".Sh COMMANDS\n", head),
            form,
            &[],
            None,
        );
    }
    assert_search_fixture(
        &source(".Sm off\n", ".Sh COMMANDS\n", ".It Bq Ar file\n"),
        "[file]",
        &["file"],
        Some(0),
    );
    for (head, form, group) in [
        (r".It No ? Ar \& Ar RE", "?RE", None),
        (r".It No ? Ar x?y Ns \z? Ar OFFSET", "?x?yOFFSET", None),
        (
            ".It Xo\n.No /\n.Ar R/\n/\n.Ar offset\n.Xc",
            "/R//offset",
            Some(1),
        ),
        (
            ".It Xo\n.No /\n.Ar R/\n\\z/\n.Ar offset\n.Xc",
            "/R/offset",
            None,
        ),
    ] {
        assert_search_fixture(
            &source(".Sm off\n", ".Sh COMMANDS\n", &format!("{head}\n")),
            form,
            &[],
            group,
        );
    }
}

#[test]
fn identical_visible_search_spellings_can_be_real_names() {
    // A styled, separated '?' is an independent command. No/Sy argument
    // words can also spell a literal command without an Ar parameter.
    for (head, mode, form, name) in [
        (".It No \\fB?\\fP Ar RE\n", ".Sm on\n", "? RE", "?"),
        (".It No ? No RE\n", ".Sm off\n", "?RE", "?RE"),
        (".It No ? Sy RE\n", ".Sm off\n", "?RE", "?RE"),
        (".It No ? Ns Ar RE , Cm later\n", "", "?RE, later", "later"),
        (
            ".It No \\z? Ar RE , Cm later\n",
            ".Sm off\n",
            "RE,later",
            "later",
        ),
    ] {
        assert_search_fixture(
            &source(mode, ".Sh COMMANDS\n", head),
            form,
            &[name],
            Some(0),
        );
    }
}

#[test]
fn only_a_proved_template_or_own_body_releases_run_boundaries() {
    let before = ".Sm off\n";
    let section = ".Sh COMMANDS\n";
    for (head, expected) in [
        (".It No ? Ar RE\nTEMPLATE_BODY\n", vec![(1, 3)]),
        (".It No UNKNOWN Ns Ar PARAM\n.It No ? Ar RE\n", vec![]),
        (".It No ? Ar RE\n.It No UNKNOWN Ns Ar PARAM\n", vec![]),
        (
            ".It Cm a\n.It Cm A\nFIRST_BODY\n.It No ? Ar RE\n",
            vec![(0, 2), (3, 5)],
        ),
        (".It No ? Ar RE\n.It No / Ar RE\n", vec![(2, 4)]),
    ] {
        let query = load(&source(before, section, head));
        assert_eq!(
            list_groups(query.document.as_ref().unwrap()),
            [expected],
            "{head}"
        );
        let shared_text = mant_render::render_explanation_text(&explain(&query, "n"));
        assert!(!shared_text.contains("FIRST_BODY") && !shared_text.contains("TEMPLATE_BODY"));
    }
    let nested = format!(
        "{HEADER}{before}{section}{LIST}.It Cm outer\nOUTER_BODY\n{LIST}.It No ? Ar RE\n{COMMANDS}.El\n"
    );
    let query = load(&nested);
    assert_eq!(
        list_groups(query.document.as_ref().unwrap()),
        [vec![], vec![(1, 3)]]
    );
    assert!(mant_render::render_explanation_text(&explain(&query, "n")).contains("GROUP_BODY"));
    let broken = format!(
        "{HEADER}{before}{section}{LIST}.It No ? Ar RE\n.It Cm n\n.Pp\n.It Cm N\nGROUP_BODY\n.El\n"
    );
    let query = load(&broken);
    assert_eq!(list_groups(query.document.as_ref().unwrap()), [vec![]]);
    assert!(explain(&query, "n").supports.is_empty(), "{broken}");
}

#[test]
fn expanded_heads_with_identical_coordinates_keep_distinct_owners() {
    // Each PAIR's three It BLOCKs share their invocation coordinate in
    // pinned -Ttree. The second expansion is a fresh sequence of owners.
    let source = format!(
        "{HEADER}.Sm off\n.Sh COMMANDS\n{LIST}.de PAIR\n.It No ? Ar RE\n.It Cm n\n.It Cm N\n\\\\$1\n..\n.PAIR FIRST_BODY\n.PAIR SECOND_BODY\n.El\n"
    );
    let query = load(&source);
    let document = query.document.as_ref().unwrap();
    let items = super::definitions(document);
    assert_eq!(items.len(), 6);
    for triple in items.as_chunks::<3>().0 {
        assert_eq!(triple[0].source, triple[1].source);
        assert_eq!(triple[1].source, triple[2].source);
        for (item, spelling, names) in [
            (triple[0], "?RE", &[][..]),
            (triple[1], "n", &["n"][..]),
            (triple[2], "N", &["N"][..]),
        ] {
            check_head(item, spelling, names);
        }
    }
    assert_eq!(list_groups(document), [vec![(1, 3), (4, 6)]]);
    let result = explain(&query, "n");
    result.validate_references().unwrap();
    assert_eq!(result.counts.direct_entry.total, 2);
    let supports = result
        .supports
        .iter()
        .filter_map(ExplanationSupport::items)
        .collect::<Vec<_>>();
    assert_eq!(supports.len(), 2);
    for (members, provider) in supports.into_iter().zip([items[2], items[5]]) {
        assert_eq!(members.last().unwrap(), provider);
    }
    let text = mant_render::render_explanation_text(&result);
    for token in ["FIRST_BODY", "SECOND_BODY"] {
        assert_eq!(text.matches(token).count(), 1);
    }
}
