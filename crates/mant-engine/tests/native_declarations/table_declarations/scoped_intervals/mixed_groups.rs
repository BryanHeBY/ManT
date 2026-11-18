//! One source owner keeps independently proved siblings in native order.

use super::{
    continuations::{Match, check, insert_table},
    definitions,
};
use mant_ir::{EntryKind, EntryOwner, ResolvedContent};

const COMMA: &str = r".No \&,";
const PIPE: &str = r".No \&|";
const FUNCTIONS: [&[&str]; 2] = [
    &[r#".Fn run "int x""#],
    &[".Fo run", r#".Fa "int x""#, ".Fc"],
];
const SIBLINGS: [(&str, &str, &str); 6] = [
    ("DESCRIPTION", "Ev SECOND", "SECOND"),
    ("OPTIONS", "Fl b", "-b"),
    ("COMMANDS", "Cm next", "next"),
    ("DESCRIPTION", "Er EINVAL", "EINVAL"),
    ("DESCRIPTION", "Dv VALUE", "VALUE"),
    ("DESCRIPTION", r#"Fn other "int y""#, "other"),
];

#[test]
fn declared_siblings_share_the_function_owner_in_both_source_orders() {
    // Fixed mdoc_term.c termp_fn_pre/termp_fo_pre/post emit the signature
    // parentheses inside the function. The independent No punctuation
    // belongs to the HEAD sequence, and Fa never supplies a name here.
    for function in FUNCTIONS {
        for (section, request, name) in SIBLINGS {
            let request_line = format!(".{request}");
            for separator in [COMMA, PIPE] {
                for function_first in [true, false] {
                    let sibling = &[request_line.as_str()][..];
                    let (first, last) = if function_first {
                        (function, sibling)
                    } else {
                        (sibling, function)
                    };
                    let sequence: Vec<_> = first
                        .iter()
                        .copied()
                        .chain(std::iter::once(separator))
                        .chain(last.iter().copied())
                        .collect();
                    for at in [None, Some(0), Some(sequence.len())] {
                        let head = insert_table(sequence.clone(), at);
                        let content = check(
                            section,
                            &head,
                            Some(request),
                            &[
                                ("run", Match::Owner),
                                (name, Match::Both),
                                ("x", Match::Absent),
                            ],
                        );
                        let expected = if function_first {
                            ["run", name]
                        } else {
                            [name, "run"]
                        };
                        assert_mixed_names(&content, &expected, &head);
                    }
                }
            }
        }
    }
}

#[test]
fn an_authored_separator_is_independent_of_an_earlier_weak_name() {
    for (section, request, name) in [
        ("ENVIRONMENT", "Ev GOOD", "GOOD"),
        ("OPTIONS", "Fl b", "-b"),
        ("DESCRIPTION", "Er EINVAL", "EINVAL"),
        ("COMMANDS", "Cm next", "next"),
    ] {
        // The same visible text has either one No operand with an internal
        // comma or a distinct delimiter word (mdoc_macro.c dword/in_line).
        let request_line = format!(".{request}");
        for (mut head, expected) in [
            (vec![r".No 99\&,"], Match::Decoy),
            (vec![".No 99", COMMA], Match::Both),
        ] {
            head.push(&request_line);
            for at in [None, Some(0), Some(head.len())] {
                let head = insert_table(head.clone(), at);
                check(section, &head, Some(request), &[(name, expected)]);
            }
        }
    }
    for prefix in [
        ".No note",
        ".No 99",
        r#".No "Words here""#,
        ".Pa file",
        ".Sy note",
    ] {
        for enclosed in [false, true] {
            let mut head = vec![prefix, COMMA];
            head.extend(enclosed.then_some(".Bo"));
            head.push(".Er EINVAL");
            head.extend(enclosed.then_some(".Bc"));
            for at in [None, Some(head.len())] {
                let head = insert_table(head.clone(), at);
                check(
                    "DESCRIPTION",
                    &head,
                    Some("Er EINVAL"),
                    &[("EINVAL", Match::Both)],
                );
            }
        }
    }
    let prose = vec![r#".No "Words, here""#, ".Ev GOOD"];
    for at in [None, Some(prose.len())] {
        let head = insert_table(prose.clone(), at);
        check("ENVIRONMENT", &head, None, &[("Words", Match::Absent)]);
    }
}

#[test]
fn zero_width_style_controls_do_not_change_native_separator_identity() {
    // Fixed roff_escape.c classifies size/color as ESCAPE_IGNORE; term_word
    // still executes zero-width cells in the same No operand as the comma.
    for (section, sibling, name) in [
        ("OPTIONS", "Fl b", "-b"),
        ("DESCRIPTION", "Ev SECOND", "SECOND"),
        ("COMMANDS", "Cm next", "next"),
    ] {
        let sibling_line = format!(".{sibling}");
        for styled_comma in [r"\s+2\&,\s-2", r"\m[red]\&,\m[]", r"\M[blue]\&,\M[]"] {
            for (literal_prefix, expected) in [("", Match::Both), ("99", Match::Decoy)] {
                let punctuation = format!(".No {literal_prefix}{styled_comma}");
                for at in [None, Some(0), Some(3)] {
                    let head = insert_table(
                        vec![
                            r#".Fn run "void (*cb)(int, long)""#,
                            &punctuation,
                            &sibling_line,
                        ],
                        at,
                    );
                    let mut probes = vec![(name, expected)];
                    if literal_prefix.is_empty() {
                        probes.push(("run", Match::Owner));
                    }
                    check(section, &head, Some(sibling), &probes);
                }
            }
        }
    }
}

#[test]
fn a_native_option_run_consumes_its_own_key_without_joining_another_flag() {
    // termp_fl_pre emits the dash separately from its word. The second
    // invocation separated by Ns is not part of the first Fl operand.
    for (flag, probes) in [
        (".Fl q-b", &[("-q-b", Match::Owner)][..]),
        (
            ".Fl q Ns Fl b",
            &[
                ("-q-b", Match::Absent),
                ("-q", Match::Absent),
                ("-b", Match::Absent),
            ],
        ),
        (".Fl Fl long", &[("--long", Match::Owner)]),
        (
            ".Fl q Ar target",
            &[("-q", Match::Owner), ("target", Match::Absent)],
        ),
    ] {
        for at in [None, Some(0), Some(3)] {
            let head = insert_table(vec![r#".Fn run "int x""#, COMMA, flag], at);
            let expect: Vec<_> = [("run", Match::Owner)]
                .into_iter()
                .chain(probes.iter().copied())
                .collect();
            check("OPTIONS", &head, None, &expect);
        }
    }
}

fn assert_mixed_names(content: &ResolvedContent, names: &[&str], head: &[&str]) {
    let items = definitions(content.document.as_ref().unwrap());
    let owner = EntryOwner::Definition(items[0]);
    assert_eq!(owner.validated_names().unwrap(), names, "{head:?}");
    assert_eq!(owner.facts().unwrap().kind, EntryKind::Term, "{head:?}");
}
