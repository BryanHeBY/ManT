#![cfg(feature = "annotated")]

//! Native zero-width positions are final display boundaries, not borrowed runs.

use libmandoc_rs::annotated::{AnnotatedDisplayPoint, AnnotatedDocument, AnnotatedRenderer};
use libmandoc_rs::{InputFormat, SourceBundle};

fn render(source: &[u8], format: InputFormat) -> AnnotatedDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert("x.1", source.to_vec()).unwrap();
    AnnotatedRenderer::default()
        .render_bundle("x.1", &bundle, format)
        .unwrap()
}

fn valid_point(page: &AnnotatedDocument, point: AnnotatedDisplayPoint) {
    match point {
        AnnotatedDisplayPoint::RowColumn { row, column } => {
            assert!(row > 0);
            let body_row = &page.rows[usize::try_from(row - 1).unwrap()];
            assert!(column <= body_row.column_count);
        }
        AnnotatedDisplayPoint::DocumentEnd { row_count } => {
            assert_eq!(usize::try_from(row_count).unwrap(), page.rows.len());
        }
    }
}

fn anchor_point(page: &AnnotatedDocument, name: &str) -> AnnotatedDisplayPoint {
    page.marks
        .iter()
        .find(|mark| mark.kind == 4 && mark.name.as_deref() == Some(name))
        .and_then(|mark| mark.point)
        .expect("authored anchor with final position")
}

fn text_column(page: &AnnotatedDocument, needle: &str) -> (u32, u32) {
    for row in &page.rows {
        let first = usize::try_from(row.first_run).unwrap();
        let end = first + usize::try_from(row.run_count).unwrap();
        for run in &page.runs[first..end] {
            let start = usize::try_from(run.byte_start).unwrap();
            let stop = usize::try_from(run.byte_start + run.byte_count).unwrap();
            if let Some(offset) = page.text[start..stop].find(needle) {
                // These targeted fixtures use ASCII visible text only.
                return (row.key, run.column + u32::try_from(offset).unwrap());
            }
        }
    }
    panic!("missing final text {needle:?}");
}

#[test]
fn empty_man_tp_keeps_a_point_without_borrowing_its_body() {
    // Exact input ran on pinned CVS -Tutf8 -O width=78. man_term.c::pre_TP
    // establishes the item before its legal empty head; BODY remains separate.
    let page = render(b".TH T 1\n.SH D\n.TP\n\\&\nBODY\n", InputFormat::Man);
    let owner = page.marks.iter().find(|mark| mark.kind == 2).unwrap();
    assert_eq!(owner.selection_count, 0);
    valid_point(&page, owner.point.expect("empty item position"));
}

#[test]
fn empty_mdoc_it_keeps_a_point_without_a_visible_term() {
    // Exact input ran on pinned CVS -Tutf8 -O width=78. mdoc_term.c::
    // termp_it_pre establishes the item after list spacing, with no term glyph.
    let page = render(
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag\n.It\n.El\n",
        InputFormat::Mdoc,
    );
    let owner = page.marks.iter().find(|mark| mark.kind == 2).unwrap();
    assert_eq!(owner.selection_count, 0);
    valid_point(&page, owner.point.expect("empty item position"));
}

#[test]
fn trailing_tag_after_list_stays_at_final_body_end() {
    // Exact input ran on pinned CVS -Tutf8/-Thtml. tag.c::tag_postprocess
    // leaves Tail after the list; mdoc_term.c calls term_tag_write there.
    let page = render(
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\nbefore\n.Bl -bullet\n.It\nbody\n.El\n.Tg Tail\n",
        InputFormat::Mdoc,
    );
    let anchor = page
        .marks
        .iter()
        .find(|mark| mark.kind == 4 && mark.name.as_deref() == Some("Tail"))
        .unwrap();
    assert_eq!(
        anchor.point,
        Some(AnnotatedDisplayPoint::DocumentEnd {
            row_count: u32::try_from(page.rows.len()).unwrap()
        })
    );
    assert_eq!(anchor.line, 10);
}

#[test]
fn tab_skip_places_tag_before_the_next_authored_word() {
    // Exact input ran on pinned CVS -Ttree/-Thtml/-Tutf8 -O width=78.
    // tag.c leaves Gap on .Tg; term.c::term_field skips the literal tab,
    // then term_ascii.c::ascii_advance emits only actual device spaces.
    let page = render(
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\npre\t\n.Tg Gap\npost\n",
        InputFormat::Mdoc,
    );
    let (pre_row, pre_col) = text_column(&page, "pre");
    let (post_row, post_col) = text_column(&page, "post");
    assert_eq!(pre_row, post_row);
    assert_eq!(post_col - pre_col, 6); // CVS body row: "pre   post".
    assert_eq!(
        anchor_point(&page, "Gap"),
        AnnotatedDisplayPoint::RowColumn {
            row: post_row,
            column: post_col - 1
        }
    );
}

#[test]
fn discarded_trailing_spaces_do_not_push_a_tag_past_body_end() {
    // Exact input ran on pinned CVS -Ttree/-Thtml/-Tutf8 -O width=78.
    // term.c::term_field defers trailing blanks; term_flushln discards them.
    let page = render(
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\npre   \n.Tg Gap\n",
        InputFormat::Mdoc,
    );
    let (row, column) = text_column(&page, "pre");
    assert_eq!(column, 5);
    // The tag is still on the final active body row when term_flushln()
    // drops the trailing blanks; it is not after that row's line break.
    assert_eq!(
        anchor_point(&page, "Gap"),
        AnnotatedDisplayPoint::RowColumn {
            row,
            column: column + 3
        }
    );
}

#[test]
fn tag_inside_first_column_survives_partial_multicolumn_flush() {
    // Exact input ran on pinned CVS -Ttree/-Thtml/-Tutf8 -O width=78.
    // mdoc_term.c::termp_it_pre enters the -column item; term.c::
    // term_flushln partially consumes column one before column two is placed.
    let page = render(
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Bl -column \"AAAAAA\" \"BBBBBB\"\n.It\n.No alpha\n.Tg Gap\n.Ta\n.No beta\n.El\n",
        InputFormat::Mdoc,
    );
    let (alpha_row, alpha_col) = text_column(&page, "alpha");
    let (beta_row, beta_col) = text_column(&page, "beta");
    assert_eq!(alpha_row, beta_row);
    assert!(beta_col > alpha_col + 5);
    assert_eq!(
        anchor_point(&page, "Gap"),
        AnnotatedDisplayPoint::RowColumn {
            row: alpha_row,
            column: alpha_col + 5
        }
    );
}

#[test]
fn tag_after_overstrike_uses_the_surviving_display_column() {
    // Exact input ran on pinned CVS -Ttree/-Thtml/-Tutf8 -O width=78.
    // term.c::encode1 emits a\bb for \o'ab'; the display normalizer keeps
    // only b in the occupied column before the following zero-width tag.
    let page = render(
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n\\o'ab'\n.Tg Gap\npost\n",
        InputFormat::Mdoc,
    );
    let (survivor_row, survivor_col) = text_column(&page, "b");
    let (post_row, post_col) = text_column(&page, "post");
    assert_eq!(survivor_row, post_row);
    assert_eq!(post_col - survivor_col, 2); // CVS body row: "b post".
    assert_eq!(
        anchor_point(&page, "Gap"),
        AnnotatedDisplayPoint::RowColumn {
            row: post_row,
            column: post_col - 1
        }
    );
}
