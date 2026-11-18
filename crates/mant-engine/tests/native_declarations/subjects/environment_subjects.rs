use super::{DECLARATION_HEADER, check_query, explain, load, section_fixture};
use mant_ir::{EntryKind, Inline, LinkTarget, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceClass};

#[test]
fn environment_group_syntax_distinguishes_template_assignment_and_alias_boundaries() {
    // CVS mdoc_macro.c::in_line keeps punctuation inside one Ev operand,
    // but that same operand can carry complete author-written aliases.
    // The visible group grammar decides which delimiters are top-level.
    for (value, exact, assignment) in [
        ("BSD_{PROGRAM,SCRIPT,DATA,MAN}[_DIR]", None, false),
        ("BSD_[PROGRAM|SCRIPT]", None, false),
        ("BSD_<PROGRAM,SCRIPT>", None, false),
        ("BASH_FUNC_home()", Some("BASH_FUNC_home()"), false),
        ("VALUE", Some("VALUE"), false),
        ("PATH=a,b", Some("PATH"), true),
    ] {
        for separator in [",", "|"] {
            for explicit in [false, true] {
                let gap = if explicit && separator == "|" {
                    " "
                } else {
                    ""
                };
                let (syntax, space) = if explicit { (" Ev ", " ") } else { ("", "") };
                let before = if explicit { " " } else { "" };
                let name = exact.into_iter();
                for value_first in [true, false] {
                    let (left, right, names) = if value_first {
                        let mut names = name.clone().collect::<Vec<_>>();
                        if !assignment || explicit {
                            names.push("OTHER");
                        }
                        (value, "OTHER", names)
                    } else {
                        (
                            "OTHER",
                            value,
                            [Some("OTHER"), exact].into_iter().flatten().collect(),
                        )
                    };
                    let head = format!("Ev {left}{before}{separator}{syntax}{right}");
                    let form = format!("{left}{gap}{separator}{space}{right}");
                    check(
                        &head,
                        &names,
                        &form,
                        &["PROGRAM", "SCRIPT", "DATA", "MAN", "a", "b"],
                    );
                }
            }
        }
    }
    for (head, names, form) in [
        (
            "Ev BASH_FUNC_home() , PATH",
            &["BASH_FUNC_home()", "PATH"][..],
            "BASH_FUNC_home(), PATH",
        ),
        (
            "Ev BSD_{PROGRAM=ONE,SCRIPT},OTHER",
            &["OTHER"][..],
            "BSD_{PROGRAM=ONE,SCRIPT},OTHER",
        ),
        (
            "Ev BSD_{PROGRAM,SCRIPT}|OTHER",
            &["OTHER"][..],
            "BSD_{PROGRAM,SCRIPT}|OTHER",
        ),
        ("Ev PATH=foo[,b", &["PATH"][..], "PATH=foo[,b"),
        ("Ev A1,B2", &["A1", "B2"][..], "A1,B2"),
        ("Ev A1=foo,bar| Ev B2", &["A1", "B2"][..], "A1=foo,bar| B2"),
        ("Ev A1=foo,bar| B2", &["A1"][..], "A1=foo,bar| B2"),
        (
            "Ev A1=foo,bar| Ns Ev B2",
            &["A1", "B2"][..],
            "A1=foo,bar|B2",
        ),
        ("Ev A1=foo,bar, Ev B2", &["A1", "B2"][..], "A1=foo,bar, B2"),
        ("Ev A1=foo,bar, B2", &["A1"][..], "A1=foo,bar, B2"),
        ("Ev A,B Ns Ar C", &["A"][..], "A,BC"),
    ] {
        check(head, names, form, &["PROGRAM", "SCRIPT", "ONE", "b"]);
    }
}

#[test]
fn internal_styles_preserve_the_whole_executed_environment_name() {
    // termp_under_pre applies the supplemental macro's style after Ev;
    // intra-word roff font escapes stay inside the original Ev operand.
    for (tail, suffix, heading) in [
        ("", "", ""),
        (
            "Sx FILES,FAKE",
            "FILES,FAKE",
            ".Sh \"FILES,FAKE\"\nDESTINATION.\n",
        ),
        ("Sx FILES", "FILES", ".Sh FILES\nDESTINATION.\n"),
        ("Pa files,FAKE", "files,FAKE", ""),
        ("Pa FILE,FAKE", "FILE,FAKE", ""),
        ("Pa FILES", "FILES", ""),
        (
            "Lk https://example.test/a,b FILE",
            "FILE: https://example.test/a,b",
            "",
        ),
        ("Mt y@example.test", "y@example.test", ""),
        ("Em FILE", "FILE", ""),
        ("Ar FILE", "FILE", ""),
        ("Ar A=B", "A=B", ""),
        ("Dv FILE", "FILE", ""),
        ("Ic FILE", "FILE", ""),
        ("Sy FILE", "FILE", ""),
        ("Li FILE", "FILE", ""),
        ("Vt int", "int", ""),
        ("Fa arg", "arg", ""),
        ("Pq Ic FILE", "(FILE)", ""),
        ("No FILE", "FILE", ""),
    ] {
        for (spelling, emphasis, strong) in [
            ("HOME", false, false),
            (r"H\fIOM\fPE", true, false),
            (r"H\fBOM\fPE", false, true),
        ] {
            for (join, space) in [("", " "), (" Ns", "")] {
                if tail.is_empty() && !join.is_empty() {
                    continue;
                }
                let head = if tail.is_empty() {
                    format!("Ev {spelling}")
                } else {
                    format!("Ev {spelling}{join} {tail}")
                };
                let form = if tail.is_empty() {
                    "HOME".to_owned()
                } else {
                    format!("HOME{space}{suffix}")
                };
                let query = load(&format!(
                    "{DECLARATION_HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It {head}\nOWNERBODY.\n.El\n{heading}"
                ));
                let items = super::super::super::definitions(query.document.as_ref().unwrap());
                let term = &items[0].terms[0];
                assert_eq!(inline_plain_text(term), form, "{head}");
                assert_name_style(term, &head, emphasis, strong);
                if !heading.is_empty() {
                    assert_link_to_section(term, suffix, &query);
                } else if tail.starts_with("Lk") {
                    assert!(term.iter().any(|node| matches!(
                        node, Inline::Link { target: LinkTarget::External { uri }, children, .. }
                            if uri == "https://example.test/a,b" && inline_plain_text(children) == "FILE"
                    )), "{head}");
                }
                let foreign_join =
                    !join.is_empty() && !tail.starts_with("Pq ") && !tail.starts_with("No ");
                if foreign_join {
                    assert!(items[0].entry.is_none(), "{head}");
                    assert_eq!(
                        explain(&query, &form).counts.direct_entry.total,
                        0,
                        "{head}"
                    );
                    assert!(mant_render::render_query_text(&query).contains("OWNERBODY."));
                    continue;
                }
                let names =
                    if (join.is_empty() || tail.starts_with("Pq ")) && !tail.starts_with("No ") {
                        &["HOME"][..]
                    } else if !join.is_empty() && tail.starts_with("No ") {
                        &["HOMEFILE"][..]
                    } else {
                        &[][..]
                    };
                check_query(
                    &query,
                    &head,
                    EntryKind::EnvironmentVariable,
                    names,
                    &form,
                    &["FAKE", "FILES", "FILE", "b"],
                );
            }
        }
    }
}

fn assert_name_style(term: &[Inline], head: &str, emphasis: bool, strong: bool) {
    if emphasis {
        assert!(
            term.iter().any(|node| matches!(
                node, Inline::Emphasis { children } if inline_plain_text(children) == "OM"
            )),
            "{head}"
        );
    }
    if strong {
        assert!(
            term.iter().any(|node| matches!(
                node, Inline::Strong { children } if inline_plain_text(children) == "OM"
            )),
            "{head}"
        );
    }
}

#[test]
fn literal_parenthesis_preserves_scoped_references_and_literal_operands() {
    // mdoc_macro.c::in_line emits literal punctuation outside the Pa/Lk/No
    // operands; the reference has no native BODY ID but stays inside the pair.
    for (tail, visible) in [
        ("Pa default , fake", "default, fake"),
        ("No default , fake", "default, fake"),
        (
            "Lk https://e.test default , fake",
            "default, fake: https://e.test",
        ),
    ] {
        check(
            &format!("Ev SHELL ( {tail} ) , Ev PATH"),
            &["SHELL", "PATH"],
            &format!("SHELL ({visible}), PATH"),
            &["fake", "default"],
        );
    }
}

#[test]
fn unproved_term_stays_visible_without_shadowing_a_proved_alternative() {
    let query = load(&format!(
        "{DECLARATION_HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Xo Ev HOME Ns Sx FILES\n.Pp\n.Ev GOOD\n.Xc\nOWNERBODY.\n.El\n.Sh FILES\nDESTINATION.\n"
    ));
    let document = query.document.as_ref().unwrap();
    let items = super::super::super::definitions(document);
    let item = items[0];
    assert_eq!(
        item.terms
            .iter()
            .map(|term| inline_plain_text(term))
            .collect::<Vec<_>>(),
        ["HOMEFILES\n", "GOOD"]
    );
    assert_link_to_section(&item.terms[0], "FILES", &query);
    let owner = mant_ir::EntryOwner::Definition(item);
    assert_eq!(owner.validated_names().unwrap(), ["GOOD"]);
    assert_eq!(
        owner
            .forms()
            .unwrap()
            .iter()
            .map(inline_plain_text)
            .collect::<Vec<_>>(),
        ["GOOD"]
    );
    for (query_text, direct) in [("GOOD", 1), ("HOMEFILES", 0)] {
        assert_eq!(
            explain(&query, query_text).counts.direct_entry.total,
            direct
        );
    }
}

#[test]
fn the_declared_name_and_section_link_keep_different_owners() {
    let query = load(&format!(
        "{DECLARATION_HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Ev H\\fIOM\\fPE Sx FILES,FAKE\nFIRST_OWNER_SHARED\n.It Ev OTHER\nSECOND_OWNER_SHARED\n.El\n.Sh \"FILES,FAKE\"\nDESTINATION.\n"
    ));
    let items = super::super::super::definitions(query.document.as_ref().unwrap());
    assert_link_to_section(&items[0].terms[0], "FILES,FAKE", &query);
    assert_eq!(explain(&query, "FAKE").counts.direct_entry.total, 0);
    let result = explain(&query, "HOME");
    assert_eq!(result.counts.direct_entry.total, 1);
    let hit = result
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source, items[0].source);
    let excerpt =
        mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())]).unwrap();
    let text = mant_render::render_excerpt_text(&excerpt);
    assert!(text.contains("FIRST_OWNER_SHARED"));
    assert!(!text.contains("SECOND_OWNER_SHARED"));
}

fn assert_link_to_section(term: &[Inline], label: &str, query: &mant_ir::ResolvedContent) {
    let links = term
        .iter()
        .filter_map(|node| match node {
            Inline::Link {
                target: LinkTarget::Section { id },
                children,
                ..
            } if inline_plain_text(children) == label => Some(id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [id] = links.as_slice() else {
        panic!("one preserved Sx: {term:?}");
    };
    let excerpt = mant_query::select_excerpt(query, &[ContentSelector::id(id.as_str())]).unwrap();
    assert!(mant_render::render_excerpt_text(&excerpt).contains("DESTINATION."));
}

fn check(head: &str, names: &[&str], form: &str, unbound: &[&str]) {
    let query = section_fixture("ENVIRONMENT", "", head);
    check_query(
        &query,
        head,
        EntryKind::EnvironmentVariable,
        names,
        form,
        unbound,
    );
}
