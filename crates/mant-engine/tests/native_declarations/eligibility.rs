use super::{
    definitions,
    support::{explain, load},
};
use mant_ir::{EntryKind, EntryOwner, ParameterKind, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceBasis};

const HEADER: &str =
    ".Dd November 17, 2026\n.Dt PROBE 3\n.Os\n.Sh NAME\n.Nm probe\n.Nd role probe\n";

#[test]
fn joined_placeholders_do_not_supply_concrete_names() {
    // Pinned mdoc_term.c::termp_ns_pre suppresses spacing without changing
    // the operands executed by Fa, Vt or Ar. Fonts affect only their display.
    for (section, macro_name, prefix, kind) in [
        ("ENVIRONMENT", "Fa", "field_", EntryKind::Term),
        ("COMMANDS", "Vt", "type_", EntryKind::Term),
        ("ENVIRONMENT", "No", "ENV_", EntryKind::Term),
        ("COMMANDS", "No", "verb_", EntryKind::Term),
        ("ENVIRONMENT", "Ev", "ENV_", EntryKind::EnvironmentVariable),
        ("COMMANDS", "Va", "cache_", EntryKind::Term),
        ("ENVIRONMENT", "Dv", "CONST_", EntryKind::Term),
        ("COMMANDS", "Er", "ERR_", EntryKind::Term),
    ] {
        let name = format!("{prefix}KEY");
        for (opening, closing, font, form) in [
            ("", "", "", name.clone()),
            ("Bq ", "", r"\fB", format!("[{name}]")),
            ("Pq ", "", r"\fR", format!("({name})")),
            ("Xo\n.", "\n.Xc", r"\fI", name.clone()),
        ] {
            let head = format!("{opening}{macro_name} {prefix} Ns Ar {font}KEY{closing}");
            check_head(section, &head, &form, kind, &[], &[&name, &form, "KEY"]);
        }
    }
    for (section, head, form) in [
        ("ENVIRONMENT", "Bq Fa field_ Ns Ar KEY", "[field_KEY]"),
        ("ENVIRONMENT", "Bq Vt type_ Ns Ar KEY", "[type_KEY]"),
        ("ENVIRONMENT", "No -- Ns Ar fake", "--fake"),
        ("ENVIRONMENT", "Ar PREFIX Ns Ev VALUE", "PREFIXVALUE"),
        ("DESCRIPTION", "Ar FIRST Ns Ar SECOND", "FIRSTSECOND"),
        ("COMMANDS", "Fa field_ Ns Va cache", "field_cache"),
        ("DESCRIPTION", "Fa field_ Ns No KEY", "field_KEY"),
        ("COMMANDS", "Bq Vt type_ Ns No KEY", "[type_KEY]"),
        ("ENVIRONMENT", "No ENV_ Ns Fa KEY", "ENV_KEY"),
        ("ENVIRONMENT", "No ENV_ Ar KEY", "ENV_ KEY"),
        (
            "ENVIRONMENT",
            r#"Fn checked "int x" Ns Ar KEY"#,
            "checked(int x)KEY",
        ),
        (
            "DESCRIPTION",
            "Xo\n.Fo checked\n.Fa \"int x\"\n.Fc\n.Ar KEY\n.Xc",
            "checked(int x) KEY",
        ),
    ] {
        check_head(section, head, form, EntryKind::Term, &[], &[form]);
    }
}

#[test]
fn styled_command_prefixes_need_an_actual_operand_boundary() {
    // CVS mdoc_term.c::termp_ns_pre joins the two operands while leaving
    // Ar's role intact. The font transition alone cannot finish a name.
    for (head, form, name) in [
        (r"No \f[B]?\fP Ns Ar RE", "?RE", None),
        (r"No \fBfind\fP Ns Ar FILE", "findFILE", None),
        ("Cm find Ns No _ Ns Ar FILE", "find_FILE", None),
        (r"No \f[B]?\fP Ar RE", "? RE", Some("?")),
        ("Cm find Ar FILE", "find FILE", Some("find")),
        ("Cm find Ns Op Ar FILE", "find[FILE]", Some("find")),
    ] {
        let kind = if name.is_some() {
            EntryKind::Command
        } else {
            EntryKind::Term
        };
        let unqualified = if form.starts_with('?') { "?" } else { "find" };
        let rejected = if name.is_none() {
            &[unqualified][..]
        } else {
            &[]
        };
        check_head("COMMANDS", head, form, kind, name.as_slice(), rejected);
    }
    check_head(
        "COMMANDS",
        "Cm find Ns No _ Ns Ar FILE , Cm other",
        "find_FILE, other",
        EntryKind::Command,
        &["other"],
        &["find", "FILE"],
    );
}

#[test]
fn complete_scoped_subjects_and_argument_labels_remain_addressable() {
    for (section, head, form, kind, names) in [
        (
            "DESCRIPTION",
            "Fa field",
            "field",
            EntryKind::Term,
            &["field"][..],
        ),
        (
            "DESCRIPTION",
            r"Bq Fa field_\fBKEY",
            "[field_KEY]",
            EntryKind::Term,
            &["field_KEY"][..],
        ),
        (
            "COMMANDS",
            "Pq Vt type_t",
            "(type_t)",
            EntryKind::Term,
            &["type_t"][..],
        ),
        (
            "ENVIRONMENT",
            "Ar file",
            "file",
            EntryKind::Term,
            &["file"][..],
        ),
        (
            "ENVIRONMENT",
            r"Ar f\fBile",
            "file",
            EntryKind::Term,
            &["file"][..],
        ),
        (
            "DESCRIPTION",
            "Bq Ar file",
            "[file]",
            EntryKind::Term,
            &["file"][..],
        ),
        (
            "DESCRIPTION",
            "Ar file , Ar path",
            "file, path",
            EntryKind::Term,
            &["file", "path"][..],
        ),
        (
            "DESCRIPTION",
            "Ar file , No standalone",
            "file, standalone",
            EntryKind::Term,
            &["file", "standalone"][..],
        ),
        (
            "COMMANDS",
            "No verb_ Ar KEY",
            "verb_ KEY",
            EntryKind::Term,
            &["verb_"][..],
        ),
        (
            "ENVIRONMENT",
            r#"Va "const char *cache""#,
            "const char *cache",
            EntryKind::Variable,
            &["cache"][..],
        ),
        (
            "DESCRIPTION",
            r#"Fn "int checked" "int x""#,
            "int checked(int x)",
            EntryKind::Term,
            &["checked"][..],
        ),
        (
            "DESCRIPTION",
            "Xo\n.Fo checked\n.Fa \"int x\"\n.Fc\n.Xc",
            "checked(int x)",
            EntryKind::Term,
            &["checked"][..],
        ),
    ] {
        check_head(section, head, form, kind, names, &[]);
    }
}

#[test]
fn later_declarations_keep_their_names_when_an_earlier_candidate_is_rejected() {
    // In mdoc_macro.c::in_line, a quoted parameter stays one operand.
    // Bare punctuation outside that operand can follow the scope closing.
    for (section, head, form, kind, names) in [
        (
            "DESCRIPTION",
            r#"Dv FIRST Fa "int x" , Dv SECOND"#,
            "FIRST int x, SECOND",
            EntryKind::Term,
            &["FIRST", "SECOND"][..],
        ),
        (
            "DESCRIPTION",
            "Dv CONST_ Ns Ar KEY , Dv VALID",
            "CONST_KEY, VALID",
            EntryKind::Term,
            &["VALID"][..],
        ),
        (
            "COMMANDS",
            "Bq Er E_FIRST , Er E_SECOND",
            "[E_FIRST, E_SECOND]",
            EntryKind::Term,
            &["E_FIRST", "E_SECOND"][..],
        ),
        (
            "COMMANDS",
            "Er ERR_ Ns Ar KEY , Er E_VALID",
            "ERR_KEY, E_VALID",
            EntryKind::Term,
            &["E_VALID"][..],
        ),
        (
            "ENVIRONMENT",
            "Ev FIRST , Ev SECOND",
            "FIRST, SECOND",
            EntryKind::EnvironmentVariable,
            &["FIRST", "SECOND"][..],
        ),
        (
            "ENVIRONMENT",
            "Ev ENV_ Ns Ar KEY , Ev VALID",
            "ENV_KEY, VALID",
            EntryKind::EnvironmentVariable,
            &["VALID"][..],
        ),
        (
            "ENVIRONMENT",
            r"Ev ENV_ Ns Ar \fBKEY , Ev VALID",
            "ENV_KEY, VALID",
            EntryKind::EnvironmentVariable,
            &["VALID"][..],
        ),
        (
            "ENVIRONMENT",
            r#"Ev ENV_ Ns Ar "KEY, HIDDEN" , Ev VALID"#,
            "ENV_KEY, HIDDEN, VALID",
            EntryKind::EnvironmentVariable,
            &["VALID"][..],
        ),
        (
            "ENVIRONMENT",
            "Ev ENV_ Ar KEY , Ev VALID",
            "ENV_ KEY, VALID",
            EntryKind::EnvironmentVariable,
            &["ENV_", "VALID"][..],
        ),
        (
            "ENVIRONMENT",
            "No ENV_ Ns Ar KEY , No VALID",
            "ENV_KEY, VALID",
            EntryKind::EnvironmentVariable,
            &["VALID"][..],
        ),
    ] {
        check_head(
            section,
            head,
            form,
            kind,
            names,
            &["ENV_KEY", "ERR_KEY", "CONST_KEY", "KEY", "HIDDEN"],
        );
    }
}

#[test]
fn delimiters_inside_parameters_never_start_later_declarations() {
    for (section, head, form, kind, names) in [
        (
            "ENVIRONMENT",
            r#"No ENV_ Ns Ar "KEY," No SHADOW"#,
            "ENV_KEY, SHADOW",
            EntryKind::Term,
            &[][..],
        ),
        (
            "ENVIRONMENT",
            r#"Ev ENV_ Ns Ar "\fRKEY," No SHADOW"#,
            "ENV_KEY, SHADOW",
            EntryKind::EnvironmentVariable,
            &[][..],
        ),
        (
            "COMMANDS",
            r#"No verb_ Ns Ar "\fRKEY," No shadow"#,
            "verb_KEY, shadow",
            EntryKind::Term,
            &[][..],
        ),
        (
            "COMMANDS",
            r#"No verb_ Ns Ar "\fRKEY," No shadow , Sy valid"#,
            "verb_KEY, shadow, valid",
            EntryKind::Command,
            &["valid"][..],
        ),
        (
            "DESCRIPTION",
            r#"Dv CONST_ Ns Ar "KEY," Dv SHADOW"#,
            "CONST_KEY, SHADOW",
            EntryKind::Term,
            &[][..],
        ),
        (
            "OPTIONS",
            "Fl color Ar KEY , Fl all",
            "-color KEY, -all",
            EntryKind::Parameter {
                parameter_kind: ParameterKind::Option,
            },
            &["-color", "-all"][..],
        ),
    ] {
        check_head(
            section,
            head,
            form,
            kind,
            names,
            &[
                "ENV_KEY",
                "CONST_KEY",
                "verb_KEY",
                "KEY",
                "SHADOW",
                "shadow",
            ],
        );
    }
}

fn check_head(
    section: &str,
    head: &str,
    form: &str,
    kind: EntryKind,
    names: &[&str],
    rejected: &[&str],
) {
    let query = load(&format!(
        "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\nBODY\n.El\n"
    ));
    let items = definitions(query.document.as_ref().unwrap());
    let [item] = items.as_slice() else {
        panic!("{head}")
    };
    let owner = EntryOwner::Definition(item);
    let facts = owner.facts().unwrap();
    assert_eq!(facts.kind, kind, "{head}");
    assert_eq!(owner.validated_names().unwrap(), names, "{head}");
    assert!(facts.alias_groups.is_empty(), "{head}");
    let forms = owner.forms().unwrap();
    let mut forms = forms.iter();
    assert_eq!(inline_plain_text(forms.next().unwrap()), form, "{head}");
    assert!(forms.next().is_none(), "{head}");
    for (name, expected) in names
        .iter()
        .map(|name| (name, true))
        .chain(rejected.iter().map(|name| (name, false)))
    {
        let result = explain(&query, name);
        let direct_match = result.evidence.iter().find(|entry| {
            entry
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
        });
        assert_eq!(direct_match.is_some(), expected, "{head} / {name}");
        if let Some(hit) = direct_match {
            assert_eq!(result.counts.direct_entry.total, 1, "{head} / {name}");
            let excerpt =
                mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                    .unwrap();
            assert!(mant_render::render_excerpt_text(&excerpt).contains("BODY"));
        }
    }
    let markdown = mant_codec::encode::render_markdown(&query);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    let text = mant_render::render_query_text(&readback);
    assert!(
        text.contains(form) && text.contains("BODY"),
        "{head}: {text}"
    );
}
