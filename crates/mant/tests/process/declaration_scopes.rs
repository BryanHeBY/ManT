//! Native HEAD scopes cannot supply alias boundaries to their parents.

use crate::process_support::run_text_input;
use mant_ir::{Block, inline_plain_text};
use mant_protocol::{EvidenceBasis, EvidenceClass, ExplanationContent, QueryExplanation};

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

#[test]
fn independent_separators_and_attached_paths_keep_both_names_and_forms() {
    // in_line() closes Li/Sy before the next Fl; termp_fl_pre supplies
    // the dash, and mdoc_html.c keeps Pa and Ar outside the flag.
    for role in ["No", "Li", "Sy"] {
        for (separator, printed) in [("/", "/"), (r"\&,", ","), (r"\&|", "|")] {
            check_declaration(
                &format!("Fl x {role} {separator} Fl Fl long"),
                &format!("-x {printed} --long"),
                &["-x", "--long"],
                &["--fake"],
            );
        }
        for value in [
            "data / --fake",
            "/ --fake",
            "data , --fake",
            "data | --fake",
        ] {
            check_declaration(
                &format!(r#"Fl x Ar file {role} "{value}""#),
                &format!("-x file {value}"),
                &["-x"],
                &["--fake"],
            );
        }
    }
    for role in ["Pa", "Ar"] {
        check_declaration(
            &format!("Fl x Ar file No / Fl y Ns {role} path"),
            "-x file / -ypath",
            &["-x", "-y"],
            &["--fake"],
        );
    }
}

#[test]
fn attached_path_needs_a_distinct_form_to_allow_exact_lookup() {
    // CVS mdoc_macro.c::in_line closes Fl before Pa; termp_ns_pre only
    // suppresses the intervening space. Neither Pa spelling extends Fl;
    // termp_xr_pre emits the manual name and section as separate words.
    for (head, form) in [
        ("Fl k Ar src No / Fl q Ns Pa leaf", "-k src / -qleaf"),
        ("Fl q Ns Pa leaf No / Fl k Ar src", "-qleaf / -k src"),
    ] {
        let names = if head.starts_with("Fl k") {
            ["-k", "-q"]
        } else {
            ["-q", "-k"]
        };
        check_declaration(head, form, &names, &["-qleaf", "-kleaf"]);
    }
    check_declaration("Fl q Pa files", "-q files", &["-q"], &["-qfiles"]);
    check_declaration("Fl q Ns Ar files", "-qfiles", &["-q"], &["-qfile"]);

    for (head, query, expected) in [
        ("Fl q Ns Pa files", "-q", 1),
        ("Fl q Ns Pa files", "-qfiles", 0),
        ("Fl q Ns Pa leaf No / Fl k Ns Xr printf 3", "-k", 1),
        ("Fl q Ns Pa leaf No / Fl k Ns Xr printf 3", "-qleaf", 0),
        (
            "Fl q Ns Pa leaf No / Fl k Ns Xr printf 3",
            "-qleaf / -kprintf(3)",
            0,
        ),
        ("Fl q Ns Pa leaf No / Fl k Ns Xr printf 3", "-kprintf", 0),
    ] {
        let source = format!("{HEADER}.It {head}\nOwner proof.\n.El\n");
        let flag = format!("--explain={query}");
        let output = run_text_input(
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
        assert!(output.status.success(), "{head}: {query}");
        let reply: QueryExplanation = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(reply.counts.direct_entry.total, expected, "{head}: {query}");
    }
}

#[test]
fn quoted_native_assignment_retains_the_complete_indexed_form() {
    // Qq/Dq print their quotes around the complete No or Li TEXT child.
    for (head, form) in [
        (r#"Fl x Ns = Ns Qq No "dir/ name""#, "-x=\"dir/ name\""),
        (r#"Fl x Ns = Ns Dq No "dir/ name""#, "-x=“dir/ name”"),
        (r#"Fl x Ns = Ns Qq Li "dir/ name""#, "-x=\"dir/ name\""),
    ] {
        check_declaration(head, form, &["-x"], &["--fake"]);
    }
}

fn check_declaration(head: &str, form: &str, names: &[&str], absent: &[&str]) {
    let source = format!("{HEADER}.It {head}\nOwner proof.\n.El\n");
    let mut owner_path = None;
    for query in names
        .iter()
        .copied()
        .chain([form])
        .chain(absent.iter().copied())
    {
        let flag = format!("--explain={query}");
        let output = run_text_input(
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
        assert!(output.status.success(), "{head}: {query}");
        let reply: QueryExplanation = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            reply.counts.direct_entry.total,
            u32::from(!absent.contains(&query)),
            "{head}: {query}"
        );
        if absent.contains(&query) {
            continue;
        }
        let evidence = reply
            .evidence
            .iter()
            .find(|e| e.class == EvidenceClass::DirectEntry)
            .unwrap();
        let path = evidence.outline.path().to_string();
        assert_eq!(owner_path.get_or_insert(path.clone()), &path, "{head}");
        assert_eq!(evidence.source.unwrap().line, 9, "{head}");
        assert!(
            evidence.bases.iter().any(|basis| match basis {
                EvidenceBasis::Name { matches } if query != form => matches
                    .iter()
                    .any(|m| m.name == query && !m.occurrences.is_empty()),
                EvidenceBasis::Form { matches } if query == form => matches
                    .iter()
                    .any(|m| m.text == form && !m.occurrences.is_empty()),
                _ => false,
            }),
            "missing matching basis: {head}: {query}"
        );
        let entry = evidence.entry.as_ref().unwrap();
        assert_eq!(entry.names, names, "{head}");
        assert_eq!(
            entry
                .forms
                .iter()
                .map(|f| inline_plain_text(f))
                .collect::<Vec<_>>(),
            [form],
            "{head}"
        );
        let Some(ExplanationContent::Entry { block }) = &evidence.content else {
            panic!("missing owner: {head}")
        };
        let Block::DefinitionList { items, .. } = block else {
            panic!("wrong owner: {head}")
        };
        assert_eq!(inline_plain_text(&items[0].terms[0]), form, "{head}");
        let [Block::Paragraph { children, .. }] = items[0].description.as_slice() else {
            panic!("missing body: {head}")
        };
        assert_eq!(inline_plain_text(children), "Owner proof.", "{head}");
        check_positions(evidence, block, form, names, query, head);
    }
    check_node(&source, &owner_path.unwrap(), form, head);
}

fn check_node(source: &str, path: &str, form: &str, head: &str) {
    let node_flag = format!("--node={path}");
    let output = run_text_input(
        &[
            "--input",
            "-",
            "--input-format",
            "roff",
            "--format",
            "json",
            "--compact",
            &node_flag,
        ],
        source,
    );
    assert!(output.status.success(), "{head}");
    let reply: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let block: Block = serde_json::from_value(reply["selections"][0]["entry"].clone()).unwrap();
    let Block::DefinitionList { items, .. } = block else {
        panic!("wrong node: {head}")
    };
    assert_eq!(inline_plain_text(&items[0].terms[0]), form, "{head}");
    let [Block::Paragraph { children, .. }] = items[0].description.as_slice() else {
        panic!("missing node body: {head}")
    };
    assert_eq!(inline_plain_text(children), "Owner proof.", "{head}");
}

fn check_positions(
    evidence: &mant_protocol::ExplanationEvidence,
    block: &Block,
    form: &str,
    names: &[&str],
    query: &str,
    head: &str,
) {
    let entry = evidence.entry.as_ref().unwrap();
    for basis in &evidence.bases {
        match basis {
            EvidenceBasis::Name { matches } if query != form => {
                for matched in matches.iter().filter(|matched| matched.name == query) {
                    let binding = entry
                        .name_bindings
                        .iter()
                        .find(|binding| names[binding.name_index as usize] == query)
                        .unwrap();
                    assert_eq!(matched.occurrences, binding.occurrences, "{head}: {query}");
                }
            }
            EvidenceBasis::Form { matches } if query == form => {
                for matched in matches.iter().filter(|matched| matched.text == form) {
                    for occurrence in &matched.occurrences {
                        check_occurrence(occurrence, block, form, form, head);
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!(entry.name_bindings.len(), names.len(), "{head}");
    for binding in &entry.name_bindings {
        let name = names[binding.name_index as usize];
        assert!(!binding.occurrences.is_empty(), "{head}: {name}");
        for occurrence in &binding.occurrences {
            check_occurrence(occurrence, block, form, name, head);
        }
    }
}

fn check_occurrence(
    occurrence: &mant_protocol::ExplanationOccurrence,
    block: &Block,
    form: &str,
    expected: &str,
    head: &str,
) {
    let form_text: String = occurrence
        .forms
        .iter()
        .flat_map(|range| {
            assert_eq!(range.form_index, 0);
            form.chars()
                .skip(range.start_char as usize)
                .take((range.end_char - range.start_char) as usize)
        })
        .collect();
    let content_text: String = occurrence
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
    assert_eq!(form_text, expected, "{head}");
    assert_eq!(content_text, expected, "{head}");
}

#[test]
fn attached_native_values_preserve_composites_without_naming_a_joined_flag() {
    // Each suffix below has an independently executed native value receipt.
    // Ns changes spacing, not the accepted range of the preceding Fl name.
    for (role, value) in [
        ("Li", "data"),
        ("Cm", "data"),
        ("Ic", "data"),
        ("Sy", "data"),
        ("Ev", "DATA"),
    ] {
        let right = format!("Fl v Ns {role} {value}");
        let joined = format!("-v{value}");
        for (head, form, names) in [
            (
                format!("Fl a Ar mode No / {right}"),
                format!("-a mode / {joined}"),
                ["-a", "-v"],
            ),
            (
                format!("{right} No / Fl a Ar mode"),
                format!("{joined} / -a mode"),
                ["-v", "-a"],
            ),
        ] {
            check_declaration(&head, &form, &names, &[joined.as_str()]);
        }
        assert_eq!(
            explain_attached_tail(&right, "-v")
                .counts
                .direct_entry
                .total,
            1
        );
        assert_eq!(
            explain_attached_tail(&right, &joined)
                .counts
                .direct_entry
                .total,
            0
        );
    }
}

#[test]
fn attached_literal_exact_lookup_keeps_original_owner_and_full_range() {
    let form = "-a mode / -vdata";
    let reply = explain_attached_tail("Fl a Ar mode No / Fl v Ns Li data", form);
    assert_eq!(reply.counts.direct_entry.total, 1);
    let evidence = reply
        .evidence
        .iter()
        .find(|item| item.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(evidence.outline.path().to_string(), "2/e1");
    let ranges: Vec<_> = evidence
        .bases
        .iter()
        .filter_map(|basis| match basis {
            EvidenceBasis::Form { matches } => Some(matches),
            _ => None,
        })
        .flatten()
        .filter(|matched| matched.text == form)
        .flat_map(|matched| &matched.occurrences)
        .flat_map(|occurrence| &occurrence.forms)
        .map(|r| (r.form_index, r.start_char, r.end_char))
        .collect();
    assert_eq!(ranges, [(0, 0, 16)]);
}

#[test]
fn unproved_attached_tails_do_not_gain_a_composite_form() {
    for (head, query) in [
        (
            "Fl a Ar mode No / Fl v Ns Xr printf 3",
            "-a mode / -vprintf(3)",
        ),
        (
            "Fl a Ar mode No / Fl v Ns Lk https://example.test data",
            "-a mode / -vdata: https://example.test",
        ),
    ] {
        assert_eq!(
            explain_attached_tail(head, "-v").counts.direct_entry.total,
            1
        );
        assert_eq!(
            explain_attached_tail(head, query).counts.direct_entry.total,
            0
        );
    }
    // The pre-existing styled-argument route does not admit the short
    // -v name here and is not the independent native-value route.
    let styled = "Fl a Ar mode No / Fl v Ns Em data";
    assert_eq!(
        explain_attached_tail(styled, "-v")
            .counts
            .direct_entry
            .total,
        0
    );
    assert_eq!(
        explain_attached_tail(styled, "-a mode / -vdata")
            .counts
            .direct_entry
            .total,
        1
    );
    assert_eq!(
        explain_attached_tail("Fl vdata", "-vdata")
            .counts
            .direct_entry
            .total,
        1
    );
}

fn explain_attached_tail(head: &str, query: &str) -> QueryExplanation {
    let source = format!("{HEADER}.It {head}\nOwner proof.\n.El\n");
    let flag = format!("--explain={query}");
    let output = run_text_input(
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
    assert!(output.status.success(), "{head}: {query}");
    serde_json::from_slice(&output.stdout).unwrap()
}
