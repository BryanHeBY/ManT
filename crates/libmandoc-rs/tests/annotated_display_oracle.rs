#![cfg(feature = "annotated")]

use libmandoc_rs::annotated::{AnnotatedDocument, AnnotatedRenderer};
use libmandoc_rs::{InputFormat, SourceBundle};

fn surface(page: &AnnotatedDocument) -> String {
    let mut visible = String::new();
    for row in &page.rows {
        let first = row.first_run as usize;
        let end = first + row.run_count as usize;
        for run in &page.runs[first..end] {
            let start = usize::try_from(run.byte_start).unwrap();
            let end = start + usize::try_from(run.byte_count).unwrap();
            visible.push_str(&page.text[start..end]);
        }
        if row.break_after {
            visible.push('\n');
        }
    }
    visible
}

fn render(input: &[u8], format: InputFormat) -> AnnotatedDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", input.to_vec()).unwrap();
    AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle, format)
        .unwrap()
}

#[test]
fn simple_man_final_cells_keep_only_body_rows() {
    // Pinned man_term.c::print_man_node/term.c::term_vspace and
    // term_ascii.c::utf8_letter: the exact input was run through the fixed
    // CVS -Tutf8 -O width=78 reference before this assertion. The expected
    // surface folds its bold overstrike and excludes page header/footer,
    // including their framing blank rows.
    let page = render(b".TH T 1\n.SH D\nbody\n", InputFormat::Man);
    assert_eq!(surface(&page), "D\n     body\n");
}

#[test]
fn table_after_no_fill_preserves_final_cells_without_duplicate_text() {
    // Pinned term.c::term_field and tbl_term.c::tbl_word: the exact input
    // was checked with fixed CVS -Tutf8 -O width=78. The table's cell is
    // printed once after the no-fill line, independent of its style/owner.
    let page = render(
        b".TH T 1\n.SH D\n.nf\nbefore\n.TS\ntab(;);\nl l.\na;b\n.TE\n.fi\n",
        InputFormat::Man,
    );
    assert_eq!(surface(&page), "D\n     before\n     a   b\n");
}

#[test]
fn real_overstrike_retires_hidden_glyphs() {
    // Pinned term.c::term_field and term_ascii.c::utf8_letter: the exact
    // input was checked with fixed CVS -Tutf8 -O width=78. A\bB and a\bb
    // finish with B and b, not searchable hidden A/a.
    let page = render(
        b".TH T 1\n.SH D\n.nf\n\\zAB\n\\o'ab'\n.fi\n",
        InputFormat::Man,
    );
    assert_eq!(surface(&page), "D\n     B\n     b\n");
}

#[test]
fn mdoc_footer_filter_keeps_body_drain() {
    // Pinned mdoc_term.c::termp_sh_pre requests a leading vspace and
    // term.c::term_vspace drains the body before footer decoration. Exact
    // input was checked with fixed CVS -Tutf8 -O width=78.
    let page = render(
        b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\nbody\n",
        InputFormat::Mdoc,
    );
    assert_eq!(surface(&page), "\nDESCRIPTION\n     body\n");
}
