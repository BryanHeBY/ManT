use super::{check, check_query, section_fixture};
use mant_ir::{EntryKind, Inline, inline_plain_text};

const BOUNDS: [(&str, &str); 5] = [
    (".Eo (", ".Ec )"),
    (r".Eo \&(", r".Ec \&)"),
    (".Po", ".Pc"),
    (".Eo (", r".Ec \&)"),
    (r".Eo \&(", ".Ec )"),
];

#[test]
fn nested_annotations_follow_native_body_parents_regardless_of_marker_spelling() {
    // CVS mdoc_macro.c::blk_part_exp emits a bare '(' before Eo HEAD;
    // blk_exp_close/append_delims puts a bare ')' after that Eo block.
    // Their child BODY still owns the text and any nested enclosure.
    for (token, name, section, kind) in [
        ("Dv", "SYS", "DESCRIPTION", EntryKind::Term),
        ("Va", "local", "DESCRIPTION", EntryKind::Variable),
        ("Ev", "HOME", "ENVIRONMENT", EntryKind::EnvironmentVariable),
    ] {
        let bare = format!("{token} {name}");
        check_query(
            &section_fixture(section, "", &bare),
            &bare,
            kind,
            &[name],
            name,
            &[],
        );
        for (left, right) in BOUNDS {
            let head = format!("Xo {token} {name} Ns\n{left}\n.Ic inner\n{right}\n.Xc");
            check_annotation(section, &head, kind, name, &format!("{name}(inner)"));
            for (inner_left, inner_right) in BOUNDS {
                // Without the outer No word, the nested opening is the
                // first evidence for the parent's actual native BODY.
                for (outer_word, printed) in [(".No outer\n", "outer "), ("", "")] {
                    let head = format!(
                        "Xo {token} {name} Ns\n{left}\n{outer_word}{inner_left}\n.Ic inner\n{inner_right}\n{right}\n.Xc"
                    );
                    let form = format!("{name}({printed}(inner))");
                    check_annotation(section, &head, kind, name, &form);
                }
            }
        }
    }
}

#[test]
fn nested_literal_bounds_do_not_certify_independent_subjects_or_unbalanced_annotations() {
    for (head, form, arguments) in [
        (
            "Xo Dv SYS Ns\n.Eo (\n.No outer\n.Eo (\n.Dv NEXT\n.Ec )\n.Ec )\n.Xc",
            "SYS(outer (NEXT))",
            &["SYS", "NEXT"][..],
        ),
        (
            "Xo Dv SYS Ns\n.Eo (\n.No outer\n.Eo (\n.Ic inner\n.Ec ]\n.Ec )\n.Xc",
            "SYS(outer (inner])",
            &["SYS", "inner"][..],
        ),
        (
            "Xo Dv SYS Ns\n.Eo (\n.No outer\n.Eo (\n.Ic inner\n.Ec )\n.Ec\n.Xc",
            "SYS(outer (inner)",
            &["SYS", "inner"][..],
        ),
    ] {
        check(head, false, EntryKind::Term, &[], form, arguments);
    }
    check(
        "Xo Dv START ,\n.Eo (\n.Eo (\n.Fa arg\n.Ec )\n.Ec ) ,\n.Dv STOP\n.Xc",
        false,
        EntryKind::Term,
        &["START", "STOP"],
        "START, ((arg)), STOP",
        &["arg"],
    );
}

fn check_annotation(section: &str, head: &str, kind: EntryKind, name: &str, form: &str) {
    let query = section_fixture(section, "", head);
    check_query(&query, head, kind, &[name], form, &["inner", "outer"]);
    let items = super::super::super::definitions(query.document.as_ref().unwrap());
    assert!(
        items[0].terms[0].iter().any(|node| matches!(
            node, Inline::Strong { children } if inline_plain_text(children) == "inner"
        )),
        "{head}"
    );
}
