//! Native entry forms, aliases and ownership across query consumers.

#[test]
fn invocation_forms_and_aliases_agree_across_query_consumers() {
    for (section, head, form, aliases, rejected) in [
        (
            "OPTIONS",
            ".BI \"-n, \" -NUM",
            "-n, -NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "OPTIONS",
            ".BI \"-n| \" -NUM",
            "-n| -NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "OPTIONS",
            ".B \"-n, \\fI-NUM\"",
            "-n, -NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "OPTIONS",
            ".B \"-h/--help\"",
            "-h/--help",
            vec!["-h", "--help"],
            "--missing",
        ),
        (
            "OPTIONS",
            ".BI \"-q \" -COUNT",
            "-q -COUNT",
            vec!["-q"],
            "-COUNT",
        ),
        (
            "COMMANDS",
            ".B \"create, duplicate\"",
            "create, duplicate",
            vec!["create", "duplicate"],
            "create, duplicate",
        ),
        (
            "ENVIRONMENT",
            ".B MODE=red,blue",
            "MODE=red,blue",
            vec!["MODE"],
            "blue",
        ),
    ] {
        assert_invocation_consumers(
            &format!(".TH NAMES 1\n.SH {section}\n.TP\n{head}\nOWNEDPAYLOAD.\n"),
            form,
            &aliases,
            rejected,
            3,
        );
    }
}

fn assert_invocation_consumers(
    source: &str,
    form: &str,
    aliases: &[&str],
    rejected: &str,
    source_line: u32,
) {
    use mant_protocol::{
        EntryProjection, OutlineNode, SearchCase, SearchQuery, SearchScope, SearchSyntax,
    };
    let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
    let index = mant_ir::SemanticIndex::build(query.document.as_ref().unwrap());
    let indexed = &index.section(&query.document.as_ref().unwrap().sections[0].id)[0];
    assert_eq!(indexed.aliases, aliases);
    assert_eq!(indexed.forms, [form]);
    let outline = crate::build_outline_projection(&query, EntryProjection::All, None).unwrap();
    let OutlineNode::DocumentSection { children, .. } = &outline.nodes[0] else {
        panic!("{outline:?}")
    };
    let OutlineNode::DocumentEntry {
        id,
        path,
        aliases: projected,
        forms,
        ..
    } = &children[0]
    else {
        panic!("{children:?}")
    };
    assert_eq!(projected, aliases);
    assert_eq!(forms, &[form]);
    let direct = crate::select_excerpt(&query, &[path.as_str()]).unwrap();
    for alias in aliases {
        let explained = crate::select_excerpt(&query, &[alias]).unwrap();
        assert_eq!(explained.selections, direct.selections);
        assert!(crate::render_excerpt_text(&explained).contains("OWNEDPAYLOAD"));
    }
    assert!(crate::select_excerpt(&query, &[rejected]).is_err());
    for scope in [SearchScope::Visible, SearchScope::Markdown] {
        let found = crate::search_query(
            &query,
            &SearchQuery {
                pattern: "OWNEDPAYLOAD".into(),
                syntax: SearchSyntax::Literal,
                case: SearchCase::Sensitive,
                scope,
                word: false,
                context_lines: 0,
                offset: 0,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(found.matches.len(), 1);
        let hit = &found.matches[0];
        assert!(
            matches!(&hit.outline.node, mant_protocol::OutlineNodeReference::DocumentEntry { id: found_id, path: found_path, .. } if found_id == id && found_path == path)
        );
        assert_eq!(hit.node_source.unwrap().line, source_line);
        assert_eq!(hit.occurrences[0].matched_text, "OWNEDPAYLOAD");
    }
}

#[test]
fn slash_alias_candidates_retain_parameter_styles_across_native_dialects() {
    for (mdoc, man, form, aliases, rejected) in [
        (
            "Fl n Ns / Ns Ar -NUM",
            ".BI \"-n/\" -NUM",
            "-n/-NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "Fl n Ns Ar /-NUM",
            ".BI -n /-NUM",
            "-n/-NUM",
            vec!["-n"],
            "-NUM",
        ),
        (
            "Fl n Ns / Ns Fl -number",
            ".B \"-n/--number\"",
            "-n/--number",
            vec!["-n", "--number"],
            "-NUM",
        ),
        (
            "Fl n Ns / Ns Ar -NUM",
            ".B \"-n/\\fI-NUM\"",
            "-n/-NUM",
            vec!["-n"],
            "-NUM",
        ),
    ] {
        assert_invocation_consumers(
            &format!(
                ".Dd September 7, 2026\n.Dt NAMES 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It {mdoc}\nOWNEDPAYLOAD.\n.El\n"
            ),
            form,
            &aliases,
            rejected,
            6,
        );
        assert_invocation_consumers(
            &format!(".TH NAMES 1\n.SH OPTIONS\n.TP\n{man}\nOWNEDPAYLOAD.\n"),
            form,
            &aliases,
            rejected,
            3,
        );
    }
}

#[test]
fn incomplete_tag_paragraphs_preserve_visible_terms_at_eof() {
    for tail in [
        ".B --unfinished",
        ".BI \"--unfinished \" VALUE",
        ".B --first\n.TP\n.B --unfinished",
    ] {
        let query =
            crate::query_roff_bytes(format!(".TH TAGS 1\n.SH OPTIONS\n.TP\n{tail}\n").as_bytes())
                .unwrap();
        for text in [
            crate::render_query_text(&query),
            crate::render_markdown(&query),
        ] {
            assert!(text.contains("--unfinished"), "{tail}: {text}");
            if tail.contains("--first") {
                assert!(text.contains("--first"), "{text}");
            }
        }
    }
}

#[test]
fn variable_subscripts_must_be_complete_authored_forms() {
    for name in ["FOO", "FOO[bar]", "FOO[0]", "$FOO[_index]"] {
        assert_names(
            &format!(".TH PROBE 1\n.SH VARIABLES\n.TP\n.B {name}\nValue.\n"),
            &[name],
            "missing",
        );
    }
    for name in [
        "FOO[bar",
        "FOO[bar]TRAILING",
        "FOO[]",
        "FOO[[bar]]",
        "FOO[a][b]",
        "FOO]",
        "FOO[bar]]",
    ] {
        let source = format!(".TH PROBE 1\n.SH VARIABLES\n.TP\n.B {name}\nValue.\n");
        let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
        let document = query.document.as_ref().unwrap();
        let index = mant_ir::SemanticIndex::build(document);
        assert!(
            index.section(&document.sections[0].id)[0]
                .aliases
                .is_empty(),
            "{name}"
        );
        assert!(document.diagnostics.iter().any(|d| d.code.as_deref() == Some("manual.semantic-entry.unclassified-definition")), "{name}: {:?}", document.diagnostics);
        assert!(crate::render_query_text(&query).contains(name), "{name}");
    }
}

#[test]
fn option_arguments_never_become_aliases() {
    for (head, aliases, missed) in [
        (".It Fl n Ar -NUM", vec!["-n"], "-NUM"),
        (".It Fl x Ar -1 | 1", vec!["-x"], "-1"),
        (
            ".It Fl n , Fl -number Ar NUM",
            vec!["-n", "--number"],
            "NUM",
        ),
        (".It Fl -arg Ns = Ns Ar VALUE", vec!["--arg"], "VALUE"),
    ] {
        let source = format!(
            ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh OPTIONS\n.Bl -tag -width Ds\n{head}\nDescription.\n.El\n"
        );
        assert_names(&source, &aliases, missed);
    }
    for head in [".BI \"-n \" -NUM", ".B -n\n.I -NUM", ".B \"-n -NUM\""] {
        assert_names(
            &format!(".TH PROBE 1\n.SH OPTIONS\n.TP\n{head}\nDescription.\n"),
            &["-n"],
            "-NUM",
        );
    }
}

fn assert_names(source: &str, names: &[&str], missed: &str) {
    let query = crate::query_roff_bytes(source.as_bytes()).unwrap();
    let index = mant_ir::SemanticIndex::build(query.document.as_ref().unwrap());
    let entries = index.section(&query.document.as_ref().unwrap().sections[0].id);
    assert_eq!(entries[0].aliases, names, "{source}");
    for name in names {
        assert!(
            crate::select_excerpt(&query, &[name]).is_ok(),
            "{source}: {name}"
        );
    }
    assert!(
        crate::select_excerpt(&query, &[missed]).is_err(),
        "{source}: {missed}"
    );
}

#[test]
fn environment_assignment_values_are_not_alias_groups() {
    for form in ["FOO=one,two", "FOO=one|two", "\"FOO=one,two\""] {
        assert_names(
            &format!(".TH PROBE 1\n.SH ENVIRONMENT\n.TP\n.B {form}\nSet values.\n"),
            &["FOO"],
            "two",
        );
    }
    assert_names(
        ".TH PROBE 1\n.SH ENVIRONMENT\n.TP\n.B \"FOO, BAR\"\nSet values.\n",
        &["FOO", "BAR"],
        "missing",
    );
    for form in ["FOO=one, BAR=two", "FOO=one|BAR=two", "FOO=one BAR=two"] {
        let query = crate::query_roff_bytes(
            format!(".TH PROBE 1\n.SH ENVIRONMENT\n.TP\n.B \"{form}\"\nSet values.\n").as_bytes(),
        )
        .unwrap();
        assert!(crate::select_excerpt(&query, &["BAR"]).is_err());
        assert!(crate::select_excerpt(&query, &["FOO"]).is_err());
        assert!(query.document.unwrap().diagnostics.iter().any(|d| d.code.as_deref() == Some("manual.semantic-entry.unclassified-definition")));
    }
}

#[test]
fn command_alias_groups_survive_styled_names_and_argument_boundaries() {
    for head in [
        ".It Ic clone , Ic copy",
        ".It clone , copy",
        ".It Ic clone Ar PATH , Ic copy Ar PATH",
        ".It Ic clone | Ic copy",
    ] {
        assert_names(
            &format!(
                ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n.Bl -tag -width Ds\n{head}\nClone repository.\n.El\n"
            ),
            &["clone", "copy"],
            "PATH",
        );
    }
    for head in [
        ".B \"clone, copy\"",
        ".BR clone \", \" copy",
        ".B \"clone|copy\"",
    ] {
        assert_names(
            &format!(".TH PROBE 1\n.SH COMMANDS\n.TP\n{head}\nClone repository.\n"),
            &["clone", "copy"],
            "PATH",
        );
    }
}

#[test]
fn visual_indentation_does_not_change_a_top_level_command_role() {
    for (open, close) in [
        ("", ""),
        (".RS 4\n", ".RE\n"),
        (".RS 0\n.RS 8\n", ".RE\n.RE\n"),
    ] {
        assert_names(
            &format!(
                ".TH PROBE 1\n.SH COMMANDS\n{open}.TP\n.B clone\nClone a repository.\n{close}"
            ),
            &["clone"],
            "missing",
        );
    }
    for (open, close) in [
        ("", ""),
        (".Bd -ragged -offset 4n\n", ".Ed\n"),
        (".Bl -bullet\n.It\n", ".El\n"),
    ] {
        assert_names(
            &format!(
                ".Dd September 7, 2026\n.Dt PROBE 1\n.Os\n.Sh COMMANDS\n{open}.Bl -tag -width Ds\n.It Ic clone\nClone a repository.\n.El\n{close}"
            ),
            &["clone"],
            "missing",
        );
    }
}
