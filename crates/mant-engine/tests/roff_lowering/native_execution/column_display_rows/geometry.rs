//! Native display origin and gap receipts reach both public reading paths.

use super::{
    Block, DocumentIndex, FINISH, HEADER, lower, markdown_rows, reading_body, table, word_rows,
};

const DISPLAYS: [&str; 3] = ["filled", "literal", "unfilled"];
const TAGS: [&str; 2] = ["", ".Tg Mixed.Target\n"];

fn positions(rows: &str, words: &[&str]) -> Vec<(usize, usize)> {
    words
        .iter()
        .map(|word| {
            rows.lines()
                .enumerate()
                .find_map(|(row, text)| text.find(word).map(|column| (row, column)))
                .unwrap_or_else(|| panic!("missing {word}: {rows:?}"))
        })
        .collect()
}

#[test]
fn display_rows_use_the_current_cell_origin_and_translate_with_the_parent() {
    // These 24 inputs ran pristine CVS in all five profiles, lint=0.
    // termp_it_pre sets this BODY's offset before Bd enters. term_field()
    // prints its position once; list offset adds the same 3 cells to each.
    for kind in DISPLAYS {
        for (width, last) in [("b", 10), ("\"bbbbbbbb\"", 17)] {
            for (parent, shift) in [("", 0), (" -offset 3n", 3)] {
                for tag in TAGS {
                    let source = format!(
                        "{HEADER}.Bl -column a {width} c{parent}\n.It F Ta \\&\n{tag}.Bd -{kind}\nMID\n.Ed\n.No \\& Ta Z\n{FINISH}"
                    );
                    let query = lower(&source);
                    let rendered = mant_render::render_query_text(&query);
                    assert_eq!(
                        positions(reading_body(&rendered), &["F", "MID", "Z"]),
                        [(0, shift), (1, shift + 5), (1, shift + last)],
                        "{source}"
                    );
                    let row = &table(&query)[0].cells;
                    assert_eq!(
                        row.iter().map(|cell| cell.break_after).collect::<Vec<_>>(),
                        [true, false, false],
                        "{source}"
                    );
                    let [Block::Preformatted { inline_layout, .. }] = row[1].blocks.as_slice()
                    else {
                        panic!("expected middle display owner: {source}");
                    };
                    assert!(inline_layout.row_hints.is_empty(), "{source}");
                    assert_eq!(markdown_rows(&query), ["F", "MID Z"], "{source}");
                    let index = DocumentIndex::build(query.document.as_ref().unwrap());
                    assert_eq!(
                        index.fragment_target("Mixed.Target").is_some(),
                        !tag.is_empty()
                    );
                }
            }
        }
    }
}

#[test]
fn display_row_hints_retain_only_the_local_signed_offset() {
    // Four exact sources, all CVS profiles. Native Bd pre allows a signed
    // offset relative to the declared column (mdoc_term.c:1449-1455).
    // Negative displacement may use the documented viewport stack fallback.
    for offset in [-2, 2] {
        for (parent, shift) in [("", 0), (" -offset 3n", 3)] {
            let source = format!(
                "{HEADER}.Bl -column a b c{parent}\n.It F Ta \\&\n.Bd -filled -offset {offset}n\nMID\n.Ed\n.No \\& Ta Z\n{FINISH}"
            );
            let query = lower(&source);
            let row = &table(&query)[0].cells;
            let [Block::Preformatted { inline_layout, .. }] = row[1].blocks.as_slice() else {
                panic!("expected one display owner: {source}");
            };
            assert_eq!(
                inline_layout.row_hints[0].indent_columns, offset,
                "{source}"
            );
            if offset > 0 {
                let rendered = mant_render::render_query_text(&query);
                assert_eq!(
                    positions(reading_body(&rendered), &["F", "MID", "Z"]),
                    [(0, shift), (1, shift + 7), (2, shift + 10)],
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn last_cell_display_spacing_is_independent_of_the_previous_fields_close() {
    // 36 exact sources ran pristine CVS before adding these assertions.
    // print_bvspace() calls term_newln() even for compact blocks, then
    // term_vspace() only when appropriate (mdoc_term.c:585-627). A filled
    // empty source line has the existing CVS lint warning; the rows survive.
    for kind in DISPLAYS {
        for compact in [false, true] {
            let qualifier = if compact { " -compact" } else { "" };
            for operand in ["MID", "\\&", ""] {
                for tag in TAGS {
                    let source = format!(
                        "{HEADER}.Bl -column a b c\n.It F Ta M Ta \\&\n{tag}.Bd -{kind}{qualifier}\n{operand}\n.Ed\n.It K Ta L Ta M\n{FINISH}"
                    );
                    let query = lower(&source);
                    let mut expected = vec!["F M"];
                    expected.extend(std::iter::repeat_n("", usize::from(!compact)));
                    if operand == "MID" {
                        expected.extend(["MID", ""]);
                    } else {
                        expected.extend(["", ""]);
                    }
                    expected.push("K L M");
                    let rendered = mant_render::render_query_text(&query);
                    assert_eq!(word_rows(reading_body(&rendered)), expected, "{source}");
                    assert_eq!(markdown_rows(&query), expected, "{source}");
                    let first = &table(&query)[0].cells;
                    assert!(first[1].break_after, "{source}");
                    assert!(!first[2].break_after, "{source}");
                    if operand == "MID" {
                        let cell = &first[2].blocks;
                        let expected_gap = u16::from(!compact);
                        let before = cell.first().map_or(0, mant_ir::geometry::block_gap);
                        assert_eq!(before, expected_gap, "{source}");
                        assert!(
                            matches!(cell.last(), Some(Block::VerticalSpace { lines: 1, .. })),
                            "{source}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn visible_first_cell_and_prefilled_last_cell_keep_their_own_rows() {
    // Nine exact CVS inputs. A compact first cell has no spacing request;
    // a prefilled last cell closes its own PRE row before display spacing.
    for kind in DISPLAYS {
        let source = format!(
            "{HEADER}.Bl -column a b\n.It \\&\n.Bd -{kind} -compact\nMID\n.Ed\n.No \\& Ta Z\n{FINISH}"
        );
        let query = lower(&source);
        let rendered = mant_render::render_query_text(&query);
        assert_eq!(
            positions(reading_body(&rendered), &["MID", "Z"]),
            [(0, 0), (0, 5)],
            "{source}"
        );
        assert_eq!(markdown_rows(&query), ["MID Z"], "{source}");
        for compact in [false, true] {
            let qualifier = if compact { " -compact" } else { "" };
            let source = format!(
                "{HEADER}.Bl -column a b c\n.It F Ta M Ta PRE\n.Bd -{kind}{qualifier}\nMID\n.Ed\n.It K Ta L Ta M\n{FINISH}"
            );
            let query = lower(&source);
            let expected = if compact {
                &["F M PRE", "MID", "", "K L M"][..]
            } else {
                &["F M PRE", "", "MID", "", "K L M"][..]
            };
            let rendered = mant_render::render_query_text(&query);
            assert_eq!(word_rows(reading_body(&rendered)), expected, "{source}");
            assert_eq!(markdown_rows(&query), expected, "{source}");
            assert!(!table(&query)[0].cells[1].break_after, "{source}");
        }
    }
}

#[test]
fn explicit_column_gaps_remain_separate_from_the_preceding_field_close() {
    // Pristine CVS term_vspace and .sp/paragraph paths executed these
    // exact inputs. A run can contain more than one retained layout row.
    for (control, before) in [(".sp 1", 2), (".sp 2", 3), (".Pp\n.Pp", 2)] {
        let source = format!(
            "{HEADER}.Bl -column a b c\n.It F Ta M Ta \\&\n.Bd -literal\n{control}\nMID\n.Ed\n.It K Ta L Ta M\n{FINISH}"
        );
        let query = lower(&source);
        let mut expected = vec!["F M"];
        expected.extend(std::iter::repeat_n("", before));
        expected.extend(["MID", "", "K L M"]);
        let rendered = mant_render::render_query_text(&query);
        assert_eq!(word_rows(reading_body(&rendered)), expected, "{source}");
        assert_eq!(markdown_rows(&query), expected, "{source}");
    }
}

#[test]
fn first_cell_display_keeps_spacing_without_a_preceding_field() {
    // Each filled/literal/unfilled/ragged variant ran pristine CVS.
    // print_bvspace() and roff_term_pre_sp() can assert a real gap at the
    // first field, even if term_newln() has no printed field to close.
    for kind in DISPLAYS.into_iter().chain(["ragged"]) {
        for (compact, before, inside, gaps) in [
            ("", "", "", 1),
            ("", "", ".br\n", 1),
            ("", "", ".sp -1\n", 1),
            (" -compact", "", "", 0),
            (" -compact", ".sp 1\n", "", 1),
            (" -compact", "", ".Pp\n", 1),
            (" -compact", ".Tg X.X\n.sp 1\n", "", 1),
            (" -compact", ".sp 2\n", "", 2),
        ] {
            let source = format!(
                "{HEADER}.Bl -column a b c\n.It \\&\n{before}.Bd -{kind}{compact}\n{inside}MID\n.Ed\n.No \\& Ta MIDEND Ta LAST\n.It K Ta L Ta N\n{FINISH}"
            );
            let query = lower(&source);
            let cell = &table(&query)[0].cells[0];
            assert_eq!(
                cell.blocks.first().map_or(0, mant_ir::geometry::block_gap),
                gaps,
                "{source}"
            );
            assert!(!cell.break_after, "{source}");
            let mut expected = vec![""; usize::from(gaps)];
            expected.extend(["MID MIDEND", "LAST", "K L N"]);
            let rendered = mant_render::render_query_text(&query);
            assert_eq!(word_rows(reading_body(&rendered)), expected, "{source}");
            // Portable Markdown does not copy the terminal's constraint
            // on the final field's visual row. The executed gap remains.
            let mut expected_markdown = vec![""; usize::from(gaps)];
            expected_markdown.extend(["MID MIDEND LAST", "K L N"]);
            assert_eq!(markdown_rows(&query), expected_markdown, "{source}");
            let index = DocumentIndex::build(query.document.as_ref().unwrap());
            assert_eq!(
                index.fragment_target("X.X").is_some(),
                before.contains(".Tg"),
                "{source}"
            );
        }
    }
}
