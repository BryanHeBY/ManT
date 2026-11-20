//! Exact names and queryable signatures follow the native operand boundaries.

use super::{ExplanationOptions, ExplanationQuery, assert_name_positions, owners};
use crate::json_roundtrip::json_string_roundtrip;
use mant_ir::inline_plain_text;

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n";

fn check(section: &str, head: &str, names: &[&str], label: &str, full: bool, hidden: &[&str]) {
    let source =
        format!("{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {head}\nOwner proof.\n.El\n");
    let native = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    let content = json_string_roundtrip(&native);
    let entries = owners(content.document.as_ref().unwrap());
    let [owner] = entries.as_slice() else {
        panic!("expected one native It owner: {source}");
    };
    let facts = owner.facts().unwrap();
    assert_eq!(facts.names, names, "{head}");
    let actual = owner
        .forms()
        .unwrap()
        .iter()
        .map(inline_plain_text)
        .collect::<Vec<_>>();
    let forms = if full {
        std::slice::from_ref(&label)
    } else {
        names
    };
    assert_eq!(actual, forms, "{head}");
    for name in names {
        assert_name_positions(&content, *owner, name);
    }
    for (query, expected) in names
        .iter()
        .map(|name| (*name, true))
        .chain(std::iter::once((label, full)))
        .chain(hidden.iter().map(|name| (*name, false)))
    {
        let explanation = mant_query::explain_query(
            &content,
            &ExplanationQuery {
                entry: query.into(),
                options: ExplanationOptions::default(),
            },
        )
        .unwrap();
        assert_eq!(
            explanation.counts.direct_entry.total,
            u32::from(expected),
            "{head}: {query}"
        );
        if expected {
            assert!(mant_render::render_explanation_text(&explanation).contains("Owner proof."));
        }
    }
}

#[test]
fn authored_brackets_cross_native_children_without_absorbing_following_aliases() {
    // blk_part_exp emits BLOCK-leading punctuation before its HEAD and
    // BODY. All examples were compared with CVS tree, HTML and both
    // terminal devices. Pipe operands are distinct from comma aliases.
    for (head, names, form) in [
        (
            "Nm alpha Po ( Po Ar file ) Pc Pc | Nm beta , Nm gamma",
            &["gamma"][..],
            "alpha (((file))) | beta, gamma",
        ),
        (
            "Nm alpha Po Po ( Ar x Pc Pc ) , Nm gamma",
            &["alpha", "gamma"],
            "alpha (((x))), gamma",
        ),
        (
            "Nm alpha ( Po Po Ar x Pc Pc ) , Nm gamma",
            &["alpha", "gamma"],
            "alpha (((x))), gamma",
        ),
        (
            "Nm alpha ( Po Ar file ) Pc , Nm gamma",
            &["alpha", "gamma"],
            "alpha ((file)), gamma",
        ),
        (
            "Nm alpha Oo Oo [ Ar x Oc Oc ] , Nm gamma",
            &["alpha", "gamma"],
            "alpha [[[x]]], gamma",
        ),
    ] {
        check(
            "DESCRIPTION",
            head,
            names,
            form,
            true,
            &["beta", "file", "x"],
        );
    }
}

#[test]
fn native_assignment_forms_require_a_complete_right_hand_side() {
    // Fl, Li, Ar and the enclosure handlers can execute separate words.
    // The fixed profiles confirm each visible assignment and its It BODY.
    for equals in ["=", r"Li \&=", r#"Li "=""#, r"No \&="] {
        for (rhs, form, full) in [
            ("Ar x", "-a=x", true),
            ("Op Ar x", "-a=[x]", true),
            ("Oo Ar x Oc", "-a=[x]", true),
            ("Po Ar x Pc", "-a=(x)", true),
            ("Bro Ar x Brc", "-a={x}", true),
            ("Ar x Ns Li / Ns Ar y", "-a=x/y", true),
            ("Ar x No this is prose", "-a=x this is prose", false),
            ("Ar x Ns Ev FAKE", "-a=xFAKE", false),
            ("Ar x Ns Li / Ns Ev FAKE", "-a=x/FAKE", false),
            ("Op Ar x Li tail", "-a=[x tail]", false),
        ] {
            let head = format!("Fl a Ns {equals} Ns {rhs}");
            check("OPTIONS", &head, &["-a"], form, full, &["x", "y", "FAKE"]);
        }
    }
    check(
        "DESCRIPTION",
        r"Fl a Ns Li \&= Ns Ar x Lk https://example.invalid docs",
        &["-a"],
        "-a=x docs: https://example.invalid",
        false,
        &["x", "docs"],
    );
}

#[test]
fn the_flag_word_proves_its_own_value_without_proving_independent_operands() {
    // CVS tree shows the value inside the Fl word and the Li/Ev suffixes
    // outside it. term.c::encode1() overprints A with B in the clipped form.
    for (head, label, full) in [
        (r"Fl q=\zAB", "-q=B", true),
        ("Fl q=FILE Ns Li bogus", "-q=FILEbogus", false),
        ("Fl q=FILE Ns Ev bogus", "-q=FILEbogus", false),
        (r"Fl q=\zAB Ns Li bogus", "-q=Bbogus", false),
    ] {
        check("OPTIONS", head, &["-q"], label, full, &["bogus"]);
    }
}
