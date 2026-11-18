//! Native tbl content in a phrasing owner survives every reading projection.

use mant_ir::{Block, DocumentIndex, ResolvedContent, inline_plain_text};
use mant_protocol::{SearchCase, SearchQuery, SearchScope, SearchSyntax, default_search_limit};

const HEADER: &str =
    ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n";

fn verify_consumers(query: &ResolvedContent, expected: &[&str], source: &str) {
    let markdown = mant_codec::encode::render_markdown(query);
    let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for text in [
        mant_render::render_query_text(query),
        mant_render::render_query_text(&decoded),
    ] {
        let mut previous = 0;
        for word in expected {
            let positions = text.match_indices(word).collect::<Vec<_>>();
            assert_eq!(positions.len(), 1, "{word}: {source}\n{text}");
            assert!(positions[0].0 >= previous, "{source}\n{text}");
            previous = positions[0].0;
        }
    }
    for word in expected {
        let found = mant_query::search_query(
            query,
            &SearchQuery {
                pattern: (*word).into(),
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
        assert!(found.content_complete, "{source}");
        assert_eq!(found.matches.len(), 1, "{word}: {source}");
    }
}

fn lower(source: &str) -> ResolvedContent {
    let document = mant_loader::load_roff_bytes(source.as_bytes()).unwrap();
    crate::json_roundtrip::json_string_roundtrip(&document)
}

#[test]
fn native_cells_stay_in_their_head_or_block_owner_in_source_order() {
    // print_mdoc_node handles ROFFT_TBL by passing span directly to
    // term_tbl/print_tbl, including in Xo inside a definition HEAD.
    // All 32 inputs were checked in fixed CVS tree, ASCII, UTF-8 and HTML.
    for owner in ["head", "body", "body-xo", "paragraph"] {
        for (spec, cells) in [
            ("l.\nXYZ\n", &["XYZ"][..]),
            ("tab(|);\nl l.\nXYZ|SECOND\n", &["XYZ", "SECOND"][..]),
        ] {
            for prefix in ["", ".No PRE\n"] {
                for tag in ["", ".Tg Table.Point\n"] {
                    let middle = format!("{prefix}{tag}.TS\n{spec}.TE\n.No AFTER\n");
                    let body = match owner {
                        "head" => format!(".Bl -tag -width Ds\n.It Xo\n{middle}.Xc\nEND\n.El\n"),
                        "body" => format!(".Bl -tag -width Ds\n.It TAG\n{middle}END\n.El\n"),
                        "body-xo" => {
                            format!(".Bl -tag -width Ds\n.It TAG\n.Xo\n{middle}.Xc\nEND\n.El\n")
                        }
                        _ => format!("{middle}END\n"),
                    };
                    let source = format!("{HEADER}{body}");
                    let query = lower(&source);
                    let document = query.document.as_ref().unwrap();
                    let section = &document.sections[1].blocks;
                    if owner == "head" {
                        let [Block::DefinitionList { items, .. }] = section.as_slice() else {
                            panic!("{source}\n{section:?}");
                        };
                        let [item] = items.as_slice() else {
                            panic!("{source}\n{section:?}");
                        };
                        let term = inline_plain_text(&item.terms[0]);
                        assert!(
                            cells.iter().all(|word| term.contains(word)),
                            "{source}: {term}"
                        );
                        assert!(!term.contains("END"), "{source}: {term}");
                        if let Some(entry) = &item.entry {
                            assert!(
                                entry
                                    .names
                                    .iter()
                                    .all(|name| !cells.iter().any(|word| name.contains(word))),
                                "{source}: {entry:?}"
                            );
                        }
                    }
                    if !tag.is_empty() {
                        assert!(
                            DocumentIndex::build(document)
                                .fragment_target("Table.Point")
                                .is_some(),
                            "{source}"
                        );
                    }
                    let mut words = Vec::new();
                    if !prefix.is_empty() {
                        words.push("PRE");
                    }
                    words.extend_from_slice(cells);
                    words.extend(["AFTER", "END"]);
                    verify_consumers(&query, &words, &source);
                }
            }
        }
    }
}

#[test]
fn table_rows_and_formula_control_keep_printable_content_in_one_head() {
    // The fixed tree gives one native span per table row. EQ keeps its own
    // box tree at the same location and continues to use inline equation IR.
    for (fragment, words) in [
        (
            ".TS\ntab(|);\nl l.\nFOO|BAR\nQUX|BAZ\n.TE\n",
            &["FOO", "BAR", "QUX", "BAZ"][..],
        ),
        (".EQ\nXYZ\n.EN\n", &["XYZ"][..]),
        (
            ".TS\nl.\nT{\nA word\nSecond word\nT}\n.TE\n",
            &["A word", "Second word"][..],
        ),
        (
            ".TS\n|l|l|.\nXYZ\tSECOND\n_\nTHIRD\tFOURTH\n.TE\n",
            &["XYZ", "SECOND", "THIRD", "FOURTH"][..],
        ),
        (".TS\nlb li.\nBOLD\tITALIC\n.TE\n", &["BOLD", "ITALIC"][..]),
        (
            ".Ev HOME\n.TS\nl.\nFAKE_ENV\n.TE\n",
            &["HOME", "FAKE_ENV"][..],
        ),
    ] {
        let source = format!(
            "{HEADER}.Bl -tag -width Ds\n.It Xo\n.No PRE\n{fragment}.No AFTER\n.Xc\nEND\n.El\n"
        );
        let query = lower(&source);
        let mut expected = vec!["PRE"];
        expected.extend_from_slice(words);
        expected.extend(["AFTER", "END"]);
        verify_consumers(&query, &expected, &source);
    }
}

#[test]
fn head_rows_run_empty_columns_and_width_dependent_buffers() {
    // Fixed CVS tbl_term.c clears BACK* on each execution column, even
    // without a data cell. It then drains written numeric and rule buffers.
    // Only the one genuinely empty slot preserves its final armed \z.
    for (layout, row, expected) in [
        ("l.", r"\z", "FTER"),
        ("l l.", r"\z", "AFTER"),
        ("l l l.", r"\z", "AFTER"),
        ("l _.", r"\z", "AFTER"),
        ("l l.", r"_|\z", "AFTER"),
        ("l l.", r"=|\z", "AFTER"),
        ("l l.", r"\_|\z", "AFTER"),
        ("l l.", r"\l'0'|\z", "AFTER"),
        ("n l.", r"|\z", "AFTER"),
        ("l n.", r"\0|\z", "AFTER"),
        ("l l.", r"CELL|\z", "AFTER"),
    ] {
        let source = format!(
            "{HEADER}.Bl -tag -width Ds\n.It Xo\n.No PRE\n.TS\ntab(|);\n{layout}\n{row}\n.TE\n.No AFTER\n.Xc\nENDING\n.El\n"
        );
        verify_consumers(&lower(&source), &["PRE", expected, "ENDING"], &source);
    }
}
