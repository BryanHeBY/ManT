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

#[test]
fn empty_man_owner_head_and_body_regions_have_their_own_points() {
    // Both exact inputs ran on pinned CVS -Ttree/-Tutf8 -O width=78.
    // man_term.c::pre_TP establishes HEAD/BODY separately; post_TP flushes
    // the head before a missing body can be assigned its final position.
    let empty_head = render(b".TH X 1\n.SH D\n.TP\n\\&\nbody\n", InputFormat::Man);
    let head = empty_head
        .marks
        .iter()
        .find(|mark| mark.kind == 5 && mark.region_kind == 3)
        .unwrap();
    assert_eq!(head.selection_count, 0);
    valid_point(&empty_head, head.point.expect("empty TP head point"));

    let empty_body = render(b".TH X 1\n.SH D\n.TP\nterm\n", InputFormat::Man);
    let body = empty_body
        .marks
        .iter()
        .find(|mark| mark.kind == 5 && mark.region_kind == 4)
        .unwrap();
    assert_eq!(body.selection_count, 0);
    valid_point(&empty_body, body.point.expect("empty TP body point"));
}

#[test]
fn empty_mdoc_list_item_and_literal_regions_do_not_borrow_following_text() {
    // Exact inputs ran on pinned CVS -Ttree/-Tutf8 -O width=78.
    // mdoc_term.c::termp_it_pre and termp_bd_pre apply spacing before their
    // structural starts; subsequent text must not supply an empty selection.
    let item = render(
        b".Dd September 23, 2026\n.Dt X 1\n.Os\n.Sh D\n.Bl -tag\n.It\n.El\n",
        InputFormat::Mdoc,
    );
    for kind in [3, 4, 5] {
        let region = item
            .marks
            .iter()
            .find(|mark| mark.kind == 5 && mark.region_kind == kind)
            .unwrap();
        assert_eq!(region.selection_count, 0);
        valid_point(&item, region.point.expect("empty list region point"));
    }

    let literal = render(
        b".Dd September 23, 2026\n.Dt X 1\n.Os\n.Sh D\n.Bd -literal\n.Ed\nafter\n",
        InputFormat::Mdoc,
    );
    let region = literal
        .marks
        .iter()
        .find(|mark| mark.kind == 5 && mark.region_kind == 6)
        .unwrap();
    assert_eq!(region.selection_count, 0);
    valid_point(&literal, region.point.expect("empty literal point"));
    assert_eq!(text_column(&literal, "after").1, 5);
}

#[test]
fn empty_section_body_points_to_its_own_boundary_before_next_title() {
    // Exact input ran on pinned CVS -Ttree/-Tutf8 -O width=78.
    // man_term.c::post_SH flushes FIRST's head before its empty BODY;
    // pre_SH suppresses extra vspace before NEXT because FIRST is empty.
    let page = render(b".TH X 1\n.SH FIRST\n.SH NEXT\ntext\n", InputFormat::Man);
    let body = page
        .marks
        .iter()
        .find(|mark| mark.kind == 5 && mark.region_kind == 2)
        .unwrap();
    assert_eq!(body.selection_count, 0);
    let (next_row, _) = text_column(&page, "NEXT");
    assert_eq!(
        body.point,
        Some(AnnotatedDisplayPoint::RowColumn {
            row: next_row,
            column: 0
        })
    );
}

#[test]
fn empty_display_and_inline_equations_keep_final_region_points() {
    // Both exact inputs ran on pinned CVS -Ttree/-Thtml/-Tutf8 -O width=78.
    // eqn_term.c::term_eqn calls eqn_box() directly: an empty box emits no
    // glyph and may remain inline between surrounding authored words.
    // roff.c::roff_eqndelim() reparses the inline delimiters as synthetic
    // .EQ/.EN; read.c::mparse_buf_r() retains source identity but not an
    // authored line/column for that generated equation node.
    let standalone = render(
        b".TH X 1\n.SH D\nbefore\n.EQ\n.EN\nafter\n",
        InputFormat::Man,
    );
    let region = standalone
        .marks
        .iter()
        .find(|mark| mark.kind == 5 && mark.region_kind == 8 && mark.line == 4)
        .unwrap();
    assert_eq!(region.selection_count, 0);
    let (before_row, before_col) = text_column(&standalone, "before");
    let (after_row, after_col) = text_column(&standalone, "after");
    assert_eq!(before_row, after_row);
    match region.point.expect("empty display equation point") {
        AnnotatedDisplayPoint::RowColumn { row, column } => {
            assert_eq!(row, before_row);
            assert!((before_col + 6..=after_col).contains(&column));
        }
        point @ AnnotatedDisplayPoint::DocumentEnd { .. } => {
            panic!("equation lost its inline row: {point:?}")
        }
    }

    let inline = render(
        b".TH X 1\n.SH D\n.EQ\ndelim $$\n.EN\nbefore $$ after\n",
        InputFormat::Man,
    );
    let equations = inline
        .marks
        .iter()
        .filter(|mark| mark.kind == 5 && mark.region_kind == 8)
        .collect::<Vec<_>>();
    assert_eq!(equations.len(), 2);
    assert_eq!(equations[0].line, 3);
    let region = equations[1];
    assert_eq!((region.source, region.line, region.column), (1, 0, 0));
    assert_eq!(region.flags & 1, 0);
    assert_eq!(region.selection_count, 0);
    let (before_row, before_col) = text_column(&inline, "before");
    let (after_row, after_col) = text_column(&inline, "after");
    assert_eq!(before_row, after_row);
    match region.point.expect("empty inline equation point") {
        AnnotatedDisplayPoint::RowColumn { row, column } => {
            assert_eq!(row, before_row);
            assert!((before_col + 6..=after_col).contains(&column));
        }
        point @ AnnotatedDisplayPoint::DocumentEnd { .. } => {
            panic!("inline equation lost its row: {point:?}")
        }
    }
}
