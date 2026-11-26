//! Native HEAD scopes cannot supply alias boundaries to their parents.

use crate::process_support::run_text_input;
use mant_ir::{Block, inline_plain_text};
use mant_protocol::{EvidenceClass, ExplanationContent, QueryExplanation};

const HEADER: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh OPTIONS\n.Bl -tag -width Ds\n";

#[test]
fn executed_enclosures_do_not_end_outer_assignments() {
    // CVS mdoc_term.c::termp_quote_pre/post emit the native enclosure
    // around its operands; termp_fl_pre emits each option inside that scope.
    for (head, form) in [
        (
            "Fl a= Ns Ar <n>0 Po Ar y | Fl b= Ns Ar val Pc | Fl v Ns Ar > , Fl c",
            "-a=<n>0 (y | -b=val) | -v>, -c",
        ),
        (
            "Fl a= Ns Ar <n>0 Po Ar y , Fl b Ar val Pc | Fl v Ns Ar > , Fl c",
            "-a=<n>0 (y, -b val) | -v>, -c",
        ),
        (
            "Fl a= Ns Ar <n>0 Po Ar y | Fl b Pc | Fl v Ns Ar > , Fl c",
            "-a=<n>0 (y | -b) | -v>, -c",
        ),
    ] {
        check(head, form, &["-a", "-c"]);
    }

    for value in ["Ar ⟨z", "Li ⟨z", "Cm ⟨z", r"Li \(laz"] {
        check(
            &format!("Fl a= Ns Ar <root Bo {value} Bc | Fl v Ns Ar > , Fl b"),
            "-a=<root [⟨z] | -v>, -b",
            &["-a", "-b"],
        );
    }
}

#[test]
fn native_quote_text_cannot_leak_to_the_enclosing_or_next_assignment() {
    // Fixed CVS tree places the literal apostrophe in Ar inside Bo or Po.
    // In both positions, the outer alternative remains part of the form.
    check(
        r#"Fl a= Ns Ar <root Bo Ar "it's" Bc | Fl v Ns Ar > , Fl b"#,
        "-a=<root [it's] | -v>, -b",
        &["-a", "-b"],
    );
    check(
        r#"Fl a= Ns Ar root Po Ar "it's" Pc , Fl b= Ns Ar <yes | Fl v Ns Ar > , Fl c"#,
        "-a=root (it's), -b=<yes | -v>, -c",
        &["-a", "-b", "-c"],
    );
}

#[test]
fn separate_native_values_resolve_only_same_scope_numeric_angles() {
    // The fixed CVS tree and HTML show distinct flag operands followed by
    // Ar or Pa. Both terminals preserve the literal later angle glyph.
    for (head, form) in [
        (
            "Fl a= Ns Ar ⟨min⟩2 | Fl b Ar max⟩3 , Fl c",
            "-a=⟨min⟩2 | -b max⟩3, -c",
        ),
        (
            "Fl a= Ns Ar <min>2 | Fl b Ar max>=3 , Fl c",
            "-a=<min>2 | -b max>=3, -c",
        ),
        (
            "Fl a= Ns Ar ⟨min⟩2 , Fl b Pa /path⟩3 , Fl c",
            "-a=⟨min⟩2, -b /path⟩3, -c",
        ),
        (
            "Fl a= Ns Ar <min>2 , Fl b Pa /path>3 , Fl c",
            "-a=<min>2, -b /path>3, -c",
        ),
    ] {
        check(head, form, &["-a", "-b", "-c"]);
    }
    check(
        "Fl a= Ns Ar <min>2 | Fl v Ns Ar > , Fl b",
        "-a=<min>2 | -v>, -b",
        &["-a", "-b"],
    );
}

#[test]
fn bounded_assignment_shields_flags_even_if_a_typed_value_has_no_complete_form() {
    // Fixed CVS tree/ASCII/UTF-8/HTML: termp_fl_pre emits each flag, but
    // the literal typed angles keep the inner alternative inside the value.
    for head in [
        "Fl a= Ns Ar <root Va x | Fl v Ns Ar > , Fl b",
        "Fl a= Ns Ar ⟨root Fn test | Fl v Ns Ar ⟩ , Fl b",
    ] {
        let source = format!("{HEADER}.It {head}\nOwner proof.\n.El\n");
        for (query, expected) in [("-a", 1), ("-b", 1), ("-v", 0)] {
            let flag = format!("--explain={query}");
            let result = run_text_input(
                &[
                    "--input",
                    "-",
                    "--input-format",
                    "roff",
                    "--format",
                    "json",
                    &flag,
                ],
                &source,
            );
            assert!(result.status.success(), "{head}: {query}");
            let reply: QueryExplanation = serde_json::from_slice(&result.stdout).unwrap();
            assert_eq!(reply.counts.direct_entry.total, expected, "{head}: {query}");
            if let Some(evidence) = reply
                .evidence
                .iter()
                .find(|item| item.class == EvidenceClass::DirectEntry)
            {
                assert_eq!(
                    evidence.entry.as_ref().unwrap().names,
                    ["-a", "-b"],
                    "{head}"
                );
                assert_eq!(evidence.source.unwrap().line, 9, "{head}");
            }
        }
    }

    check(
        "Fl a= Ns Ar <user's-file | Fl v Ns Ar > , Fl b",
        "-a=<user's-file | -v>, -b",
        &["-a", "-b"],
    );
    // A flag outside the assignment remains a legitimate alias.
    check(
        "Fl a Ar subject , Fl v , Fl b",
        "-a subject, -v, -b",
        &["-a", "-v", "-b"],
    );
}

fn check(head: &str, form: &str, names: &[&str]) {
    let source = format!("{HEADER}.It {head}\nOwner proof.\n.El\n");
    for query in ["-a", "-b", "-c", "-v", form] {
        let flag = format!("--explain={query}");
        let result = run_text_input(
            &[
                "--input",
                "-",
                "--input-format",
                "roff",
                "--format",
                "json",
                "--compact",
                &flag,
            ],
            &source,
        );
        assert!(result.status.success(), "{head}: {query}: {result:?}");
        let reply: QueryExplanation = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(
            reply.counts.direct_entry.total,
            u32::from(query == form || names.contains(&query)),
            "{head}: {query}"
        );
        if let Some(evidence) = reply
            .evidence
            .iter()
            .find(|evidence| evidence.class == EvidenceClass::DirectEntry)
        {
            let entry = evidence.entry.as_ref().unwrap();
            assert_eq!(entry.names, names, "{head}");
            let [entry_form] = entry.forms.as_slice() else {
                panic!("expected the full declaration for {head}");
            };
            assert_eq!(inline_plain_text(entry_form), form, "{head}");
            assert_eq!(evidence.source.unwrap().line, 9, "{head}");
            let Some(ExplanationContent::Entry { block }) = &evidence.content else {
                panic!("missing declaration owner for {head}");
            };
            let Block::DefinitionList { items, .. } = block else {
                panic!("wrong declaration owner for {head}");
            };
            let [Block::Paragraph { children, .. }] = items[0].description.as_slice() else {
                panic!("missing original description for {head}");
            };
            assert_eq!(inline_plain_text(children), "Owner proof.", "{head}");

            assert_eq!(entry.name_bindings.len(), names.len(), "{head}");
            for binding in &entry.name_bindings {
                let name = &names[binding.name_index as usize];
                let [occurrence] = binding.occurrences.as_slice() else {
                    panic!("expected one occurrence of {name} in {head}");
                };
                let form_spelling: String = occurrence
                    .forms
                    .iter()
                    .flat_map(|range| {
                        assert_eq!(range.form_index, 0, "{head}: {name}");
                        form.chars()
                            .skip(range.start_char as usize)
                            .take((range.end_char - range.start_char) as usize)
                    })
                    .collect();
                assert_eq!(&form_spelling, name, "{head}");
                let content_spelling: String = occurrence
                    .content
                    .iter()
                    .map(|range| {
                        let text = range.resolve(block).unwrap().safe_text();
                        let chars = range.char_range();
                        text.chars()
                            .skip(chars.start)
                            .take(chars.len())
                            .collect::<String>()
                    })
                    .collect();
                assert_eq!(&content_spelling, name, "{head}");
            }
        }
    }
}
