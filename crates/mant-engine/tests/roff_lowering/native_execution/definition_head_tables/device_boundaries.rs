//! First tbl print passes, frame entry and cell/span endline ownership.

use super::{
    Block, REVIEW_HEADER, lower,
    row_boundaries::{assert_rows, reading_rows},
    whole_word,
};
use mant_ir::{ResolvedContent, inline_plain_text};

const FOOTER: &str = ".Sh EXAMPLES\nTAIL\n";
const PREFIX: &str = ".No LONGPREFIX\n.br\n";

fn table(layout: &str, options: &str, rows: &str) -> String {
    format!(".TS\n{options}{layout}.\n{rows}\n.TE\n")
}

fn head(style: &str, prefix: &str, table: &str) -> String {
    format!(
        "{REVIEW_HEADER}.Bl -{style} -width 4n\n.It Xo\n{prefix}{table}.Xc\nENDING\n.El\n{FOOTER}"
    )
}

fn term(content: &ResolvedContent) -> String {
    let doc = content.document.as_ref().unwrap();
    let [Block::DefinitionList { items, .. }] = doc.sections[1].blocks.as_slice() else {
        panic!("expected one definition list");
    };
    assert!(
        items[0].entry.is_none(),
        "table text cannot name a declaration"
    );
    let text = inline_plain_text(&items[0].terms[0]);
    assert!(!text.contains("ENDING"), "BODY must have its own owner");
    text
}

#[test]
fn first_aligned_table_pass_distinguishes_breakable_and_fixed_blanks() {
    // Fixed CVS term_fill() excludes an ordinary trailing SP from vbr,
    // while ASCII_NBRSP counts as graph. tblcalc still sees the full cell.
    for (kind, wide) in [
        ("l", "WIDESTCELL"),
        ("c", "WIDESTCELL"),
        ("r", "WWWWWWWW"),
        ("n", "WIDESTCELL"),
    ] {
        for (tail, fixed) in [
            ("", ""),
            ("  ", ""),
            (r"\~\~", "\u{a0}\u{a0}"),
            (r"\~ ", "\u{a0}"),
        ] {
            let leads = if kind == "n" { &[""][..] } else { &["", " "] };
            for lead in leads {
                let rows = format!("{lead}XYZ{tail}\n{wide}");
                let source = head("hang", PREFIX, &table(kind, "", &rows));
                let separated = !lead.is_empty()
                    || kind == "n"
                    || (kind == "c" && fixed != "\u{a0}\u{a0}")
                    || (kind == "r" && fixed.is_empty());
                let gap = if separated { " " } else { "" };
                let first = format!("LONGPREFIX{gap}XYZ{fixed}");
                let query = lower(&source);
                assert_rows(&query, &[&first, wide, "ENDING"], &source);
                assert!(term(&query).contains(&format!("XYZ{fixed}")), "{source}");
                assert_eq!(
                    whole_word(&query, "XYZ").matches.len(),
                    usize::from(separated),
                    "{source}"
                );
                assert_eq!(
                    whole_word(&query, "LONGPREFIXXYZ").matches.len(),
                    usize::from(!separated),
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn an_adjacent_left_edge_has_a_word_boundary_even_when_vspace_is_suppressed() {
    // In tbl_term.c, the initial left rule can be the downward arm of the
    // next actual span. A rule two rows away cannot insert anything here.
    for (layout, at_first_row) in [("|l", true), ("l,\n|l", true), ("l,\nl,\n|l", false)] {
        for (request, space_suppressed) in [(".br\n", false), (".sp -1\n", true)] {
            let source = head(
                "hang",
                &format!(".No LONGPREFIX\n{request}"),
                &table(layout, "", "XYZ\nWIDESTCELL\nTHIRD"),
            );
            let mut expected = if !space_suppressed {
                vec!["LONGPREFIX", "XYZ"]
            } else if at_first_row {
                vec!["LONGPREFIX XYZ"]
            } else {
                vec!["LONGPREFIXXYZ"]
            };
            expected.extend(["WIDESTCELL", "THIRD", "ENDING"]);
            assert_rows(&lower(&source), &expected, &source);
        }
    }
}

#[test]
fn a_table_edge_closes_only_the_device_row_it_actually_ends() {
    // CVS term_tbl performs TBL_OPT_VSPACE before its top frame. A late
    // right edge alone does not request spacing; skipvsp may suppress the
    // inferred space but does not suppress a native top-frame endline.
    for (layout, options) in [
        ("l", ""),
        ("|l", ""),
        ("l|", ""),
        ("l,\nl|", ""),
        ("l", "box;\n"),
        ("l", "doublebox;\n"),
        ("l", "allbox;\n"),
    ] {
        for prefix in ["", PREFIX, ".No LONGPREFIX\n.sp -1\n"] {
            let spaced_edge = matches!(layout, "|l" | "l|");
            let source = head("hang", prefix, &table(layout, options, "XYZ\nWIDESTCELL"));
            let first: &[&str] = if prefix.is_empty() {
                if spaced_edge { &["", "XYZ"] } else { &["XYZ"] }
            } else if !options.is_empty() || (spaced_edge && prefix == PREFIX) {
                &["LONGPREFIX", "XYZ"]
            } else if layout == "|l" {
                &["LONGPREFIX XYZ"]
            } else {
                &["LONGPREFIXXYZ"]
            };
            let mut rows = first.to_vec();
            rows.extend(["WIDESTCELL", "ENDING"]);
            let query = lower(&source);
            assert_rows(&query, &rows, &source);
            assert!(term(&query).contains("XYZ"), "{source}");
            assert_eq!(
                whole_word(&query, "XYZ").matches.len(),
                usize::from(first != ["LONGPREFIXXYZ"]),
                "{source}"
            );
        }
    }
}

#[test]
fn word_end_markers_and_data_span_endlines_are_distinct() {
    // CVS term_word writes a marker, term_fill consumes it at a breakable
    // space, and term_flushln ignores a suffix of blanks/zero-width cells.
    // The data span nonetheless always completes its own row.
    for owner in ["head", "body", "block"] {
        for (word, first) in [
            (r"AA\pBB", &["AABB"][..]),
            (r"AA\p BB", &["AA", "BB"]),
            (r"AA\pBB\p", &["AABB"]),
            (r"AA\p", &["AA"]),
            (r"\p", &[""]),
            (r"AA\p  ", &["AA"]),
            (r"AA\p\~BB", &["AA\u{a0}BB"]),
            (r"AA\p \&", &["AA"]),
            (r"AA\p \~", &["AA", "\u{a0}"]),
        ] {
            for adjacent in [false, true] {
                let (layout, right, other) = if adjacent {
                    ("l l", "\tRIGHT", "\tOTHERCELL")
                } else {
                    ("l", "", "")
                };
                let table = table(layout, "", &format!("{word}{right}\nLAST{other}"));
                let source = match owner {
                    "head" => head("tag", "", &table),
                    "body" => format!(
                        "{REVIEW_HEADER}.Bl -tag -width 4n\n.It TAGWORD\n{table}ENDING\n.El\n{FOOTER}"
                    ),
                    _ => format!("{REVIEW_HEADER}{table}ENDING\n{FOOTER}"),
                };
                let query = lower(&source);
                let rows = expected_cell_rows(
                    owner,
                    first,
                    adjacent,
                    if owner == "head" { first.len() - 1 } else { 0 },
                );
                let rows = rows.iter().map(String::as_str).collect::<Vec<_>>();
                if owner == "head" || !adjacent {
                    assert_rows(&query, &rows, &source);
                } else {
                    // CLI can interleave physical column rows. The fenced
                    // Markdown fallback reads each cell in source order.
                    // Both preserve the hard line and span endings.
                    let markdown = mant_codec::encode::render_markdown(&query);
                    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
                    let text = mant_render::render_query_text(&query).replace('|', "");
                    assert_eq!(reading_rows(&text), rows, "{source}");
                    let text = mant_render::render_query_text(&readback).replace('|', "");
                    let exported = expected_cell_rows(owner, first, adjacent, first.len() - 1);
                    assert_eq!(reading_rows(&text), exported, "{source}\n{markdown}");
                }
                assert_eq!(whole_word(&query, "LAST").matches.len(), 1, "{source}");
                if owner == "head" {
                    assert!(term(&query).contains("LAST"), "{source}");
                }
            }
        }
    }
}

fn expected_cell_rows(owner: &str, first: &[&str], adjacent: bool, position: usize) -> Vec<String> {
    let mut rows = Vec::new();
    if owner == "body" {
        rows.push("TAGWORD".into());
    }
    for (index, word) in first.iter().enumerate() {
        if adjacent && index == position {
            if word.is_empty() {
                rows.push("RIGHT".into());
            } else {
                rows.push(format!("{word} RIGHT"));
            }
        } else {
            rows.push((*word).into());
        }
    }
    rows.push(if adjacent { "LAST OTHERCELL" } else { "LAST" }.into());
    rows.push("ENDING".into());
    rows
}
