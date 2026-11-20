//! Display proofs retain source occurrence, cell, and reading-row identity.

use mant_ir::{Block, Document, Inline, inline_plain_text};

use super::{
    super::{AstStructure, AstTopology, compare_structure, ir_profile},
    parsed_structure,
};

const HEAD: &str = concat!(
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd sample\n.Sh DESCRIPTION\n"
);
const TAIL: &str = ".Sh END\nDone.\n";

fn compare(native: &AstStructure, topology: &AstTopology, document: &Document) -> Vec<String> {
    let (ir, actual) = ir_profile(document);
    compare_structure(native, &ir, topology, &actual)
}

fn missing(native: &AstStructure, topology: &AstTopology, document: &Document) -> usize {
    compare(native, topology, document)
        .iter()
        .filter(|line| line.starts_with("literal-displays:"))
        .count()
}

#[test]
fn short_display_witnesses_include_delimiters_and_their_native_spacing() {
    // Fixed CVS mdoc_macro.c::blk_part_imp and mdoc_term.c::print_mdoc_node
    // keep siblings before/after BODY in the field. Even a \&-only BODY
    // can produce printed delimiters, with exact spacing from term_word.
    for kind in ["Dl", "D1"] {
        for (operand, good, wrong) in [
            ("( SENTRY )", "(SENTRY)", "SENTRY)"),
            ("[ SENTRY ]", "[SENTRY]", "SENTRY]"),
            (r"SENTRY \& .", "SENTRY .", "SENTRY."),
            (r"( \& )", "()", ""),
            (r"\& .", ".", ""),
        ] {
            for suffix in ["", ".Sm off\n", ".sp 0\n", ".Dl OTHER\\h'-2n'XY\n"] {
                let header = ".Dd November 19, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd test\n.Sh DESCRIPTION\n.Bl -column aaa bbb\n.It A Ta B\n";
                let source = format!("{header}.{kind} {operand}\n{suffix}.El\n.Sh ENDPROBE\n");
                let (native, topology, mut doc) = parsed_structure(&source);
                assert_eq!(
                    native.literal_display_reviews.len(),
                    2 * usize::from(suffix.starts_with(".Dl ")),
                    "{source}"
                );
                assert_eq!(missing(&native, &topology, &doc), 0, "{source}");
                replace_second_cell_row(&mut doc, good, wrong);
                assert_eq!(missing(&native, &topology, &doc), 1, "{source}");
            }
        }
    }
}

fn replace_second_cell_row(document: &mut Document, before: &str, after: &str) {
    let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
        panic!()
    };
    let Block::Preformatted { children, .. } = &mut rows[0].cells[1].blocks[0] else {
        panic!()
    };
    let start = children
        .iter()
        .position(|n| matches!(n, Inline::LineBreak {}))
        .unwrap()
        + 1;
    let end = start
        + children[start..]
            .iter()
            .take_while(|n| !matches!(n, Inline::LineBreak {}))
            .count();
    assert_eq!(inline_plain_text(&children[start..end]), before);
    children.splice(
        start..end,
        [Inline::Text {
            value: after.into(),
        }],
    );
}

#[test]
fn missing_or_blank_display_cannot_borrow_a_fragment_from_another_occurrence() {
    // Fixed ASCII, UTF-8, HTML and tree each retain ALPHA, BETA and OMEGA.
    // termp_d1_pre() and termp_bl_post() can split the first display; .de
    // expansion then reports all its fragments at the second display's line.
    for expanded in [false, true] {
        for middle in [
            ".br",
            ".D1 INNER",
            ".Bl -tag -width Ds\n.It KEY\nINNER\n.El",
        ] {
            let body = format!(
                ".Bd -literal -compact\nALPHA\n{middle}\nBETA\n.Ed\n.Bd -literal -compact\nOMEGA\n.Ed\n"
            );
            let source = if expanded {
                format!("{HEAD}.de EXAMPLE\n{body}..\n.EXAMPLE\n{TAIL}")
            } else {
                format!("{HEAD}{body}{TAIL}")
            };
            let (native, topology, intact) = parsed_structure(&source);
            assert!(compare(&native, &topology, &intact).is_empty(), "{source}");
            for replace_with_space in [false, true] {
                let mut document = intact.clone();
                let blocks = &mut document.sections[1].blocks;
                let index = blocks.iter().position(|block| {
                    matches!(block, Block::Preformatted { children, .. } if inline_plain_text(children) == "OMEGA")
                }).unwrap();
                if replace_with_space {
                    let Block::Preformatted { children, .. } = &mut blocks[index] else {
                        unreachable!();
                    };
                    *children = vec![Inline::Text {
                        value: "   ".to_owned(),
                    }];
                } else {
                    blocks.remove(index);
                }
                assert_eq!(missing(&native, &topology, &document), 1, "{source}");
            }
        }
    }
}

#[test]
fn repeated_column_displays_claim_disjoint_rows_even_when_neighbor_labels_match() {
    // mdoc_it_pre(): the final Ta BODY owns every display in that cell.
    // Fixed devices retain both first-row displays and the second-row
    // display, including the case where their cell prefix is also SAME.
    for prefix in ["", "SAME", "VISIBLE"] {
        let source = format!(
            "{HEAD}.Bl -column aaa bbb ccc\n.It Ta A Ta {prefix}\n.Dl SAME\n.Dl SAME\n\
             .It Ta B Ta {prefix}\n.Dl SAME\n.El\n{TAIL}"
        );
        let (native, topology, intact) = parsed_structure(&source);
        assert_eq!(native.literal_displays, 3);
        let original = compare(&native, &topology, &intact);
        assert!(original.is_empty(), "{source}: {original:?}");
        for move_to_next_row in [false, true] {
            let mut document = intact.clone();
            let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
                panic!("expected native column list");
            };
            let cell = &mut rows[0].cells[2];
            let Block::Preformatted { children, .. } = &mut cell.blocks[0] else {
                panic!("expected the cell's merged display");
            };
            let index = children
                .iter()
                .rposition(|child| matches!(child, Inline::Text { value } if value == "SAME"))
                .unwrap();
            let glyphs = std::mem::replace(
                &mut children[index],
                Inline::Text {
                    value: "  ".to_owned(),
                },
            );
            if move_to_next_row {
                let Block::Preformatted { children, .. } = &mut rows[1].cells[2].blocks[0] else {
                    panic!("expected the second cell's display");
                };
                children.extend([Inline::LineBreak {}, glyphs]);
            }
            assert_eq!(missing(&native, &topology, &document), 1, "{source}");
        }
    }
}

#[test]
fn fixed_font_and_generated_word_rows_keep_their_cell_occurrence() {
    for (word, needle) in [
        ("Em OMEGA", "OMEGA"),
        ("Sy OMEGA", "OMEGA"),
        ("Li OMEGA", "OMEGA"),
        ("Fl x", "-x"),
        ("Fn run", "run()"),
        (r"Xr \& \&", "()"),
        ("Xr printf 3", "printf(3)"),
        ("OMEGA .", "OMEGA."),
    ] {
        let source = format!("{HEAD}.Bl -column aaa bbb\n.It A Ta B\n.Dl {word}\n.El\n{TAIL}");
        let (native, topology, mut document) = parsed_structure(&source);
        let original = compare(&native, &topology, &document);
        assert!(original.is_empty(), "{source}: {original:?}");
        let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
            panic!("expected native column list");
        };
        let Block::Preformatted { children, .. } = &mut rows[0].cells[1].blocks[0] else {
            panic!("expected the cell's merged display");
        };
        assert_eq!(inline_plain_text(children), format!("B\n{needle}"));
        let row_start = children
            .iter()
            .position(|child| matches!(child, Inline::LineBreak {}))
            .unwrap();
        children.truncate(row_start + 1);
        assert_eq!(missing(&native, &topology, &document), 1, "{source}");
    }
}

#[test]
fn an_unrelated_tbl_cell_cannot_keep_a_section_display_obligation() {
    // Fixed mdoc_d1_pre() creates the section display before the table.
    // The tbl consumes LEFT and RIGHT as strings, without a D1 subtree.
    let source = format!("{HEAD}.Dl OUTSIDE\n.TS\nl l.\nLEFT\tRIGHT\n.TE\n{TAIL}");
    let (native, topology, intact) = parsed_structure(&source);
    assert!(compare(&native, &topology, &intact).is_empty());
    for strip_table_source in [false, true] {
        let mut document = intact.clone();
        let blocks = &mut document.sections[1].blocks;
        let display = blocks.remove(0);
        let Some(Block::Table { rows, source, .. }) = blocks
            .iter_mut()
            .find(|block| matches!(block, Block::Table { .. }))
        else {
            panic!("expected native tbl");
        };
        if strip_table_source {
            *source = None;
        }
        rows[0].cells[0].blocks.push(display);
        assert_eq!(missing(&native, &topology, &document), 1);
    }
}

const MATRIX_HEAD: &str =
    ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd example\n.Sh DESCRIPTION\n";

fn display(kind: &str, argument: &str) -> String {
    if matches!(kind, "D1" | "Dl") {
        return format!(".{kind} {argument}\n");
    }
    let mac = argument.split(' ').next().unwrap_or_default();
    let control = if matches!(
        mac,
        "Em" | "Sy" | "Ev" | "Dv" | "Er" | "Bq" | "Pa" | "Op" | "Lk" | "Xr" | "Fl"
    ) {
        "."
    } else {
        ""
    };
    format!(".Bd -{kind} -compact\n{control}{argument}\n.Ed\n")
}

#[test]
fn native_display_row_spellings_keep_their_cell_and_complete_field() {
    // Fixed ASCII/UTF-8 terminal rows prove the lexical output. Fixed HTML
    // identifies the owning td independently. In particular, Lk emits its
    // description and target, while the Dl post delimiters follow its BODY.
    for kind in ["Dl", "D1", "literal", "unfilled"] {
        for arg in [
            "SENTRY",
            "SENTRY AND MORE",
            "SENTRY .",
            "Em SENTRY",
            "Em SENTRY ,",
            "Em SENTRY .",
            "Pa /SENTRY",
            "Ev SENTRY",
            "Dv SENTRY",
            "Er SENTRY",
            "Em SENTRY Sy LAST",
            "Bq SENTRY",
            "Sy SENTRY Ar FILE",
            "caféSENTRY",
            r"SENTRY\-M",
            r"SENTRY\&",
            "Lk https://example.test SENTRY",
            "Xr SENTRY 3",
            "Fl SENTRY",
            "Op Fl SENTRY",
            "SENTRY , COMMA",
            "SENTRY . )",
        ] {
            for prefix in [None, Some(""), Some(" B"), Some(" Sy B")] {
                let body = display(kind, arg);
                let source = if let Some(prefix) = prefix {
                    format!("{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta{prefix}\n{body}.El\n")
                } else {
                    format!("{MATRIX_HEAD}{body}")
                };
                let (native, topology, doc) = parsed_structure(&source);
                assert_eq!(native.literal_displays, 1, "{source}");
                assert!(native.literal_display_reviews.is_empty(), "{source}");
                assert!(compare(&native, &topology, &doc).is_empty(), "{source}");
            }
        }
    }
}

#[test]
fn ordinary_cell_rows_are_reserved_across_filled_boundaries_and_trailing_requests() {
    // Pinned mdoc_term.c::print_mdoc_node() closes filled TEXT whose
    // source line starts with space. termp_sm_pre() emits no ink and
    // roff_term_pre_sp() still flushes at .sp 0.
    for kind in ["Dl", "D1", "literal", "unfilled"] {
        for (before, after, rows, at) in [
            (" LEAD\n", "", "B\nLEAD\nSENTRY", 2),
            (" SENTRY\n", "", "B\nSENTRY\nSENTRY", 2),
            ("", "SENTRY\n", "B\nSENTRY\nSENTRY", 1),
            ("", "OTHER\n", "B\nSENTRY\nOTHER", 1),
            (".br\nLEAD\n", "", "B\nLEAD\nSENTRY", 2),
        ] {
            for suffix in ["", ".Sm off\n", ".sp 0\n"] {
                let display = display(kind, "SENTRY");
                let source = format!(
                    "{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta B\n{before}{display}{after}{suffix}.El\n{TAIL}"
                );
                let (native, topology, intact) = parsed_structure(&source);
                assert!(native.literal_display_reviews.is_empty(), "{source}");
                assert_eq!(missing(&native, &topology, &intact), 0, "{source}");
                for replacement in ["WRONG", " "] {
                    let mut document = intact.clone();
                    let Block::Table { rows: table, .. } = &mut document.sections[1].blocks[0]
                    else {
                        panic!("{source}");
                    };
                    let Block::Preformatted { children, .. } = &mut table[0].cells[1].blocks[0]
                    else {
                        panic!("{source}");
                    };
                    let physical = children
                        .split(|node| matches!(node, Inline::LineBreak {}))
                        .map(|row| inline_plain_text(row).trim().to_owned())
                        .collect::<Vec<_>>();
                    assert_eq!(physical.join("\n"), rows, "{source}");
                    let start = children
                        .iter()
                        .enumerate()
                        .filter(|(_, node)| matches!(node, Inline::LineBreak {}))
                        .nth(at - 1)
                        .unwrap()
                        .0
                        + 1;
                    let end = start
                        + children[start..]
                            .iter()
                            .take_while(|node| !matches!(node, Inline::LineBreak {}))
                            .count();
                    assert_eq!(
                        inline_plain_text(&children[start..end]),
                        "SENTRY",
                        "{source}"
                    );
                    children.splice(
                        start..end,
                        [Inline::Text {
                            value: replacement.to_owned(),
                        }],
                    );
                    assert_eq!(missing(&native, &topology, &document), 1, "{source}");
                }
            }
        }
    }
}

#[test]
fn column_display_proof_uses_the_spacing_mode_at_each_ordinary_row() {
    // mdoc_term.c::termp_sm_pre changes the persistent spacing mode
    // at execution time, including while processing a previous display.
    for kind in ["Dl", "D1", "literal", "unfilled"] {
        for (mode, ordinary) in [
            (".Sm off\n", "LEFTRIGHT"),
            (".Sm\n", "LEFTRIGHT"),
            (".Sm off\n.Sm on\n", "LEFT RIGHT"),
        ] {
            for first in [
                format!("{mode}.Dl OTHER\n"),
                format!(".Dl OTHER\n{mode}"),
                format!(".Bd -literal -compact\nOTHER\n{mode}.Ed\n"),
            ] {
                let last = display(kind, ordinary);
                let source = format!(
                    "{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta B\n{first}.No LEFT No RIGHT\n{last}.El\n{TAIL}"
                );
                let (native, topology, intact) = parsed_structure(&source);
                assert!(native.literal_display_reviews.is_empty(), "{source}");
                assert!(compare(&native, &topology, &intact).is_empty(), "{source}");
                let last_line = last.lines().next().unwrap();
                let last_opening = source
                    .lines()
                    .enumerate()
                    .filter(|(_, row)| *row == last_line)
                    .last()
                    .unwrap()
                    .0
                    + 1;
                for replacement in ["WRONG", " "] {
                    let mut document = intact.clone();
                    replace_column_payload_at(&mut document, 3, ordinary, replacement);
                    let errors = compare(&native, &topology, &document);
                    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                    assert!(
                        errors[0].contains(&format!(
                            "0..=1 missing row(s) among column display candidates {last_opening}:2"
                        )),
                        "{source}: {errors:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn missing_equal_column_rows_use_the_physical_slot_or_report_all_candidates() {
    // termp_d1_pre/termp_bd_pre close each display field. The optional
    // roff_term_pre_sp row remains in the buffer between the two owners.
    for first in ["Dl", "literal"] {
        for second in ["D1", "unfilled"] {
            for second_word in ["SENTRY", "DIFFERENT"] {
                for gap in ["", ".sp 1\n"] {
                    let prefix = format!("{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta B\n");
                    let before = display(first, "SENTRY");
                    let after = display(second, second_word);
                    let starts = [
                        prefix.lines().count() + 1,
                        (prefix.clone() + &before + gap).lines().count() + 1,
                    ];
                    let source = format!("{prefix}{before}{gap}{after}.El\n{TAIL}");
                    let (native, topology, intact) = parsed_structure(&source);
                    assert!(compare(&native, &topology, &intact).is_empty(), "{source}");
                    for (at, word, line) in [
                        (1, "SENTRY", starts[0]),
                        (2 + usize::from(!gap.is_empty()), second_word, starts[1]),
                    ] {
                        for replacement in ["WRONG", " "] {
                            let mut document = intact.clone();
                            replace_column_payload_at(&mut document, at, word, replacement);
                            let errors = compare(&native, &topology, &document);
                            assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                            let expected = if gap.is_empty() && second_word == "SENTRY" {
                                format!(
                                    "1 missing row(s) among column display candidates {}:2 occurrence 1 (0..=1 missing row(s)), {}:2 occurrence 1 (0..=1 missing row(s))",
                                    starts[0], starts[1]
                                )
                            } else {
                                format!("for {line}:2 ")
                            };
                            assert!(errors[0].contains(&expected), "{source}: {errors:?}");
                        }
                    }
                    if second_word == "SENTRY" {
                        let mut document = intact;
                        replace_column_payload_at(&mut document, 1, "SENTRY", "");
                        let errors = compare(&native, &topology, &document);
                        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                        let expected = if gap.is_empty() {
                            format!(
                                "1 missing row(s) among column display candidates {}:2 occurrence 1 (0..=1 missing row(s)), {}:2 occurrence 1 (0..=1 missing row(s))",
                                starts[0], starts[1]
                            )
                        } else {
                            format!("for {}:2 ", starts[0])
                        };
                        assert!(errors[0].contains(&expected), "{source}: {errors:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn multiline_display_slots_constrain_equal_rows_across_native_spacing() {
    // CVS print_bvspace(), called by termp_bd_pre(), adds one physical
    // row at non-compact Bd entry. A separate .sp adds another row.
    for compact in ["", " -compact"] {
        for gap in ["", ".sp 1\n"] {
            let prefix = format!("{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta B\n");
            let first = format!(".Bd -unfilled{compact}\nFIRST\nSAME\n.Ed\n");
            let second = format!(".Bd -literal{compact}\nSAME\nLAST\n.Ed\n");
            let source = format!("{prefix}{first}{gap}{second}.El\n{TAIL}");
            let first_opening = prefix.lines().count() + 1;
            let second_opening = first_opening + first.lines().count() + gap.lines().count();
            let (native, topology, intact) = parsed_structure(&source);
            assert!(native.literal_display_reviews.is_empty(), "{source}");
            assert!(compare(&native, &topology, &intact).is_empty(), "{source}");
            let automatic = usize::from(compact.is_empty());
            let first_same = 2 + automatic;
            let second_same = first_same + 1 + automatic + gap.lines().count();
            for (row, opening) in [(first_same, first_opening), (second_same, second_opening)] {
                for replacement in ["WRONG", " ", ""] {
                    let mut doc = intact.clone();
                    replace_column_payload_at(&mut doc, row, "SAME", replacement);
                    let errors = compare(&native, &topology, &doc);
                    let expected = if second_same - first_same == 1 {
                        format!(
                            "1 missing row(s) among column display candidates {first_opening}:2 occurrence 1 (0..=1 missing row(s)), {second_opening}:2 occurrence 1 (0..=1 missing row(s))"
                        )
                    } else {
                        format!("for {opening}:2 ")
                    };
                    assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                    assert!(errors[0].contains(&expected), "{source}: {errors:?}");
                }
            }
        }
    }
}

#[test]
fn column_row_damage_preserves_ordinary_and_invisible_slot_alternatives() {
    // CVS mdoc_term.c::print_mdoc_node closes the no-fill row containing
    // ASCII_NBRZW. The automatic Bd gap and the ordinary PIN are also
    // separate physical slots in the executed field.
    let head = ".Dd November 19, 2026\n.Dt PROBE 1\n.Os\n.Sh NAME\n.Nm probe\n.Nd field probe\n.Sh DESCRIPTION\n.Bl -column aaa bbb\n.It A Ta B\n";
    for (body, losses, expected) in [
        (
            ".Bd -literal\nFIRST\nSAME\n.Ed\nPIN\n.Dl SAME\n",
            &[(4, "PIN", "WRONG")][..],
            &[][..],
        ),
        (
            ".Bd -literal\nFIRST\nSAME\n.Ed\nPIN\n.Dl SAME\n",
            &[(4, "PIN", " ")][..],
            &[][..],
        ),
        (
            ".Bd -literal\nFIRST\nSAME\n.Ed\nPIN\n.Dl SAME\n",
            &[(3, "SAME", ""), (2, "FIRST", "")][..],
            &["missing sourced block for 10:2 occurrence 1 (2 missing rows)"][..],
        ),
        (
            ".Dl SAME\nSAME\n.D1 SAME\n",
            &[(2, "SAME", "")][..],
            &[
                "0..=1 missing row(s) among column display candidates 10:2 occurrence 1 (0..=1 missing row(s)), 12:2 occurrence 1 (0..=1 missing row(s)); cell loss 0..=1 row(s)",
            ][..],
        ),
        (
            ".Dl SAME\nSAME\n.D1 SAME\n",
            &[(3, "SAME", ""), (2, "SAME", "")][..],
            &[
                "1..=2 missing row(s) among column display candidates 10:2 occurrence 1 (0..=1 missing row(s)), 12:2 occurrence 1 (0..=1 missing row(s)); cell loss 1..=2 row(s)",
            ][..],
        ),
        (
            ".Bd -literal -compact\n\\&\nFIRST\nSAME\n.Ed\n.Dl SAME\n",
            &[(3, "SAME", "WRONG")][..],
            &[
                "1 missing row(s) among column display candidates 10:2 occurrence 1 (0..=1 missing row(s)), 15:2 occurrence 1 (0..=1 missing row(s)); cell loss 1 row(s)",
            ][..],
        ),
        (
            ".Bd -literal\nFIRST\nSAME\n.Ed\nPIN\n.D1 SAME\n.Dl LAST\n",
            &[(6, "LAST", ""), (3, "SAME", "")][..],
            &[
                "missing sourced block for 10:2 occurrence 1",
                "missing sourced block for 16:2 occurrence 1",
            ][..],
        ),
    ] {
        let source = format!("{head}{body}.El\n");
        let (native, topology, mut document) = parsed_structure(&source);
        assert!(native.literal_display_reviews.is_empty(), "{source}");
        assert!(
            compare(&native, &topology, &document).is_empty(),
            "{source}"
        );
        for &(at, word, replacement) in losses {
            replace_column_payload_at(&mut document, at, word, replacement);
        }
        let errors = compare(&native, &topology, &document);
        assert_eq!(errors.len(), expected.len(), "{source}: {errors:?}");
        for (got, want) in errors.iter().zip(expected) {
            assert_eq!(
                got.strip_prefix("literal-displays: "),
                Some(*want),
                "{source}"
            );
        }
    }
}

fn replace_column_payload_at(
    document: &mut Document,
    at: usize,
    expected: &str,
    replacement: &str,
) {
    let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
        panic!()
    };
    let Block::Preformatted { children, .. } = &mut rows[0].cells[1].blocks[0] else {
        panic!()
    };
    let start = at.checked_sub(1).map_or(0, |break_index| {
        children
            .iter()
            .enumerate()
            .filter(|(_, node)| matches!(node, Inline::LineBreak {}))
            .nth(break_index)
            .unwrap()
            .0
            + 1
    });
    let end = start
        + children[start..]
            .iter()
            .take_while(|node| !matches!(node, Inline::LineBreak {}))
            .count();
    assert_eq!(inline_plain_text(&children[start..end]), expected);
    if replacement.is_empty() {
        if start == 0 {
            children.drain(..(end + 1).min(children.len()));
        } else {
            children.drain(start - 1..end);
        }
    } else {
        children.splice(
            start..end,
            [Inline::Text {
                value: replacement.into(),
            }],
        );
    }
}

#[test]
fn column_edit_budgets_never_make_equal_owners_more_certain() {
    // The exact sources ran pinned ASCII, UTF-8, HTML, tree and lint.
    // termp_d1_pre leaves each short display independently executed.
    let prefix = format!("{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta B\n");
    for count in [9, 10] {
        let source = format!("{prefix}{}.El\n{TAIL}", ".Dl SAME\n".repeat(count));
        let (native, topology, mut document) = parsed_structure(&source);
        assert!(compare(&native, &topology, &document).is_empty());
        for slot in (2..=count).rev() {
            replace_column_payload_at(&mut document, slot, "SAME", "");
        }
        let errors = compare(&native, &topology, &document);
        assert_eq!(errors.len(), 1, "{errors:?}");
        let start = prefix.lines().count() + 1;
        assert!((start..start + count).all(|line| errors[0].contains(&format!("{line}:2"))));
        if count == 9 {
            assert!(errors[0].starts_with("literal-displays: 8 missing row(s)"));
        } else {
            assert!(errors[0].starts_with("literal-displays: unresolved column slot alignment"));
            assert!(errors[0].ends_with("; cell loss 9..=10 row(s)"));
        }
    }
    for slots in [7_280, 7_281] {
        let padding = "PAD\n".repeat(slots - 3);
        let source =
            format!("{prefix}.Bd -literal -compact\n{padding}.Ed\n.Dl SAME\n.D1 SAME\n.El\n{TAIL}");
        let (native, topology, mut document) = parsed_structure(&source);
        replace_column_payload_at(&mut document, slots - 2, "SAME", "");
        let first = prefix.lines().count() + slots;
        assert_eq!(
            compare(&native, &topology, &document),
            [format!(
                "literal-displays: 1 missing row(s) among column display candidates {first}:2 occurrence 1 (0..=1 missing row(s)), {}:2 occurrence 1 (0..=1 missing row(s)); cell loss 1 row(s)",
                first + 1
            )]
        );
    }
}

#[test]
fn shifted_cells_keep_the_same_minimum_loss_as_replaced_rows() {
    // CVS termp_bd_pre/termp_d1_pre establish the original literal
    // rows. Mutated IR keeps LAST; an inserted WRONG cannot displace it
    // as evidence of the surviving source row.
    let prefix = format!("{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta B\n");
    let source = format!("{prefix}.Bd -literal -compact\nSAME\nLAST\n.Ed\n.El\n{TAIL}");
    let (native, topology, mut document) = parsed_structure(&source);
    replace_column_payload_at(&mut document, 1, "SAME", "");
    let Block::Table { rows, .. } = &mut document.sections[1].blocks[0] else {
        panic!()
    };
    let Block::Preformatted { children, .. } = &mut rows[0].cells[1].blocks[0] else {
        panic!()
    };
    children.extend([
        Inline::line_break(),
        Inline::Text {
            value: "WRONG".into(),
        },
    ]);
    assert_eq!(
        compare(&native, &topology, &document),
        ["literal-displays: missing sourced block for 10:2 occurrence 1"]
    );

    let source = format!("{prefix}.Dl SAME\n.D1 SAME\n.Dl LAST\n.El\n{TAIL}");
    let (native, topology, mut document) = parsed_structure(&source);
    replace_column_payload_at(&mut document, 3, "LAST", "WRONG");
    replace_column_payload_at(&mut document, 1, "SAME", "");
    assert_eq!(
        compare(&native, &topology, &document),
        [
            "literal-displays: 1 missing row(s) among column display candidates 10:2 occurrence 1 (0..=1 missing row(s)), 11:2 occurrence 1 (0..=1 missing row(s)); cell loss 2 row(s)",
            "literal-displays: missing sourced block for 12:2 occurrence 1",
        ]
    );
}

#[test]
fn an_unknown_column_tail_does_not_turn_an_ordinary_edit_into_display_loss() {
    // Both literal rows reach term_flushln in the original CVS cell.
    // roff_term_pre_ta changes tab stops without emitting a third row.
    let source =
        format!("{HEAD}.Bl -column aaa bbb\n.It A Ta B\n.Dl FIRST\n.Dl LAST\n.ta 3n\n.El\n{TAIL}");
    let (native, topology, mut document) = parsed_structure(&source);
    assert_eq!(native.literal_display_reviews.len(), 2);
    assert!(compare(&native, &topology, &document).is_empty());
    replace_column_payload_at(&mut document, 0, "B", "WRONG");
    assert_eq!(
        compare(&native, &topology, &document),
        [
            "literal-displays: unresolved column slot alignment for candidates 10:2 occurrence 1, 11:2 occurrence 1"
        ]
    );
}

#[test]
fn incomplete_cells_bound_literal_losses_without_reusing_identical_rows() {
    // CVS termp_bd_pre/termp_it_post print both original rows in the
    // cell. roff_term_pre_ta changes the tab stops after those rows.
    let prefix = format!("{HEAD}.Bl -column aaa bbb\n.It A Ta B\n");
    for (body, removed, expected) in [
        (
            ".Bd -literal -compact\nFIRST\nLAST\n.Ed\n",
            "LAST",
            "10:2 occurrence 1 (1..=2 missing row(s)); cell loss 1..=2 row(s)",
        ),
        (
            ".Dl SAME\n.Dl SAME\n",
            "SAME",
            "10:2 occurrence 1 (0..=1 missing row(s)), 11:2 occurrence 1 (0..=1 missing row(s)); cell loss 1..=2 row(s)",
        ),
    ] {
        let source = format!("{prefix}{body}.ta 3n\n.El\n{TAIL}");
        let (native, topology, mut document) = parsed_structure(&source);
        assert!(compare(&native, &topology, &document).is_empty());
        replace_column_payload_at(
            &mut document,
            if removed == "LAST" { 2 } else { 1 },
            removed,
            "",
        );
        assert_eq!(
            compare(&native, &topology, &document),
            [format!(
                "literal-displays: unresolved column slot alignment for candidates {expected}"
            )]
        );
    }
}

#[test]
fn expanded_display_reviews_identify_the_source_occurrence_and_column() {
    // mdoc_term.c prints the Dl owners in different It BODY fields even
    // if the roff macro expansion gives them the same source coordinates.
    let source = format!(
        "{HEAD}.Bl -column aaa bbb\n.de XX\n.It A Ta B\n.Dl FIRST\n.It A Ta B\n.Dl LAST\n.ta 3n\n..\n.XX\n.El\n{TAIL}"
    );
    let (native, topology, document) = parsed_structure(&source);
    assert!(compare(&native, &topology, &document).is_empty());
    let [review] = native.literal_display_reviews.as_slice() else {
        panic!("{source}");
    };
    let cell = review.cell.expect("column cell");
    assert_eq!(review.column, 2);
    assert_eq!(review.occurrence, 2);
    assert_eq!(
        (cell.table_line, cell.table_column, cell.table_occurrence),
        (8, 2, 1)
    );
    assert_eq!((cell.row, cell.column), (1, 1));
}

#[test]
fn expanded_tables_have_page_wide_occurrences_across_parent_scopes() {
    // In pinned CVS the macro expands every Bl and Dl at .XX:2.
    // termp_it_pre() still visits each table and BODY independently.
    for group in ["same", "item", "section"] {
        for count in [2, 3] {
            for marked in [1, 2, 7] {
                let mut body = if group == "item" {
                    ".Bl -item\n".to_owned()
                } else {
                    String::new()
                };
                for i in 0..count {
                    if group == "item" {
                        body.push_str(".It\n");
                    } else if group == "section" {
                        body.push_str(&format!(".Ss TOP{i}\n"));
                    }
                    body.push_str(&format!(
                        ".Bl -column aa bb\n.It MARK{i} Ta B\n.Dl WORD{i}\n"
                    ));
                    if marked & (1 << i) != 0 {
                        body.push_str(".ta 3n\n");
                    }
                    body.push_str(".El\n");
                }
                if group == "item" {
                    body.push_str(".El\n");
                }
                let source = format!("{HEAD}.de XX\n{body}..\n.XX\n{TAIL}");
                let call = source.lines().position(|line| line == ".XX").unwrap() as u32 + 1;
                let (native, topology, document) = parsed_structure(&source);
                assert_eq!(missing(&native, &topology, &document), 0, "{source}");
                let actual = native.literal_display_reviews.iter().map(|review| {
                    let cell = review.cell.unwrap();
                    (
                        (review.line, review.column, review.occurrence),
                        (cell.table_line, cell.table_column, cell.table_occurrence),
                        (cell.row, cell.column),
                    )
                });
                let expected = (0..count)
                    .filter(|&i| marked & (1 << i) != 0)
                    .map(|i| ((call, 2, i + 1), (call, 2, i + 1), (0, 1)));
                assert_eq!(
                    actual.collect::<Vec<_>>(),
                    expected.collect::<Vec<_>>(),
                    "{source}"
                );
            }
        }
    }
}

#[test]
fn filtered_displays_keep_their_source_occurrence() {
    // CVS tree retains every at-call-site BLOCK. mdoc_html.c emits its
    // display BODY even when it contains only a zero-width character.
    let head = ".Dd November 19, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd example\n.Sh DESCRIPTION\n";
    let column = ".Bl -column aa bb\n.It A Ta B\n";
    for prefix in [
        ".Dl \\&\n",
        ".D1 \\&\n",
        ".Bd -literal -compact\n\\&\n.Ed\n",
        ".Bd -unfilled -compact\n\\&\n.Ed\n",
        ".Dl FIRST\n",
    ] {
        for count in [1, 2] {
            let first = prefix.repeat(count);
            for tail in [
                ".Dl LAST\n",
                ".D1 LAST\n",
                ".Bd -literal -compact\nLAST\n.Ed\n",
            ] {
                for same_cell in [false, true] {
                    let body = if same_cell {
                        format!("{column}{first}{tail}")
                    } else {
                        format!("{first}{column}{tail}")
                    };
                    let source =
                        format!("{head}.de XX\n{body}.ta 3n\n.El\n..\n.XX\n.Sh ENDPROBE\nDone.\n");
                    let call = source.lines().position(|line| line == ".XX").unwrap() as u32 + 1;
                    let (native, topology, document) = parsed_structure(&source);
                    assert_eq!(
                        native.literal_displays,
                        1 + count * usize::from(prefix.starts_with(".Dl FIRST"))
                    );
                    assert_eq!(missing(&native, &topology, &document), 0, "{source}");
                    let last = native.literal_display_reviews.last().unwrap();
                    let cell = last.cell.unwrap();
                    assert_eq!(
                        (last.line, last.column, last.occurrence),
                        (call, 2, count + 1)
                    );
                    assert_eq!(
                        (cell.table_line, cell.table_column, cell.table_occurrence),
                        (call, 2, 1)
                    );
                    assert_eq!((cell.row, cell.column), (0, 1));
                }
            }
        }
    }
}

#[test]
fn complete_and_partial_cells_keep_loss_bounds_at_both_edit_limits() {
    // CVS termp_it_pre and termp_bd_pre put B and all the literal rows
    // in the same cell. The terminal, tree and HTML keep the same rows
    // when a trailing .ta prevents the auditor from proving the full plan.
    let prefix = format!("{HEAD}.Bl -column aaa bbb\n.It A Ta B\n.Bd -literal -compact\n");
    for count in [8, 9, 10, 3_853, 3_854] {
        for distinct in [false, true] {
            let words: Vec<_> = (0..count)
                .map(|i| {
                    if distinct {
                        format!("WORD{i}")
                    } else {
                        "SAME".into()
                    }
                })
                .collect();
            let text = words.join("\n");
            for extra in ["", ".ta 3n\n"] {
                let source = format!("{prefix}{text}\n.Ed\n{extra}.El\n{TAIL}");
                let (native, topology, document) = parsed_structure(&source);
                assert!(compare(&native, &topology, &document).is_empty());
                let deletions = if count < 20 {
                    [count - 1, count]
                } else {
                    [1, 2]
                };
                for removed in deletions {
                    let mut mutated = document.clone();
                    for i in (count - removed..count).rev() {
                        replace_column_payload_at(&mut mutated, i + 1, &words[i], "");
                    }
                    let errors: Vec<_> = compare(&native, &topology, &mutated)
                        .into_iter()
                        .filter(|error| error.starts_with("literal-displays:"))
                        .collect();
                    assert_eq!(errors.len(), 1, "{count}/{extra}: {errors:?}");
                    let detail = &errors[0];
                    let exact = format!("({removed} missing rows)");
                    let lower_bound = format!("; cell loss {removed}..=");
                    assert!(
                        detail.contains(&exact)
                            || detail.contains(&lower_bound)
                            || removed == 1
                                && detail.starts_with("literal-displays: missing sourced block"),
                        "{count}/{extra}/{removed}: {errors:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn ambiguous_loss_reports_the_total_and_each_source_occurrence_range() {
    // termp_bd_pre and termp_d1_pre create three independent display
    // owners. Dropping two SAME rows leaves the multirow owner with a
    // proved lower bound of one, while the short display may still live.
    let body = ".Bd -literal -compact\nSAME\nSAME\n.Ed\n.Dl SAME\n.Dl LAST\n";
    for (display, first, second) in [
        (body.to_owned(), "10:2 occurrence 1", "14:2 occurrence 1"),
        (
            format!(".de XX\n{body}..\n.XX\n"),
            "18:2 occurrence 1",
            "18:2 occurrence 2",
        ),
    ] {
        let source = format!("{HEAD}.Bl -column aaa bbb\n.It A Ta B\n{display}.El\n{TAIL}");
        let (native, topology, mut document) = parsed_structure(&source);
        assert!(
            compare(&native, &topology, &document).is_empty(),
            "{source}"
        );
        replace_column_payload_at(&mut document, 2, "SAME", "");
        replace_column_payload_at(&mut document, 1, "SAME", "");
        assert_eq!(
            compare(&native, &topology, &document),
            [format!(
                "literal-displays: 2 missing row(s) among column display candidates {first} (1..=2 missing row(s)), {second} (0..=1 missing row(s)); cell loss 2 row(s)"
            )],
            "{source}"
        );
    }
}

#[test]
fn an_unproved_cell_tail_cannot_certify_a_display_with_the_same_ink() {
    // roff_term_pre_ta() affects tab evaluation without emitting a word.
    // The literal auditor has not proved the following raw-cell range.
    for kind in ["Dl", "D1", "literal", "unfilled"] {
        let display = display(kind, "SENTRY");
        let source = format!(
            "{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta B\n{display}SENTRY\n.ta 3n\n.El\n{TAIL}"
        );
        let (native, topology, intact) = parsed_structure(&source);
        assert_eq!(native.literal_display_reviews.len(), 1, "{source}");
        assert_eq!(missing(&native, &topology, &intact), 0, "{source}");
        let mut replaced = intact.clone();
        replace_second_cell_row(&mut replaced, "SENTRY", "WRONG");
        assert_eq!(missing(&native, &topology, &replaced), 1, "{source}");
        let mut deleted = intact;
        replace_second_cell_row(&mut deleted, "SENTRY", "");
        // The merged observed block cannot tell which identical row is
        // gone. The source Review must remain even with a matching prefix.
        assert_eq!(missing(&native, &topology, &deleted), 0, "{source}");
        assert_eq!(native.literal_display_reviews.len(), 1, "{source}");
    }
}

#[test]
fn an_ordinary_cell_row_cannot_replace_missing_display_ink() {
    // The pristine termp_it_pre and termp_d1_pre/termp_bd_pre put ordinary
    // and display rows in the same cell. Preserve the non-display ink and
    // mutate only the target row so equal neighboring glyphs are adversarial.
    for kind in ["Dl", "D1", "literal", "unfilled"] {
        for (before, after, wanted, count) in [
            ("B", "", 1, 1),
            ("SAME", "", 1, 2),
            ("Sy SAME", "", 1, 2),
            ("Pa SAME", "", 1, 2),
            ("B\n.br\nSAME", "", 2, 2),
            ("B", "SAME\n", 1, 2),
            ("B\n.br\n.Em SAME", "", 2, 2),
            ("B", ".Em SAME\n", 1, 2),
        ] {
            let display = display(kind, "SAME");
            let source = format!(
                "{MATRIX_HEAD}.Bl -column aaa bbb\n.It A Ta {before}\n{display}{after}.El\n"
            );
            let (native, topology, intact) = parsed_structure(&source);
            assert!(native.literal_display_reviews.is_empty(), "{source}");
            assert_eq!(missing(&native, &topology, &intact), 0, "{source}");
            for replacement in [" ", "WRONG"] {
                let mut doc = intact.clone();
                let Block::Table { rows, .. } = &mut doc.sections[1].blocks[0] else {
                    panic!("missing cell")
                };
                let Block::Preformatted { children, .. } = &mut rows[0].cells[1].blocks[0] else {
                    panic!("missing display")
                };
                assert_eq!(inline_plain_text(children).matches("SAME").count(), count);
                let mut row = 0;
                let mut changes = 0;
                for node in children.iter_mut() {
                    if matches!(node, Inline::LineBreak {}) {
                        row += 1;
                    } else if row == wanted
                        && inline_plain_text(std::slice::from_ref(node)) == "SAME"
                    {
                        *node = Inline::Text {
                            value: replacement.to_owned(),
                        };
                        changes += 1;
                    }
                }
                assert_eq!(changes, 1, "{source}");
                assert_eq!(missing(&native, &topology, &doc), 1, "{source}");
            }
        }
    }
}
