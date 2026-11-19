//! An invocation retains its complete form and binds only proved names.
use mant_protocol::{ContentSelector, EvidenceBasis, EvidenceClass};

use super::support::{check_head, explain, explanation_ranges, load, roundtrip};

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n";

pub(super) fn check(
    source: &str,
    form: &str,
    names: &[&str],
    operands: &[&str],
    body: &str,
    head_line: u32,
) {
    let content = load(source);
    let items = super::definitions(content.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{source}");
    };
    assert_eq!(item.source.unwrap().line, head_line);
    check_head(item, form, names);
    let mut path = None;
    for (selector, is_name) in names
        .iter()
        .map(|name| (*name, true))
        .chain([(form, false)])
    {
        let result = roundtrip(&explain(&content, selector));
        assert_eq!(result.counts.direct_entry.total, 1, "{source} {selector}");
        assert!(explanation_ranges::assert_ranges(&result, selector) > 0);
        let hit = result
            .evidence
            .iter()
            .find(|entry| entry.class == EvidenceClass::DirectEntry)
            .unwrap();
        assert_eq!(hit.source, item.source);
        assert!(hit.bases.iter().any(|basis| match basis {
            EvidenceBasis::Name { .. } => is_name,
            EvidenceBasis::Form { .. } => !is_name,
            _ => false,
        }));
        assert_eq!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. })),
            is_name
        );
        let owner = hit.outline.path();
        assert_eq!(*path.get_or_insert_with(|| owner.to_owned()), owner);
    }
    let excerpt =
        mant_query::select_excerpt(&content, &[ContentSelector::path(path.unwrap())]).unwrap();
    assert!(mant_render::render_excerpt_text(&excerpt).contains(body));
    for operand in operands {
        assert!(form.contains(operand));
        assert_eq!(
            explain(&content, operand).counts.direct_entry.total,
            0,
            "{source} {operand}"
        );
    }
}

#[test]
fn assignment_value_alternatives_keep_their_original_form_and_key() {
    // mdoc_macro.c::in_line closes and reopens Cm at bare delimiters.
    // CVS terminals keep every RHS alternative, including with Sm off.
    let body = "Set mode to red, blue, or green.";
    for section in ["DESCRIPTION", "OPTIONS"] {
        for (assignment, prefix) in [
            ("Sy mode =", "mode = "),
            ("Sy mode No =", "mode = "),
            ("Sy mode Ns = Ns", "mode="),
            ("Sy mode=", "mode= "),
        ] {
            for (separator, spaced, joined) in [(",", ", ", ","), ("|", " | ", "|")] {
                for (before, after, key, sep, line) in [
                    ("", "", prefix, spaced, 9),
                    (".Sm off\n", ".Sm on\n", "mode=", joined, 10),
                ] {
                    let source = format!(
                        "{HEADER}.Nd sample\n.Sh {section}\n.Bl -tag -width Ds\n{before}.It {assignment} Cm red {separator} blue {separator} green\n{after}{body}\n.El\n"
                    );
                    let form = format!("{key}red{sep}blue{sep}green");
                    check(
                        &source,
                        &form,
                        &["mode"],
                        &["red", "blue", "green"],
                        body,
                        line,
                    );
                }
            }
        }
    }
}

#[test]
fn a_symbolic_environment_assignment_exposes_the_key_and_complete_form() {
    let body = "The MODE setting takes one or two.";
    for macro_name in ["Sy", "Ev"] {
        for equality in ["=", " = "] {
            let form = format!("MODE{equality}one | two");
            let source = format!(
                "{HEADER}.Nd test probe\n.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It {macro_name} {form}\n{body}\n.El\n"
            );
            check(&source, &form, &["MODE"], &["one", "two"], body, 9);
        }
    }
}

#[test]
fn argument_separators_and_explicit_command_siblings_keep_separate_roles() {
    // in_line() closes Ar/Sy for punctuation, and a Cm following a comma
    // has its own macro source position. The complete form keeps both.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for macro_name in ["Cm", "Ic"] {
            let body = "Specify permissions and provide the filesystem or volume to modify.";
            for (pipe, pipe_text) in [(" | ", " | "), (" Ns | Ns ", "|")] {
                for (middle, middle_text) in [(" , ", ", "), (" | ", " | ")] {
                    let form = format!(
                        "allow one{pipe_text}two{middle_text}three filesystem{pipe_text}volume"
                    );
                    let source = format!(
                        "{HEADER}.Nd sample\n.Sh {section}\n.Bl -tag -width Ds\n.It {macro_name} allow Ar one{pipe}Ar two{middle}Ar three Ar filesystem{pipe}Ar volume\n{body}\n.El\n"
                    );
                    check(
                        &source,
                        &form,
                        &["allow"],
                        &["one", "two", "three", "filesystem", "volume"],
                        body,
                        9,
                    );
                }
            }
            let body = "The symbolic arguments are choices. The comma-delimited extra command, when present, is an independent name.";
            for (tail, form, names, operands) in [
                (
                    "Sy left | right",
                    "run left | right",
                    &["run"][..],
                    &["left", "right"][..],
                ),
                (
                    "Sy left , right",
                    "run left, right",
                    &["run"][..],
                    &["left", "right"][..],
                ),
                (
                    r#"Sy "left|right""#,
                    "run left|right",
                    &["run"][..],
                    &["left", "right"][..],
                ),
                (
                    "Sy left , right , Cm extra",
                    "run left, right, extra",
                    &["run", "extra"][..],
                    &["left", "right"][..],
                ),
                (
                    r#"Sy "left|right" , Cm extra"#,
                    "run left|right, extra",
                    &["run", "extra"][..],
                    &["left", "right"][..],
                ),
                (
                    "Ar file , Cm extra Ar input",
                    "run file, extra input",
                    &["run", "extra"][..],
                    &["file", "input"][..],
                ),
            ] {
                let source = format!(
                    "{HEADER}.Nd sample\n.Sh {section}\n.Bl -tag -width Ds\n.It {macro_name} run {tail}\n{body}\n.El\n"
                );
                check(&source, form, names, operands, body, 9);
            }
        }
    }
}

#[test]
fn attached_parentheses_keep_reopened_command_text_inside_the_argument() {
    // CVS marks bare '(' and ')' as delimiters but leaves the attached
    // spelling in Ic. Both trees have the same visible argument group.
    let text = "split_network (include | local_lan) network/mask, ...";
    for section in ["DESCRIPTION", "COMMANDS", "OPTIONS"] {
        for (head, form, names) in [
            (
                "Ic split_network (include | local_lan) Ar network/mask, ...",
                text.to_owned(),
                &["split_network"][..],
            ),
            (
                "Ic split_network ( include | local_lan ) Ar network/mask, ...",
                text.to_owned(),
                &["split_network"][..],
            ),
            (
                "Ic split_network (include | local_lan) Ar network/mask, ... , Ic independent",
                format!("{text}, independent"),
                &["split_network", "independent"][..],
            ),
        ] {
            let source = format!(
                "{HEADER}.Nd sample\n.Sh {section}\n.Bl -tag -width Ds\n.It {head}\nBODY\n.El\n"
            );
            check(
                &source,
                &form,
                names,
                &["include", "local_lan", "local_lan)"],
                "BODY",
                9,
            );
        }
    }
}

#[test]
fn the_name_limit_counts_proved_aliases_without_counting_assignment_values() {
    // CVS uses one Cm invocation for the assignment values and separate Fl
    // invocations for the aliases. Every spelling remains in the form.
    for count in [255, 256, 257] {
        let values = (0..count)
            .map(|index| format!("value{index}"))
            .collect::<Vec<_>>();
        let flags = (0..count)
            .map(|index| format!(".Fl alias{index}"))
            .collect::<Vec<_>>();
        let assignment = format!("Sy mode = Cm {}", values.join(" | "));
        let aliases = format!("Xo\n{}\n.Xc", flags.join("\n.No \\&,\n"));
        for (head, expected_names, limited) in [
            (assignment, 1, false),
            (aliases, if count == 257 { 0 } else { count }, count == 257),
        ] {
            let source = format!(
                "{HEADER}.Nd sample\n.Sh OPTIONS\n.Bl -tag -width Ds\n.It {head}\nBODY\n.El\n"
            );
            let content = load(&source);
            let document = content.document.as_ref().unwrap();
            let items = super::definitions(document);
            let [item] = items.as_slice() else {
                panic!("one definition owner");
            };
            let form = mant_ir::inline_plain_text(&item.terms[0]);
            let names = mant_ir::EntryOwner::Definition(item)
                .validated_names()
                .unwrap();
            assert_eq!(item.source.unwrap().line, 9);
            assert_eq!(names.len(), expected_names);
            assert_eq!(
                document
                    .diagnostics
                    .iter()
                    .filter(|diagnostic| diagnostic.code.as_deref()
                        == Some("manual.semantic-entry.name-limit"))
                    .count(),
                usize::from(limited)
            );
            if head.starts_with("Sy") {
                assert_eq!(names, ["mode"]);
                assert_eq!(form, format!("mode = {}", values.join(" | ")));
                assert_eq!(
                    explain(&content, values.last().unwrap())
                        .counts
                        .direct_entry
                        .total,
                    0
                );
            } else {
                assert!(form.ends_with(&format!("-alias{}", count - 1)));
                assert_eq!(
                    explain(&content, &format!("-alias{}", count - 1))
                        .counts
                        .direct_entry
                        .total,
                    u32::from(!limited)
                );
            }
        }
    }
}
