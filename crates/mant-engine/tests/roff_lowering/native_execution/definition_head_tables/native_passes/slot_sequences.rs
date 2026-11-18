//! Device slots decide which source intervals and explicit rows survive.

use super::{Block, between, first_table, input, lower, projections, whole_word};

#[test]
fn later_table_slot_writes_change_the_accepted_word_and_physical_row() {
    // Fixed term_word() applies each draw, move, and overstrike to the
    // slot buffer before term_fill() examines a marker and its blank.
    // The exact five pristine profiles were run for all three owners.
    for (operations, distance, xyz, q) in [
        (r"Q\l'1n '\h'-1n'", 3, Some(2), Some(1)),
        (r"\l'1n.'\h'-1n'", 2, None, None),
        (r"Q\p \o' '", 3, Some(2), Some(1)),
        (r"\l'2n '\h'|1n'\h'|3n'", 3, Some(2), None),
        (r"\l'2n.'", 3, Some(2), None),
        (r"\l'2n '", 2, None, None),
        (r"\o'ab'", 3, Some(2), None),
        (r"Q\l'1n '\h'0n'\h'|5n'", 3, Some(2), Some(1)),
        (r"\l'2n.'\h'|1n'\h'|2n'", 3, Some(2), None),
        (r"\l'2n.'\h'|1n'\h'|0n'", 2, Some(1), None),
        (r"\l'2n.'\h'|0n'\l'1n '", 3, Some(2), None),
        (r"\l'2n.'\h'|0n'\o' '", 3, Some(2), None),
        (r"Q\p \l'2n.'\h'|0n'\h'|3n'", 3, Some(2), Some(1)),
        (r"Q\p \l'2n.'\h'|3n'\h'|0n'", 2, Some(1), None),
        (r"Q\p \l'2n '\o' '", 3, None, Some(1)),
        (r"Q\p \l'2n '\h'|1n'\h'|3n'", 3, None, Some(1)),
        (r"\l'2n.'\l'2n '\h'-3n'", 3, Some(2), None),
        (r"\l'2n.'\l'2n '\h'-4n'", 2, Some(1), None),
    ] {
        for owner in ["head", "body", "block"] {
            check_sequence(owner, operations, distance, xyz, q);
        }
    }
}

#[test]
fn cell_width_only_limits_slot_writes_outside_the_native_no_break_head() {
    // tbl_term leaves the surrounding NOBREAK flag intact. Its terminal
    // scanner uses the page margin in a HANG HEAD, but the measured
    // column width in BODY and ordinary table ownership.
    for (operations, head_distance, head_xyz, cell_distance, cell_xyz) in [
        (r"Q\l'2n.'\l'2n '\h'1n'", 3, Some(2), 3, None),
        (r"Q\l'2n '\l'2n.'\h'1n'", 3, Some(2), 4, None),
        (r"Q\l'2n '\l'2n.'\h'|2n'", 2, Some(1), 3, Some(2)),
    ] {
        check_sequence("head", operations, head_distance, head_xyz, Some(1));
        for owner in ["body", "block"] {
            check_sequence(owner, operations, cell_distance, cell_xyz, Some(1));
        }
    }
}

#[test]
fn a_device_only_first_pass_and_an_empty_rejected_pass_are_both_rows() {
    // The accepted drawn point has no reading-text glyph. Its physical
    // row and the following rejected pass still precede the next span.
    for owner in ["head", "body", "block"] {
        check_sequence(owner, r"\l'1n.' ", 3, None, None);
        check_sequence(owner, r"\l'1n.'", 3, Some(2), None);
    }
}

fn check_sequence(
    owner: &str,
    operations: &str,
    distance: usize,
    xyz: Option<usize>,
    q: Option<usize>,
) {
    let source = input(owner, "", "l", &format!("START\n{operations}\\p XYZ\nLAST"));
    let query = lower(&source);
    for text in projections(&query) {
        for marker in ["START", "LAST", "ENDING"] {
            assert_eq!(text.matches(marker).count(), 1, "{source}\n{text}");
        }
        let rows = between(&text, "START", "LAST");
        assert_eq!(rows.len() + 1, distance, "{source}\n{text}");
        assert_eq!(word_row(&rows, "XYZ"), xyz, "{source}\n{text}");
        assert_eq!(word_row(&rows, "Q"), q, "{source}\n{text}");
    }
    assert_eq!(
        whole_word(&query, "XYZ").matches.len(),
        usize::from(xyz.is_some()),
        "{source}"
    );
    assert_cell_owner(owner, &query, xyz.is_some(), q.is_some(), &source);
}

fn word_row(rows: &[String], word: &str) -> Option<usize> {
    rows.iter()
        .position(|row| row.contains(word))
        .map(|row| row + 1)
}

fn assert_cell_owner(
    owner: &str,
    query: &mant_ir::ResolvedContent,
    xyz: bool,
    q: bool,
    source: &str,
) {
    let blocks = &query.document.as_ref().unwrap().sections[1].blocks;
    let spelling = if owner == "head" {
        let [Block::DefinitionList { items, .. }] = blocks.as_slice() else {
            panic!("{source}: expected the HEAD owner");
        };
        let term = mant_ir::inline_plain_text(&items[0].terms[0]);
        assert!(term.contains("START") && term.contains("LAST"), "{source}");
        term
    } else {
        let Some(Block::Table { rows, .. }) = first_table(blocks) else {
            panic!("{source}: expected the table owner");
        };
        let [Block::Paragraph { children, .. }, ..] = rows[1].cells[0].blocks.as_slice() else {
            // The original data span remains even after an empty accepted
            // or rejected word contributes only its closing row.
            assert!(!xyz && !q, "{source}");
            return;
        };
        mant_ir::inline_plain_text(children)
    };
    assert_eq!(spelling.contains("XYZ"), xyz, "{source}: {spelling}");
    assert_eq!(spelling.contains('Q'), q, "{source}: {spelling}");
    assert!(!spelling.contains("ENDING"), "{source}: {spelling}");
}
