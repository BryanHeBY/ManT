//! Quoted search parameters retain their original operand and owner boundaries.
use std::ops::Range;

use super::operands::{explain, load};
use super::search_templates::{check_three, list_groups, source};
use mant_ir::{
    EntryInlineRoot, EntryOwner, ResolvedContent, inline_plain_text, project_content_slice,
};
use mant_protocol::{
    ContentSelector, EntryProjection, EvidenceBasis, EvidenceClass, ExplanationSupport,
    OutlineNode, QueryExcerpt, QueryExplanation, QueryOutline,
};
use pulldown_cmark::{Event, Parser};

const COMMANDS: &str = ".Sh COMMANDS\n.Sm off\n";
const ENCLOSURES: &[(&str, &str, &str)] = &[
    ("Bq", "[", "]"),
    ("Pq", "(", ")"),
    ("Brq", "{", "}"),
    ("Dq", "“", "”"),
    ("Qq", "\"", "\""),
    ("Aq", "⟨", "⟩"),
];
const PARAMETERS: &[(&str, &str)] = &[
    ("RE", "RE"),
    (r#""RE,later""#, "RE,later"),
    ("\"查找\"", "查找"),
];

#[derive(Clone, Copy)]
enum Suffix {
    None,
    Unknown,
    Command,
    Flag,
}

impl Suffix {
    fn source_and_form(self) -> (&'static str, &'static str) {
        match self {
            Self::None => ("", ""),
            Self::Unknown => (" No !unknown", "!unknown"),
            Self::Command => (" , Cm later", ",later"),
            Self::Flag => (" , Fl q", ",-q"),
        }
    }

    fn name(self) -> Option<&'static str> {
        match self {
            Self::Command => Some("later"),
            Self::Flag => Some("-q"),
            Self::None | Self::Unknown => None,
        }
    }
}

fn query_round_trip(query: &ResolvedContent, name: &str) -> QueryExplanation {
    serde_json::from_str(&serde_json::to_string(&explain(query, name)).unwrap()).unwrap()
}

fn no_name(query: &ResolvedContent, name: &str, input: &str) {
    let result = query_round_trip(query, name);
    assert_eq!(result.counts.direct_entry.total, 0, "{name}: {input}");
    assert!(
        result.evidence.iter().all(|hit| {
            hit.bases
                .iter()
                .all(|basis| !matches!(basis, EvidenceBasis::Name { .. }))
        }),
        "{name}: {input}"
    );
    assert!(result.supports.is_empty(), "{name}: {input}");
}

fn assert_bound_range(owner: EntryOwner<'_>, name: &str, expected: Range<usize>, input: &str) {
    let facts = owner.facts().unwrap();
    let index = facts.names.iter().position(|value| value == name).unwrap();
    let binding = facts
        .name_bindings
        .iter()
        .find(|binding| binding.name == index)
        .unwrap();
    let [occurrence] = binding.occurrences.as_slice() else {
        panic!("one independently declared {name}: {input}");
    };
    let mut cursor = expected.start;
    let mut selected = String::new();
    for slice in &occurrence.parts {
        let span = project_content_slice(owner, slice).unwrap();
        assert_eq!(span.root, EntryInlineRoot::Term { index: 0 }, "{input}");
        assert_eq!(span.chars.start, cursor, "{input}");
        cursor = span.chars.end;
        selected.push_str(&inline_plain_text(&owner.content_slice(slice).unwrap()));
    }
    assert_eq!(cursor, expected.end, "{input}");
    assert_eq!(selected, name, "{input}");
}

fn assert_declared_name(query: &ResolvedContent, name: &str, input: &str) {
    let result = query_round_trip(query, name);
    assert_eq!(result.counts.direct_entry.total, 1, "{input}");
    let items = super::definitions(query.document.as_ref().unwrap());
    let hit = result
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(hit.source, items[0].source, "{input}");
    assert!(
        hit.bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. })),
        "{input}"
    );
    let supports: Vec<_> = result
        .supports
        .iter()
        .filter_map(|support| {
            let ExplanationSupport::DeclarationGroup { group, .. } = support else {
                return None;
            };
            Some((group, support.items().unwrap()))
        })
        .collect();
    let [(group, owners)] = supports.as_slice() else {
        panic!("one declaration-group support for {name}: {input}");
    };
    assert_eq!((group.start_item, group.end_item), (0, 3), "{input}");
    assert_eq!(owners.last().unwrap(), items[2], "{input}");

    let excerpt =
        mant_query::select_excerpt(query, &[ContentSelector::path(hit.outline.path())]).unwrap();
    let excerpt: QueryExcerpt =
        serde_json::from_str(&serde_json::to_string(&excerpt).unwrap()).unwrap();
    let original = mant_render::render_excerpt_text(&excerpt);
    assert!(!original.contains("GROUP_BODY"), "{input}");
    assert_eq!(
        mant_render::render_explanation_text(&result)
            .matches("GROUP_BODY")
            .count(),
        1,
        "{input}"
    );
    let markdown = mant_render::render_explanation_markdown(&result);
    let visible: String = Parser::new(&markdown)
        .filter_map(|event| match event {
            Event::Text(text) | Event::Code(text) => Some(text.into_string()),
            _ => None,
        })
        .collect();
    assert_eq!(visible.matches("GROUP_BODY").count(), 1, "{input}");
}

fn check_case(input: &str, form: &str, arguments: &[&str], bindings: &[(&str, Range<usize>)]) {
    let names: Vec<_> = bindings.iter().map(|(name, _)| *name).collect();
    let query = check_three(input, form, &names, (!names.is_empty()).then_some(0));
    let items = super::definitions(query.document.as_ref().unwrap());
    for &(name, ref range) in bindings {
        assert_bound_range(EntryOwner::Definition(items[0]), name, range.clone(), input);
        assert_declared_name(&query, name, input);
    }
    for name in arguments.iter().flat_map(|argument| argument.split(',')) {
        if !names.contains(&name) {
            no_name(&query, name, input);
        }
    }

    let outline = mant_query::build_outline_projection(&query, EntryProjection::All, None).unwrap();
    let outline: QueryOutline =
        serde_json::from_str(&serde_json::to_string(&outline).unwrap()).unwrap();
    let published: Vec<_> = outline
        .nodes
        .last()
        .unwrap()
        .children()
        .iter()
        .filter_map(|node| {
            if let OutlineNode::DocumentEntry { names, .. } = node {
                Some(names.as_slice())
            } else {
                None
            }
        })
        .collect();
    assert_eq!(published, [names.as_slice(), &["n"], &["N"]], "{input}");
}

fn check_quote_matrix(suffix: Suffix, covered: bool) {
    let sign = if covered { r"\z/" } else { "/" };
    let printed_sign = if covered { "" } else { "/" };
    let (suffix_source, suffix_form) = suffix.source_and_form();
    for &(enclosure, open, close) in ENCLOSURES {
        for &(raw_parameter, parameter) in PARAMETERS {
            for outside in [false, true] {
                let (prefix, printed_prefix) = if outside {
                    (
                        format!("Pf {sign} {enclosure}"),
                        format!("{printed_sign}{open}"),
                    )
                } else {
                    (
                        format!("{enclosure} Pf {sign}"),
                        format!("{open}{printed_sign}"),
                    )
                };
                let input = source(
                    "",
                    COMMANDS,
                    &format!(".It {prefix} Ar {raw_parameter}{suffix_source}\n"),
                );
                let form = format!("{printed_prefix}{parameter}{suffix_form}{close}");
                let binding = suffix.name().map(|name| {
                    let start = printed_prefix.chars().count() + parameter.chars().count() + 1;
                    (name, start..start + name.chars().count())
                });
                check_case(&input, &form, &[parameter], binding.as_slice());
            }
        }
    }
}

#[test]
fn quote_scopes_keep_parameters_unnamed_with_or_without_a_surviving_sign() {
    // Fixed CVS mdoc_macro.c::blk_part_imp() keeps Ar as a separate operand
    // and No as a separate tail. mdoc_term.c::termp_quote_pre() emits its
    // opening word before Ar. term.c::encode1() overstrikes \z/ with either
    // the generated mark or the initial Ar glyph, according to placement.
    for covered in [false, true] {
        for suffix in [Suffix::None, Suffix::Unknown] {
            check_quote_matrix(suffix, covered);
        }
    }
}

#[test]
fn full_quote_candidates_keep_the_confirmed_prefix_when_a_name_follows() {
    // The pinned AST puts the comma and Cm beside Ar. A comma within the
    // quoted Ar text payload stays inside that operand. Verify both places
    // the initial sign can be written before the actual overstrike.
    check_quote_matrix(Suffix::Command, true);
}

#[test]
fn surviving_signs_and_flags_keep_their_independent_name_boundaries() {
    // The fixed CVS tree places Cm and Fl after a literal comma outside
    // the Ar operand, including when the unoverstruck sign remains
    // visible. termp_fl_pre() prints the option's dash before its child.
    check_quote_matrix(Suffix::Command, false);
    for covered in [false, true] {
        check_quote_matrix(Suffix::Flag, covered);
    }
}

#[test]
fn a_quote_cutoff_keeps_the_next_independent_operand_separate() {
    for (head, form, parameter, name, range) in [
        (
            r".It Pf \z/ Bq Ar RE , Ar tail",
            "[RE,tail]",
            "RE",
            "tail",
            4..8,
        ),
        (r".It Pf \z/ Bq Ar RE , Cm RE", "[RE,RE]", "RE", "RE", 4..6),
        (
            r#".It Pf \z/ Bq Ar "RE,later" , Cm later"#,
            "[RE,later,later]",
            "RE,later",
            "later",
            10..15,
        ),
        (
            r#".It Pf \z/ Bq Ar "RE,later" , Cm next"#,
            "[RE,later,next]",
            "RE,later",
            "next",
            10..14,
        ),
    ] {
        let input = source("", COMMANDS, &format!("{head}\n"));
        check_case(&input, form, &[parameter], &[(name, range)]);
    }

    let input = source("", COMMANDS, ".It Bq Ar RE , Cm later\n");
    check_case(&input, "[RE,later]", &[], &[("RE", 1..3), ("later", 4..9)]);
}

#[test]
fn a_quote_resumes_at_the_first_unconsumed_word() {
    // CVS emits the same visible form for the glyphless No and Dv operands,
    // but the typed Dv stops the inherited prefix before the independent Ar.
    let input = source("", COMMANDS, ".It Pf \\z/ Bq No \\& Ar RE , Cm later\n");
    check_case(&input, "[RE,later]", &["RE"], &[("later", 4..9)]);
    let input = source("", COMMANDS, ".It Pf \\z/ Bq Dv \\& Ar RE , Cm later\n");
    check_case(&input, "[RE,later]", &[], &[("RE", 1..3), ("later", 4..9)]);

    // The second Pf begins its own expression at the resumed quote cursor.
    let input = source(
        "",
        COMMANDS,
        ".It Pf \\z/ Bq Ar RE , Pf \\z? Ar OTHER , Cm later\n",
    );
    check_case(
        &input,
        "[RE,OTHER,later]",
        &["RE", "OTHER"],
        &[("later", 10..15)],
    );
}

#[test]
fn quoted_parameter_prefixes_follow_real_scope_and_hard_line_boundaries() {
    // Native explicit Bo bodies and Xo/Bf/Bk scopes preserve their own
    // print order. Verify both nested and explicit closers.
    for (head, form, start) in [
        (r".It Pf \z/ Bq Pq Ar RE , Cm later", "[(RE,later)]", 5),
        (r".It Bq Pq Pf \z/ Ar RE , Cm later", "[(RE,later)]", 5),
        (
            ".It Xo\n.No \\z/\n.Bo Ar RE , Cm later\n.Bc\n.Xc",
            "[RE,later]",
            4,
        ),
        (
            ".It Xo\n.No \\z/\n.Bo Bq Ar RE , Cm later\n.Bc\n.Xc",
            "[[RE,later]]",
            5,
        ),
        (
            ".It Xo\n.Bk -words\n.Bf -symbolic\n.No \\z/\n.Bq Ar RE , Cm later\n.Ef\n.Ek\n.Xc",
            "[RE,later]",
            4,
        ),
    ] {
        let input = source("", COMMANDS, &format!("{head}\n"));
        check_case(&input, form, &["RE"], &[("later", start..start + 5)]);
    }
    for (head, form) in [
        (
            r".It Pf \z/ Bq Ar RE No !unknown Ar tail",
            "[RE!unknowntail]",
        ),
        (
            r".It Pf \z/ Bq No !unknown Ar RE , Cm later",
            "[!unknownRE,later]",
        ),
        (r#".It Pf \z/ Bq Ar "RE\p TAIL""#, "[RE\nTAIL]"),
    ] {
        let input = source("", COMMANDS, &format!("{head}\n"));
        check_case(&input, form, &["RE", "tail", "TAIL"], &[]);
    }
    // roff_term.c::roff_term_pre_br() flushes an ohang HEAD's partial row.
    let input = source(
        "",
        COMMANDS,
        ".It Xo\n.No \\z/\n.Bo Ar RE\n.br\n.Em \\&\n.Ar TAIL\n.Bc\n.Xc\n",
    )
    .replace("-tag -width Ds", "-ohang");
    check_case(&input, "[RE\nTAIL]", &["RE", "TAIL"], &[]);
}

#[test]
fn expanded_quote_scopes_bind_identical_text_to_the_correct_owner() {
    // roff.c::roff_userdef() reparses both It nodes with the same
    // line and column. The second Ar has no opener in its own scope.
    let input = source(
        "",
        COMMANDS,
        ".de PAIR\n.It Pf \\z/ Bq Ar RE , Cm later\n.It Bq Ar RE , Cm other\n..\n.PAIR\n",
    );
    let query = load(&input);
    let document = query.document.as_ref().unwrap();
    let items = super::definitions(document);
    let [search, unprotected, n, upper_n] = items.as_slice() else {
        panic!("{input}");
    };
    assert_eq!(search.source, unprotected.source, "{input}");
    assert_eq!(
        EntryOwner::Definition(search).validated_names().unwrap(),
        ["later"]
    );
    assert_eq!(
        EntryOwner::Definition(unprotected)
            .validated_names()
            .unwrap(),
        ["RE", "other"]
    );
    assert!(search.description.is_empty() && unprotected.description.is_empty());
    assert!(n.description.is_empty(), "{input}");
    assert!(mant_ir::blocks_have_readable_content(&upper_n.description));
    assert_eq!(list_groups(document), [vec![(0, 4)]]);
    assert_bound_range(EntryOwner::Definition(search), "later", 4..9, &input);
    assert_bound_range(EntryOwner::Definition(unprotected), "RE", 1..3, &input);
    let result = query_round_trip(&query, "RE");
    assert_eq!(result.counts.direct_entry.total, 1, "{input}");
    let hit = result
        .evidence
        .iter()
        .find(|row| row.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert!(
        hit.bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. }))
    );
    let excerpt =
        mant_query::select_excerpt(&query, &[ContentSelector::path(hit.outline.path())]).unwrap();
    let excerpt: QueryExcerpt =
        serde_json::from_str(&serde_json::to_string(&excerpt).unwrap()).unwrap();
    let excerpt = mant_render::render_excerpt_text(&excerpt);
    assert!(excerpt.contains("[RE,other]"), "{excerpt}");
    assert!(
        !excerpt.contains("later") && !excerpt.contains("GROUP_BODY"),
        "{excerpt}"
    );
}
