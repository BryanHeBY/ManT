//! The first written tbl graph and the actual span close are independent.

use super::{assert_prefix, between, input, lower, projections, whole_word};

#[test]
fn source_cell_motion_uses_final_written_extent_and_an_independent_cursor() {
    // Fixed term_word executes both relative and absolute motion before
    // term_fill measures the accepted cells. These are the source-derived
    // minimum second-row widths that leave a visible prefix separator
    // among the exact E3 width controls. A threshold 11/14 means no gap
    // within the tested three/four widths.
    for (tail, center, right) in [
        ("", 9, 8),
        (r"\h'1n'", 10, 9),
        (r"\h'3n'", 11, 11),
        (r"\h'|3n'", 9, 8),
        (r"\h'|5n'", 11, 10),
        (r"\l'3n'", 11, 11),
    ] {
        for (kind, threshold) in [("c", center), ("r", right)] {
            for width in 8..=10 {
                assert_prefix(
                    kind,
                    &format!("XYZ{tail}"),
                    &"W".repeat(width),
                    ".br",
                    width >= threshold,
                );
            }
        }
    }
    // Negative motion changes p->col without truncating lastcol. Q
    // overwrites the existing native run at the new cursor.
    for (tail, center, right) in [
        (r"\h'1n'Q", 11, 10),
        (r"\h'3n'Q", 13, 12),
        (r"\h'3n'\h'-2n'Q", 12, 11),
        (r"\h'4n'\h'-3n'Q", 13, 12),
    ] {
        for (kind, threshold) in [("c", center), ("r", right)] {
            for width in 10..=13 {
                assert_prefix(
                    kind,
                    &format!("XYZ{tail}"),
                    &"W".repeat(width),
                    ".br",
                    width >= threshold,
                );
            }
        }
    }
}

#[test]
fn drawn_blank_and_styled_strokes_use_term_word_native_slots() {
    // term_word's drawing repeats encode1: an ASCII blank is a break
    // candidate, while the bold/underline bytes advance p->col but
    // term_fill measures just the printed stroke on each overstrike.
    for (tail, center, right) in [
        (r"\l'3n '", 9, 8),
        (r"\fI\l'3n'\h'|10n'Q", 13, 12),
        (r"\fB\l'3n'\h'|10n'Q", 13, 12),
    ] {
        for (alignment, threshold) in [("c", center), ("r", right)] {
            for width in 8..=14 {
                let source = input(
                    "head",
                    ".No LONGPREFIX\n.br\n",
                    alignment,
                    &format!("XYZ{tail}\n{}", "W".repeat(width)),
                );
                for text in projections(&lower(&source)) {
                    let line = text
                        .lines()
                        .find(|line| line.contains("LONGPREFIX"))
                        .unwrap();
                    let remainder = line.split_once("LONGPREFIX").unwrap().1;
                    assert_eq!(
                        remainder.starts_with(' '),
                        width >= threshold,
                        "{source}\n{text}"
                    );
                    assert!(
                        remainder.trim_start_matches(' ').starts_with("XYZ"),
                        "{source}\n{text}"
                    );
                }
            }
        }
    }
}

#[test]
fn invisible_earlier_cells_only_separate_when_native_output_supplies_a_boundary() {
    // Every input has an actual earlier cell. term_field doesn't output
    // plain SP without a glyph, but a later column offset or a rule can
    // still separate the first graph from LONGPREFIX.
    for layout in ["l0 l", "l1|l", "l l"] {
        for first in ["", "  ", r"\&"] {
            let separated = layout == "l1|l" || (layout == "l l" && first == "  ");
            assert_prefix(
                layout,
                &format!("{first}\tXYZ"),
                "1\tQ",
                ".sp -1",
                separated,
            );
        }
    }
}

#[test]
fn unprinted_breakpoint_suffixes_do_not_create_completed_table_rows() {
    // term_flushln skips a tail containing only break markers,
    // ordinary blanks, breakpoints and NBRZW. A real remaining XYZ
    // can cause another pass to be attempted even if it later rejects.
    for owner in ["head", "body", "block"] {
        for (tail, expected) in [
            (r"\p\: \&", &[][..]),
            (r"\p\: \&\p", &[]),
            (r"\p\: \&\p \&", &[]),
            (r"\p\: \&\p\: ", &[]),
            (r"\p\: \&\p XYZ", &["", "XYZ"]),
            (r"\p \p XYZ", &[""]),
            (r"\p \p\& XYZ", &["", "XYZ"]),
        ] {
            let prefix = if owner == "head" {
                ".No LONGPREFIX\n.br\n"
            } else {
                ""
            };
            let source = input(owner, prefix, "l", &format!("START{tail}\nLAST"));
            let query = lower(&source);
            for text in projections(&query) {
                assert_eq!(
                    between(&text, "START", "LAST"),
                    expected,
                    "{source}\n{text}"
                );
            }
            assert_eq!(
                whole_word(&query, "XYZ").matches.len(),
                usize::from(expected.contains(&"XYZ")),
                "{source}"
            );
        }
    }
}

#[test]
fn styled_word_endlines_keep_the_same_physical_row_owners() {
    for owner in ["head", "body", "block"] {
        for (tail, expected) in [(r"\p\: \&\p \&", &[][..]), (r"\p\: \&\p XYZ", &["", "XYZ"])] {
            for font in [r"\fB", r"\fI"] {
                let prefix = if owner == "head" {
                    ".No LONGPREFIX\n.br\n"
                } else {
                    ""
                };
                let source = input(owner, prefix, "l", &format!("{font}START{tail}\nLAST"));
                for text in projections(&lower(&source)) {
                    assert_eq!(
                        between(&text, "START", "LAST"),
                        expected,
                        "{source}\n{text}"
                    );
                }
            }
        }
    }
}
