//! Declaration names are narrower than their complete displayed forms.
use super::{check, explain, fixture, load, roundtrip};
use mant_ir::EntryKind::Term;
use mant_protocol::{ContentSelector, EvidenceClass};

#[test]
fn numbered_errors_bind_the_symbol_through_styled_descriptions() {
    // In pinned CVS mdoc_macro.c::in_line closes Er at Em. The number and
    // symbol are separate TEXT children of Er, with Em under the It HEAD.
    for (number, number_text) in [("", ""), ("1 ", "1 ")] {
        for (description, displayed) in [
            ("", ""),
            (
                r#" Em "Operation not permitted""#,
                " Operation not permitted",
            ),
        ] {
            let (punctuation, printed_punctuation) =
                if !number.is_empty() && !description.is_empty() {
                    (" .", ".")
                } else {
                    ("", "")
                };
            check(
                &format!("Er {number}EPERM{description}{punctuation}"),
                false,
                Term,
                &["EPERM"],
                &format!("{number_text}EPERM{displayed}{printed_punctuation}"),
                &["1", "Operation", "permitted"],
            );
        }
    }
    for (head, names, form, ordinary_words) in [
        (
            r#"Er 1 EPERM Em "Operation, not permitted" , Er 2 ENOENT"#,
            &["EPERM", "ENOENT"][..],
            "1 EPERM Operation, not permitted, 2 ENOENT",
            &["Operation", "permitted"][..],
        ),
        (
            r#"Er 0 EOK Sy "No error" :"#,
            &["EOK"],
            "0 EOK No error:",
            &["0", "error"],
        ),
        (
            r#"Er \-1 ERESTART Em "Restart syscall" ."#,
            &["ERESTART"],
            "-1 ERESTART Restart syscall.",
            &["-1", "syscall"],
        ),
        ("Er −1 ERESTART", &["ERESTART"], "−1 ERESTART", &["−1"]),
        ("Er +1 ERESTART", &["ERESTART"], "+1 ERESTART", &["+1"]),
        (r#"Er "1 EPERM""#, &[], "1 EPERM", &["EPERM"]),
        ("Er 1 2 EPERM", &[], "1 2 EPERM", &["EPERM"]),
        ("Er 1 EPERM OTHER", &["EPERM"], "1 EPERM OTHER", &["OTHER"]),
    ] {
        check(head, false, Term, names, form, ordinary_words);
    }
}

#[test]
fn constant_arguments_keep_their_roles_through_native_parentheses() {
    // In the fixed tree Fa, Ar and Vt are independent siblings of Dv,
    // or children of the Pq BODY. All remain in the complete printed form.
    for role in ["Fa", "Ar", "Vt"] {
        for wrapped in [false, true] {
            let wrapper = if wrapped { "Pq " } else { "" };
            for (text, shown) in [("int", "int"), (r#""int *x""#, "int *x")] {
                let parameter = if wrapped {
                    format!("({shown})")
                } else {
                    shown.to_owned()
                };
                check(
                    &format!("Dv GET {wrapper}{role} {text}"),
                    false,
                    Term,
                    &["GET"],
                    &format!("GET {parameter}"),
                    &["int", "x"],
                );
            }
        }
    }
    check(
        "Dv GET Pq Vt int .",
        false,
        Term,
        &["GET"],
        "GET (int).",
        &["int"],
    );
    check(
        "Dv GET Pq Vt int Em note",
        false,
        Term,
        &["GET"],
        "GET (int note)",
        &["int", "note"],
    );
}

#[test]
fn styled_suffixes_need_a_visible_boundary_or_native_delimiter() {
    // mdoc_term.c::termp_ns_pre does not insert a separator between words.
    for (head, space_off, form, symbol) in [
        ("Er FIRST Ns Em tail", false, "FIRSTtail", "FIRST"),
        ("Dv SYS Ns Em .cache", false, "SYS.cache", "SYS"),
        ("Dv GET Vt int", true, "GETint", "GET"),
    ] {
        check(head, space_off, Term, &[], form, &[symbol]);
    }
    check(
        "Dv GET Pq Vt int",
        true,
        Term,
        &["GET"],
        "GET(int)",
        &["int"],
    );
}

#[test]
fn separate_native_symbols_require_a_real_group_boundary() {
    // CVS mdoc_macro.c::in_line reopens Dv after comma/pipe, so only the
    // separated variants have independent subject receipts and delimiters.
    for (head, names, form, other_words) in [
        (
            "Dv FIONREAD int",
            &["FIONREAD"][..],
            "FIONREAD int",
            &["int"][..],
        ),
        (
            "Dv securelevel and init",
            &["securelevel"],
            "securelevel and init",
            &["and", "init"],
        ),
        (
            "Dv PPIGDATA PPISDATA",
            &["PPIGDATA"],
            "PPIGDATA PPISDATA",
            &["PPISDATA"],
        ),
        (
            "Dv PPIGDATA , PPISDATA",
            &["PPIGDATA", "PPISDATA"],
            "PPIGDATA, PPISDATA",
            &[],
        ),
        (
            "Dv PPIGDATA , Dv PPISDATA",
            &["PPIGDATA", "PPISDATA"],
            "PPIGDATA, PPISDATA",
            &[],
        ),
        (
            "Dv PPIGDATA | PPISDATA",
            &["PPIGDATA", "PPISDATA"],
            "PPIGDATA | PPISDATA",
            &[],
        ),
        (
            "Dv PPIGDATA Dv PPISDATA",
            &[],
            "PPIGDATA PPISDATA",
            &["PPIGDATA", "PPISDATA"],
        ),
        (
            "Dv PPIGDATA No + Dv PPISDATA",
            &[],
            "PPIGDATA + PPISDATA",
            &["PPIGDATA", "PPISDATA"],
        ),
        ("Dv 0 INVALID", &[], "0 INVALID", &["INVALID"]),
        (r#"Dv "GET int""#, &[], "GET int", &["GET", "int"]),
        (
            "Fa first Fa second",
            &[],
            "first second",
            &["first", "second"],
        ),
        (
            "Fa first , Fa second",
            &["first", "second"],
            "first, second",
            &[],
        ),
    ] {
        check(head, false, Term, names, form, other_words);
    }
}

#[test]
fn argument_groups_and_subject_roles_advance_before_names_are_decided() {
    // Pinned CVS makes `a` and `b` distinct Fa siblings both with and
    // without a comma between them. Closing Pc returns to the outer HEAD.
    for (head, form, absent) in [
        (
            "Dv GET , Fa a Fa b , Dv STOP",
            "GET, a b, STOP",
            &["a", "b", "STOP"][..],
        ),
        (
            "Dv GET , Fa a , Fa b , Dv STOP",
            "GET, a, b, STOP",
            &["a", "b", "STOP"],
        ),
        (
            "Xo Dv GET ,\n.Po\n.Fa a\n.Fa b\n.Pc ,\n.Dv STOP\n.Xc",
            "GET, (a b), STOP",
            &["a", "b", "STOP"],
        ),
    ] {
        check(head, false, Term, &["GET"], form, absent);
    }
    for (prefix, names, form) in [
        (
            "Dv START , ",
            &["START", "field", "END"][..],
            "START, bad[2], field, END",
        ),
        ("", &["field", "END"], "bad[2], field, END"),
    ] {
        check(
            &format!("{prefix}Va bad[2] , Fa field , Dv END"),
            false,
            Term,
            names,
            form,
            &["bad"],
        );
    }
    for (head, form) in [
        (
            r#"Dv START , Vt "const char *" , Fa field , Dv END"#,
            "START, const char *, field, END",
        ),
        (
            "Dv START , Er 12 , Fa field , Dv END",
            "START, 12, field, END",
        ),
    ] {
        check(
            head,
            false,
            Term,
            &["START", "field", "END"],
            form,
            &["char", "12"],
        );
    }
}

#[test]
fn field_eligibility_is_invariant_to_unrelated_prefixes() {
    // Pinned CVS Eo's HEAD and TAIL are empty. It still owns BODY/Fa.
    let query = fixture("Xo\n.Eo\n.Fa arg\n.Ec\n.Xc", false);
    let items = super::super::super::definitions(query.document.as_ref().unwrap());
    assert_eq!(items.len(), 1);
    assert!(items[0].entry.is_none());
    assert_eq!(mant_ir::inline_plain_text(&items[0].terms[0]), "arg");
    assert!(mant_render::render_query_text(&query).contains("OWNERBODY."));
    assert_eq!(explain(&query, "arg").counts.direct_entry.total, 0);
    check(
        "Xo Va extra ,\n.Eo\n.Fa arg\n.Ec\n.Xc",
        false,
        mant_ir::EntryKind::Variable,
        &["extra"],
        "extra, arg",
        &["arg"],
    );
    check(
        "Xo\n.Eo <<\n.Fa arg\n.Ec >>\n.Xc",
        false,
        Term,
        &["arg"],
        "<<arg>>",
        &[],
    );
}

#[test]
fn identical_symbols_keep_separate_owner_bodies_and_mentions() {
    let query = load(concat!(
        ".Dd November 18, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd checker\n",
        ".Sh DESCRIPTION\n.Bl -tag -width Ds\n",
        ".It Er 1 EPERM Em \"operation denied\" .\nFIRSTBODY.\n",
        ".It Er 1 EPERM Em \"operation denied\" .\nSECONDBODY.\n.El\n",
    ));
    let result = roundtrip(&explain(&query, "EPERM"));
    assert_eq!(result.counts.direct_entry.total, 2);
    for (hit, body) in result
        .evidence
        .iter()
        .filter(|hit| hit.class == EvidenceClass::DirectEntry)
        .zip(["FIRSTBODY.", "SECONDBODY."])
    {
        let by_id = roundtrip(
            &mant_query::select_excerpt(&query, &[ContentSelector::id(hit.outline.node.id())])
                .unwrap(),
        );
        let by_path =
            mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        assert_eq!(by_id, by_path);
        let text = mant_render::render_excerpt_text(&by_id);
        assert!(text.contains(body), "{text}");
    }
    assert_eq!(explain(&query, "operation").counts.direct_entry.total, 0);
}

#[test]
fn identical_native_operands_outside_definition_heads_are_mentions() {
    let query = load(concat!(
        ".Dd November 18, 2026\n.Dt CHECK 1\n.Os\n.Sh NAME\n.Nm check\n.Nd checker\n",
        ".Sh SYNOPSIS\n.Er 1 ESYN Em \"unowned synopsis\" .\n",
        ".Sh DESCRIPTION\n.Dv DESCRIBED Ar int\n",
        ".Bl -tag -width Ds\n.It Dv OWNER Vt int\n.Er 2 EBODY Em \"unowned body\" .\n.El\n",
        ".Bl -column Ds\n.It Bq Er 3 ETABLE Em \"table note\"\n.El\n",
    ));
    for (name, class) in [
        ("ESYN", EvidenceClass::ContextMention),
        ("DESCRIBED", EvidenceClass::ContextMention),
        ("EBODY", EvidenceClass::EntryMention),
        ("ETABLE", EvidenceClass::ContextMention),
    ] {
        let result = explain(&query, name);
        assert_eq!(result.counts.direct_entry.total, 0, "{name}");
        assert!(
            result.evidence.iter().any(|hit| hit.class == class),
            "{name}: {result:?}"
        );
    }
    assert_eq!(explain(&query, "OWNER").counts.direct_entry.total, 1);
}
