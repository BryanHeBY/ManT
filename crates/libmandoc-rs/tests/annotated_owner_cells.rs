#![cfg(feature = "annotated")]

//! R01 native owner boundaries; these are not final IR ranges.

use libmandoc_rs::annotated::{AnnotatedDocument, AnnotatedRenderer, AnnotatedTextJoin};
use libmandoc_rs::{InputFormat, SourceBundle};

fn render(source: &[u8]) -> AnnotatedDocument {
    render_at_width(source, 78)
}

fn render_at_width(source: &[u8], width: u32) -> AnnotatedDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert("x.1", source.to_vec()).unwrap();
    AnnotatedRenderer::new(width)
        .unwrap()
        .render_bundle("x.1", &bundle, InputFormat::Man)
        .unwrap()
}

fn run_contains(page: &AnnotatedDocument, needle: &str, owner: u32) -> bool {
    page.runs.iter().any(|run| {
        let start = usize::try_from(run.byte_start).unwrap();
        let end = usize::try_from(run.byte_start + run.byte_count).unwrap();
        run.label.owner == owner && page.text[start..end].contains(needle)
    })
}

fn selected_text(page: &AnnotatedDocument, key: u32) -> String {
    let mark = &page.marks[usize::try_from(key - 1).unwrap()];
    let first = usize::try_from(mark.selection_first).unwrap();
    let count = usize::try_from(mark.selection_count).unwrap();
    let mut text = String::new();
    for part in &page.selection_parts[first..first + count] {
        let run = &page.runs[usize::try_from(part.run - 1).unwrap()];
        let start = usize::try_from(run.byte_start + part.start_byte).unwrap();
        let end = usize::try_from(run.byte_start + part.end_byte).unwrap();
        text.push_str(&page.text[start..end]);
    }
    text
}

fn selection_joins(page: &AnnotatedDocument, key: u32) -> Vec<(AnnotatedTextJoin, String)> {
    let mark = &page.marks[usize::try_from(key - 1).unwrap()];
    let first = usize::try_from(mark.selection_first).unwrap();
    let count = usize::try_from(mark.selection_count).unwrap();
    page.selection_parts[first..first + count]
        .iter()
        .map(|part| {
            let start = usize::try_from(part.join_text_start).unwrap();
            let end = usize::try_from(part.join_text_start + part.join_text_len).unwrap();
            (part.join_before, page.join_text[start..end].to_owned())
        })
        .collect()
}

#[test]
fn soft_wrap_keeps_the_exact_consumed_separator_count() {
    // Both exact inputs first ran on pinned CVS -Tutf8/-Ttree at width 20.
    // term.c::term_flushln consumes one or three authored ASCII spaces at
    // WRAP, producing identical device rows; the join retains their count.
    for (middle, expected) in [(" ", " "), ("   ", "   ")] {
        let input = format!(".TH T 1\n.SH D\n.UR https://x.test\nalpha beta{middle}gamma\n.UE\n");
        let page = render_at_width(input.as_bytes(), 20);
        let heading = page.marks.iter().find(|mark| mark.kind == 1).unwrap();
        let joins = selection_joins(&page, heading.body_region);
        assert!(
            page.runs.iter().any(|run| run.label.role == 5),
            "wrapped row must retain proven native indentation"
        );
        assert!(
            joins.iter().any(|(join, text)| {
                *join == AnnotatedTextJoin::AuthoredSeparator && text == expected
            }),
            "expected {expected:?} in {joins:?}"
        );
    }
}

#[test]
fn generated_wrap_space_is_not_an_authored_separator() {
    // Exact input first ran on pinned CVS -Tutf8 at width 20.  In
    // term.c::term_word(), the inter-node blank is AUTO_SPACE; term_flushln()
    // later consumes it at WRAP, but there is no authored separator to keep.
    let page = render_at_width(b".TH T 1\n.SH D\n.B 123456789012345\n.I abcdefghij\n", 20);
    let heading = page.marks.iter().find(|mark| mark.kind == 1).unwrap();
    let joins = selection_joins(&page, heading.body_region);
    assert!(
        !joins
            .iter()
            .any(|(join, _)| *join == AnnotatedTextJoin::AuthoredSeparator),
        "generated separator became authored in {joins:?}"
    );
}

#[test]
fn native_box_border_does_not_allocate_a_run_per_drawn_character() {
    // Exact table first ran on pinned CVS -Tutf8.  tbl_term.c::
    // tbl_direct_border() draws the horizontal rule character by character;
    // those zero-origin decorations can still share one final run.
    let page = render(b".TH X 1\n.SH D\n.TS\nbox;\nl.\nhello\n.TE\n");
    assert!(page.runs.iter().any(|run| {
        let start = usize::try_from(run.byte_start).unwrap();
        let end = usize::try_from(run.byte_start + run.byte_count).unwrap();
        page.text[start..end].contains("────")
    }));
}

#[test]
fn no_fill_newline_is_hard_but_escaped_style_continuation_can_contact() {
    // Exact inputs first ran on pinned CVS -Tutf8/-Ttree.  The ordinary
    // NODE_LINE triggers term_newln(); term.c::ESCAPE_NOSPACE from \c
    // suppresses a break, while B/I leave distinct final style runs.
    let hard = render(b".TH T 1\n.SH D\n.nf\nalpha  beta\ngamma\n.fi\n");
    let heading = hard.marks.iter().find(|mark| mark.kind == 1).unwrap();
    assert!(
        selection_joins(&hard, heading.body_region)
            .iter()
            .any(|(join, _)| *join == AnnotatedTextJoin::HardBoundary)
    );

    let contact = render(b".TH T 1\n.SH D\n.UR https://x.test\n.B alpha\\c\n.I beta\n.UE\n");
    let heading = contact.marks.iter().find(|mark| mark.kind == 1).unwrap();
    assert!(
        selection_joins(&contact, heading.body_region)
            .iter()
            .any(|(join, _)| *join == AnnotatedTextJoin::DirectContact)
    );
}

#[test]
fn heading_title_and_body_keep_direct_survivor_selections() {
    // Exact input first checked with pinned CVS -Tutf8/-Ttree.  In
    // man_term.c::pre_SH/post_SH, HEAD and BODY are separate native scopes
    // even though their terminal output belongs to one section.
    let page = render(b".TH T 1\n.SH \"SEE ALSO\"\n.MR printf 3\n");
    let heading = page.marks.iter().find(|mark| mark.kind == 1).unwrap();
    assert_eq!(selected_text(&page, heading.title_region), "SEE ALSO");
    assert_eq!(selected_text(&page, heading.body_region), "printf(3)");
}

#[test]
fn empty_term_has_no_surviving_head_text_and_cannot_borrow_body() {
    // Exact input first checked with pinned CVS -Tutf8/-Ttree/-Thtml.
    // man_term.c::post_TP flushes HEAD; the \& tag is a legal empty dt.
    let page = render(b".TH T 1\n.SH D\n.TP\n\\&\nBODY\n");
    let owner = page.marks.iter().find(|mark| mark.kind == 2).unwrap();
    assert_eq!(owner.selection_count, 0);
    assert_eq!(selected_text(&page, owner.title_region), "");
    assert_eq!(selected_text(&page, owner.body_region), "BODY");
}

#[test]
fn overwritten_glyphs_never_reenter_direct_selection() {
    // Both exact inputs first checked with pinned CVS -Tutf8/-Thtml.
    // term.c::term_word encodes \o and \z, then term_field places the
    // surviving glyph after backspace. HTML independently keeps only b/B.
    for (text, expected) in [("\\o'ab'", "b"), ("\\zAB", "B")] {
        let input = format!(".TH T 1\n.SH D\n.nf\n{text}\n.fi\n");
        let page = render(input.as_bytes());
        let heading = page.marks.iter().find(|mark| mark.kind == 1).unwrap();
        assert_eq!(selected_text(&page, heading.body_region), expected);
    }
}

#[test]
fn tq_has_its_own_owner_head_and_body() {
    // Exact input first checked against the pinned CVS -Tutf8/-Ttree paths.
    // man_macro.c::blk_imp creates a distinct TQ block; man_term.c traverses
    // its own HEAD and BODY even when it follows TP without intervening body.
    let page = render(b".TH X 1\n.SH D\n.TP\nfirst\n.TQ\nsecond\nbody\n");
    let owners: Vec<_> = page.marks.iter().filter(|mark| mark.kind == 2).collect();
    assert_eq!(owners.len(), 2);
    let tp = owners[0];
    let tq = owners[1];
    assert_ne!(tp.key, tq.key);
    assert!(tq.title_region != 0 && tq.body_region != 0);
    assert!(run_contains(&page, "first", tp.title_region));
    assert!(run_contains(&page, "second", tq.title_region));
    assert!(run_contains(&page, "body", tq.body_region));
    assert_eq!(selected_text(&page, tp.title_region), "first");
    assert_eq!(selected_text(&page, tq.title_region), "second");
    assert_eq!(selected_text(&page, tp.body_region), "");
    assert_eq!(selected_text(&page, tq.body_region), "body");
}

#[test]
fn table_text_and_empty_cells_keep_independent_native_owners() {
    // Exact input first checked against pinned CVS -Tutf8/-Ttree.  In
    // tbl_term.c::tbl_word is the only authored text writer; term_tbl() also
    // reports native positions for legal cells that emit no text.
    let page = render(b".TH X 1\n.SH D\n.TS\ntab(;);\nl l.\na;b\n;c\nd;\n;\n.TE\n.PP\nafter\n");
    let cells: Vec<_> = page
        .marks
        .iter()
        .filter(|mark| mark.kind == 5 && mark.region_kind == 9)
        .collect();
    assert_eq!(cells.len(), 8);
    for (index, cell) in cells.iter().enumerate() {
        assert_eq!(
            cell.native_table_position.unwrap().0,
            u32::try_from(index % 2).unwrap()
        );
        assert_eq!(cell.owner, cell.parent);
        assert_eq!(cell.line, 0); // Row source known; exact cell column is not.
    }
    assert!(run_contains(&page, "a", cells[0].key));
    assert!(run_contains(&page, "b", cells[1].key));
    assert!(run_contains(&page, "c", cells[3].key));
    assert!(run_contains(&page, "d", cells[4].key));
    for empty in [cells[2].key, cells[5].key, cells[6].key, cells[7].key] {
        assert!(!page.runs.iter().any(|run| run.label.owner == empty));
        assert_eq!(selected_text(&page, empty), "");
    }
    assert_eq!(selected_text(&page, cells[0].key), "a");
    assert_eq!(selected_text(&page, cells[1].key), "b");
    assert_eq!(selected_text(&page, cells[3].key), "c");
    assert_eq!(selected_text(&page, cells[4].key), "d");
    assert!(page.runs.iter().any(|run| {
        let start = usize::try_from(run.byte_start).unwrap();
        let end = usize::try_from(run.byte_start + run.byte_count).unwrap();
        run.label.owner != 0
            && !cells.iter().any(|cell| cell.key == run.label.owner)
            && page.text[start..end].contains("after")
    }));
}

#[test]
fn sparse_second_layout_keeps_only_its_real_cell() {
    // Exact variable-width layout input checked against pinned CVS utf8/tree.
    // tbl_term.c::term_tbl() still visits the second native column, but the
    // second layout row has no corresponding tbl_cell and no cell owner.
    let page = render(b".TH X 1\n.SH D\n.TS\ntab(;);\nl l,\nl.\na;b\nc\n.TE\n.PP\nafter\n");
    let cells: Vec<_> = page
        .marks
        .iter()
        .filter(|mark| mark.kind == 5 && mark.region_kind == 9)
        .collect();
    assert_eq!(cells.len(), 3);
    assert_eq!(cells[0].native_table_position.unwrap().0, 0);
    assert_eq!(cells[1].native_table_position.unwrap().0, 1);
    assert_eq!(cells[2].native_table_position.unwrap().0, 0);
    assert!(run_contains(&page, "a", cells[0].key));
    assert!(run_contains(&page, "b", cells[1].key));
    assert!(run_contains(&page, "c", cells[2].key));
    assert!(!run_contains(&page, "after", cells[2].key));
}

#[test]
fn spanning_layout_skips_the_covered_column_owner() {
    // Exact input checked against pinned CVS utf8/tree.  tbl_term.c skips
    // TBL_CELL_SPAN while retaining the native indices of columns 0 and 2.
    let page = render(b".TH X 1\n.SH D\n.TS\ntab(;);\nl s l.\naa;cc\n.TE\n.PP\nafter\n");
    let cells: Vec<_> = page
        .marks
        .iter()
        .filter(|mark| mark.kind == 5 && mark.region_kind == 9)
        .collect();
    assert_eq!(cells.len(), 2);
    assert_eq!(cells[0].native_table_position.unwrap().0, 0);
    assert_eq!(cells[1].native_table_position.unwrap().0, 2);
    assert!(run_contains(&page, "aa", cells[0].key));
    assert!(run_contains(&page, "cc", cells[1].key));
    assert!(!run_contains(&page, "after", cells[1].key));
}
