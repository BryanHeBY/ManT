//! Device row outcomes and portable projected rows of tables in a term.

use super::{Block, REVIEW_HEADER, ResolvedContent, inline_plain_text, lower, whole_word};

const FOOTER: &str = ".Sh EXAMPLES\nTAIL\n";

pub(super) fn reading_rows(text: &str) -> Vec<String> {
    let body = text
        .split_once("\nDESCRIPTION\n")
        .unwrap()
        .1
        .split_once("\n\nEXAMPLES\n")
        .unwrap()
        .0;
    body.lines()
        .map(|line| {
            let line = line.trim_matches(' ');
            if line == "•" {
                return String::new();
            }
            line.trim_start_matches("• ")
                .split_ascii_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

pub(super) fn assert_rows(query: &ResolvedContent, expected: &[&str], source: &str) {
    let markdown = mant_codec::encode::render_markdown(query);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [query, &readback] {
        assert_eq!(
            reading_rows(&mant_render::render_query_text(content)),
            expected,
            "{source}\n{markdown}"
        );
    }
}

#[test]
fn table_entry_flush_preserves_prefix_word_boundaries_and_data_rows() {
    // All 36 exact inputs ran pristine CVS tree, ASCII, UTF-8, HTML and
    // lint. term_newln() can commit a NOBREAK/HANG tag prefix without
    // closing the device row. tbl_term.c owns every data-span endline.
    for style in ["tag", "hang"] {
        for prefix in ["", ".No PRE\n", ".No PRE\n.br\n"] {
            for empties in 0..=2 {
                let blanks = "\\&\n".repeat(empties);
                for suffix in ["", ".No AFTER\n"] {
                    let source = format!(
                        "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n{prefix}.TS\nl.\nFIRST\n{blanks}LAST\n.TE\n{suffix}.Xc\nENDING\n.El\n{FOOTER}"
                    );
                    let query = lower(&source);
                    let prefix_break = style == "tag" && prefix.contains(".br");
                    let mut rows = match (prefix.is_empty(), prefix_break) {
                        (true, _) => vec!["FIRST"],
                        (false, true) => vec!["PRE", "FIRST"],
                        (false, false) => vec!["PRE FIRST"],
                    };
                    rows.extend(std::iter::repeat_n("", empties));
                    rows.push("LAST");
                    let head_rows = rows.len();
                    match (suffix.is_empty(), prefix_break) {
                        (true, _) => rows.push("ENDING"),
                        (false, true) => rows.extend(["AFTER", "ENDING"]),
                        (false, false) => rows.push("AFTER ENDING"),
                    }
                    assert_rows(&query, &rows, &source);
                    let doc = query.document.as_ref().unwrap();
                    let [Block::DefinitionList { items, .. }] = doc.sections[1].blocks.as_slice()
                    else {
                        panic!("HEAD table needs a definition owner: {source}");
                    };
                    let [item] = items.as_slice() else {
                        panic!("expected one source item: {source}");
                    };
                    let [term] = item.terms.as_slice() else {
                        panic!("expected one source term: {source}");
                    };
                    let text = inline_plain_text(term);
                    assert_eq!(
                        text.lines().take(head_rows).collect::<Vec<_>>(),
                        rows[..head_rows],
                        "{source}"
                    );
                    assert!(!text.contains("ENDING"), "{source}");
                    assert_eq!(
                        whole_word(&query, "PRE").matches.len(),
                        usize::from(!prefix.is_empty()),
                        "{source}"
                    );
                    assert!(
                        whole_word(&query, "PREFIRST").matches.is_empty(),
                        "{source}"
                    );
                    for word in ["FIRST", "LAST", "ENDING"] {
                        assert_eq!(
                            whole_word(&query, word).matches.len(),
                            1,
                            "{word}: {source}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn table_blank_row_positions_remain_distinct_from_entry_and_exit() {
    // term_tbl() ends each data span, including a cell with only NBRZW.
    // In the first blank row a still-open prefix can provide the glyphs.
    for position in 0..=2 {
        for empties in 0..=2 {
            for style in ["tag", "hang"] {
                let mut cells = vec!["FIRST", "LAST"];
                cells.splice(position..position, std::iter::repeat_n("\\&", empties));
                let cells = cells.join("\n");
                let source = format!(
                    "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n.No PRE\n.TS\nl.\n{cells}\n.TE\n.No AFTER\n.Xc\nENDING\n.El\n{FOOTER}"
                );
                let mut expected = Vec::new();
                if position == 0 && empties > 0 {
                    expected.push("PRE");
                    expected.extend(std::iter::repeat_n("", empties - 1));
                    expected.extend(["FIRST", "LAST"]);
                } else {
                    expected.push("PRE FIRST");
                    expected.extend(std::iter::repeat_n(
                        "",
                        if position == 1 { empties } else { 0 },
                    ));
                    expected.push("LAST");
                    expected.extend(std::iter::repeat_n(
                        "",
                        if position == 2 { empties } else { 0 },
                    ));
                }
                expected.push("AFTER ENDING");
                assert_rows(&lower(&source), &expected, &source);
            }
        }
    }
}

#[test]
fn wholly_empty_data_rows_have_no_extra_markdown_boundary() {
    // Fixed tbl_term.c ends every data span even without printed cell text.
    // The requested first blank can carry Markdown's generated marker when
    // no HEAD word owns that row. It must not add a separate source gap.
    for style in ["tag", "hang"] {
        for empty_rows in 1..=3 {
            let blanks = "\\&\n".repeat(empty_rows);
            for (suffix, ending) in [("", "ENDING"), (".No AFTER\n", "AFTER ENDING")] {
                let source = format!(
                    "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n.TS\nl.\n{blanks}.TE\n{suffix}.Xc\nENDING\n.El\n{FOOTER}"
                );
                let mut expected = vec![""; empty_rows];
                expected.push(ending);
                assert_rows(&lower(&source), &expected, &source);
            }
        }
    }
}

#[test]
fn a_wholly_empty_table_row_remains_distinct_from_body_spacing() {
    // term_tbl ends one row at the span; a later roff_term_pre_sp runs
    // term_vspace independently. All four sources matched fixed CVS.
    for style in ["tag", "hang"] {
        for (request, gap) in [("", 0), (".sp 1\n", 1)] {
            let source = format!(
                "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n.TS\nl.\n\\&\n.TE\n.Xc\n{request}ENDING\n.El\n{FOOTER}"
            );
            let query = lower(&source);
            let document = query.document.as_ref().unwrap();
            let [Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice()
            else {
                panic!("expected one native definition list");
            };
            let (row, body) = items[0].description.split_first().unwrap();
            assert!(matches!(
                row,
                Block::Paragraph { children, .. } if children == &[mant_ir::Inline::line_break()]
            ));
            assert_eq!(
                body.iter().map(mant_ir::geometry::block_gap).sum::<u16>(),
                gap,
                "{source}"
            );
            let expected = if gap == 0 {
                &["", "ENDING"][..]
            } else {
                &["", "", "ENDING"]
            };
            assert_rows(&query, expected, &source);
        }
    }
}

#[test]
fn empty_head_table_rows_and_layout_requests_each_complete_once() {
    // Pinned tbl_term.c::term_tbl ends data spans, while term.c::term_vspace
    // independently writes an empty row. The surrounding CVS rule lines
    // have decorative glyphs and are omitted from the phrasing projection.
    for style in ["tag", "hang"] {
        for (spec, table_rows) in [("\\&", 1), ("\\&\n\\&", 2), ("\\&\n_", 1), ("_\n\\&", 1)] {
            for request_before in [false, true] {
                for (request, gap) in [(".sp 1\n", 1), (".sp 2\n", 2), (".br\n", 0)] {
                    // Both rule positions were run against native .sp 1.
                    if spec.contains('_') && gap != 1 {
                        continue;
                    }
                    let (before, after) = if request_before {
                        (request, "")
                    } else {
                        ("", request)
                    };
                    let source = format!(
                        "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n{before}.TS\nl.\n{spec}\n.TE\n{after}.Xc\nENDING\n.El\n{FOOTER}"
                    );
                    let query = lower(&source);
                    let mut expected = vec![""; table_rows + gap];
                    expected.push("ENDING");
                    assert_eq!(
                        reading_rows(&mant_render::render_query_text(&query)),
                        expected,
                        "{source}"
                    );
                    // Markdown simplifies any positive layout gap to one.
                    let markdown = mant_codec::encode::render_markdown(&query);
                    let restored = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    if gap > 1 {
                        expected.remove(0);
                    }
                    assert_eq!(
                        reading_rows(&mant_render::render_query_text(&restored)),
                        expected,
                        "{source}\n{markdown}"
                    );
                }
            }
        }
    }
}

#[test]
fn identical_words_in_a_head_table_and_next_item_keep_their_owners() {
    use mant_protocol::OutlineNodeReference;

    // The four exact CVS inputs place one FIRST in a tbl span and the
    // other in the next It BODY, with a real blank data row between.
    for style in ["tag", "hang"] {
        for (cells, first_row) in [
            ("FIRST|SECOND", "PRE FIRST SECOND"),
            ("SECOND|FIRST", "PRE SECOND FIRST"),
        ] {
            let source = format!(
                "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n.No PRE\n.TS\ntab(|);\nl l.\n{cells}\n\\&|\\&\nLAST|FINAL\n.TE\n.Xc\nOWNER-A\n.It Other\nFIRST\n.El\n{FOOTER}"
            );
            let query = lower(&source);
            assert_rows(
                &query,
                &[first_row, "", "LAST FINAL", "OWNER-A", "", "Other FIRST"],
                &source,
            );
            let document = query.document.as_ref().unwrap();
            let [Block::DefinitionList { items, .. }] = document.sections[1].blocks.as_slice()
            else {
                panic!("expected one native definition list");
            };
            let [first, next] = items.as_slice() else {
                panic!("expected two native items");
            };
            assert_eq!(
                inline_plain_text(&first.terms[0])
                    .lines()
                    .collect::<Vec<_>>(),
                [first_row, "", "LAST FINAL"],
            );
            let [Block::Paragraph { children, .. }] = next.description.as_slice() else {
                panic!("expected next item's description");
            };
            assert_eq!(inline_plain_text(children), "FIRST");
            let search = whole_word(&query, "FIRST");
            let [table_hit, next_item_hit] = search.matches.as_slice() else {
                panic!("expected exactly two whole-word hits: {source}");
            };
            assert!(matches!(
                &table_hit.outline.node,
                OutlineNodeReference::DocumentSection { title, .. } if title == "DESCRIPTION"
            ));
            assert!(matches!(
                &next_item_hit.outline.node,
                OutlineNodeReference::DocumentEntry { title, .. } if title == "Other"
            ));
            for hit in [table_hit, next_item_hit] {
                assert_eq!(hit.occurrences.len(), 1);
                assert_eq!(hit.occurrences[0].matched_text, "FIRST");
            }
        }
    }
}

#[test]
fn table_cell_spans_and_rules_keep_their_reading_boundaries() {
    // The exact sources ran fixed CVS; omit the decorative rule row in
    // the phrasing projection, independently from its native endline.
    for style in ["tag", "hang"] {
        for (request, separate, blanks) in [
            (".br\n", style == "tag", 0),
            (".sp 1\n", true, 0),
            (".sp 2\n", true, 1),
            (".No \\&\n", false, 0),
            ("", false, 0),
        ] {
            let source = format!(
                "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n.No PRE\n{request}.TS\ntab(|);\nl l.\nFIRST|RIGHT\n_\nLAST|FINAL\n.TE\n.No AFTER\n.Xc\nENDING\n.It Other\nNEXT-ITEM\n.El\n{FOOTER}"
            );
            let mut expected = if separate {
                let mut rows = vec!["PRE"];
                rows.extend(std::iter::repeat_n("", blanks));
                rows.push("FIRST RIGHT");
                rows
            } else {
                vec!["PRE FIRST RIGHT"]
            };
            expected.push("LAST FINAL");
            if separate && style == "tag" {
                expected.extend(["AFTER", "ENDING"]);
            } else {
                expected.push("AFTER ENDING");
            }
            expected.extend(["", "Other NEXT-ITEM"]);
            assert_rows(&lower(&source), &expected, &source);
        }
        for (layout, cells, second) in [
            ("l s l.", "FIRST|RIGHT\nLAST|FINAL", "LAST FINAL"),
            ("l l, ^ l.", "FIRST|RIGHT\n\\^|FINAL", "FINAL"),
        ] {
            let source = format!(
                "{REVIEW_HEADER}.Bl -{style} -width Ds\n.It Xo\n.No PRE\n.TS\ntab(|);\n{layout}\n{cells}\n.TE\n.Xc\nENDING\n.It Other\nNEXT-ITEM\n.El\n{FOOTER}"
            );
            assert_rows(
                &lower(&source),
                &["PRE FIRST RIGHT", second, "ENDING", "", "Other NEXT-ITEM"],
                &source,
            );
        }
    }
}
