use super::{DECLARATION_HEADER, check_query, explain, load, section_fixture};
use mant_ir::EntryKind;

#[test]
fn annotation_keeps_assignment_value_separators_opaque() {
    // CVS mdoc_macro.c::in_line() keeps HOME=value,other in one Ev TEXT
    // operand. The Pq macro executes separately from that assignment.
    for separator in [',', '|'] {
        for (value, names) in [
            (format!("HOME=value{separator}other"), &["HOME"][..]),
            (format!("ONE=a{separator}FAKE=b"), &[][..]),
        ] {
            for (suffix, printed) in [
                ("", ""),
                (" Pq Em ann", " (ann)"),
                (" Pq No ann", " (ann)"),
                (" Ns Pq Ic ann", "(ann)"),
            ] {
                check(
                    &format!("Ev {value}{suffix}"),
                    names,
                    &format!("{value}{printed}"),
                    &["other", "FAKE", "ONE", "ann"],
                );
            }
        }
    }
}

#[test]
fn separately_executed_environment_aliases_get_independent_value_and_annotation_bounds() {
    for (head, names, form) in [
        (
            "Xo Ev HOME=value,other ,\n.Ev PATH Pq Em ann\n.Xc",
            &["HOME", "PATH"][..],
            "HOME=value,other, PATH (ann)",
        ),
        (
            "Xo Ev PATH Pq Em ann ,\n.Ev HOME=value,other\n.Xc",
            &["PATH", "HOME"][..],
            "PATH (ann), HOME=value,other",
        ),
        (
            "Xo Ev HOME=value,other Pq Em ann ,\n.Ev PATH\n.Xc",
            &["HOME", "PATH"][..],
            "HOME=value,other (ann), PATH",
        ),
        (
            "Xo Ev HOME=value,other ,\n.Ev PATH = Ar value\n.Xc",
            &["HOME", "PATH"][..],
            "HOME=value,other, PATH = value",
        ),
        (
            "Xo Ev HOME=value,other ,\n.No PATH\n.Xc",
            &["HOME"][..],
            "HOME=value,other, PATH",
        ),
        (
            "Xo Ev HOME = value ,\n.Ev PATH Pq Em ann\n.Xc",
            &["HOME", "PATH"][..],
            "HOME = value, PATH (ann)",
        ),
        (
            "Xo Ev HOME = value , other\n.Pq Em ann\n.Xc",
            &["HOME"][..],
            "HOME = value, other (ann)",
        ),
    ] {
        check(head, names, form, &["other", "value", "ann"]);
    }

    let head = "Xo\n.ZZ\n.Xc";
    let preamble = ".de ZZ\n.Ev HOME=value,other ,\n.Ev PATH Pq Em ann\n..\n";
    let query = section_fixture("ENVIRONMENT", preamble, head);
    check_query(
        &query,
        head,
        EntryKind::EnvironmentVariable,
        &["HOME", "PATH"],
        "HOME=value,other, PATH (ann)",
        &["other", "ann"],
    );
}

#[test]
fn a_reopened_environment_macro_in_the_value_does_not_start_an_alias() {
    // Even when Ev executes again to supply the RHS, i.e. instead of
    // appearing as literal text, a comma inside that word or reopening
    // the same invocation after punctuation cannot split the value.
    for (head, names, form) in [
        (
            "Ev HOME= Ns Ev value,other Pq Em ann",
            &["HOME"][..],
            "HOME=value,other (ann)",
        ),
        (
            "Ev HOME= Ns Ev value|other Pq Em ann",
            &["HOME"][..],
            "HOME=value|other (ann)",
        ),
        (
            "Ev HOME= Ns Ev value , other Pq Em ann",
            &["HOME"][..],
            "HOME=value, other (ann)",
        ),
        (
            "Xo Ev HOME= Ns Ev value , other Pq Em ann ,\n.Ev PATH\n.Xc",
            &["HOME", "PATH"][..],
            "HOME=value, other (ann), PATH",
        ),
        ("Ev FIRST,SECOND", &["FIRST", "SECOND"][..], "FIRST,SECOND"),
    ] {
        check(head, names, form, &["value", "other", "ann"]);
    }
}

#[test]
fn annotation_starts_after_the_executed_environment_spelling() {
    // CVS tree keeps the internal () in the Ev word. Pq's own BODY
    // prints the external parentheses, with or without Ns spacing.
    for name in ["HOME", "BASH_FUNC_home()"] {
        for (join, gap) in [("", " "), ("Ns ", "")] {
            for style in ["Em", "No", "Ic"] {
                check(
                    &format!("Ev {name} {join}Pq {style} ann"),
                    &[name],
                    &format!("{name}{gap}(ann)"),
                    &["ann", "BASH_FUNC_home"],
                );
            }
        }
    }
    check(
        "Ev BASH_FUNC_home()",
        &["BASH_FUNC_home()"],
        "BASH_FUNC_home()",
        &["BASH_FUNC_home"],
    );
}

#[test]
fn executed_description_and_reference_operands_keep_their_own_word_boundaries() {
    // The pinned tree gives Ar, Em, Sy, Lk, Mt and Xr independent operands.
    // termp_lk_pre further emits a generated colon and the URI. None of
    // these words may extend the Ev spelling when Ns glues the display.
    for (tail, value, supplemental) in [
        ("Ar FILE", "FILE", true),
        ("Em FILE", "FILE", true),
        ("Sy FILE", "FILE", true),
        (
            "Lk https://example.test FILE",
            "FILE: https://example.test",
            true,
        ),
        ("Mt y@example.test", "y@example.test", true),
        ("Xr printf 3", "printf(3)", false),
    ] {
        for (join, gap) in [("", " "), ("Ns ", "")] {
            let head = format!("Ev HOME {join}{tail}");
            let form = format!("HOME{gap}{value}");
            if !join.is_empty() {
                let query = section_fixture("ENVIRONMENT", "", &head);
                let items = super::super::super::definitions(query.document.as_ref().unwrap());
                assert_eq!(mant_ir::inline_plain_text(&items[0].terms[0]), form);
                assert!(items[0].entry.is_none());
                assert_eq!(explain(&query, &form).counts.direct_entry.total, 0);
                continue;
            }
            let names = if supplemental && join.is_empty() {
                &["HOME"][..]
            } else {
                &[]
            };
            check(&head, names, &form, &["FILE", "printf"]);
        }
    }
}

#[test]
fn link_operands_keep_their_native_annotation_scope() {
    // Each expected form comes from the pinned terminal. mdoc_term.c's
    // termp_lk_pre, termp_under_pre and termp_xr_pre all execute inside Pq.
    for (macro_name, name, section, kind) in [
        ("Dv", "SYS", "DESCRIPTION", EntryKind::Term),
        ("Va", "local", "DESCRIPTION", EntryKind::Variable),
        ("Ev", "HOME", "ENVIRONMENT", EntryKind::EnvironmentVariable),
    ] {
        for (tail, label) in [
            ("Lk https://example.test FILE", "FILE: https://example.test"),
            ("Mt y@example.test", "y@example.test"),
            ("Xr printf 3", "printf(3)"),
        ] {
            let head = format!("{macro_name} {name} Pq {tail}");
            let query = section_fixture(section, "", &head);
            check_query(
                &query,
                &head,
                kind,
                &[name],
                &format!("{name} ({label})"),
                &["FILE", "printf"],
            );
        }
    }
}

#[test]
fn link_and_description_punctuation_cannot_open_an_alias() {
    // CVS keeps the comma in the description or URI and the unmatched
    // parenthesis in the link label. The trailing comma is a separate TEXT
    // node and can delimit a later Ev, including after a native annotation.
    for (head, names, form) in [
        (
            r#"Ev MAIN Em "some, text" , Ev PATH"#,
            &["MAIN", "PATH"][..],
            "MAIN some, text, PATH",
        ),
        (
            "Ev MAIN Lk https://example.test/a,b FILE",
            &["MAIN"][..],
            "MAIN FILE: https://example.test/a,b",
        ),
        (
            "Ev MAIN Lk https://example.test/a,b",
            &[][..],
            "MAIN https://example.test/a,b",
        ),
        (
            r#"Ev DEMO_HOME Lk https://example.test "FILE (draft""#,
            &["DEMO_HOME"][..],
            "DEMO_HOME FILE (draft: https://example.test",
        ),
        (
            r#"Ev FIRST , Ev SECOND Lk https://example.test "FILE (draft""#,
            &["FIRST", "SECOND"][..],
            "FIRST, SECOND FILE (draft: https://example.test",
        ),
        (
            "Xo Ev HOME Pq Em ann ,\n.Ev PATH Lk https://example.test FILE\n.Xc",
            &["HOME", "PATH"][..],
            "HOME (ann), PATH FILE: https://example.test",
        ),
        (
            "Xo Ev PATH Lk https://example.test FILE ,\n.Ev HOME Pq Em ann\n.Xc",
            &["PATH", "HOME"][..],
            "PATH FILE: https://example.test, HOME (ann)",
        ),
        (
            "Ev HOME=value,other Lk https://example.test FILE",
            &["HOME"][..],
            "HOME=value,other FILE: https://example.test",
        ),
        ("Ev ONE=a, Em FAKE=b", &[][..], "ONE=a, FAKE=b"),
        ("Ev ONE=a, Ns Em FAKE=b", &[][..], "ONE=a,FAKE=b"),
    ] {
        check(
            head,
            names,
            form,
            &["text", "b", "draft", "FILE", "other", "FAKE"],
        );
    }
}

#[test]
fn rejected_annotation_cannot_be_rescanned_as_an_environment_name() {
    for (head, form) in [
        (
            "Xo Ev HOME Ns\n.Po\n.Em note\n.Pc Ns\n.No tail\n.Xc",
            "HOME(note)tail",
        ),
        (
            "Xo Ev HOME Ns\n.Po\n.Em note\n.Pc\n.No tail\n.Xc",
            "HOME(note) tail",
        ),
        ("Xo Ev HOME Ns\n.Eo x\n.Ic note\n.Ec z\n.Xc", "HOMExnotez"),
        ("Xo Ev HOME Ns\n.Eo (\n.Ic note\n.Ec\n.Xc", "HOME(note"),
    ] {
        check(head, &[], form, &["HOME", "OTHER", "note", "tail"]);
    }
}

#[test]
fn assignments_and_annotations_in_body_and_synopsis_do_not_define_names() {
    let query = load(&format!(
        "{DECLARATION_HEADER}.Sh SYNOPSIS\n.Ev HOME=value,other Pq Em ann\n.Ev BASH_FUNC_home() Ns Pq Em ann\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It entry\n.Ev HOME=value|other Pq Ic ann\n.Ev BASH_FUNC_home() Ns Pq No ann\n.El\n"
    ));
    let text = mant_render::render_query_text(&query);
    assert!(text.contains("HOME=value,other (ann) BASH_FUNC_home()(ann)"));
    assert!(text.contains("HOME=value|other (ann) BASH_FUNC_home()(ann)"));
    for name in ["HOME", "other", "ann", "BASH_FUNC_home()"] {
        assert_eq!(explain(&query, name).counts.direct_entry.total, 0, "{name}");
    }
}

fn check(head: &str, names: &[&str], form: &str, arguments: &[&str]) {
    let query = section_fixture("ENVIRONMENT", "", head);
    check_query(
        &query,
        head,
        EntryKind::EnvironmentVariable,
        names,
        form,
        arguments,
    );
}
