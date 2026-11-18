//! The environment declaration form and each name have independent evidence.
use super::{DECLARATION_HEADER, check_query, explain, load, section_fixture};
use mant_ir::{EntryKind, EntryOwner, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceBasis, EvidenceClass};

use crate::support::explanation_ranges;

const ENVIRONMENT: EntryKind = EntryKind::EnvironmentVariable;

#[test]
fn environment_names_follow_the_executed_operands_at_each_join() {
    // mdoc_macro.c::in_line closes Ev before the next callable macro.
    // Native Nm, An, Ox, Path and Sx glyphs never become Ev name bytes.
    for (tail, rendered) in [
        ("Nm FILES", "FILES"),
        ("An FILES", "FILES"),
        ("Ox", "OpenBSD"),
        ("Pa FILES", "FILES"),
        ("Sx FILES", "FILES"),
        ("No FILES", "FILES"),
        ("Ev FILES", "FILES"),
    ] {
        for (join, gap) in [
            (" Ns ", ""),
            (" ", " "),
            (" , ", ", "),
            (" | ", " | "),
            (" No or ", " or "),
        ] {
            let name_source = tail.starts_with("Ev ") || tail.starts_with("No ");
            let head = format!("Ev HOME{join}{tail}");
            let form = format!("HOME{gap}{rendered}");
            let query = load(&format!(
                "{DECLARATION_HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It {head}\nBODY-A.\n.El\n.Sh FILES\nDEST.\n"
            ));
            let item = super::super::super::definitions(query.document.as_ref().unwrap())[0];
            assert_eq!(inline_plain_text(&item.terms[0]), form, "{head}");
            let expected = match (join, name_source) {
                (" Ns ", true) if tail.starts_with("No ") => &["HOMEFILES"][..],
                (" , " | " | ", true) => &["HOME", "FILES"],
                (" No or ", true) if tail.starts_with("Ev ") => &["HOME", "FILES"],
                (" , " | " | " | " ", false) => &["HOME"],
                _ => &[],
            };
            let owner = EntryOwner::Definition(item);
            let actual = owner
                .facts()
                .map_or(&[][..], |_| owner.validated_names().unwrap());
            assert_eq!(actual, expected, "{head}");
            let glued_foreign = join == " Ns " && !tail.starts_with("No ");
            assert_eq!(
                explain(&query, &form).counts.direct_entry.total,
                u32::from(!glued_foreign),
                "{head}"
            );
            for name in ["HOME", "FILES", "OpenBSD", "or"] {
                assert_eq!(
                    explain(&query, name).counts.direct_entry.total,
                    u32::from(expected.contains(&name)),
                    "{head}: {name}"
                );
            }
            if !glued_foreign && expected.len() == 2 {
                assert_eq!(owner.facts().unwrap().kind, ENVIRONMENT, "{head}");
            }
        }
    }
}

#[test]
fn generated_system_words_cannot_extend_the_environment_form() {
    // post_xx (and post_at/post_bx) supplies these visible native words.
    for (tail, label) in [
        ("Bx", "BSD"),
        ("Bsx", "BSD/OS"),
        ("Fx", "FreeBSD"),
        ("Nx", "NetBSD"),
        ("Ox", "OpenBSD"),
        ("Ux", "Unix"),
        ("Dx", "DragonFly"),
        ("At", "AT&T UNIX"),
    ] {
        for (join, visible) in [(" Ns ", ""), (" ", " ")] {
            let head = format!("Ev HOME{join}{tail}");
            let query = section_fixture("ENVIRONMENT", "", &head);
            let item = super::super::super::definitions(query.document.as_ref().unwrap())[0];
            let form = format!("HOME{visible}{label}");
            assert_eq!(inline_plain_text(&item.terms[0]), form, "{head}");
            let direct = u32::from(join == " ");
            assert_eq!(
                explain(&query, "HOME").counts.direct_entry.total,
                direct,
                "{head}"
            );
            assert_eq!(
                explain(&query, &form).counts.direct_entry.total,
                direct,
                "{head}"
            );
            assert_eq!(
                explain(&query, label).counts.direct_entry.total,
                0,
                "{head}"
            );
        }
    }
}

#[test]
fn fallback_macro_operands_and_unowned_text_cannot_extend_the_environment_form() {
    // print_mdoc_node leaves Tn/St to their own children, and
    // termp_in_pre/post wraps the independent include operand in angles.
    for (tail, visible) in [
        ("Tn FILES", "FILES"),
        ("St -ansiC", "ANSI X3.159-1989 (“ANSI\u{a0}C89”)"),
        ("In files.h", "<files.h>"),
    ] {
        for (join, spacer) in [(" Ns ", ""), (" , ", ", "), (" | ", " | ")] {
            let head = format!("Ev HOME{join}{tail}");
            let query = section_fixture("ENVIRONMENT", "", &head);
            let item = super::super::super::definitions(query.document.as_ref().unwrap())[0];
            let form = format!("HOME{spacer}{visible}");
            assert_eq!(inline_plain_text(&item.terms[0]), form, "{head}");
            for key in ["HOME", &form] {
                assert_eq!(
                    explain(&query, key).counts.direct_entry.total,
                    u32::from(!spacer.is_empty()),
                    "{head}"
                );
            }
        }
    }
    for suffix in ["FILES", r"\&FILES", "No FILES"] {
        let head = format!("Ev HOME Ns {suffix}");
        let query = section_fixture("ENVIRONMENT", "", &head);
        let item = super::super::super::definitions(query.document.as_ref().unwrap())[0];
        assert_eq!(inline_plain_text(&item.terms[0]), "HOMEFILES");
        assert_eq!(
            explain(&query, "HOMEFILES").counts.direct_entry.total,
            u32::from(suffix.starts_with("No ")),
            "{head}"
        );
        assert_eq!(explain(&query, "HOME").counts.direct_entry.total, 0);
    }
}

#[test]
fn rejected_full_form_keeps_the_proved_sibling_on_its_own_original_range() {
    for (head, visible) in [
        ("Ev GOOD , Ev HOME Ns Nm FILES", "GOOD, HOMEFILES"),
        ("Ev HOME Ns An FILES , Ev GOOD", "HOMEFILES, GOOD"),
        ("Ev A,B Ns Ar C", "A,BC"),
    ] {
        let query = load(&format!(
            "{DECLARATION_HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It {head}\nBODY-A.\n.It Ev HOMEFILES\nBODY-B.\n.El\n.Sh FILES\nDEST.\n"
        ));
        let items = super::super::super::definitions(query.document.as_ref().unwrap());
        assert_eq!(inline_plain_text(&items[0].terms[0]), visible);
        let name = if visible.starts_with('A') {
            "A"
        } else {
            "GOOD"
        };
        for (selected, owner, body) in [(name, 0, "BODY-A."), ("HOMEFILES", 1, "BODY-B.")] {
            let result = explain(&query, selected);
            assert_eq!(result.counts.direct_entry.total, 1, "{head}: {selected}");
            let hit = result
                .evidence
                .iter()
                .find(|hit| hit.class == EvidenceClass::DirectEntry)
                .unwrap();
            assert_eq!(hit.source, items[owner].source);
            assert!(
                hit.bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
            );
            assert!(explanation_ranges::assert_ranges(&result, selected) > 0);
            let excerpt =
                mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())])
                    .unwrap();
            let rendered = mant_render::render_excerpt_text(&excerpt);
            assert!(rendered.contains(body), "{rendered}");
            if owner == 0 {
                assert!(rendered.contains(visible), "{rendered}");
            }
            assert!(
                !rendered.contains(if owner == 0 { "BODY-B." } else { "BODY-A." }),
                "{rendered}"
            );
        }
        assert_eq!(
            explain(&query, visible).counts.direct_entry.total,
            0,
            "{head}"
        );
    }
}

const ALTERNATIVE_CASES: &[(&str, &str, &[&str], &str)] = &[
    (
        "",
        "Ev USER No or Ev LOGNAME",
        &["USER", "LOGNAME"],
        "USER or LOGNAME",
    ),
    (
        "",
        "Ev USER No or Ev LOGNAME No or Ev SIGNIN",
        &["USER", "LOGNAME", "SIGNIN"],
        "USER or LOGNAME or SIGNIN",
    ),
    (
        "",
        "Xo Ev USER\n.No or\n.Ev LOGNAME\n.Xc",
        &["USER", "LOGNAME"],
        "USER or LOGNAME",
    ),
    (
        ".de ZZ\n.Ev USER No or Ev LOGNAME\n..\n",
        "Xo\n.ZZ\n.Xc",
        &["USER", "LOGNAME"],
        "USER or LOGNAME",
    ),
    (
        ".de ZZ\n.Ev USER\n.No or\n.Ev LOGNAME\n..\n",
        "Xo\n.ZZ\n.Xc",
        &["USER", "LOGNAME"],
        "USER or LOGNAME",
    ),
    ("", "Ev USER or LOGNAME", &[], "USER or LOGNAME"),
    ("", "Ev \"USER or LOGNAME\"", &[], "USER or LOGNAME"),
    ("", "Ev USER No or LOGNAME", &[], "USER or LOGNAME"),
    ("", "Ev USER No Or Ev LOGNAME", &[], "USER Or LOGNAME"),
    (
        "",
        "Ev USER ( No default ) No or Ev LOGNAME",
        &["USER", "LOGNAME"],
        "USER (default) or LOGNAME",
    ),
    (
        "",
        "Xo Ev USER\n.Po\n.No default\n.Pc\n.No or\n.Ev LOGNAME\n.Xc",
        &["USER", "LOGNAME"],
        "USER (default) or LOGNAME",
    ),
    (
        "",
        "Ev USER No or Ev LOGNAME Pq No default",
        &["USER", "LOGNAME"],
        "USER or LOGNAME (default)",
    ),
    (
        "",
        "Ev USER No or Ev LOGNAME,LOGIN",
        &["USER", "LOGNAME", "LOGIN"],
        "USER or LOGNAME,LOGIN",
    ),
    (
        "",
        "Ev USER No or Ev \"not a name\"",
        &[],
        "USER or not a name",
    ),
    (
        "",
        "Ev USER=value No or Ev LOGNAME",
        &["USER"],
        "USER=value or LOGNAME",
    ),
    (
        "",
        "Ev USER No = Ev VALUE No or Ev LOGNAME",
        &["USER"],
        "USER = VALUE or LOGNAME",
    ),
    (
        "",
        "Ev USER Pq No or Ev LOGNAME",
        &["USER"],
        "USER (or LOGNAME)",
    ),
    (
        "",
        "Ev USER ( Ev DEFAULT No or Ev OTHER )",
        &["USER"],
        "USER (DEFAULT or OTHER)",
    ),
    ("", "Ev HOME No \"or or\" Ev FILES", &[], "HOME or or FILES"),
];

#[test]
fn only_a_complete_standalone_connector_can_establish_alternative_names() {
    for &(preamble, head, names, form) in ALTERNATIVE_CASES {
        let query = section_fixture("ENVIRONMENT", preamble, head);
        let unbound = ["USER", "LOGNAME", "VALUE", "DEFAULT", "OTHER", "or"]
            .into_iter()
            .filter(|word| !names.contains(word))
            .collect::<Vec<_>>();
        check_query(&query, head, ENVIRONMENT, names, form, &unbound);
    }
}

#[test]
fn repeated_environment_alias_records_both_ranges_in_one_owner() {
    let query = section_fixture("ENVIRONMENT", "", "Ev HOME No or Ev HOME");
    let item = super::super::super::definitions(query.document.as_ref().unwrap())[0];
    let owner = EntryOwner::Definition(item);
    assert_eq!(owner.validated_names().unwrap(), ["HOME"]);
    let entry = owner.facts().unwrap();
    assert_eq!(entry.name_bindings[0].occurrences.len(), 2);
    assert_eq!(inline_plain_text(&item.terms[0]), "HOME or HOME");
    assert_eq!(explain(&query, "HOME").counts.direct_entry.total, 1);
}

#[test]
fn synopsis_and_body_alternatives_do_not_create_definition_owners() {
    let query = load(&format!(
        "{DECLARATION_HEADER}.Sh SYNOPSIS\n.Ev USER No or Ev LOGNAME\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It TAG\n.Ev USER No or Ev LOGNAME\nBODY-A.\n.El\n"
    ));
    for name in ["USER", "LOGNAME", "USER or LOGNAME"] {
        assert_eq!(explain(&query, name).counts.direct_entry.total, 0);
    }
    assert_eq!(
        mant_render::render_query_text(&query)
            .matches("USER or LOGNAME")
            .count(),
        2
    );
}
