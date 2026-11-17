use super::{
    definitions,
    support::{explain, load},
};
use mant_ir::{EntryKind, EntryOwner, ParameterKind, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceBasis, EvidenceClass};

const HEADER: &str = ".Dd November 17, 2026\n.Dt TEST 3\n.Os\n.Sh NAME\n.Nm test\n.Nd role probe\n";

fn example(section: &str, head: &str) -> mant_ir::ResolvedContent {
    load(&format!(
        "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\nSUBJECT_BODY\n.El\n"
    ))
}

fn assert_subject(
    query: &mant_ir::ResolvedContent,
    head: &str,
    kind: EntryKind,
    names: &[&str],
    form: &str,
) {
    let items = definitions(query.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{head}")
    };
    let owner = EntryOwner::Definition(item);
    assert_eq!(owner.facts().unwrap().kind, kind, "{head}");
    assert_eq!(owner.validated_names().unwrap(), names, "{head}");
    assert_eq!(
        owner
            .forms()
            .unwrap()
            .iter()
            .map(inline_plain_text)
            .collect::<Vec<_>>(),
        [form],
        "{head}"
    );
    for name in names {
        let result = explain(query, name);
        assert_eq!(result.counts.direct_entry.total, 1, "{head} / {name}");
        let hit = &result.evidence[0];
        assert!(
            hit.bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        );
        let excerpt =
            mant_query::select_excerpt(query, &[ContentSelector::path(hit.outline.path())])
                .unwrap();
        assert!(mant_render::render_excerpt_text(&excerpt).contains("SUBJECT_BODY"));
    }
}

#[test]
fn native_subjects_precede_inherited_section_categories() {
    // The pinned tree uses the same HEAD operands in all three sections;
    // mdoc_term.c's Er, Dv, Fn, Ar, Va, Vt and Fa handlers keep their role
    // independently of the effective font or Sh title.
    use EntryKind::{Command, EnvironmentVariable, Term, Variable};
    for section in ["DESCRIPTION", "ENVIRONMENT", "COMMANDS"] {
        for (head, kind, name, form) in [
            ("Va local_value", Variable, "local_value", "local_value"),
            ("Dv EXAMPLE_LIMIT", Term, "EXAMPLE_LIMIT", "EXAMPLE_LIMIT"),
            ("Er EIO", Term, "EIO", "EIO"),
            ("Vt size_t", Term, "size_t", "size_t"),
            (r#"Fn checked "int x""#, Term, "checked", "checked(int x)"),
            ("Ar file", Term, "file", "file"),
            ("Fa field", Term, "field", "field"),
            (
                "Ev EXAMPLE_PATH",
                EnvironmentVariable,
                "EXAMPLE_PATH",
                "EXAMPLE_PATH",
            ),
            (
                "Fl q",
                EntryKind::Parameter {
                    parameter_kind: ParameterKind::Option,
                },
                "-q",
                "-q",
            ),
        ] {
            assert_subject(&example(section, head), head, kind, &[name], form);
        }
        for (head, name) in [("Ic redraw", "redraw"), ("Cm change", "change")] {
            let kind = match section {
                "COMMANDS" => Command,
                "ENVIRONMENT" => EnvironmentVariable,
                _ => Term,
            };
            assert_subject(&example(section, head), head, kind, &[name], name);
        }
    }
}

#[test]
fn accepted_variables_and_symbols_bind_the_subject_and_keep_other_content() {
    // Va and Dv execute their actual children. Types, braces and other
    // arguments stay visible in the complete form.
    use EntryKind::{Term, Variable};
    let cases = [
        (
            r"Va const char *cache",
            Variable,
            &["cache"][..],
            "const char *cache",
        ),
        (
            r#"Va "const char *cache""#,
            Variable,
            &["cache"][..],
            "const char *cache",
        ),
        (
            r#"Va "struct stat *state""#,
            Variable,
            &["state"][..],
            "struct stat *state",
        ),
        (
            r#"Va "void (*handler)(int)""#,
            Term,
            &[][..],
            "void (*handler)(int)",
        ),
        (r#"Va "unsigned int""#, Term, &[][..], "unsigned int"),
        (r#"Va "int *""#, Term, &[][..], "int *"),
        (r#"Va "int values[4]""#, Term, &[][..], "int values[4]"),
        ("Va first second", Term, &[][..], "first second"),
        (
            r#"Dv BIOCGBLEN Fa "u_int *""#,
            Term,
            &["BIOCGBLEN"][..],
            "BIOCGBLEN u_int *",
        ),
        (
            "Dv BPF_LD No + Dv BPF_IMM",
            Term,
            &[][..],
            "BPF_LD + BPF_IMM",
        ),
        (
            "Bq Er EIO , Er EINVAL",
            Term,
            &["EIO", "EINVAL"][..],
            "[EIO, EINVAL]",
        ),
    ];
    for (head, kind, names, form) in cases {
        assert_supported_declaration(head, kind, names, form);
    }
}

#[test]
fn accepted_function_operands_bind_the_subject_and_keep_types_and_arguments() {
    // termp_fn_pre uses the actual first operand. post_fo drops excess
    // HEAD operands before termp_fo_pre sees them.
    use EntryKind::Term;
    let cases = [
        (
            r#"Fn "int checked" "int x""#,
            Term,
            &["checked"][..],
            "int checked(int x)",
        ),
        (
            r#"Fn int checked "int x""#,
            Term,
            &[][..],
            "int(checked, int x)",
        ),
        (
            r#"Fn "const char *checked" "char **x""#,
            Term,
            &["checked"][..],
            "const char *checked(char **x)",
        ),
        (
            r#"Fn checked "int (*callback)(int)""#,
            Term,
            &["checked"][..],
            "checked(int (*callback)(int))",
        ),
        (
            r#"Fn "void (*handler)(int)""#,
            Term,
            &[][..],
            "void (*handler)(int)()",
        ),
        (
            "Xo\n.Fo checked\n.Fa \"int x\"\n.Fa \"char *out\"\n.Fc\n.Xc",
            Term,
            &["checked"][..],
            "checked(int x, char *out)",
        ),
        (
            "Xo\n.Fo checked ignored\n.Fa \"int x\"\n.Fc\n.Xc",
            Term,
            &["checked"][..],
            "checked(int x)",
        ),
        ("Xo\n.Fo\n.Fa \"int x\"\n.Fc\n.Xc", Term, &[][..], "(int x)"),
    ];
    for (head, kind, names, form) in cases {
        assert_supported_declaration(head, kind, names, form);
    }
}

fn assert_supported_declaration(head: &str, kind: EntryKind, names: &[&str], form: &str) {
    let query = example("DESCRIPTION", head);
    assert_subject(&query, head, kind, names, form);
    for unbound in [
        "void", "const", "int", "u_int", "handler", "callback", "ignored",
    ] {
        assert_eq!(
            explain(&query, unbound).counts.direct_entry.total,
            0,
            "{head} / {unbound}"
        );
    }
}

#[test]
fn an_argument_or_type_prefix_cannot_lend_a_name_to_an_unproved_subject() {
    for (head, names, form) in [
        (
            r#"Ar file , Va "void (*handler)(int)""#,
            &["file"][..],
            "file, void (*handler)(int)",
        ),
        (
            r#"Ft void Va "(*handler)(int)""#,
            &[][..],
            "void (*handler)(int)",
        ),
        (
            "Xo\n.Ft void\n.Va \"(*handler)(int)\"\n.Xc",
            &[][..],
            "void (*handler)(int)",
        ),
        (r#"Va "long short odd""#, &[][..], "long short odd"),
        (r#"Va "unsigned float odd""#, &[][..], "unsigned float odd"),
        (
            r#"Ft int Fn checked "int x""#,
            &["checked"][..],
            "int checked(int x)",
        ),
    ] {
        assert_supported_declaration(head, EntryKind::Term, names, form);
    }
}

#[test]
fn generated_qq_quotes_can_wrap_roles_while_authored_quotes_remain_in_the_form() {
    // Pinned termp_quote_pre/post print ASCII quotes for Qq. Identical
    // glyphs in a Va operand or No words belong to authored content.
    for (head, kind, names, form) in [
        (
            "Qq Va local_value",
            EntryKind::Variable,
            &["local_value"][..],
            "\"local_value\"",
        ),
        ("Qq Er EIO", EntryKind::Term, &["EIO"][..], "\"EIO\""),
        (
            r#"Qq Fn checked "int x""#,
            EntryKind::Term,
            &["checked"][..],
            "\"checked(int x)\"",
        ),
        (
            r"Va \[dq]local_value\[dq]",
            EntryKind::Term,
            &[][..],
            "\"local_value\"",
        ),
        (
            r"No \[dq] Ns Va local_value Ns No \[dq]",
            EntryKind::Term,
            &[][..],
            "\"local_value\"",
        ),
    ] {
        assert_subject(&example("ENVIRONMENT", head), head, kind, names, form);
    }
}

#[test]
fn generated_head_wrappers_and_font_changes_leave_exact_subject_ranges() {
    for (opener, left, right) in [("Bq", '[', ']'), ("Pq", '(', ')'), ("Op", '[', ']')] {
        for (mac, name, kind) in [
            ("Er", "EIO", EntryKind::Term),
            ("Dv", "EXAMPLE_LIMIT", EntryKind::Term),
            ("Va", "local_value", EntryKind::Variable),
        ] {
            for font in ["", "\\fB", "\\fR", "\\fI"] {
                let head = format!("{opener} {mac} {font}{name}");
                assert_subject(
                    &example("DESCRIPTION", &head),
                    &head,
                    kind,
                    &[name],
                    &format!("{left}{name}{right}"),
                );
            }
        }
    }
}

#[test]
fn wrapped_fl_marker_names_need_their_full_native_spelling() {
    let option = EntryKind::Parameter {
        parameter_kind: ParameterKind::Option,
    };
    for (head, kind, names, form) in [
        (r"Fl \-", option, &["--"][..], "--"),
        (r"Op Fl \-", option, &["--"][..], "[--]"),
        (r"Fl \-%", option, &["--%"][..], "--%"),
        (r"Op Fl \-%", option, &["--%"][..], "[--%]"),
        (r"Pq Fl \fI\-%", option, &["--%"][..], "(--%)"),
        (r"Fl \fR\-", option, &["--"][..], "--"),
        ("Cm --", EntryKind::Term, &["--"][..], "--"),
        ("Fl Ar signal_number", option, &[][..], "-signal_number"),
        (
            "Xo\n.Ar \\&\n.Fl real\n.Xc",
            option,
            &["-real"][..],
            " -real",
        ),
    ] {
        assert_subject(&example("DESCRIPTION", head), head, kind, names, form);
    }
    // term.c::encode1 replaces the generated dash after the zero-advance
    // word. The remaining letters cannot prove the lost flag prefix.
    let query = example("DESCRIPTION", "Xo\n.Ar \\z\\&\n.Fl real\n.Xc");
    let items = definitions(query.document.as_ref().unwrap());
    assert_eq!(inline_plain_text(&items[0].terms[0]), " real");
    assert_eq!(explain(&query, "-real").counts.direct_entry.total, 0);
}

#[test]
fn actual_head_owners_distinguish_symbol_uses_and_synopsis_declarations() {
    let query = example("SYNOPSIS", r#"Fn checked "int x""#);
    assert_subject(
        &query,
        "explicit SYNOPSIS definition",
        EntryKind::Term,
        &["checked"],
        "checked  (int x);",
    );
    let query = load(&format!(
        "{HEADER}.Sh SYNOPSIS\n.Ft void\n.Fn unowned \"int x\"\n.Sh DESCRIPTION\n.Fn unowned \"int x\"\n.Va plain_variable\n.Bl -tag -width Ds\n.It Sy owner\n.Fn unowned \"int x\"\n.Er EUNOWNED\n.Bl -tag -width Ds\n.It Bq Er ENESTED\nNESTED_BODY\n.El\n.El\n",
    ));
    for occurrence in ["unowned", "plain_variable", "EUNOWNED"] {
        assert_eq!(explain(&query, occurrence).counts.direct_entry.total, 0);
    }
    assert_eq!(explain(&query, "ENESTED").counts.direct_entry.total, 1);

    let query = load(&format!(
        "{HEADER}.Sh DESCRIPTION\n.Bl -column Ds\n.It Bq Er EUNOWNED\n.It Xo\n.Bl -tag -width Ds\n.It Er ENESTED\nNESTED_BODY\n.El\n.Xc\n.El\n",
    ));
    assert_eq!(explain(&query, "EUNOWNED").counts.direct_entry.total, 0);
    assert_eq!(explain(&query, "ENESTED").counts.direct_entry.total, 1);
}

#[test]
fn symbol_bindings_use_executed_text_and_group_evidence_still_follows_source() {
    let query = load(&format!(
        "{HEADER}.Sh DESCRIPTION\n.ds ECODE EIO\n.Bl -tag -width Ds\n.It Bq Er \\*[ECODE]\nSUBJECT_BODY\n.El\n",
    ));
    assert_subject(
        &query,
        "executed Er string",
        EntryKind::Term,
        &["EIO"],
        "[EIO]",
    );
    let query = example("DESCRIPTION", r"Va \zSAVED");
    assert_subject(
        &query,
        "overwritten Va glyph",
        EntryKind::Variable,
        &["AVED"],
        "AVED",
    );
    assert_eq!(explain(&query, "SAVED").counts.direct_entry.total, 0);

    let query = load(&format!(
        "{HEADER}.Sh COMMANDS\n.Bl -tag -width Ds\n.It Er E_FIRST\n.It Er E_SECOND\nSECOND_BODY\n.El\n",
    ));
    let result = explain(&query, "E_FIRST");
    assert_eq!(result.counts.direct_entry.total, 1);
    assert_eq!(result.supports.len(), 1);
    assert!(
        serde_json::to_string(&result.supports[0])
            .unwrap()
            .contains("SECOND_BODY")
    );
}

#[test]
fn accepted_symbol_groups_use_the_shared_name_budget() {
    use std::fmt::Write as _;

    // Each member is a real Er child under the executed Xo HEAD in -Ttree.
    // The complete form and the BODY remain visible above the name budget.
    for size in [1, 16, 256, 257, 1024] {
        let mut head = String::from("Xo\n");
        let names = (0..size).map(|n| format!("ERROR_{n}")).collect::<Vec<_>>();
        for (index, name) in names.iter().enumerate() {
            writeln!(
                head,
                ".Er {name}{}",
                if index + 1 < size { " ," } else { "" }
            )
            .unwrap();
        }
        head.push_str(".Xc");
        let query = example("DESCRIPTION", &head);
        let document = query.document.as_ref().unwrap();
        let items = definitions(document);
        let [item] = items.as_slice() else {
            panic!("{size} symbols")
        };
        let owner = EntryOwner::Definition(item);
        assert_eq!(
            inline_plain_text(owner.forms().unwrap().iter().next().unwrap()),
            names.join(", ")
        );
        assert_eq!(
            owner.validated_names().unwrap(),
            if size <= 256 { names.as_slice() } else { &[] }
        );
        assert_eq!(
            document
                .diagnostics
                .iter()
                .filter(|diag| {
                    diag.code.as_deref() == Some("manual.semantic-entry.name-limit")
                        && diag.impact == mant_ir::DiagnosticImpact::SemanticCoverage
                })
                .count(),
            usize::from(size > 256),
        );
        assert!(mant_render::render_query_text(&query).contains("SUBJECT_BODY"));
    }
}

#[test]
fn openbsd_bpf_symbols_keep_their_distinct_definition_bodies_and_erf_stays_prose() {
    let bpf = load(include_str!(
        "../../../../tests/fixtures/roff/real/scoped-layout/openbsd-bpf.4"
    ));
    for (name, expected) in [
        ("EINVAL", &["is negative", "microsecond value"][..]),
        ("BIOCGBLEN", &["Returns the required buffer length"][..]),
        ("BIOCSRTIMEOUT", &["BIOCSRTIMEOUT"][..]),
        ("BIOCGRTIMEOUT", &["Sets or gets the read timeout"][..]),
    ] {
        let result = explain(&bpf, name);
        assert_eq!(
            result.counts.direct_entry.total as usize,
            expected.len(),
            "{name}"
        );
        let hits = result
            .evidence
            .iter()
            .filter(|hit| hit.class == EvidenceClass::DirectEntry);
        for (hit, body) in hits.zip(expected) {
            let excerpt =
                mant_query::select_excerpt(&bpf, &[ContentSelector::path(hit.outline.path())])
                    .unwrap();
            let text = mant_render::render_excerpt_text(&excerpt);
            assert!(text.contains(body), "{name}: {text}");
            if name == "BIOCSRTIMEOUT" {
                assert!(!text.contains("Sets or gets the read timeout"), "{text}");
            }
            for other in expected.iter().filter(|other| *other != body) {
                assert!(!text.contains(other), "{name}: {text}");
            }
        }
    }
    let erf = load(include_str!(
        "../../../../tests/fixtures/roff/real/mathematical-export/openbsd-erf.3"
    ));
    for name in ["erf", "erff", "erfc"] {
        let result = explain(&erf, name);
        assert_eq!(result.counts.direct_entry.total, 0, "{name}");
        assert!(result.counts.context_mention.total > 0, "{name}");
    }
}
