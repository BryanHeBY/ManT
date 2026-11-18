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
        ("Ev HOME Ns Ar OTHER", "HOMEOTHER"),
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
