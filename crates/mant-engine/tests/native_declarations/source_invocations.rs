//! Accepted source roles delimit compound names and their invocation operands.
use mant_protocol::{ContentSelector, EvidenceClass};

use super::{
    assignment_and_arguments::check,
    support::{explain, explanation_ranges, load, roundtrip},
};

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n";
const BODY: &str = "Set the documented option or run this command.";
const VALUE_ROLES: [(&str, &str, &str); 5] = [
    ("Dv", "RED", "BLUE"),
    ("Er", "ENOENT", "EPERM"),
    ("Va", "red", "blue"),
    ("Ev", "RED", "BLUE"),
    ("Cm", "red", "blue"),
];

fn source(section: &str, head: &str, body: &str) -> String {
    format!("{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\n{body}\n.El\n")
}

#[test]
fn a_complete_program_prefix_does_not_depend_on_its_argument_syntax() {
    // CVS runs Nm and Cm before the optional and argument macros, both
    // inline in It and in the extended HEAD. The selected body is identical.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (tail, visible, operands) in [
            ("Ar file", "file", &["file"][..]),
            ("Op Fl j", "[-j]", &["-j"][..]),
            (
                "Ar file Ar directory",
                "file directory",
                &["file", "directory"][..],
            ),
            (
                "Ar file | Ar directory",
                "file | directory",
                &["file", "directory"][..],
            ),
            (
                "Ar file , Ar directory",
                "file, directory",
                &["file", "directory"][..],
            ),
            (
                "Op Fl j Ar file | Ar directory",
                "[-j file | directory]",
                &["-j", "file", "directory"][..],
            ),
            ("Fl t Ar token", "-t token", &["-t", "token"][..]),
        ] {
            let form = format!("tool mount {visible}");
            for head in [
                format!("Nm tool Cm mount {tail}"),
                format!("Xo\n.Nm tool\n.Cm mount\n.{tail}\n.Xc"),
            ] {
                check(
                    &source(section, &head, BODY),
                    &form,
                    &["tool mount"],
                    operands,
                    BODY,
                    9,
                );
            }
        }
    }
}

#[test]
fn independent_compound_names_bind_after_separators_and_through_typed_suffixes() {
    // The first Cm prefix can contain multiple words. An independent comma
    // closes the group even when the next Nm receipt owns its leading space.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (head, form, names, operands) in [
            (
                "Nm tool Cm mount Ar path , Nm other Cm run",
                "tool mount path, other run",
                &["tool mount", "other run"][..],
                &["path"][..],
            ),
            (
                "Dv READY , Nm tool Cm mount Op Fl j",
                "READY, tool mount [-j]",
                &["READY", "tool mount"][..],
                &["-j"][..],
            ),
            (
                "Nm tool Cm mount Cm child Ar path",
                "tool mount child path",
                &["tool mount child"][..],
                &["path"][..],
            ),
            (
                r#"Nm tool Cm "mount child" Ar path"#,
                "tool mount child path",
                &["tool mount child"][..],
                &["path"][..],
            ),
        ] {
            check(
                &source(section, head, "BODY"),
                form,
                names,
                operands,
                "BODY",
                9,
            );
        }
    }
}

#[test]
fn embedded_or_joined_program_text_does_not_open_a_compound_declaration() {
    // The pinned tree puts the same Nm/Cm inside Op or after Ar, or
    // places it in the body. Ns keeps adjacent source operands one word.
    for (section, head, body, visible) in [
        ("DESCRIPTION", "Op Nm tool Cm mount", "BODY", "[tool mount]"),
        ("COMMANDS", "Op Nm tool Cm mount", "BODY", "[tool mount]"),
        (
            "DESCRIPTION",
            "Ar path Nm tool Cm mount",
            "BODY",
            "path tool mount",
        ),
        (
            "COMMANDS",
            "Ar path Nm tool Cm mount",
            "BODY",
            "path tool mount",
        ),
        (
            "DESCRIPTION",
            "Nm tool Ns Cm mount Ar path",
            "BODY",
            "toolmount path",
        ),
        (
            "COMMANDS",
            "Nm tool Ns Cm mount Ar path",
            "BODY",
            "toolmount path",
        ),
        (
            "DESCRIPTION",
            "Nm tool Cm mount Ns Ar path",
            "BODY",
            "tool mountpath",
        ),
        (
            "COMMANDS",
            "Nm tool Cm mount Ns Ar path",
            "BODY",
            "tool mountpath",
        ),
        (
            "DESCRIPTION",
            "Ar LABEL",
            "BODY\n.Nm tool\n.Cm mount",
            "tool mount",
        ),
        (
            "SYNOPSIS",
            "Nm tool Cm mount Op Fl j",
            "BODY",
            "tool mount [-j]",
        ),
    ] {
        let input = if section == "SYNOPSIS" {
            format!(
                "{HEADER}.Sh SYNOPSIS\n.Nm tool\n.Cm mount\n.Op Fl j\n.Sh DESCRIPTION\n{body}\n"
            )
        } else {
            source(section, head, body)
        };
        let content = load(&input);
        let query = explain(&content, "tool mount");
        assert_eq!(query.counts.direct_entry.total, 0, "{head}");
        assert!(
            mant_render::render_query_text(&content).contains(visible),
            "{head}"
        );
    }
}

#[test]
fn separators_inside_one_assignment_invocation_do_not_open_names() {
    // mdoc_macro.c::in_line() closes and reopens Dv/Er/Ev/Va at bare
    // punctuation. A subsequently executed Dv gets its own invocation.
    for section in ["DESCRIPTION", "OPTIONS"] {
        for (role, first, second) in VALUE_ROLES {
            for (separator, print) in [(",", ", "), ("|", " | ")] {
                for (lead, key) in [("Sy mode =", "mode"), ("Ev MODE =", "MODE")] {
                    let head = format!("{lead} {role} {first} {separator} {second}");
                    let form = format!("{key} = {first}{print}{second}");
                    check(
                        &source(section, &head, BODY),
                        &form,
                        &[key],
                        &[first, second],
                        BODY,
                        9,
                    );
                }
                let head = format!("Sy mode = {role} {first} {separator} {second} , Dv EXTRA");
                let form = format!("mode = {first}{print}{second}, EXTRA");
                check(
                    &source(section, &head, "BODY"),
                    &form,
                    &["mode", "EXTRA"],
                    &[first, second],
                    "BODY",
                    9,
                );
            }
        }
    }
}

#[test]
fn a_separate_definition_can_bind_an_assignment_value_as_its_own_name() {
    for (role, first, second) in VALUE_ROLES.into_iter().filter(|(role, ..)| *role != "Cm") {
        let input = source(
            "DESCRIPTION",
            &format!("Sy mode = {role} {first} | {second}"),
            &format!("ASSIGNMENTBODY\n.It {role} {second}\nINDEPENDENTBODY"),
        );
        let content = load(&input);
        for (selector, line, body) in [
            ("mode", 9, "ASSIGNMENTBODY"),
            (second, 11, "INDEPENDENTBODY"),
        ] {
            let query = roundtrip(&explain(&content, selector));
            assert_eq!(query.counts.direct_entry.total, 1, "{input}");
            assert!(explanation_ranges::assert_ranges(&query, selector) > 0);
            let hit = query
                .evidence
                .iter()
                .find(|hit| hit.class == EvidenceClass::DirectEntry)
                .unwrap();
            assert_eq!(hit.source.unwrap().line, line);
            let excerpt =
                mant_query::select_excerpt(&content, &[ContentSelector::path(hit.outline.path())])
                    .unwrap();
            let rendered = mant_render::render_excerpt_text(&excerpt);
            assert!(
                rendered.contains(body) && rendered.matches("BODY").count() == 1,
                "{rendered}"
            );
        }
    }
}

#[test]
fn a_reopened_flag_value_is_distinct_from_another_executed_flag() {
    // CVS in_line() reopens Fl at the same ppos around a bare delimiter.
    // A separately expanded request can have the same ppos, but the native
    // tree marks its own execution line with NODE_LINE. Both forms read alike.
    for (separator, printed) in [(",", ", "), ("|", " | ")] {
        for independent in [false, true] {
            let names: &[&str] = if independent {
                &["mode", "-b", "EXTRA"]
            } else {
                &["mode", "EXTRA"]
            };
            let operands: &[&str] = if independent { &["-a"] } else { &["-a", "-b"] };
            let flag = if independent { "Fl b" } else { "b" };
            let head = format!("Sy mode = Fl a {separator} {flag} , Dv EXTRA");
            check(
                &source("DESCRIPTION", &head, "BODY"),
                &format!("mode = -a{printed}-b, EXTRA"),
                names,
                operands,
                "BODY",
                9,
            );
            let expansion = if independent { "\n.Fl b" } else { " b" };
            let input = format!(
                "{HEADER}.Sh DESCRIPTION\n.de FLAGS\n.Fl a {separator}{expansion}\n..\n.Bl -tag -width Ds\n.It Xo\n.Sy mode =\n.FLAGS\n.No ,\n.Dv EXTRA\n.Xc\nBODY\n.El\n"
            );
            check(
                &input,
                &format!("mode = -a{printed}-b , EXTRA"),
                names,
                operands,
                "BODY",
                if independent { 13 } else { 12 },
            );
        }
    }
}

#[test]
fn display_content_does_not_hide_a_same_row_compound_name() {
    // The pinned tree places both words and the table under the extended
    // It HEAD. A preceding table actually ends rows between Nm and Cm.
    const TABLE: &str = ".br\n.TS\nl.\nCELLWITNESS\n.TE\n";
    for section in ["DESCRIPTION", "COMMANDS"] {
        for (qualifier, eligible) in [("", true), (".Ar operand\n", false)] {
            let invocation = format!("{qualifier}.Nm tool\n.Cm mount\n");
            for (head, compound_scheme) in [
                (format!("Xo\n{invocation}{TABLE}.Xc"), true),
                (format!("Xo\n{TABLE}{invocation}.Xc"), false),
            ] {
                let input = source(section, &head, "BODY");
                let content = load(&input);
                let rendered = mant_render::render_query_text(&content);
                let witness = if compound_scheme {
                    "tool mount\nCELLWITNESS"
                } else {
                    "tool\nmount"
                };
                assert!(rendered.contains(witness), "{rendered}");
                assert_eq!(rendered.matches("CELLWITNESS").count(), 1);
                let found = roundtrip(&explain(&content, "tool mount"));
                let direct = compound_scheme && eligible;
                assert_eq!(
                    found.counts.direct_entry.total,
                    u32::from(direct),
                    "{input}"
                );
                if direct {
                    assert!(explanation_ranges::assert_ranges(&found, "tool mount") > 0);
                    let hit = found
                        .evidence
                        .iter()
                        .find(|hit| hit.class == EvidenceClass::DirectEntry)
                        .unwrap();
                    assert_eq!(hit.source.unwrap().line, 9);
                    let excerpt = mant_query::select_excerpt(
                        &content,
                        &[ContentSelector::path(hit.outline.path())],
                    )
                    .unwrap();
                    let rendered = mant_render::render_excerpt_text(&excerpt);
                    assert!(
                        rendered.contains("BODY") && rendered.contains("CELLWITNESS"),
                        "{rendered}"
                    );
                }
            }
        }
    }
}

#[test]
fn the_standalone_program_owns_the_form_with_an_enclosed_command_choice() {
    // The groff_mdoc(7) example has a complete leading Nm. CVS opens a
    // Brq BODY for both Cm alternatives rather than a compound prefix.
    let head = "Nm ztar Xo\n.Brq Cm c | Cm x\n.Op Fl w Op Fl y | Fl z\n.Op Fl f Ar archive\n.Ar member No ...\n.Xc";
    check(
        &source("DESCRIPTION", head, "BODY"),
        "ztar {c | x} [-w [-y | -z]] [-f archive] member ...",
        &["ztar"],
        &["c", "x", "-w", "-y", "-z", "-f", "archive", "member"],
        "BODY",
        9,
    );
}
