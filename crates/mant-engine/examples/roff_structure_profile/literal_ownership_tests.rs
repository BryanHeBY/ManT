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
                        errors[0].contains(&format!("for {last_opening}:2 ")),
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
                            assert!(
                                errors[0].contains(&format!("for {line}:2 ")),
                                "{source}: {errors:?}"
                            );
                        }
                    }
                    if second_word == "SENTRY" {
                        let mut document = intact;
                        replace_column_payload_at(&mut document, 1, "SENTRY", "");
                        let errors = compare(&native, &topology, &document);
                        assert_eq!(errors.len(), 1, "{source}: {errors:?}");
                        let expected = if gap.is_empty() {
                            format!(
                                "1 missing row(s) among column display candidates {}:2, {}:2",
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
                    let expected = if replacement.is_empty() && second_same - first_same == 1 {
                        format!(
                            "1 missing row(s) among column display candidates {first_opening}:2, {second_opening}:2"
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
    assert_eq!(inline_plain_text(&children[start..end]), expected);
    if replacement.is_empty() {
        children.drain(start - 1..end);
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
