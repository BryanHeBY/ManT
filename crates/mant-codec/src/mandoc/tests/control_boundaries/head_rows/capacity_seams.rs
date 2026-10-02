//! Final field receipts determine a definition's semantic word seam.

use super::*;
use mant_ir::DefinitionBodyAlignment;

const HEADER: &str =
    ".Dd October 2, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n";

fn item_from_capacity_source(body: &str) -> mant_ir::DefinitionItem {
    let source = format!("{HEADER}{body}");
    let document = parse_manual_bytes(
        std::path::Path::new("definition-capacity-seam.1"),
        source.as_bytes(),
    )
    .expect("lower exact field-capacity source");
    let Block::DefinitionList { items, .. } = &document.sections[1].blocks[0] else {
        panic!("expected definition list: {document:#?}");
    };
    items[0].clone()
}

fn description_text(item: &mant_ir::DefinitionItem) -> String {
    item.description
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                Some(inline_text(children))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn measured_zero_capacity_is_not_an_unconfigured_field() {
    // These 18 exact sources passed five pristine profiles before assertions.
    // mdoc_term.c:735-747 computes width=-2n + 2n = 0; term_flushln:233-253
    // still keeps an occupied HANG row open with minbl=0. Neither capacity
    // nor origin zero means absence. Empty HEAD is the unoccupied negative.
    for width in [-2, -1, 0] {
        for length in [0, 1, 2] {
            for control in [".sp", ".br"] {
                let word = "X".repeat(length);
                let operand = if length == 0 { "\"\"" } else { &word };
                let body = format!(
                    ".Bl -hang -width {width}n\n.It Xo\n{control}\n.No {operand}\n.Xc\n.No BODY\n.El\n.Sh NEXT\n.No END\n"
                );
                let item = item_from_capacity_source(&body);
                let joined = length > 0 && i32::try_from(length).unwrap() >= width + 2;
                assert_eq!(
                    item.layout.head_body_relation.joins_without_separator(),
                    joined,
                    "{body}: {item:#?}"
                );
                assert_eq!(description_text(&item), "BODY");
            }
        }
    }
}

#[test]
fn cleared_head_capacity_uses_final_position_and_minbl() {
    // All 252 exact sources ran pristine ASCII/UTF-8/HTML/tree/lint first.
    // 216 are lint=0; the 36 redundant .fi sources are diagnosed (lint=1).
    // mdoc_term.c::termp_it_pre() adds two cells to the list width and
    // selects NOSPACE at BODY. pre_br clears trailspace; term_flushln()
    // retains the HANG row and minbl=trailspace (term.c:233-253). Thus
    // term_field() adds max(offset-viscol,minbl) cells (113-116,389-427):
    // capacity-1 separates; equality and capacity+1 join, even in one pass.
    for width in [0, 2, 4] {
        let capacity = width + 2;
        for length in [capacity - 1, capacity, capacity + 1] {
            let word = "X".repeat(length);
            for control in [
                "", ".br\n", ".sp\n", ".sp 0\n", ".sp -1\n", ".fi\n", ".nf\n",
            ] {
                for macro_name in ["No", "Em", "Sy", "Li"] {
                    let body = format!(
                        ".Bl -hang -width {width}n\n.It Xo\n{control}.{macro_name} {word}\n.Xc\n.No BODY\n.El\n"
                    );
                    let item = item_from_capacity_source(&body);
                    let expected = if control == ".nf\n" {
                        HeadBodyRelation::Separate
                    } else if !control.is_empty() && length >= capacity {
                        HeadBodyRelation::joined(DefinitionBodyAlignment::Indented)
                    } else {
                        HeadBodyRelation::separated(DefinitionBodyAlignment::Indented)
                    };
                    assert_eq!(
                        item.layout.head_body_relation, expected,
                        "{body}: {item:#?}"
                    );
                    assert!(
                        inline_text(&item.terms[0]).ends_with(&word),
                        "{body}: {item:#?}"
                    );
                    assert_eq!(description_text(&item), "BODY", "{body}: {item:#?}");
                }
            }
        }
    }
}

#[test]
fn native_special_cells_share_the_same_capacity_receipt() {
    // These 21 exact sources passed all five pristine profiles (lint=0).
    // A scanned ASCII_HYPH becomes '-' (term_fill():307-324); its presence
    // alone cannot prove a final row break. encode1() buffers \zX before
    // marking BACKBEFORE; term_field() skips NBRZW but prints fixed space
    // (term.c:397-434,920). Every glyph remains under its native owner.
    for (source_word, projected_word, reaches_body) in [
        ("X-X", "X-X", false),
        ("X-XX", "X-XX", true),
        ("X-XXX", "X-XXX", true),
        ("XXX\\zX", "XXXX", true),
        ("XXXX\\zX", "XXXXX", true),
        ("XX\\0X", "XX\u{a0}X", true),
        ("XXXX\\&", "XXXX", true),
    ] {
        for control in ["", ".br\n", ".sp\n"] {
            let body = format!(
                ".Bl -hang -width 2n\n.It Xo\n{control}.No {source_word}\n.Xc\n.No BODY\n.El\n"
            );
            let item = item_from_capacity_source(&body);
            let expected = if !control.is_empty() && reaches_body {
                HeadBodyRelation::joined(DefinitionBodyAlignment::Indented)
            } else {
                HeadBodyRelation::separated(DefinitionBodyAlignment::Indented)
            };
            assert_eq!(
                item.layout.head_body_relation, expected,
                "{body}: {item:#?}"
            );
            assert!(
                inline_text(&item.terms[0]).ends_with(projected_word),
                "{body}: {item:#?}"
            );
            assert_eq!(description_text(&item), "BODY", "{body}: {item:#?}");
        }
    }
}

#[test]
fn fractional_body_origin_uses_actual_device_cell_rounding() {
    // All 24 exact sources passed five pristine profiles (lint=0).
    // ascii_hspan() truncates EN distances to basic units; ascii_advance()
    // prints a cell only when viscol+halfEN < destination (term_ascii.c:
    // 307-336,299-304). A 3.5-cell BODY origin following XXX adds no cell;
    // 3.6 cells does. The receipt keeps this real cell fact separate from
    // the preferred, rounded layout origin carried by DefinitionLayout.
    for (width, first_joined_length) in [
        ("1.49n", 3),
        ("1.5n", 3),
        ("1.51n", 3),
        ("1.6n", 4),
        ("2.49n", 4),
        ("2.5n", 4),
        ("2.51n", 4),
        ("2.6n", 5),
    ] {
        for length in [3, 4, 5] {
            let word = "X".repeat(length);
            let body =
                format!(".Bl -hang -width {width}\n.It Xo\n.sp\n.No {word}\n.Xc\n.No BODY\n.El\n");
            let item = item_from_capacity_source(&body);
            let expected = if length >= first_joined_length {
                HeadBodyRelation::joined(DefinitionBodyAlignment::Indented)
            } else {
                HeadBodyRelation::separated(DefinitionBodyAlignment::Indented)
            };
            assert_eq!(
                item.layout.head_body_relation, expected,
                "{body}: {item:#?}"
            );
            assert!(
                inline_text(&item.terms[0]).ends_with(&word),
                "{body}: {item:#?}"
            );
            assert_eq!(description_text(&item), "BODY", "{body}: {item:#?}");
        }
    }
}

#[test]
fn capacity_owner_switch_keeps_empty_word_cells_and_pending_glyphs() {
    // These 36 exact sources passed all five pristine profiles (lint=0).
    // term_word()573-616 consumes NOSPACE even for an empty/font operand.
    // Its next automatic buffer cell is BODY content; Joined forbids an
    // additional synthetic gap and does not erase that accepted cell.
    // A pending HEAD \zZ prints on the new row reached by term_fill(), so
    // the final receipt, rather than the older X row, determines its gap.
    for length in [3, 4, 5] {
        let word = "X".repeat(length);
        for head_owner in [true, false] {
            for (operand, body_text) in [
                ("\"\"", " BODY"),
                ("\\&", " BODY"),
                ("\\fB", " BODY"),
                ("\" \"", "  BODY"),
                ("\\zZ", "ZBODY"),
                ("\\z", " ODY"),
            ] {
                let middle = format!(".No {operand}\n");
                let body = format!(
                    ".Bl -hang -width 2n\n.It Xo\n.sp\n.No {word}\n{}.Xc\n{}.No BODY\n.El\n",
                    if head_owner { &middle } else { "" },
                    if head_owner { "" } else { &middle },
                );
                let item = item_from_capacity_source(&body);
                let expected = if head_owner && operand == "\\zZ" {
                    HeadBodyRelation::separated(DefinitionBodyAlignment::AfterTerm)
                } else if length >= 4 {
                    HeadBodyRelation::joined(DefinitionBodyAlignment::Indented)
                } else {
                    HeadBodyRelation::separated(DefinitionBodyAlignment::Indented)
                };
                assert_eq!(
                    item.layout.head_body_relation, expected,
                    "{body}: {item:#?}"
                );
                assert_eq!(
                    description_text(&item),
                    if head_owner { "BODY" } else { body_text },
                    "{body}: {item:#?}"
                );
                assert!(
                    inline_text(&item.terms[0]).contains(&word),
                    "{body}: {item:#?}"
                );
            }
        }
    }
}

#[test]
fn capacity_controls_execute_in_the_definition_head() {
    // Exact tree output for the complete capacity matrix confirms Xo/No
    // are under It HEAD; No BODY is under It BODY. A source-shaped matrix
    // must prove these actual AST owners, not infer them from .It spelling.
    fn first_item(node: &libmandoc_rs::Node) -> Option<&libmandoc_rs::Node> {
        if node.kind == libmandoc_rs::NodeKind::Block && node.macro_name.as_deref() == Some("It") {
            Some(node)
        } else {
            node.children.iter().find_map(first_item)
        }
    }
    fn has_word(node: &libmandoc_rs::Node, word: &str) -> bool {
        node.text.as_deref() == Some(word)
            || node.children.iter().any(|child| has_word(child, word))
    }
    for length in [3, 4, 5] {
        let word = "X".repeat(length);
        let source =
            format!("{HEADER}.Bl -hang -width 2n\n.It Xo\n.sp\n.No {word}\n.Xc\n.No BODY\n.El\n");
        let report = Parser::default()
            .parse_bytes("capacity-owner.1", source.as_bytes())
            .unwrap();
        let item = first_item(&report.document.root).expect("It block");
        let head = item
            .children
            .iter()
            .find(|child| child.kind == libmandoc_rs::NodeKind::Head)
            .expect("HEAD");
        let body = item
            .children
            .iter()
            .find(|child| child.kind == libmandoc_rs::NodeKind::Body)
            .expect("BODY");
        assert!(has_word(head, &word) && !has_word(body, &word), "{item:#?}");
        assert!(
            has_word(body, "BODY") && !has_word(head, "BODY"),
            "{item:#?}"
        );
    }
}
