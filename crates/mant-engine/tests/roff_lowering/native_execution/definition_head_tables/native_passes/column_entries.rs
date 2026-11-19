//! Table fill starts with the native column's existing writes and bounds.

use super::{Block, first_table, lower, projections, whole_word};

const MAN: &str =
    ".TH PROBE 1 \"November 19, 2026\"\n.SH NAME\nprobe \\- example\n.SH DESCRIPTION\n";
const MDOC: &str = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd test page\n.Sh DESCRIPTION\n";

#[test]
fn numeric_alignment_changes_the_native_word_entry_slots() {
    // Both fixed devices agree: tbl_number() writes its alignment pad with
    // tbl_fill_char(ASCII_NBRSP). When at least two such bytes precede the
    // blank overstrike, ESCAPE_OVERSTRIKE trims the tail before writing \p.
    // One pad, or no pad in a left-aligned cell, still rejects XYZ.
    for layout in ["l", "n", "nb", "ni", "n l", "n0 l"] {
        let multi = layout.contains(' ');
        let side = if multi { "\tSIDE" } else { "" };
        let next = if multi { "\tNEXT" } else { "" };
        for digits in [7, 8, 9] {
            let witness = "9".repeat(digits);
            for word in ["\\o' '\\p XYZ", "T{\n\\o' '\\p XYZ\nT}"] {
                let source =
                    format!("{MAN}.TS\n{layout}.\n{word}{side}\n{witness}{next}\n.TE\nENDING\n");
                let shows_word = layout.starts_with('n') && digits >= 8;
                let query = check_reading(&source, shows_word);
                let section = &query.document.as_ref().unwrap().sections[1];
                let Some(Block::Table { rows, .. }) = first_table(&section.blocks) else {
                    panic!("expected table owner: {source}");
                };
                let [Block::Paragraph { children, .. }, ..] = rows[0].cells[0].blocks.as_slice()
                else {
                    panic!("expected first data cell: {source}");
                };
                let text = mant_ir::inline_plain_text(children);
                assert_eq!(text.contains("XYZ"), shows_word, "{source}: {text:?}");
                assert!(!text.contains('9') && !text.contains("SIDE"), "{source}");
                for reading in projections(&query) {
                    assert_eq!(reading.matches(&witness).count(), 1, "{source}\n{reading}");
                    if multi {
                        assert_eq!(reading.matches("SIDE").count(), 1, "{source}\n{reading}");
                        assert_eq!(reading.matches("NEXT").count(), 1, "{source}\n{reading}");
                    }
                }
            }
        }
    }
}

#[test]
fn alignment_blanks_can_accept_a_pass_before_an_unedited_word() {
    // An NBRSP sets graph in term_fill() before it inspects the marker.
    // A second marker and blank can still reject the next pass; only the
    // current column owns the accepted buffer, even with a prior tcol.
    for layout in ["l", "n", "n l", "l n", "n n"] {
        let before = if layout == "l n" { "SIDE\t" } else { "" };
        let after = if matches!(layout, "n l" | "n n") {
            "\tSIDE"
        } else {
            ""
        };
        for digits in [2, 7, 8] {
            let witness = "9".repeat(digits);
            for marker in [r"\p XYZ", r"\p\: XYZ", r"\p \p XYZ"] {
                let source = format!(
                    "{MAN}.TS\n{layout}.\n{before}{marker}{after}\n{before}{witness}{after}\n.TE\nENDING\n"
                );
                check_reading(
                    &source,
                    layout != "l" && digits >= 7 && marker != r"\p \p XYZ",
                );
            }
        }
    }
}

#[test]
fn a_prior_numeric_column_can_consume_the_next_padding() {
    // tbl_fill_string() guards each pad against the earlier columns' coloff.
    // A narrow or empty number advances it past the second pad's target.
    // A wide first number and a nonnumeric first column keep that target.
    for layout in ["l n", "n n", "n0 n"] {
        for first in ["8", "88888888", ""] {
            for word in [r"\p XYZ", r"\o' '\p XYZ"] {
                let source = format!(
                    "{MAN}.TS\n{layout}.\n{first}\t{word}\n88888888\t99999999\n.TE\nENDING\n"
                );
                let shows_word = layout == "l n" || first == "88888888";
                let query = check_reading(&source, shows_word);
                let section = &query.document.as_ref().unwrap().sections[1];
                let Some(Block::Table { rows, .. }) = first_table(&section.blocks) else {
                    panic!("expected table owner: {source}");
                };
                for (column, cell) in rows[0].cells.iter().enumerate() {
                    let [Block::Paragraph { children, .. }, ..] = cell.blocks.as_slice() else {
                        assert_eq!(column, 0, "{source}");
                        continue;
                    };
                    assert_eq!(
                        mant_ir::inline_plain_text(children).contains("XYZ"),
                        column == 1 && shows_word,
                        "{source}"
                    );
                }
            }
        }
    }
}

#[test]
fn tbl_uses_the_finite_native_margin_of_its_row() {
    // term_flushln() fills a NOBREAK field against maxrmargin, measured from
    // the current cell offset. A tbl node following .nf has no NODE_NOFILL
    // in the fixed tree. Its ordinary column stays finite even while the
    // surrounding reading owner is a no-fill display.
    for owner in ["head", "block", "no-fill"] {
        for margin in [20, 78] {
            for stroke in [14, 15, 72, 73] {
                let table = format!(".TS\nl.\n\\l'{stroke}n ' Q\\p XYZ\n2\n.TE\n");
                let body = match owner {
                    "head" => format!(".Bl -hang -width 4n\n.It Xo\n{table}.Xc\nENDING\n.El\n"),
                    "no-fill" => format!(".nf\n{table}.fi\nENDING\n"),
                    _ => format!("{table}ENDING\n"),
                };
                let source = format!("{MDOC}.ll {margin}n\n{body}");
                check_reading(&source, owner == "head" && stroke < margin - 5);
            }
        }
    }
}

fn check_reading(source: &str, shows_word: bool) -> mant_ir::ResolvedContent {
    let query = lower(source);
    let markdown = mant_codec::encode::render_markdown(&query);
    let readback = mant_loader::load_markdown_text(&markdown, None).unwrap();
    for content in [&query, &readback] {
        let text = mant_render::render_query_text(content);
        assert_eq!(
            text.matches("XYZ").count(),
            usize::from(shows_word),
            "{source}\n{text}"
        );
        assert_eq!(text.matches("ENDING").count(), 1, "{source}\n{text}");
        assert_eq!(
            whole_word(content, "XYZ").matches.len(),
            usize::from(shows_word),
            "{source}\n{markdown}"
        );
    }
    query
}
