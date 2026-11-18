//! Table-independent roles use the same grammar and actual name ranges.
use super::{HEADER, check_direct, definitions, exact_word, explain, load};
use mant_ir::{EntryOwner, inline_plain_text};
use mant_protocol::EvidenceBasis;

const DECLARATIONS: &[(&str, &str, &str, bool)] = &[
    ("DESCRIPTION", "Fl q Em --fake", "-q", true),
    ("DESCRIPTION", "Fl q Ar --fake", "-q", true),
    ("DESCRIPTION", "Fl q Pa --fake", "-q", true),
    ("DESCRIPTION", "Fl q Sx --fake", "-q", true),
    ("DESCRIPTION", "Er EINVAL", "EINVAL", false),
    ("DESCRIPTION", "Bq Er EINVAL", "EINVAL", false),
    ("DESCRIPTION", "Dv VALUE", "VALUE", false),
    ("DESCRIPTION", "Va variable", "variable", false),
    ("COMMANDS", "Cm command", "command", false),
    ("COMMANDS", "Ic config", "config", false),
    ("COMMANDS", "Nm probe Cm run", "probe run", false),
    ("ENVIRONMENT", "Ev HOME Pa --fake", "HOME", true),
];

#[test]
fn native_style_proof_is_invariant_on_either_side_of_a_table() {
    // Native print_mdoc_node and termp_under_pre / termp_bold_pre keep Fl,
    // its alternate styled operand, and the table's data owner distinct.
    // All 144 exact inputs ran fixed CVS tree/ASCII/UTF8/HTML/lint.
    for &(section, syntax, name, has_fake) in DECLARATIONS {
        for style in ["tag", "hang"] {
            for position in ["none", "before", "after"] {
                let display = format!(".TS\nl.\nSENTINEL\n{name}\n.TE\n");
                let declaration = match position {
                    "before" => format!("{display}.{syntax}\n"),
                    "after" => format!(".{syntax}\n{display}"),
                    _ => format!(".{syntax}\n"),
                };
                for swapped in [false, true] {
                    let owner = format!(".It Xo\n{declaration}.Xc\nOWNERBODY.\n");
                    let decoy = format!(".It Xo\n.TS\nl.\n{name}\n.TE\n.Xc\nDECOYBODY.\n");
                    let ordered = if swapped {
                        format!("{decoy}{owner}")
                    } else {
                        format!("{owner}{decoy}")
                    };
                    let source = format!(
                        "{HEADER}.Sh {section}\n.Bl -{style} -width Ds\n{ordered}.El\n.Sh FILES\nFILES BODY.\n.Sh --fake\nFAKE BODY.\n"
                    );
                    let query = load(&source);
                    let items = definitions(query.document.as_ref().unwrap());
                    let [first, second] = items.as_slice() else {
                        panic!("{source}")
                    };
                    let item = if swapped { second } else { first };
                    let owner = EntryOwner::Definition(item);
                    assert_eq!(
                        owner.validated_names().unwrap_or_default(),
                        [name],
                        "{source}\n{item:?}"
                    );
                    check_direct(&query, name, item.source.unwrap().line, "OWNERBODY.");
                    assert_eq!(
                        exact_word(&query, "SENTINEL"),
                        usize::from(position != "none"),
                        "{source}"
                    );
                    if has_fake {
                        assert_eq!(
                            explain(&query, "--fake").counts.direct_entry.total,
                            0,
                            "{source}"
                        );
                    }
                    let term = inline_plain_text(&item.terms[0]);
                    if position == "none" {
                        let found = explain(&query, term.trim());
                        assert_eq!(found.counts.direct_entry.total, 1, "{source}");
                        assert!(
                            found.evidence[0]
                                .bases
                                .iter()
                                .any(|basis| matches!(basis, EvidenceBasis::Form { .. })),
                            "{source}"
                        );
                    } else {
                        assert!(term.contains("SENTINEL"), "{source}");
                        assert!(
                            owner
                                .forms()
                                .unwrap()
                                .iter()
                                .all(|form| inline_plain_text(form) == name),
                            "{source}"
                        );
                    }
                }
            }
        }
    }
}
