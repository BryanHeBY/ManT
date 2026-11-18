//! Real environment separators, native operand identities and malformed forms.
use super::{HEADER, TABLE, check_direct, definitions, explain, load};
use mant_ir::{EntryOwner, ResolvedContent, inline_plain_text};
use mant_protocol::EvidenceBasis;

#[test]
fn environment_separators_carry_their_native_operands_across_display() {
    // Native in_line issues separate Ev invocations and an independent
    // No operand. The 54 exact cases ran fixed tree/terminal/HTML/lint.
    for (first, second) in [("FIRST", "SECOND"), ("SECOND", "FIRST")] {
        for separator in [r"No \&,", r"No \&|", "No or"] {
            for scoped in [false, true] {
                for position in ["none", "before", "after", "both", "enclosed"] {
                    if position == "enclosed" && !scoped {
                        continue;
                    }
                    let scope = if scoped {
                        let inner_table = if position == "enclosed" { TABLE } else { "" };
                        format!(".Po\n{inner_table}.Ev HIDDEN\n.Pc\n")
                    } else {
                        String::new()
                    };
                    let before = table_at(matches!(position, "before" | "both"));
                    let after = table_at(matches!(position, "after" | "both"));
                    let head =
                        format!(".Ev {first}\n{scope}{before}.{separator}\n{after}.Ev {second}");
                    let source = source(&head);
                    let content = load(&source);
                    assert_names(&content, &[first, second], &source);
                    if position == "none" {
                        assert_exact_form(&content, true);
                    }
                }
            }
        }
    }
}

#[test]
fn untyped_connectors_and_invisible_annotations_cannot_supply_names() {
    let prose = source(&format!(".Ev FIRST\n{TABLE}.No \"or SECOND\""));
    let content = load(&prose);
    assert_names(&content, &["FIRST"], &prose);
    assert_eq!(explain(&content, "SECOND").counts.direct_entry.total, 0);

    let before = source(&format!(
        ".Ev GOOD\n{TABLE}.Eo\n.Ev HIDDEN\n.Ec ,\n.Ev NEXT"
    ));
    let content = load(&before);
    assert_names(&content, &["GOOD", "NEXT"], &before);
}

#[test]
fn a_malformed_first_group_cannot_reinstate_a_rejected_foreign_join() {
    // termp_ns_pre glues the actual Ev and Nm spellings. The unclosed
    // source tail leaves an authored label without proving an exact form.
    for tail in ["", " Ns No [x", " Ns No (open", " Ns No {broken"] {
        let source = source(&format!(".Ev HOME Ns Nm FILES{tail}"));
        let content = load(&source);
        let item = definitions(content.document.as_ref().unwrap())[0];
        let visible = inline_plain_text(&item.terms[0]);
        let expected = tail.strip_prefix(" Ns No ").unwrap_or("");
        assert_eq!(visible, format!("HOMEFILES{expected}"), "{source}");
        assert_exact_form(&content, false);
        assert_eq!(explain(&content, "HOMEFILES").counts.direct_entry.total, 0);
        assert_eq!(explain(&content, "HOME").counts.direct_entry.total, 0);
    }
    for head in [".Ev HOME ( Em other", ".Po\n.Ev HIDDEN\n.Pc"] {
        let content = load(&source(head));
        assert_exact_form(&content, true);
        assert_eq!(explain(&content, "HOME").counts.direct_entry.total, 0);
        assert_eq!(explain(&content, "HIDDEN").counts.direct_entry.total, 0);
    }
}

fn source(head: &str) -> String {
    format!("{HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Xo\n{head}\n.Xc\nOWNERBODY.\n.El\n")
}

fn table_at(present: bool) -> &'static str {
    if present { TABLE } else { "" }
}

fn assert_names(content: &ResolvedContent, names: &[&str], source: &str) {
    let item = definitions(content.document.as_ref().unwrap())[0];
    assert_eq!(
        EntryOwner::Definition(item).validated_names().unwrap(),
        names,
        "{source}"
    );
    for name in names {
        check_direct(content, name, item.source.unwrap().line, "OWNERBODY.");
    }
    for rejected in ["or", "HIDDEN", "SENTINEL"] {
        assert_eq!(
            explain(content, rejected).counts.direct_entry.total,
            0,
            "{source}"
        );
    }
}

fn assert_exact_form(content: &ResolvedContent, present: bool) {
    let item = definitions(content.document.as_ref().unwrap())[0];
    let visible = inline_plain_text(&item.terms[0]);
    let result = explain(content, visible.trim());
    assert_eq!(
        result.counts.direct_entry.total,
        u32::from(present),
        "{visible}"
    );
    if present {
        assert!(
            result.evidence[0]
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Form { .. })),
            "{visible}"
        );
    }
}
