//! Adjacent native output keeps its own declaration eligibility.
use super::{
    definitions,
    support::{explain, load},
};
use crate::support::explanation_ranges;
use mant_ir::{EntryOwner, Inline, LinkTarget, inline_plain_text};
use mant_protocol::{ContentSelector, EvidenceBasis, EvidenceClass};

const HEADER: &str =
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd exercise descriptors\n";

fn source(head: &str, section: &str) -> String {
    format!(
        "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\nOWNERBODY.\n.El\n.Sh FILES\nFILES BODY.\n.Sh --fake\nFAKE BODY.\n"
    )
}

#[test]
fn generated_operands_cannot_extend_the_exact_flag() {
    // termp_ns_pre suppresses space; it does not put Xr's term_word calls
    // or other macros' operands inside Fl. Native Ar arguments and No
    // literal continuation keep their existing declaration rules.
    for (head, key, visible, complete) in [
        ("Fl q Ns Xr printf 3", "-q", "-qprintf(3)", false),
        ("Fl q Ns Ev FILES", "-q", "-qFILES", false),
        ("Fl q Ns Ox", "-q", "-qOpenBSD", false),
        ("Fl q Ns Ar VALUE", "-q", "-qVALUE", true),
        ("Fl q Ns No FILES", "-qFILES", "-qFILES", true),
        ("Fl qVALUE", "-qVALUE", "-qVALUE", true),
        ("Fl q Ns Em VALUE", "-q", "-qVALUE", true),
        ("Fl q Ns Sy VALUE", "-q", "-qVALUE", false),
        ("Fl q , Xr printf 3", "-q", "-q, printf(3)", true),
        ("Fl q Ar VALUE", "-q", "-q VALUE", true),
        ("Fl q Ns Pa files", "-q", "-qfiles", false),
        ("Fl q Ns In files.h", "-q", "-q<files.h>", false),
        (r#"Fl q Ns No "\&FILES""#, "-qFILES", "-qFILES", true),
    ] {
        let query = load(&source(head, "OPTIONS"));
        let items = definitions(query.document.as_ref().unwrap());
        let [item] = items.as_slice() else {
            panic!("{head}")
        };
        let owner = EntryOwner::Definition(item);
        assert_eq!(inline_plain_text(&item.terms[0]), visible, "{head}");
        assert_eq!(owner.validated_names().unwrap(), [key], "{head}");
        for other in ["-qprintf", "-qOpenBSD", "-qFILES", "printf"]
            .into_iter()
            .filter(|name| *name != key)
        {
            assert_eq!(
                explain(&query, other).counts.direct_entry.total,
                0,
                "{head}: {other}"
            );
        }
        let whole = explain(&query, visible);
        assert_eq!(
            whole.counts.direct_entry.total,
            u32::from(complete),
            "{head}"
        );
        if complete {
            assert!(
                whole.evidence[0]
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Form { .. })),
                "{head}"
            );
        }
        let found = explain(&query, key);
        check_owned_name(&query, &found, item, key, visible);
        if head.contains(" Xr ") {
            assert!(item.terms[0].iter().any(|inline| matches!(inline,
                Inline::Link { target: LinkTarget::Manual { name, manual_section }, children, .. }
                    if name == "printf" && manual_section.as_deref() == Some("3")
                        && inline_plain_text(children) == "printf(3)"
            )), "{head}");
        }
    }
}

fn check_owned_name(
    content: &mant_ir::ResolvedContent,
    found: &mant_protocol::QueryExplanation,
    item: &mant_ir::DefinitionItem,
    name: &str,
    visible: &str,
) {
    assert_eq!(found.counts.direct_entry.total, 1, "{visible}");
    assert!(explanation_ranges::assert_ranges(found, name) > 0);
    let hit = found
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source, item.source);
    assert!(
        hit.bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    );
    let excerpt =
        mant_query::select_excerpt(content, &[ContentSelector::path(hit.outline.path())]).unwrap();
    let text = mant_render::render_excerpt_text(&excerpt);
    assert!(
        text.contains(visible) && text.contains("OWNERBODY."),
        "{text}"
    );
}

#[test]
fn unproved_environment_clauses_never_discard_a_proved_sibling_or_restore_the_form() {
    // The native tree has three independent clauses in all six orders.
    // A None receipt for the first No must not hide a later independent Ev.
    let clauses = [
        ("Ev GOOD", "GOOD"),
        ("Ev HOME Ns Nm FILES", "HOMEFILES"),
        (r#"No "Words here""#, "Words here"),
    ];
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let head = order.map(|index| clauses[index].0).join(" , ");
        let visible = order.map(|index| clauses[index].1).join(", ");
        let content = load(&source(&head, "ENVIRONMENT"));
        let owners = definitions(content.document.as_ref().unwrap());
        let item = owners[0];
        assert_eq!(inline_plain_text(&item.terms[0]), visible, "{head}");
        let owner = EntryOwner::Definition(item);
        assert_eq!(owner.validated_names().unwrap(), ["GOOD"], "{head}");
        assert_eq!(
            owner.facts().unwrap().kind,
            mant_ir::EntryKind::EnvironmentVariable,
            "{head}"
        );
        for query in ["HOMEFILES", "Words here", &visible] {
            assert_eq!(
                explain(&content, query).counts.direct_entry.total,
                0,
                "{head}: {query}"
            );
        }
        let found = explain(&content, "GOOD");
        check_owned_name(&content, &found, item, "GOOD", &visible);
    }
}

#[test]
fn a_parenthesized_ev_keeps_an_ordinary_form_without_polluting_declarations() {
    // blk_full emits the leading '(' outside the HEAD. It still enters the
    // terminal before that HEAD and surrounds Ev's output as ordinary text.
    for (head, visible, names, whole_form) in [
        ("( Ev HIDDEN )", "(HIDDEN)", &[][..], true),
        (
            "( Ev HIDDEN ) , Ev GOOD",
            "(HIDDEN), GOOD",
            &["GOOD"],
            false,
        ),
        (
            r#"( Ev HIDDEN ) , Ev GOOD , No "Words here""#,
            "(HIDDEN), GOOD, Words here",
            &["GOOD"],
            false,
        ),
        (
            "Ev HIDDEN Ns Nm FILES , ( Ev OTHER )",
            "HIDDENFILES, (OTHER)",
            &[],
            false,
        ),
    ] {
        let content = load(&source(head, "ENVIRONMENT"));
        let owners = definitions(content.document.as_ref().unwrap());
        let item = owners[0];
        assert_eq!(inline_plain_text(&item.terms[0]), visible, "{head}");
        assert_eq!(
            EntryOwner::Definition(item)
                .validated_names()
                .unwrap_or_default(),
            names,
            "{head}"
        );
        let full_match = explain(&content, visible);
        assert_eq!(
            full_match.counts.direct_entry.total,
            u32::from(whole_form),
            "{head}"
        );
        if whole_form {
            assert!(
                full_match.evidence[0]
                    .bases
                    .iter()
                    .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
            );
        }
        for name in ["HIDDEN", "HIDDENFILES", "OTHER"] {
            assert_eq!(
                explain(&content, name).counts.direct_entry.total,
                0,
                "{head}"
            );
        }
        for name in names {
            check_owned_name(&content, &explain(&content, name), item, name, visible);
        }
    }
}

#[test]
fn punctuation_option_names_use_the_same_native_suffix_boundary_as_letters() {
    // termp_fl_pre emits the flag in its own scope. termp_ns_pre removes
    // spacing but cannot make Xr or Ev part of that spelling. in_line
    // can leave closing punctuation as TEXT next to an empty Fl. All 48
    // exact sources ran in fixed tree, terminal, HTML and lint first.
    for flag in ['q', '@', '%', ',', '.', '!', '?'] {
        for (suffix, visible, foreign) in [
            ("Xr printf 3", "printf(3)", true),
            ("Ev FILES", "FILES", true),
            ("Ar FILE", "FILE", false),
            ("No FILES", "FILES", false),
        ] {
            if suffix.starts_with("No") && !matches!(flag, 'q' | '@' | '%') {
                continue;
            }
            for joined in [false, true] {
                let spacing = if joined { " Ns" } else { "" };
                let head = format!("Fl {flag}{spacing} {suffix}");
                let flag = format!("-{flag}");
                let gap = if joined { "" } else { " " };
                let form = format!("{flag}{gap}{visible}");
                let key = if joined && suffix.starts_with("No") {
                    (flag == "-q").then_some(form.as_str())
                } else if flag == "-?" && suffix.starts_with("Ar") {
                    // The source's search-template grammar protects its
                    // question-mark sign and explicitly typed operand.
                    None
                } else {
                    Some(flag.as_str())
                };
                let content = load(&source(&head, "OPTIONS"));
                let items = definitions(content.document.as_ref().unwrap());
                let item = items[0];
                assert_eq!(inline_plain_text(&item.terms[0]), form, "{head}");
                assert_eq!(
                    EntryOwner::Definition(item).validated_names().unwrap(),
                    key.into_iter().collect::<Vec<_>>(),
                    "{head}"
                );
                if let Some(key) = key {
                    check_owned_name(&content, &explain(&content, key), item, key, &form);
                }
                assert_eq!(
                    explain(&content, &form).counts.direct_entry.total,
                    u32::from(!(joined && foreign)),
                    "{head}"
                );
                if suffix.starts_with("Xr") {
                    assert!(
                        item.terms[0].iter().any(|inline| matches!(inline,
                            Inline::Link { target: LinkTarget::Manual { name, manual_section }, .. }
                                if name == "printf" && manual_section.as_deref() == Some("3")
                        )),
                        "{head}"
                    );
                }
            }
        }
    }
}

#[test]
fn literal_environment_siblings_keep_their_role_in_both_source_orders() {
    // in_line gives No and Ev separate accepted operands in either order.
    for separator in [",", "|"] {
        for swapped in [false, true] {
            let (head, first, second) = if swapped {
                (format!("Ev SECOND {separator} No FIRST"), "SECOND", "FIRST")
            } else {
                (format!("No FIRST {separator} Ev SECOND"), "FIRST", "SECOND")
            };
            let between = if separator == "," { ", " } else { " | " };
            let visible = format!("{first}{between}{second}");
            assert_environment(&head, &visible, &[first, second], true);
        }
    }
    for (head, visible, names, complete) in [
        ("No FIRST", "FIRST", &["FIRST"][..], true),
        (
            r#"No "Words here" , Ev SECOND"#,
            "Words here, SECOND",
            &["SECOND"],
            false,
        ),
        (
            r#"No "Words here" | Ev SECOND"#,
            "Words here | SECOND",
            &["SECOND"],
            false,
        ),
        (
            "Ev FIRST=left,right , Ev SECOND",
            "FIRST=left,right, SECOND",
            &["FIRST", "SECOND"],
            true,
        ),
        (
            "Ev FIRST=left|right | Ev SECOND",
            "FIRST=left|right | SECOND",
            &["FIRST", "SECOND"],
            true,
        ),
        (
            "Ev FIRST=left , No SECOND",
            "FIRST=left, SECOND",
            &["FIRST"],
            true,
        ),
        (
            "Ev HOME Po No FIRST Pc , Ev SECOND",
            "HOME (FIRST), SECOND",
            &["HOME", "SECOND"],
            true,
        ),
    ] {
        assert_environment(head, visible, names, complete);
    }
}

#[test]
fn a_malformed_tail_cannot_cancel_closed_environment_groups() {
    // The native Ev operands and literal No tail execute independently.
    // A broken delimiter can invalidate only its own declaration group.
    for (tail, word, names) in [
        (r#"No "Words here""#, "Words here", &["GOOD"][..]),
        (r#"No "x(""#, "x(", &["GOOD"]),
        ("Ev BAD[", "BAD[", &["GOOD"]),
        (r#"No "x<""#, "x<", &["GOOD"]),
        (r#"No "x>""#, "x>", &["GOOD"]),
        (r#"No "x( R )""#, "x( R )", &["GOOD"]),
        ("Ev BAD=left[,right", "BAD=left[,right", &["GOOD", "BAD"]),
    ] {
        assert_environment(
            &format!("Ev GOOD , Ev HOME Ns Nm FILES , {tail}"),
            &format!("GOOD, HOMEFILES, {word}"),
            names,
            false,
        );
    }
}

fn assert_environment(head: &str, visible: &str, names: &[&str], complete: bool) {
    let content = load(&source(head, "ENVIRONMENT"));
    let items = definitions(content.document.as_ref().unwrap());
    let item = items[0];
    assert_eq!(inline_plain_text(&item.terms[0]), visible, "{head}");
    assert_eq!(
        EntryOwner::Definition(item).validated_names().unwrap(),
        names,
        "{head}"
    );
    assert_eq!(
        explain(&content, visible).counts.direct_entry.total,
        u32::from(complete),
        "{head}"
    );
    for name in names {
        check_owned_name(&content, &explain(&content, name), item, name, visible);
    }
    for rejected in ["HOMEFILES", "Words here", "BAD[", "left", "right"] {
        assert_eq!(
            explain(&content, rejected).counts.direct_entry.total,
            0,
            "{head}: {rejected}"
        );
    }
}
