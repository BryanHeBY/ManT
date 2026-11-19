//! Standalone and contiguous compound names in source-owned definition heads.
use mant_ir::{EntryKind, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceClass};

use super::assignment_and_arguments::check;
use super::support::{check_head, explain, explanation_ranges, load, roundtrip};

const HEADER: &str = ".Dd November 19, 2026\n.Dt TOOL 1\n.Os\n.Sh NAME\n.Nm tool\n.Nd sample\n";
const BODY: &str = "Owner proof.";

fn source(section: &str, head: &str) -> String {
    format!("{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\n{BODY}\n.El\n")
}

#[test]
fn compound_command_names_stop_at_argument_syntax_across_native_words() {
    // mdoc_macro.c::in_line keeps Cm/Ic words under the same scope, and
    // termp_bold_pre prints the same prefix with or without source quotes.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for macro_name in ["Cm", "Ic"] {
            for (argument, visible, name, negative) in [
                (
                    "mount (FILE)",
                    "mount (FILE)",
                    "tool mount",
                    "tool mount (FILE)",
                ),
                (
                    r#""mount (FILE)""#,
                    "mount (FILE)",
                    "tool mount",
                    "tool mount (FILE)",
                ),
                (
                    "mount child",
                    "mount child",
                    "tool mount child",
                    "tool mount",
                ),
                ("mount -p", "mount -p", "tool mount", "tool mount -p"),
                (
                    "mount [FILE]",
                    "mount [FILE]",
                    "tool mount",
                    "tool mount [FILE]",
                ),
            ] {
                let head = format!("Nm tool {macro_name} {argument} Ar path");
                let form = format!("tool {visible} path");
                for source_head in [head.clone(), format!("Xo\n.{head}\n.Xc")] {
                    check(
                        &source(section, &source_head),
                        &form,
                        &[name],
                        &[negative, "path"],
                        BODY,
                        9,
                    );
                }
                let head = format!("Xo\n.{head}\n.br\n.TS\nl.\nCELL\n.TE\n.Xc");
                check_display(section, &head, name, &form, negative);
            }
        }
    }
}

fn check_display(section: &str, head: &str, name: &str, form: &str, negative: &str) {
    let content = load(&source(section, head));
    let items = super::definitions(content.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{head}");
    };
    assert_eq!(item.entry.as_ref().unwrap().kind, EntryKind::Command);
    check_head(item, name, &[name]);
    assert_eq!(inline_plain_text(&item.terms[0]), format!("{form}\nCELL"));
    let result = roundtrip(&explain(&content, name));
    assert_eq!(result.counts.direct_entry.total, 1, "{head}");
    assert!(explanation_ranges::assert_ranges(&result, name) > 0);
    let hit = result
        .evidence
        .iter()
        .find(|item| item.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source.unwrap().line, 9);
    let excerpt =
        mant_query::select_excerpt(&content, &[ContentSelector::path(hit.outline.path())]).unwrap();
    let visible = mant_render::render_excerpt_text(&excerpt);
    assert!(visible.contains(BODY) && visible.contains("CELL"), "{head}");
    for query in [negative, form, "path", "CELL"] {
        assert_eq!(
            explain(&content, query).counts.direct_entry.total,
            0,
            "{head} {query}"
        );
    }
}

#[test]
fn standalone_names_precede_options_operands_and_enclosed_choices() {
    // CVS termp_nm_pre renders the actual Nm first. Cm in the Brq BODY
    // belongs to an alternative argument rather than the outer prefix.
    for section in ["DESCRIPTION", "COMMANDS"] {
        for program in ["Nm tool", "Nm"] {
            for (tail, form, negatives) in [
                ("Op Fl j", "tool [-j]", &["-j"][..]),
                ("Ar file", "tool file", &["file"][..]),
                ("Brq Cm c | Cm x", "tool {c | x}", &["c", "x"][..]),
            ] {
                check(
                    &source(section, &format!("{program} {tail}")),
                    form,
                    &["tool"],
                    negatives,
                    BODY,
                    9,
                );
            }
        }
    }
}

#[test]
fn detached_or_clipped_invocations_cannot_prove_the_program_alone() {
    // In pinned mdoc_macro.c::in_line, both words of Nm belong to its
    // element. term.c::encode1 writes an overstrike before the remaining
    // Cm spelling, so the surviving prefix is insufficient evidence.
    for (head, visible, negative) in [
        ("Nm tool mount", "tool mount", "tool mount"),
        ("Nm tool Oo Fl d Oc Cm run", "tool [-d] run", "tool run"),
        (
            r"Nm tool Cm \zXmount Op Fl j",
            "tool mount [-j]",
            "tool mount",
        ),
        (r"Nm \zXtool Op Fl j", "tool [-j]", "tool"),
    ] {
        let content = load(&source("DESCRIPTION", head));
        let item = super::definitions(content.document.as_ref().unwrap())[0];
        check_head(item, visible, &[]);
        assert_eq!(
            explain(&content, "tool").counts.direct_entry.total,
            0,
            "{head}"
        );
        assert_eq!(
            explain(&content, negative).counts.direct_entry.total,
            u32::from(visible == negative),
            "{head}"
        );
    }
}

#[test]
fn assignment_groups_bind_independent_native_operands_without_unrelated_siblings() {
    // In mdoc_macro.c::in_line, a bare separator reopens the same macro
    // scope. An explicitly called macro to its right has its own scope.
    for section in ["DESCRIPTION", "OPTIONS", "COMMANDS"] {
        for (macro_name, lhs, rhs, left, right) in [
            ("Fl", "a", "b", "-a", "-b"),
            ("Ev", "RED", "BLUE", "RED", "BLUE"),
            ("Dv", "RED", "BLUE", "RED", "BLUE"),
        ] {
            for (separator, visible) in [(",", ", "), ("|", " | ")] {
                for repeated in [false, true] {
                    let call = if repeated {
                        format!("{macro_name} ")
                    } else {
                        String::new()
                    };
                    let subhead = format!("Sy mode = {macro_name} {lhs} {separator} {call}{rhs}");
                    let value = format!("mode = {left}{visible}{right}");
                    for (before, after) in [("", ""), ("", " , Dv EXTRA"), ("Dv PREFIX , ", "")] {
                        let mut names = Vec::new();
                        if !before.is_empty() {
                            names.push("PREFIX");
                        }
                        names.push("mode");
                        if repeated {
                            names.push(right);
                        }
                        if !after.is_empty() {
                            names.push("EXTRA");
                        }
                        let input = source(section, &format!("{before}{subhead}{after}"));
                        let start = if before.is_empty() { "" } else { "PREFIX, " };
                        let tail = if after.is_empty() { "" } else { ", EXTRA" };
                        let form = format!("{start}{value}{tail}");
                        let mut operands = vec![left, "mode ="];
                        if !repeated {
                            operands.push(right);
                        }
                        check(&input, &form, &names, &operands, BODY, 9);
                    }
                }
            }
        }
    }
}
