//! Independent declaration clauses after a weak or unsupported prefix.
use super::{definitions, exact_word, load, primary::binding_start, separators::assert_query};
use mant_ir::{EntryOwner, ResolvedContent, inline_plain_text};

const HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n";
const TABLE: &str = ".TS\nl.\nSENTINEL\n.TE";
const HERE: Match = Match::Owner;
const OTHER: Match = Match::Decoy;
const BOTH: Match = Match::Both;
const NEITHER: Match = Match::Absent;

#[derive(Clone, Copy)]
pub(super) enum Match {
    Owner,
    Decoy,
    Both,
    Absent,
}

#[test]
fn weak_prefixes_do_not_veto_a_separately_executed_command() {
    // mdoc_macro.c::in_line leaves a distinct Cm after each weak prefix.
    // The same-spelled second It must never mask a lost first owner.
    for weak in [
        r#".No "Words here""#,
        ".Pa /tmp/a",
        ".Xr printf 3",
        ".No 99",
    ] {
        for sep in [r".No \&,", r".No \&|"] {
            for position in [None, Some(1), Some(2), Some(3)] {
                let head = insert_table(vec![weak, sep, ".Cm next"], position);
                check("COMMANDS", &head, Some("Cm next"), &[("next", BOTH)]);
            }
        }
    }
    for head in [
        &[r#".No "Words here""#, ".Cm next"][..],
        &[r#".No "Words here""#, r".No \&,", ".No next"],
        &[r#".No "Words here""#, r".No \&,", ".Em next"],
    ] {
        for position in [0, head.len()] {
            let head = insert_table(head.to_vec(), Some(position));
            check(
                "COMMANDS",
                &head,
                Some("Cm next"),
                &[("next", OTHER), ("Words here", NEITHER)],
            );
        }
    }
}

#[test]
fn displayed_punctuation_inside_one_no_word_does_not_separate_commands() {
    // dword() keeps all these characters inside one No TEXT. A table
    // outside both operands cannot turn the punctuation into a separator.
    // An actual intervening table still separates two reading intervals.
    for weak in [
        r".No 99\&,",
        r".No 99\&|",
        r#".No "Words here\&,""#,
        r#".No "Words, here""#,
        r#".No "Words here , ""#,
    ] {
        for position in [None, Some(0), Some(1), Some(2)] {
            let head = insert_table(vec![weak, ".Cm next"], position);
            let expected = if position == Some(1) { BOTH } else { OTHER };
            check("COMMANDS", &head, Some("Cm next"), &[("next", expected)]);
        }
    }
}

#[test]
fn actual_separators_reopen_other_native_declaration_grammars() {
    // The fixed tree keeps the later Er, Fl, Ev, Cm, Fn and Fo operands
    // independent of the weak first No and its following punctuation.
    for (section, last, name) in [
        ("DESCRIPTION", &[".Er EINVAL"][..], "EINVAL"),
        ("OPTIONS", &[".Fl b"], "-b"),
        ("ENVIRONMENT", &[".Ev SECOND"], "SECOND"),
        ("COMMANDS", &[".Cm next"], "next"),
        ("DESCRIPTION", &[r#".Fn get "int x""#], "get"),
        ("DESCRIPTION", &[".Fo get", r#".Fa "int x""#, ".Fc"], "get"),
    ] {
        let head: Vec<_> = [".No 99", r".No \&,"]
            .into_iter()
            .chain(last.iter().copied())
            .collect();
        for position in [None, Some(0), Some(1), Some(2), Some(head.len())] {
            let head = insert_table(head.clone(), position);
            check(section, &head, None, &[(name, HERE), ("99", NEITHER)]);
        }
    }
    // An escape can put a comma or pipe inside the weak operand without
    // making it a separate native separator. Check these spelled controls.
    for weak in [r".No 99\&,", r".No 99\&|"] {
        for (declaration, decoy, name) in [
            (&[".Er EINVAL"][..], "Er EINVAL", "EINVAL"),
            (&[r#".Fn get "int x""#][..], r#"Fn get "int x""#, "get"),
            (
                &[".Fo get", r#".Fa "int x""#, ".Fc"][..],
                r#"Fn get "int x""#,
                "get",
            ),
        ] {
            let head: Vec<_> = std::iter::once(weak)
                .chain(declaration.iter().copied())
                .collect();
            for position in [None, Some(0), Some(head.len())] {
                let head = insert_table(head.clone(), position);
                check("DESCRIPTION", &head, Some(decoy), &[(name, OTHER)]);
            }
        }
    }
}

#[test]
fn one_no_operand_can_write_a_connector_in_multiple_events() {
    // term_word() changes font or emits a zero-width cell between the
    // same two visible glyphs. Neither operation creates another word.
    for separator in [".No or", r".No o\fBr\fP", r".No o\&r"] {
        for position in [None, Some(1), Some(2)] {
            let errors = insert_table(vec![".Er EFAULT", separator, ".Er EINVAL"], position);
            check("DESCRIPTION", &errors, None, &[("or", NEITHER)]);
            let flags = insert_table(vec![".Fl a", separator, ".Fl b"], position);
            check(
                "OPTIONS",
                &flags,
                Some("Fl b"),
                &[("-a", HERE), ("-b", BOTH), ("or", NEITHER)],
            );
        }
    }
}

#[test]
fn a_proven_option_connector_keeps_the_following_transparent_scope() {
    // blk_part_exp opens the Eo BODY after the actual connector; with no
    // visible delimiters its distinct Fl is still part of the alternatives.
    for separator in [".No or", r".No \&,"] {
        for position in [None, Some(0), Some(1), Some(2), Some(3), Some(5)] {
            let flags = insert_table(vec![".Fl a", separator, ".Eo", ".Fl b", ".Ec"], position);
            check(
                "OPTIONS",
                &flags,
                Some("Fl b"),
                &[("-a", HERE), ("-b", BOTH), ("or", NEITHER)],
            );
        }
    }
    // A closed earlier supplemental Po must not own the next option slot.
    for (open, close) in [(".Po", ".Pc"), (".Eo", ".Ec")] {
        for separator in [r".No \&,", r".No \&|"] {
            for position in [1, 5, 7] {
                let head = insert_table(
                    vec![
                        ".Fl a",
                        open,
                        ".Fl hidden",
                        close,
                        separator,
                        ".Fl b",
                        ".No or",
                        ".Eo",
                        ".Fl c",
                        ".Ec",
                    ],
                    Some(position),
                );
                check(
                    "OPTIONS",
                    &head,
                    Some("Fl c"),
                    &[
                        ("-a", HERE),
                        ("-b", HERE),
                        ("-c", BOTH),
                        ("-hidden", NEITHER),
                        ("or", NEITHER),
                    ],
                );
            }
        }
    }
}

#[test]
fn generated_function_delimiters_keep_their_subject_and_arguments() {
    // termp_fn_pre() and termp_fo_pre/post() generate these parentheses as
    // part of the signature. An authored Po still has a separate scope.
    for signature in [
        &[".Fn get"][..],
        &[r#".Fn get "int x""#],
        &[".Fo get", r#".Fa "int x""#, ".Fc"],
    ] {
        for wrapped in [false, true] {
            let wrapped_signature = wrap(signature, wrapped);
            for position in [None, Some(0), Some(wrapped_signature.len())] {
                let head = insert_table(wrapped_signature.clone(), position);
                check(
                    "DESCRIPTION",
                    &head,
                    Some(r#"Fn get "int x""#),
                    &[("get", BOTH), ("x", NEITHER)],
                );
            }
        }
    }
    for position in [1, 2] {
        for wrapped in [false, true] {
            let signature = insert_table(vec![".Fo get", r#".Fa "int x""#, ".Fc"], Some(position));
            let head = wrap(&signature, wrapped);
            check(
                "DESCRIPTION",
                &head,
                Some("Fn get"),
                &[("get", BOTH), ("x", NEITHER)],
            );
        }
    }
}

#[test]
fn neutral_display_does_not_restart_a_rejected_alternative() {
    // The second Er belongs to the unsupported alternative even after tbl
    // executes in the same It HEAD. The actual comma opens a new subject.
    for inner in [false, true] {
        let head = [
            ".Er EFAULT",
            TABLE,
            ".No or",
            if inner { TABLE } else { "" },
            ".Er EINVAL",
            r".No \&,",
            ".Er EACCES",
        ];
        let head: Vec<_> = head
            .into_iter()
            .filter(|request| !request.is_empty())
            .collect();
        check(
            "DESCRIPTION",
            &head,
            Some("Er EINVAL"),
            &[
                ("EFAULT", HERE),
                ("EINVAL", OTHER),
                ("EACCES", HERE),
                ("or", NEITHER),
            ],
        );
    }
}

pub(super) fn insert_table(mut head: Vec<&str>, position: Option<usize>) -> Vec<&str> {
    if let Some(index) = position {
        head.insert(index, TABLE);
    }
    head
}

fn wrap<'a>(head: &[&'a str], wrapped: bool) -> Vec<&'a str> {
    wrapped
        .then_some(".Po")
        .into_iter()
        .chain(head.iter().copied())
        .chain(wrapped.then_some(".Pc"))
        .collect()
}

pub(super) fn check(
    section: &str,
    head: &[&str],
    decoy: Option<&str>,
    expectations: &[(&str, Match)],
) -> ResolvedContent {
    let head_text = head.join("\n");
    let decoy_text = decoy.map_or_else(String::new, |decoy| format!(".It {decoy}\nDECOYBODY.\n"));
    let source = format!(
        "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It Xo\n{head_text}\n.Xc\nOWNERBODY.\n{decoy_text}.El\n"
    );
    let content = load(&source);
    let items = definitions(content.document.as_ref().unwrap());
    assert_eq!(items.len(), 1 + usize::from(decoy.is_some()), "{source}");
    let owner = EntryOwner::Definition(items[0]);
    let visible = inline_plain_text(&items[0].terms[0]);
    let tables = head.iter().filter(|request| **request == TABLE).count();
    assert_eq!(exact_word(&content, "SENTINEL"), tables, "{source}");
    assert_query(&content, "SENTINEL", &[], &source);
    assert!(
        owner.forms().is_none_or(|forms| forms
            .iter()
            .all(|form| !inline_plain_text(form).contains("SENTINEL"))),
        "{source}"
    );
    for &(name, matches) in expectations {
        let source_owner = (items[0], "OWNERBODY.");
        let check_binding = || {
            assert_eq!(
                binding_start(owner, name),
                visible.find(name).unwrap(),
                "{name}\n{source}"
            );
        };
        match matches {
            Match::Owner => {
                check_binding();
                assert_query(&content, name, &[source_owner], &source);
            }
            Match::Decoy => assert_query(&content, name, &[(items[1], "DECOYBODY.")], &source),
            Match::Both => {
                check_binding();
                assert_query(
                    &content,
                    name,
                    &[source_owner, (items[1], "DECOYBODY.")],
                    &source,
                );
            }
            Match::Absent => assert_query(&content, name, &[], &source),
        }
    }
    content
}
