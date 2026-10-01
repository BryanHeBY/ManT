//! Column BODY post row-end receipts.
//!
//! An authored `\p` pass boundary that ended the represented device row
//! inside the post's `term_flushln()` pass loop must reach the cell's block
//! owner, so the following cell starts a new table row. Every source below
//! ran the pinned CVS reference (`cvs-20260927T130954Z`, sha256
//! `482cf795…05accb6`) with `-Tlint` clean before these assertions were
//! fixed; the quoted rows are its `-Tutf8 -Owidth=78` DESCRIPTION output.

use super::*;

/// Lower one `.Bl -column` DESCRIPTION body and return each cell's
/// concatenated block text, with hard row ends as `\n`.
fn column_cell_text(body: &str) -> Vec<String> {
    let source = format!(
        ".Dd September 28, 2026\n.Dt TEST 1\n.Os\n.Sh NAME\n.Nm test\n.Nd probe\n.Sh DESCRIPTION\n{body}.Sh NEXT\n.No END\n"
    );
    let document = parse_manual_bytes(
        std::path::Path::new("column-post-rows.7"),
        source.as_bytes(),
    )
    .expect("lower column source");
    let Block::Table { rows, .. } = &document.sections[1].blocks[0] else {
        panic!("expected one lowered column table: {document:#?}");
    };
    assert_eq!(rows.len(), 1, "one .It row: {rows:#?}");
    rows[0]
        .cells
        .iter()
        .map(|cell| {
            cell.blocks
                .iter()
                .map(|block| match block {
                    Block::Paragraph { children, .. } | Block::Preformatted { children, .. } => {
                        inline_text(children)
                    }
                    _ => String::new(),
                })
                .collect::<String>()
        })
        .collect()
}

#[test]
fn an_authored_breakline_column_post_hands_its_row_end_to_the_next_cell() {
    // Reference rows: "     D" / "             RightWord". The trailing
    // `\p` markers armed breakline (term.c:294-305), the pass loop ended
    // the represented row after D (term.c:217), and AFTER stayed unprinted
    // buffer input that the buffer reset discarded (term.c:143-146 with
    // 233-237). The final tail comparison (250-253) then left the fresh
    // row open under NOBREAK, so the row-end fact must come from the
    // flush receipt, not from the final `ends_row` decision.
    let cells = column_cell_text(
        ".Bl -column \"xxxx\" \"xxxx\"\n.It No \"D\\p \\p\" No AFTER Ta RightWord\n.El\n",
    );
    assert_eq!(cells, ["D\n", "RightWord"]);
}

#[test]
fn authored_column_row_ends_stay_identical_across_widths_and_multiline_bodies() {
    // Widths 8 and 20 and the Xo/Xc multiline spelling of the same body
    // print the same two rows in the reference; `.It … Ta` and `Xo/Xc Ta`
    // reach the same AST column boundary, so the receipt must not depend
    // on the source spelling (mdoc_term.c:930-964 runs one BODY post per
    // cell either way).
    for width in [4usize, 8, 20] {
        let declared = "x".repeat(width);
        let cells = column_cell_text(&format!(
            ".Bl -column \"{declared}\" \"xxxx\"\n.It No \"D\\p \\p\" No AFTER Ta RightWord\n.El\n"
        ));
        assert_eq!(cells, ["D\n", "RightWord"], "inline width {width}");
        let cells = column_cell_text(&format!(
            ".Bl -column \"{declared}\" \"xxxx\"\n.It Xo\n.No \"D\\p \\p\"\n.No AFTER\n.Xc Ta RightWord\n.El\n"
        ));
        assert_eq!(cells, ["D\n", "RightWord"], "multiline width {width}");
    }
}

#[test]
fn the_authored_column_row_end_follows_its_breaking_cell() {
    // Reference rows: a first-column break yields "D" then
    // "MidWord LastWord"; a middle-column break yields "Alpha   E" then
    // "LastWord" (the break belongs to the breaking cell's own owner, not
    // to the preceding cell); a last-column break yields
    // "Alpha   Beta    C" with no following body row, because the last
    // column's own post closes the row and an appended break would only
    // manufacture an empty row.
    let first = column_cell_text(
        ".Bl -column \"xxxx\" \"xxxx\" \"xxxx\"\n.It No \"D\\p \\p\" No AFTER Ta MidWord Ta LastWord\n.El\n",
    );
    assert_eq!(first, ["D\n", "MidWord", "LastWord"]);
    let middle = column_cell_text(
        ".Bl -column \"xxxx\" \"xxxx\" \"xxxx\"\n.It Alpha Ta No \"E\\p \\p\" No AFTER Ta LastWord\n.El\n",
    );
    assert_eq!(middle, ["Alpha", "E\n", "LastWord"]);
    let last = column_cell_text(
        ".Bl -column \"xxxx\" \"xxxx\" \"xxxx\"\n.It Alpha Ta Beta Ta No \"C\\p \\p\" No AFTER\n.El\n",
    );
    assert_eq!(last, ["Alpha", "Beta", "C"]);
}

#[test]
fn a_marker_without_printable_remainder_does_not_end_the_column_row() {
    // Reference rows: "D\p" with an accepted following word ends the row
    // after D (that marker pass has a printable continuation); "D\p \p"
    // whose remainder is only ignorable input (nothing at all, or a
    // zero-width `\&`) leaves the row open - the tail sweep reaches
    // lastcol without a loop endline (term.c:177-196) - and RightWord
    // stays on D's row. A marker-only cell owns no row identity.
    let accepted = column_cell_text(
        ".Bl -column \"xxxx\" \"xxxx\"\n.It No \"D\\p\" No AFTER Ta RightWord\n.El\n",
    );
    assert_eq!(accepted, ["D\nAFTER", "RightWord"]);
    for tail in ["", "No \"\\&\" "] {
        let cells = column_cell_text(&format!(
            ".Bl -column \"xxxx\" \"xxxx\"\n.It No \"D\\p \\p\" {tail}Ta RightWord\n.El\n"
        ));
        assert_eq!(cells, ["D", "RightWord"], "tail {tail:?}");
    }
    let marker_only =
        column_cell_text(".Bl -column \"xxxx\" \"xxxx\"\n.It No \"\\p\" Ta RightWord\n.El\n");
    assert_eq!(marker_only, ["", "RightWord"]);
}

#[test]
fn ordinary_and_overflowing_column_cells_keep_responsive_rows() {
    // Reference rows: "label AFTER" fits a width-8 column and
    // CLSET_TIMEOUT fits a width-12 column, so every cell shares one row
    // with no authored break. The same "label AFTER" in a width-4 column
    // wraps in the reference, but that placement is a device-width
    // decision: the reading projection reflows it without an inline hard
    // row. An empty cell body creates no row.
    let fitting = column_cell_text(
        ".Bl -column \"xxxxxxxx\" \"xxxx\"\n.It No \"label\" No AFTER Ta RightWord\n.El\n",
    );
    assert_eq!(fitting, ["label AFTER", "RightWord"]);
    let long_field =
        column_cell_text(".Bl -column \"xxxxxxxxxxxx\" \"xxxx\"\n.It No CLSET_TIMEOUT Ta X\n.El\n");
    assert_eq!(long_field, ["CLSET_TIMEOUT", "X"]);
    let wrapped = column_cell_text(
        ".Bl -column \"xxxx\" \"xxxx\"\n.It No \"label\" No AFTER Ta RightWord\n.El\n",
    );
    assert_eq!(wrapped, ["label AFTER", "RightWord"]);
    let empty = column_cell_text(".Bl -column \"xxxx\" \"xxxx\"\n.It Ta RightWord\n.El\n");
    assert_eq!(empty, ["", "RightWord"]);
}
