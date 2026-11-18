//! Scope ownership follows the complete native HEAD across display rows.
use super::{HEADER, check_direct, definitions, exact_word, explain, load};
use mant_ir::{EntryOwner, inline_plain_text};
use std::fmt::Write;

#[path = "scoped_intervals/environment.rs"]
mod environment;
#[path = "scoped_intervals/primary.rs"]
mod primary;
#[path = "scoped_intervals/separators.rs"]
mod separators;

const TABLE: &str = ".TS\nl.\nSENTINEL\n.TE\n";
const POSITIONS: [&str; 6] = [
    "none",
    "outside-before",
    "outside-after",
    "left",
    "right",
    "both",
];

#[test]
fn table_boundaries_never_promote_an_enclosed_operand_to_a_declaration() {
    // mdoc_term.c termp_quote_pre/post and termp_eo_pre/post execute the
    // paired BODY around both the table and the ordinary operand. The
    // second It owns the only independently written matching declaration.
    // Fixed tree, ASCII, UTF-8, HTML and lint verified all 108 exact inputs.
    for (open, close) in [("Po", "Pc"), ("Bo", "Bc"), (r"Eo \&(", r"Ec \&)")] {
        for position in POSITIONS {
            for (section, lead, inner, key, fake) in [
                ("OPTIONS", "Fl q", "Fl Fl fake", "-q", "--fake"),
                ("COMMANDS", "Cm run", "Cm fake", "run", "fake"),
                ("ENVIRONMENT", "Ev GOOD", "Ev SECRET", "GOOD", "SECRET"),
            ] {
                for nested in [false, true] {
                    let source =
                        scope_fixture(section, lead, inner, (open, close), position, nested);
                    assert_owners(&source, key, fake, position);
                }
            }
        }
    }
}

#[test]
fn literal_quotes_and_braces_keep_their_scope_across_display() {
    // mdoc_macro.c treats these No operands as literal words. They remain
    // the caller's lexical argument bounds when a table interrupts them.
    for marks in [
        (r"No \(lq", r"No \(rq"),
        (r"No \(dq", r"No \(dq"),
        ("No {", "No }"),
    ] {
        for (section, lead, inner, key, fake) in [
            ("OPTIONS", "Fl q", "Fl Fl fake", "-q", "--fake"),
            ("COMMANDS", "Cm run", "Cm fake", "run", "fake"),
        ] {
            for position in POSITIONS {
                let source = scope_fixture(section, lead, inner, marks, position, false);
                assert_owners(&source, key, fake, position);
            }
        }
    }
}

#[test]
fn an_invisible_enclosure_still_owns_its_tables_and_nested_operands() {
    // mdoc_macro.c's Eo block always has a BODY even when the HEAD and
    // TAIL write no visible glyphs; all 18 exact native trees prove this.
    for marks in [("Eo", "Ec"), (r"Eo \&", r"Ec \&")] {
        for (section, lead, inner, key, fake) in [
            ("OPTIONS", "Fl q", "Fl Fl fake", "-q", "--fake"),
            ("COMMANDS", "Cm run", "Cm fake", "run", "fake"),
            ("ENVIRONMENT", "Ev GOOD", "Ev SECRET", "GOOD", "SECRET"),
        ] {
            for position in ["left", "right", "both"] {
                let source = scope_fixture(section, lead, inner, marks, position, false);
                assert_owners(&source, key, fake, position);
            }
        }
    }
}

#[test]
fn environment_alias_after_a_paired_enclosure_requires_its_own_separator() {
    // blk_part_exp and blk_exp_close move bare Eo/Ec punctuation beyond
    // HEAD and TAIL. Either accepted literal endpoint can pair with a
    // native endpoint of the same BODY, without promoting its inner Ev.
    for (open, close) in [
        ("Po", "Pc"),
        (r"Eo \&(", "Ec )"),
        ("Eo (", r"Ec \&)"),
        ("Eo (", "Ec )"),
        ("Eo OPEN", "Ec CLOSE"),
    ] {
        for separator in [" ,", ""] {
            let source = format!(
                "{HEADER}.Sh ENVIRONMENT\n.Bl -tag -width Ds\n.It Xo\n.Ev GOOD\n.{open}\n{TABLE}.Ev HIDDEN\n.{close}{separator}\n.Ev NEXT\n.Xc\nOWNERBODY.\n.El\n",
            );
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            let item = items[0];
            let owner = EntryOwner::Definition(item);
            let names = if separator.is_empty() {
                &["GOOD"][..]
            } else {
                &["GOOD", "NEXT"]
            };
            assert_eq!(owner.validated_names().unwrap(), names, "{source}");
            for name in names {
                check_direct(&query, name, item.source.unwrap().line, "OWNERBODY.");
            }
            assert_eq!(
                explain(&query, "HIDDEN").counts.direct_entry.total,
                0,
                "{source}"
            );
            assert_eq!(
                explain(&query, "NEXT").counts.direct_entry.total,
                u32::from(!separator.is_empty()),
                "{source}"
            );
        }
    }
}

fn scope_fixture(
    section: &str,
    lead: &str,
    inner: &str,
    (open, close): (&str, &str),
    position: &str,
    nested: bool,
) -> String {
    let mut head = format!(".{lead}\n");
    add(&mut head, TABLE, position == "outside-before");
    writeln!(head, ".{open}").unwrap();
    add(&mut head, ".Po\n", nested);
    add(&mut head, TABLE, matches!(position, "left" | "both"));
    writeln!(head, ".{inner}").unwrap();
    add(&mut head, TABLE, matches!(position, "right" | "both"));
    add(&mut head, ".Pc\n", nested);
    writeln!(head, ".{close}").unwrap();
    add(&mut head, TABLE, position == "outside-after");
    format!(
        "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It Xo\n{head}.Xc\nOWNERBODY.\n.It {inner}\nDECOYBODY.\n.El\n"
    )
}

fn add(head: &mut String, value: &str, condition: bool) {
    if condition {
        head.push_str(value);
    }
}

fn assert_owners(source: &str, key: &str, fake: &str, position: &str) {
    let query = load(source);
    let items = definitions(query.document.as_ref().unwrap());
    let [first, second] = items.as_slice() else {
        panic!("{source}")
    };
    let primary = EntryOwner::Definition(first);
    assert_eq!(
        primary.validated_names().unwrap_or_default(),
        [key],
        "{source}"
    );
    assert_eq!(
        EntryOwner::Definition(second)
            .validated_names()
            .unwrap_or_default(),
        [fake],
        "{source}"
    );
    check_direct(&query, key, first.source.unwrap().line, "OWNERBODY.");
    check_direct(&query, fake, second.source.unwrap().line, "DECOYBODY.");
    assert!(
        inline_plain_text(&first.terms[0]).contains(fake),
        "{source}"
    );
    let display_count = match position {
        "none" => 0,
        "both" => 2,
        _ => 1,
    };
    assert_eq!(exact_word(&query, "SENTINEL"), display_count, "{source}");
    if display_count > 0 {
        assert!(
            primary
                .forms()
                .unwrap()
                .iter()
                .all(|form| inline_plain_text(form) == key),
            "{source}"
        );
    } else {
        assert_eq!(
            explain(&query, &inline_plain_text(&first.terms[0]))
                .counts
                .direct_entry
                .total,
            1,
            "{source}"
        );
    }
}
