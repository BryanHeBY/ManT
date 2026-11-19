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
