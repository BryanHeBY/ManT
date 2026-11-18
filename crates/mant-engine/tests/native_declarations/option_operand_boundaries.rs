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
