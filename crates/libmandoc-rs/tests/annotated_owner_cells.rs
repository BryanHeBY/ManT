#![cfg(feature = "annotated")]

//! R01 native owner boundaries; these are not final IR ranges.

use libmandoc_rs::annotated::{AnnotatedDocument, AnnotatedRenderer};
use libmandoc_rs::{InputFormat, SourceBundle};

fn render(source: &[u8]) -> AnnotatedDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert("x.1", source.to_vec()).unwrap();
    AnnotatedRenderer::default()
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
    }
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
