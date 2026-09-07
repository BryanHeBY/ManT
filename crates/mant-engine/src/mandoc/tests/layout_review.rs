//! End-to-end name, ownership and layout regression matrices.

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
            crate::select_explanation(&query, name).is_ok(),
            "{source}: {name}"
        );
    }
    assert!(
        crate::select_explanation(&query, missed).is_err(),
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
        assert!(crate::select_explanation(&query, "BAR").is_err());
        assert!(crate::select_explanation(&query, "FOO").is_err());
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
