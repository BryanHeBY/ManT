//! Native table glyphs in definition labels carry reading content only.
use super::{
    definitions,
    support::{explain, load},
};
use mant_ir::{EntryOwner, Inline, LinkTarget, ResolvedContent, inline_plain_text};
use mant_protocol::{
    ContentSelector, EvidenceBasis, EvidenceClass, SearchCase, SearchQuery, SearchScope,
    SearchSyntax, default_search_limit,
};

use crate::support::explanation_ranges;

const HEADER: &str =
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd exercise descriptors\n";
const SECTIONS: [&str; 4] = ["DESCRIPTION", "OPTIONS", "COMMANDS", "ENVIRONMENT"];

fn exact_word(query: &ResolvedContent, word: &str) -> usize {
    let result = mant_query::search_query(
        query,
        &SearchQuery {
            pattern: word.into(),
            syntax: SearchSyntax::Literal,
            case: SearchCase::Sensitive,
            scope: SearchScope::Visible,
            word: true,
            context_lines: 0,
            limit: default_search_limit(),
            offset: 0,
        },
    )
    .unwrap();
    assert!(result.content_complete);
    result.matches.iter().map(|row| row.occurrences.len()).sum()
}

fn check_direct(query: &ResolvedContent, name: &str, line: u32, sentinel: &str) {
    let result = explain(query, name);
    assert_eq!(result.counts.direct_entry.total, 1, "{name}: {result:?}");
    let direct = result
        .evidence
        .iter()
        .find(|hit| hit.class == EvidenceClass::DirectEntry)
        .unwrap();
    assert_eq!(direct.source.unwrap().line, line, "{name}");
    assert!(
        direct
            .bases
            .iter()
            .any(|basis| matches!(basis, EvidenceBasis::Name { .. })),
        "{name}: {direct:?}"
    );
    assert!(explanation_ranges::assert_ranges(&result, name) > 0);
    let excerpt =
        mant_query::select_excerpt(query, &[ContentSelector::path(direct.outline.path())]).unwrap();
    assert!(
        mant_render::render_excerpt_text(&excerpt).contains(sentinel),
        "{name}"
    );
}

#[test]
fn cells_are_searchable_in_every_owner_with_or_without_a_real_declaration() {
    // Fixed CVS mdoc_term.c visits ROFFT_TBL data independently of Fl/Ev.
    // The exact same roff sources exercised native tree, ASCII, UTF-8 and HTML.
    for section in SECTIONS {
        for (before, after, subject) in [
            ("", "", ""),
            (".Fl q\n", "", "-q"),
            ("", ".Fl q\n", "-q"),
            (".Ev HOME\n", "", "HOME"),
            ("", ".Ev HOME\n", "HOME"),
        ] {
            for cell in ["--fake", "FAKE_ENV", "fake"] {
                for tail in ["", ".No AFTER\n"] {
                    let source = format!(
                        "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It Xo\n{before}.TS\nl.\n{cell}\n.TE\n{after}{tail}.Xc\nENDING\n.El\n.Sh EXAMPLES\nTAIL\n"
                    );
                    let query = load(&source);
                    let items = definitions(query.document.as_ref().unwrap());
                    let [item] = items.as_slice() else {
                        panic!("{source}")
                    };
                    let head = inline_plain_text(&item.terms[0]);
                    assert!(head.contains(cell), "{source}: {head}");
                    assert_eq!(exact_word(&query, cell), 1, "{source}");
                    // The wire selector cannot include physical newlines.
                    // The owner must not record a queryable form with table
                    // text even where those characters block a CLI request.
                    let owner = EntryOwner::Definition(item);
                    if let Some(forms) = owner.forms() {
                        assert!(
                            forms
                                .iter()
                                .all(|form| !inline_plain_text(form).contains(cell)),
                            "{source}\n{head}"
                        );
                    }
                    for rejected in [cell, head.trim()]
                        .into_iter()
                        .filter(|value| !value.contains(char::is_control))
                    {
                        assert_eq!(
                            explain(&query, rejected).counts.direct_entry.total,
                            0,
                            "{source}\n{rejected}"
                        );
                    }
                    if after == ".Ev HOME\n" && !tail.is_empty() {
                        // The native label after the table is the single
                        // candidate HOME AFTER. Ordinary separated No text
                        // is not an admitted environment annotation.
                        assert_eq!(
                            explain(&query, subject).counts.direct_entry.total,
                            0,
                            "{source}"
                        );
                    } else if !subject.is_empty() {
                        assert_eq!(
                            owner
                                .validated_names()
                                .unwrap_or_else(|| panic!("{source}\n{item:?}")),
                            [subject],
                            "{source}"
                        );
                        check_direct(&query, subject, 9, "ENDING");
                    }
                }
            }
        }
    }
}

#[test]
fn ordinary_descriptions_keep_their_complete_semantic_forms() {
    // termp_under_pre and termp_bold_pre here execute ordinary, independent
    // HEAD operands. The display-only role is specific to native tbl cells.
    for (section, declaration, name, visible) in [
        ("DESCRIPTION", "Fl q Em sample", "-q", "-q sample"),
        ("ENVIRONMENT", "Ev HOME Sy FILES", "HOME", "HOME FILES"),
    ] {
        let source =
            format!("{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It {declaration}\nENDING\n.El\n");
        let query = load(&source);
        check_direct(&query, name, 9, "ENDING");
        let result = explain(&query, visible);
        assert_eq!(result.counts.direct_entry.total, 1, "{source}");
        assert!(
            result.evidence[0]
                .bases
                .iter()
                .any(|basis| matches!(basis, EvidenceBasis::Form { .. }))
        );
    }
}

#[test]
fn a_same_spelled_native_declaration_cannot_steal_two_cell_occurrences() {
    for section in SECTIONS {
        for (source_declaration, spelling) in [("Fl -fake", "--fake"), ("Ev FAKE_ENV", "FAKE_ENV")]
        {
            let source = format!(
                "{HEADER}.Sh {section}\n.Bl -tag -width Ds\n.It Xo\n.Fl q\n.TS\nl l.\n{spelling}\t{spelling}\n.TE\n.No AFTER\n.Xc\nFIRST_BODY\n.It {source_declaration}\nSECOND_BODY\n.El\n"
            );
            let query = load(&source);
            let items = definitions(query.document.as_ref().unwrap());
            assert_eq!(
                inline_plain_text(&items[0].terms[0])
                    .matches(spelling)
                    .count(),
                2,
                "{source}"
            );
            assert_eq!(
                EntryOwner::Definition(items[0]).validated_names().unwrap(),
                ["-q"],
                "{source}"
            );
            assert_eq!(
                EntryOwner::Definition(items[1]).validated_names().unwrap(),
                [spelling],
                "{source}"
            );
            assert_eq!(exact_word(&query, spelling), 3, "{source}");
            check_direct(
                &query,
                spelling,
                items[1].source.unwrap().line,
                "SECOND_BODY",
            );
            check_direct(&query, "-q", items[0].source.unwrap().line, "FIRST_BODY");
        }
    }
}

#[test]
fn recovered_email_and_option_in_one_cell_keep_link_and_reading_without_a_fake_name() {
    // Native CVS leaves both T{} macros as cell data. ManT's source-aware
    // cell recovery may enrich their output without changing its declaration role.
    let source = format!(
        "{HEADER}.Sh OPTIONS\n.Bl -tag -width Ds\n.It Xo\n.Fl q\n.TS\nl.\nT{{\n.Mt a@example.test\n.Fl -fake\nT}}\n.TE\n.No AFTER\n.Xc\nENDING\n.El\n.Sh EXAMPLES\nTAIL\n"
    );
    let query = load(&source);
    let items = definitions(query.document.as_ref().unwrap());
    let term = &items[0].terms[0];
    assert!(
        term.iter().any(|node| matches!(node,
        Inline::Link { target: LinkTarget::Email { address }, children, .. }
            if address == "a@example.test" && inline_plain_text(children) == "a@example.test")),
        "{term:?}"
    );
    assert_eq!(exact_word(&query, "--fake"), 1);
    assert_eq!(explain(&query, "--fake").counts.direct_entry.total, 0);
    check_direct(&query, "-q", 9, "ENDING");
}
