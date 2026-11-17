//! Independent declaration names adjacent to protected search parameters.
use super::operands::{explain, load};
use super::search_templates::{list_groups, source};
use mant_ir::{DefinitionItem, EntryOwner, ResolvedContent, inline_plain_text};
use mant_protocol::{
    ContentSelector, EntryProjection, EvidenceBasis, EvidenceClass, ExplanationContentRange,
    ExplanationSupport, OutlineNode, QueryExcerpt, QueryExplanation, QueryOutline,
};
use pulldown_cmark::{Event, Parser};
use std::ops::Range;

const SECTIONS: [&str; 3] = ["COMMANDS", "OPTIONS", "DESCRIPTION"];
const ENCLOSURES: [(&str, &str, &str); 4] = [
    ("", "", ""),
    ("Bq ", "[", "]"),
    ("Pq ", "(", ")"),
    ("Bq Pq ", "[(", ")]"),
];
const PARTS: [(&str, &str); 4] = [
    (r"Sy \z/ Ar RE", "RE"),
    ("Fl q", "-q"),
    ("Fl p", "-p"),
    ("Cm later", "later"),
];
const ORDERS: [&[usize]; 14] = [
    &[0, 1],
    &[1, 0],
    &[0, 2, 1],
    &[0, 1, 2],
    &[2, 0, 1],
    &[1, 0, 2],
    &[2, 1, 0],
    &[1, 2, 0],
    &[0, 3, 1],
    &[0, 1, 3],
    &[3, 0, 1],
    &[1, 0, 3],
    &[3, 1, 0],
    &[1, 3, 0],
];

#[test]
fn a_clipped_option_value_preserves_the_complete_key() {
    // CVS encode1() overwrites A after the assignment in the first
    // two inputs. In the third it overwrites c in the actual Fl key.
    for (head, form, names, absent) in [
        (
            r".It Sy \z/ Ar RE , Fl q=\zAB",
            "RE,-q=B",
            &[("-q", 3..5)][..],
            &["RE"][..],
        ),
        (
            r".It Bq Sy \z/ Ar RE , Fl q=\zAB",
            "[RE,-q=B]",
            &[("-q", 4..6)][..],
            &["RE"][..],
        ),
        (
            r".It Bq Sy \z/ Ar RE , Fl ob\zc=FILE",
            "[RE,-ob=FILE]",
            &[][..],
            &["RE", "-ob"][..],
        ),
        (r".It Fl q=\zAB", "-q=B", &[("-q", 0..2)][..], &[][..]),
    ] {
        check_head("OPTIONS", head, form, names, absent, !names.is_empty());
    }
}

#[test]
fn search_arguments_inside_one_invocation_do_not_change_its_name() {
    // Reduced from illumos-ksh93 and the native slash-argument fixture.
    // The native quote handlers place the inner alternatives in Op. Such
    // punctuation does not divide the head into independent declarations.
    for (head, form, name, argument) in [
        (
            r".It Fl o Sy \&? Ns Ar option",
            "-o ?option",
            "-o",
            "option",
        ),
        (r".It Fl n Ns / Ns Ar -NUM", "-n/-NUM", "-n", "-NUM"),
        (
            ".It Xo\n.Bk -words\n.Ic read Op Fl n Ar n | Fl N Ar n\n.Op Ar vname Ns Sy \\&? Ns Ar prompt\n.Ek\n.Xc",
            "read [-n n | -N n] [vname?prompt]",
            "read",
            "-N",
        ),
    ] {
        let input = source("", ".Sh COMMANDS\n.Sm on\n", &format!("{head}\n"));
        let content = load(&input);
        let document = content.document.as_ref().unwrap();
        let items = super::definitions(document);
        assert_eq!(
            EntryOwner::Definition(items[0]).validated_names().unwrap(),
            [name],
            "{input}"
        );
        assert_eq!(inline_plain_text(&items[0].terms[0]), form, "{input}");
        assert_eq!(
            roundtrip::<QueryExplanation>(&explain(&content, name))
                .counts
                .direct_entry
                .total,
            1,
            "{input}"
        );
        assert_eq!(
            roundtrip::<QueryExplanation>(&explain(&content, argument))
                .counts
                .direct_entry
                .total,
            0,
            "{input}"
        );
    }
}

#[test]
fn independent_flags_and_commands_are_named_in_source_order() {
    // All 168 native inputs are lint-clean. In mdoc_term.c, quote_pre/post
    // write the enclosure and termp_fl_pre writes the dash before executing
    // Fl's operand. Neither consumes the protected Ar as an option name.
    for section in SECTIONS {
        for (enclosure, opener, closer) in ENCLOSURES {
            for order in ORDERS {
                let mut head = format!(".It {enclosure}");
                let mut form = opener.to_owned();
                let mut names = Vec::new();
                for (index, &part) in order.iter().enumerate() {
                    if index > 0 {
                        head.push_str(" , ");
                        form.push(',');
                    }
                    let (source, visible) = PARTS[part];
                    let start = form.chars().count();
                    head.push_str(source);
                    form.push_str(visible);
                    if part != 0 {
                        names.push((visible, start..form.chars().count()));
                    }
                }
                form.push_str(closer);
                // identity_plan keeps named mixed Terms individually
                // addressable in generic DESCRIPTION, without reusing a
                // different declaration's description there.
                let groupable = section != "DESCRIPTION" || !order.contains(&3);
                check_head(section, &head, &form, &names, &["RE"], groupable);
            }
        }
    }
}

#[test]
fn identical_option_spellings_require_their_own_accepted_operand_ranges() {
    // The first pair has the same native form, but the native tree puts the
    // first -q inside a single Ar and the second in a distinct Fl scope.
    for section in SECTIONS {
        for (head, form, names, absent) in [
            (r#".It Sy \z/ Ar "-q""#, "-q", &[][..], &["-q"][..]),
            (
                r#".It Bq Sy \z/ Ar "RE,-q""#,
                "[RE,-q]",
                &[][..],
                &["RE", "-q"][..],
            ),
            (
                r".It Bq Sy \z/ Ar RE , Fl q",
                "[RE,-q]",
                &[("-q", 4..6)][..],
                &["RE"][..],
            ),
            (
                r#".It Bq Sy \z/ Ar "RE,-q" , Fl q"#,
                "[RE,-q,-q]",
                &[("-q", 7..9)][..],
                &["RE"][..],
            ),
            (
                r#".It Bq Sy \z/ Ar "-q" , Fl q"#,
                "[-q,-q]",
                &[("-q", 4..6)][..],
                &[][..],
            ),
            (
                r#".It Bq Pq Sy \z/ Ar "查找,-q" , Fl q"#,
                "[(查找,-q,-q)]",
                &[("-q", 8..10)][..],
                &["查找"][..],
            ),
            (
                r#".It Bq Sy \z/ Ar "查找,-q" , Fl q , Fl q"#,
                "[查找,-q,-q,-q]",
                &[("-q", 7..9), ("-q", 10..12)][..],
                &["查找"][..],
            ),
        ] {
            check_head(section, head, form, names, absent, true);
        }
    }
}

#[test]
fn option_admission_respects_separators_and_attached_parameters() {
    for section in SECTIONS {
        for (head, form, names, absent) in [
            (
                r".It Bq Sy \z/ Ar RE , No \-q",
                "[RE,-q]",
                &[][..],
                &["-q"][..],
            ),
            (
                r".It Bq Sy \z/ Ar RE , Dv \-q",
                "[RE,-q]",
                &[][..],
                &["-q"][..],
            ),
            (
                r".It Bq Sy \z/ Ar RE , Va \-q",
                "[RE,-q]",
                &[][..],
                &["-q"][..],
            ),
            (r".It Bq Sy \z/ Ar RE Fl q", "[RE-q]", &[][..], &["-q"][..]),
            (
                r".It Bq Sy \z/ Ar RE , Fl q Ar FILE",
                "[RE,-qFILE]",
                &[("-q", 4..6)][..],
                &["FILE", "-qFILE"][..],
            ),
            (
                r#".It Bq Sy \z/ Ar RE , Fl q Ar "-r""#,
                "[RE,-q-r]",
                &[("-q", 4..6)][..],
                &["-r", "-q-r"][..],
            ),
            (".It Bq Fl q", "[-q]", &[("-q", 1..3)][..], &[][..]),
            (
                ".It Bq Fl p , Fl q",
                "[-p,-q]",
                &[("-p", 1..3), ("-q", 4..6)][..],
                &[][..],
            ),
            (
                ".It Bq Va thing",
                "[thing]",
                &[("thing", 1..6)][..],
                &[][..],
            ),
        ] {
            check_head(section, head, form, names, absent, true);
        }
        check_head(
            section,
            ".It Bq Dv TERM",
            "[TERM]",
            &[("TERM", 1..5)],
            &[],
            section != "DESCRIPTION",
        );
    }
}

#[test]
fn option_names_require_the_complete_executed_key() {
    // encode1() can overwrite either the first letter or the last one.
    // The printed remnant has plausible option syntax but is a partial
    // key. An invisible cell that preserves the whole key is harmless.
    for section in SECTIONS {
        for (head, form, names, absent) in [
            (
                r".It Bq Sy \z/ Ar RE , Fl \zqr",
                "[RE,-r]",
                &[][..],
                &["-r"][..],
            ),
            (
                r".It Bq Sy \z/ Ar RE , Fl q\zR",
                "[RE,-q]",
                &[][..],
                &["-q"][..],
            ),
            (
                r".It Bq Sy \z/ Ar RE , Fl \zq",
                "[RE,-]",
                &[][..],
                &["-"][..],
            ),
            (
                r".It Bq Sy \z/ Ar RE , Fl \&q",
                "[RE,-q]",
                &[("-q", 4..6)][..],
                &[][..],
            ),
        ] {
            check_head(section, head, form, names, absent, true);
        }
    }
}

#[test]
fn native_marker_spellings_are_independent_of_a_protected_search_argument() {
    for section in SECTIONS {
        for (head, form, names, absent) in [
            (
                r".It Bq Sy \z/ Ar RE , Fl \-",
                "[RE,--]",
                &[("--", 4..6)][..],
                &[][..],
            ),
            (
                r".It Bq Sy \z/ Ar RE , Fl \-%",
                "[RE,--%]",
                &[("--%", 4..7)][..],
                &[][..],
            ),
            (
                r".It Pq Sy \z/ Ar RE , Fl \fI\-%",
                "(RE,--%)",
                &[("--%", 4..7)][..],
                &[][..],
            ),
            (
                r#".It Bq Sy \z/ Ar "查找,--%" , Fl \-%"#,
                "[查找,--%,--%]",
                &[("--%", 8..11)][..],
                &["查找"][..],
            ),
            (
                r#".It Bq Sy \z/ Ar "RE,--%""#,
                "[RE,--%]",
                &[][..],
                &["--%"][..],
            ),
        ] {
            check_head(section, head, form, names, absent, true);
        }
    }
}

#[test]
fn complete_operands_remain_addressable_across_independent_groups() {
    // Mandoc emits the option once from the separate Fl scope. Its quoted
    // assignment value is one TEXT word, and an optional argument has its
    // own generated enclosure rather than extending the key.
    for section in SECTIONS {
        let mixed_groupable = section != "DESCRIPTION";
        for (head, form, names, absent, groupable) in [
            (
                ".It Bq Ar FILE , Fl q",
                "[FILE,-q]",
                &[("FILE", 1..5), ("-q", 6..8)][..],
                &[][..],
                mixed_groupable,
            ),
            (
                ".It Ar FILE , Fl q",
                "FILE,-q",
                &[("FILE", 0..4), ("-q", 5..7)][..],
                &[][..],
                mixed_groupable,
            ),
            (
                r".It Bq Fl q , No !unknown , Sy \z/ Ar RE",
                "[-q,!unknown,RE]",
                &[("-q", 1..3)][..],
                &["!unknown", "RE"][..],
                true,
            ),
            (
                r#".It Bq Sy \z/ Ar RE , Fl "q=FILE,--fake""#,
                "[RE,-q=FILE,--fake]",
                &[("-q", 4..6)][..],
                &["RE", "FILE", "--fake"][..],
                true,
            ),
            (
                r#".It Bq Sy \z/ Ar RE , Fl "q=FILE|-p""#,
                "[RE,-q=FILE|-p]",
                &[("-q", 4..6)][..],
                &["RE", "FILE", "-p"][..],
                true,
            ),
            (
                r".It Sy \z/ Ar RE , Fl q Op Ar FILE",
                "RE,-q[FILE]",
                &[("-q", 3..5)][..],
                &["RE", "FILE"][..],
                true,
            ),
            (
                r".It Bq Sy \z/ Ar RE , Fl q Op Ar FILE",
                "[RE,-q[FILE]]",
                &[("-q", 4..6)][..],
                &["RE", "FILE"][..],
                true,
            ),
            (
                r".It Sy \z/ Ar RE , Fl q Qq Ar FILE",
                r#"RE,-q"FILE""#,
                &[("-q", 3..5)][..],
                &["RE", "FILE"][..],
                true,
            ),
        ] {
            check_head(section, head, form, names, absent, groupable);
        }
    }
}

fn check_head(
    section: &str,
    head: &str,
    form: &str,
    bindings: &[(&str, Range<usize>)],
    absent: &[&str],
    groupable: bool,
) {
    let input = source(
        "",
        &format!(".Sh {section}\n.Sm off\n"),
        &format!("{head}\n"),
    )
    .replace(".It Cm n\n.It Cm N\n", ".It Fl n\n.It Fl N\n");
    let content = load(&input);
    let document = content.document.as_ref().unwrap();
    let items = super::definitions(document);
    let [first, following, described] = items.as_slice() else {
        panic!("{input}");
    };
    let mut names = Vec::new();
    for &(name, _) in bindings {
        if !names.contains(&name) {
            names.push(name);
        }
    }
    let grouped = groupable && !names.is_empty();
    let groups = grouped.then_some((0, 3)).into_iter().collect::<Vec<_>>();
    assert_eq!(
        EntryOwner::Definition(first).validated_names().unwrap(),
        names,
        "{input}"
    );
    assert_eq!(list_groups(document), [groups], "{input}");
    assert_eq!(inline_plain_text(&first.terms[0]), form, "{input}");
    assert!(first.description.is_empty(), "{input}");
    assert!(following.description.is_empty(), "{input}");
    assert!(
        mant_ir::blocks_have_readable_content(&described.description),
        "{input}"
    );
    check_outline(&content, &names, form);
    for name in names {
        let ranges = bindings
            .iter()
            .filter(|(spelling, _)| *spelling == name)
            .map(|(_, range)| range.clone())
            .collect::<Vec<_>>();
        check_lookup(&content, first, described, name, &ranges, grouped, &input);
    }
    for (name, owner) in [("-n", following), ("-N", described)] {
        check_lookup(
            &content,
            owner,
            described,
            name,
            std::slice::from_ref(&(0..2)),
            grouped,
            &input,
        );
    }
    for &name in absent {
        let result = roundtrip(&explain(&content, name));
        let complete_form = name == form;
        assert_eq!(
            result.counts.direct_entry.total,
            u32::from(complete_form),
            "{name}: {input}"
        );
        if complete_form {
            assert!(
                result.evidence.iter().any(|hit| {
                    hit.bases
                        .iter()
                        .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
                }),
                "{name}: {input}"
            );
        }
        assert!(result.supports.is_empty(), "{name}: {input}");
        assert!(
            result.evidence.iter().all(|hit| {
                hit.bases
                    .iter()
                    .all(|basis| !matches!(basis, EvidenceBasis::Name { .. }))
            }),
            "{name}: {input}"
        );
    }
}

fn check_outline(content: &ResolvedContent, expected: &[&str], form: &str) {
    let outline =
        mant_query::build_outline_projection(content, EntryProjection::All, None).unwrap();
    let outline: QueryOutline = roundtrip(&outline);
    let entries = outline
        .nodes
        .last()
        .unwrap()
        .children()
        .iter()
        .filter_map(|node| {
            if let OutlineNode::DocumentEntry { names, forms, .. } = node {
                Some((names.as_slice(), forms.as_slice()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let [(names, forms), (following, _), (described, _)] = entries.as_slice() else {
        panic!("expected three visible owners for {form}");
    };
    assert_eq!(*names, expected, "{form}");
    assert_eq!(*forms, [form], "{form}");
    assert_eq!(*following, ["-n"], "{form}");
    assert_eq!(*described, ["-N"], "{form}");
}

fn check_lookup(
    content: &ResolvedContent,
    item: &DefinitionItem,
    described: &DefinitionItem,
    name: &str,
    ranges: &[Range<usize>],
    grouped: bool,
    input: &str,
) {
    let result = roundtrip(&explain(content, name));
    assert_eq!(result.counts.direct_entry.total, 1, "{name}: {input}");
    let hit = result
        .evidence
        .iter()
        .find(|row| row.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source, item.source, "{name}: {input}");
    check_occurrences(hit, name, ranges);
    if grouped {
        let [support @ ExplanationSupport::DeclarationGroup { group, .. }] =
            result.supports.as_slice()
        else {
            panic!("expected declaration support for {name}: {input}");
        };
        assert_eq!(
            (group.start_item, group.end_item),
            (0, 3),
            "{name}: {input}"
        );
        assert_eq!(
            support.items().unwrap().last(),
            Some(described),
            "{name}: {input}"
        );
    } else {
        assert!(result.supports.is_empty(), "{name}: {input}");
    }
    let excerpt =
        mant_query::select_excerpt(content, &[ContentSelector::path(hit.outline.path())]).unwrap();
    let excerpt: QueryExcerpt = roundtrip(&excerpt);
    assert_eq!(
        mant_render::render_excerpt_text(&excerpt)
            .matches("GROUP_BODY")
            .count(),
        usize::from(name == "-N"),
        "{name}: {input}"
    );
    let expected_mentions = usize::from(grouped || name == "-N");
    assert_eq!(
        mant_render::render_explanation_text(&result)
            .matches("GROUP_BODY")
            .count(),
        expected_mentions,
        "{name}: {input}"
    );
    assert_eq!(
        markdown_body_mentions(&result),
        expected_mentions,
        "{name}: {input}"
    );
}

pub(super) fn check_occurrences(
    hit: &mant_protocol::ExplanationEvidence,
    name: &str,
    ranges: &[Range<usize>],
) {
    let matches = hit
        .bases
        .iter()
        .find_map(|basis| {
            if let EvidenceBasis::Name { matches } = basis {
                Some(matches)
            } else {
                None
            }
        })
        .expect("accepted name evidence");
    let [record] = matches.as_slice() else {
        panic!("expected one matched spelling for {name}");
    };
    assert_eq!(record.name, name);
    assert_eq!(record.occurrences.len(), ranges.len(), "{name}");
    let entry = hit.entry.as_ref().unwrap();
    let display = entry
        .name_bindings
        .iter()
        .find(|binding| entry.names[binding.name_index as usize] == name)
        .unwrap();
    assert_eq!(record.occurrences, display.occurrences, "{name}");
    for (position, expected) in record.occurrences.iter().zip(ranges) {
        let mut cursor = expected.start;
        let mut spelling = String::new();
        for part in &position.forms {
            assert_eq!(part.form_index, 0, "{name}");
            let text = inline_plain_text(part.resolve(&entry.forms).unwrap());
            let start = part.start_char as usize;
            let end = part.end_char as usize;
            assert_eq!(start, cursor, "{name}");
            cursor = end;
            spelling.extend(text.chars().skip(start).take(end - start));
        }
        assert_eq!(cursor, expected.end, "{name}");
        assert_eq!(spelling, name, "{name}");
        let mut cursor = expected.start;
        for part in &position.content {
            assert!(
                matches!(
                    part,
                    ExplanationContentRange::DefinitionTerm {
                        item_index: 0,
                        term_index: 0,
                        ..
                    }
                ),
                "{name}"
            );
            let range = part.char_range();
            assert_eq!(range.start, cursor, "{name}");
            cursor = range.end;
        }
        assert_eq!(cursor, expected.end, "{name}");
    }
}

pub(super) fn roundtrip<
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
>(
    original: &T,
) -> T {
    let decoded = serde_json::from_str(&serde_json::to_string(original).unwrap()).unwrap();
    assert_eq!(&decoded, original);
    decoded
}

fn markdown_body_mentions(result: &QueryExplanation) -> usize {
    let markdown = mant_render::render_explanation_markdown(result);
    Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(text) | Event::Code(text) => Some(text.matches("GROUP_BODY").count()),
            _ => None,
        })
        .sum()
}
