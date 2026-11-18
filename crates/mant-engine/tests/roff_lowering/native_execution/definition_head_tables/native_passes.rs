//! Source-executed tbl geometry at the first and final cell passes.

use super::{Block, lower, whole_word};
use mant_ir::ResolvedContent;

const HEADER: &str = ".Dd November 18, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n";
const MAN_HEADER: &str = ".TH PROBE 1 \"November 18, 2026\" Test Manual\n.SH NAME\nprobe \\- test page\n.SH DESCRIPTION\n";

fn input(owner: &str, prefix: &str, layout: &str, data: &str) -> String {
    let table = format!("{prefix}.TS\n{layout}.\n{data}\n.TE\n");
    match owner {
        "head" => format!("{HEADER}.Bl -hang -width 4n\n.It Xo\n{table}.Xc\nENDING\n.El\n"),
        "body" => format!("{HEADER}.Bl -tag -width 4n\n.It TAG\n{table}ENDING\n.El\n"),
        "man" => format!("{MAN_HEADER}{table}ENDING\n"),
        _ => format!("{HEADER}{table}ENDING\n"),
    }
}

fn assert_prefix(layout: &str, first: &str, next: &str, request: &str, separated: bool) {
    let source = input(
        "head",
        &format!(".No LONGPREFIX\n{request}\n"),
        layout,
        &format!("{first}\n{next}"),
    );
    let content = lower(&source);
    let markdown = mant_codec::encode::render_markdown(&content);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for query in [&content, &readback] {
        assert_eq!(
            whole_word(query, "XYZ").matches.len(),
            usize::from(separated),
            "{source}\n{markdown}"
        );
        assert_eq!(
            whole_word(query, "LONGPREFIXXYZ").matches.len(),
            usize::from(!separated),
            "{source}\n{markdown}"
        );
        let text = mant_render::render_query_text(query);
        let line = text
            .lines()
            .find(|line| line.contains("LONGPREFIX"))
            .unwrap();
        assert!(
            line.contains(if separated {
                "LONGPREFIX XYZ"
            } else {
                "LONGPREFIXXYZ"
            }),
            "{source}\n{text}"
        );
    }
}

#[test]
fn leading_empty_cells_leave_the_printed_column_rule_as_a_word_boundary() {
    // tbl_term.c draws the inter-column edge after flushing each owner.
    // A downward arm also draws here. Explicit zero spacing suppresses it.
    for (layout, request, separated) in [
        ("l|l", ".sp -1", true),
        ("l||l", ".sp -1", true),
        ("l l,\nl|l", ".br", true),
        ("l l", ".sp -1", false),
        ("l0|l", ".sp -1", false),
    ] {
        assert_prefix(layout, "\tXYZ", "q\tWWWWWWWW", request, separated);
    }

    // A buffered horizontal stroke in a prior cell is independent of
    // inter-column spacing. The exact escaped '=' uses lint's existing
    // warning, while both native devices still draw its isolated rule.
    for layout in ["l0 l", "l0|l", "l0 l,\nl0|l"] {
        for prior in ["", "_", r"\_", "=", r"\="] {
            assert_prefix(
                layout,
                &format!("{prior}\tXYZ"),
                "q\tWIDECELL",
                ".sp -1",
                !prior.is_empty(),
            );
        }
    }
    // A preceding numeric fill can already have advanced coloff beyond
    // the next rule's target. Its unprinted blanks cannot stand in for
    // the suppressed rule and do not separate the reading words.
    for (layout, separated) in [("n0 l0 l", false), ("l0 l0 l", true)] {
        for prior in ["", r"\&"] {
            assert_prefix(
                layout,
                &format!("{prior}\t_\tXYZ"),
                "99\tq\tWIDECELL",
                ".sp -1",
                separated,
            );
        }
    }
}

#[test]
fn numeric_padding_and_horizontal_motion_use_actual_native_writes() {
    // tbl_fill_char's half-en numeric pad writes a complete fixed blank.
    for tail in ["", "  "] {
        for width in [6, 7, 8] {
            assert_prefix("n", &format!("XYZ{tail}"), &"W".repeat(width), ".br", true);
        }
    }
    // ESCAPE_HORIZ writes successive fixed blanks, while the responsive
    // reading text keeps its single-space projection. Exact CVS also
    // covers a fractional en and a long move, with no naked advance text.
    for (kind, fractional_separates) in [("c", false), ("r", true)] {
        for (tail, separated) in [
            ("", true),
            (r"\h'1n'", true),
            (r"\h'1.5n'", fractional_separates),
            (r"\h'3n'", false),
            (r"\h'300n'", false),
        ] {
            assert_prefix(kind, &format!("XYZ{tail}"), "WIDESTCELL", ".br", separated);
        }
    }
}

fn between(text: &str, first: &str, last: &str) -> Vec<String> {
    text.lines()
        .skip_while(|line| !line.contains(first))
        .skip(1)
        .take_while(|line| !line.contains(last))
        .map(|line| line.trim_matches(' ').to_owned())
        .collect()
}

fn projections(query: &ResolvedContent) -> [String; 2] {
    let markdown = mant_codec::encode::render_markdown(query);
    let decoded = mant_loader::load_markdown_text(&markdown, None).unwrap();
    [
        mant_render::render_query_text(query),
        mant_render::render_query_text(&decoded),
    ]
}

#[test]
fn an_empty_follow_up_tbl_pass_is_a_row_only_when_the_suffix_reaches_it() {
    // Exact pristine ASCII/UTF8 agree: the MULTICOL driver ends an
    // attempted second pass even when term_fill rejects its whole suffix.
    // The trailing sweep skips a lone breakpoint plus NBRZW instead.
    for owner in ["head", "body", "block"] {
        for (tail, middle) in [
            (r"\p \p XYZ", &[""][..]),
            (r"\p\: \&", &[]),
            (r"\p \p", &[]),
            (r"\p \p\& XYZ", &["", "XYZ"]),
            (r"\p XYZ", &["XYZ"]),
            (r"\p", &[]),
        ] {
            let source = input(owner, "", "l", &format!("START{tail}\nLAST"));
            let query = lower(&source);
            if owner != "head" {
                let document = query.document.as_ref().unwrap();
                let Some(Block::Table { rows, .. }) = first_table(&document.sections[1].blocks)
                else {
                    panic!("expected the native table owner: {source}");
                };
                let completed_rows = rows[0].cells[0]
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::VerticalSpace { lines, .. } => Some(*lines),
                        _ => None,
                    })
                    .sum::<u16>();
                assert_eq!(completed_rows, u16::from(middle == [""]), "{source}");
            }
            for text in projections(&query) {
                assert_eq!(between(&text, "START", "LAST"), middle, "{source}\n{text}");
                assert_eq!(text.matches("START").count(), 1, "{source}");
                assert_eq!(text.matches("LAST").count(), 1, "{source}");
            }
            assert_eq!(
                whole_word(&query, "XYZ").matches.len(),
                usize::from(middle.contains(&"XYZ")),
                "{source}"
            );
        }
    }
}

fn first_table(blocks: &[Block]) -> Option<&Block> {
    blocks.iter().find_map(|block| match block {
        Block::Table { .. } => Some(block),
        Block::DefinitionList { items, .. } => {
            items.iter().find_map(|item| first_table(&item.description))
        }
        _ => None,
    })
}

#[test]
fn leading_table_spacing_is_executed_inside_block_owners() {
    // term_tbl's term_vspace is separate from both the preceding br and
    // the row. A negative sp request suppresses exactly this empty line.
    for owner in ["block", "body", "man"] {
        for layout in ["l", "|l", "l|", "l|l"] {
            for request in [".br", ".sp -1"] {
                let data = if layout == "l|l" {
                    "XYZ\tA\nLAST\tB"
                } else {
                    "XYZ\nLAST"
                };
                let source = input(owner, &format!("LONGPREFIX\n{request}\n"), layout, data);
                let query = lower(&source);
                let spacing = u16::from(layout.contains('|') && request == ".br");
                let document = query.document.as_ref().unwrap();
                let table = first_table(&document.sections[1].blocks).unwrap();
                assert_eq!(mant_ir::geometry::block_gap(table), spacing, "{source}");
                for text in projections(&query) {
                    assert_eq!(
                        between(&text, "LONGPREFIX", "XYZ"),
                        vec![String::new(); usize::from(spacing)],
                        "{source}\n{text}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_real_right_cell_can_use_the_empty_second_table_pass() {
    // Exact tbl_term's MULTICOL loop can have a rejected left pass and
    // a useful right pass on the same device row.
    for owner in ["head", "body", "block"] {
        for (right, middle) in [("RIGHT", ""), (r"RIGHT\p FURTHER", "FURTHER")] {
            let source = input(
                owner,
                "",
                "l l",
                &format!("START\\p \\p XYZ\t{right}\nLAST\tOTHER"),
            );
            let query = lower(&source);
            let text = mant_render::render_query_text(&query).replace('|', "");
            assert_eq!(
                between(&text, "START", "LAST"),
                [middle],
                "{source}\n{text}"
            );
            for word in ["START", "RIGHT", "LAST", "OTHER", "ENDING"] {
                assert_eq!(whole_word(&query, word).matches.len(), 1, "{source}");
            }
            assert!(whole_word(&query, "XYZ").matches.is_empty(), "{source}");
            let [_, readback] = projections(&query);
            assert_eq!(readback.matches("RIGHT").count(), 1, "{source}");
            assert!(!readback.contains("XYZ"), "{source}");
        }
    }
}

#[test]
fn an_example_containing_a_ruled_table_has_both_opening_gaps() {
    // The installed Linux ascii(7) observation was independently checked
    // against fixed ASCII and UTF-8. This synthetic version exercises
    // the same .P / .EX / inner rule without importing a system manual.
    let source = format!(
        "{MAN_HEADER}ESCAPES end here.\n.P\n.EX\n.TS\nl l l l | l l l l.\nOct\tDec\tHex\tChar\tO2\tD2\tH2\tC2\n_\n000\t0\t00\tNUL\t100\t64\t40\t@\n.TE\n.EE\nENDING\n"
    );
    let query = lower(&source);
    let text = mant_render::render_query_text(&query);
    assert_eq!(between(&text, "ESCAPES", "Oct"), ["", ""], "{text}");
    assert_eq!(whole_word(&query, "ENDING").matches.len(), 1);
}
