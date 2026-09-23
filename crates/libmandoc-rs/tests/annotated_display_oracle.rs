#![cfg(feature = "annotated")]

use libmandoc_rs::annotated::{AnnotatedDocument, AnnotatedRenderer};
use libmandoc_rs::{InputFormat, SourceBundle};
use std::process::Command;

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
fn auto_format_uses_the_native_parser_choice_for_man_and_mdoc() {
    // Pinned read.c::choose_parser scans the primary input for .TH/.Dd;
    // mparse_result returns that parser's macroset. Both exact inputs were
    // run through the fixed CVS -Tutf8 -O width=78 reference first.
    let man = render(b".TH AUTO 1\n.SH NAME\nauto \\- man\n", InputFormat::Auto);
    assert_eq!(man.metadata.macroset, 1);
    assert_eq!(man.sources[0].format, 1);
    assert!(surface(&man).contains("auto - man"));

    let mdoc = render(
        b".Dd September 24, 2026\n.Dt AUTO 1\n.Os\n.Sh NAME\n.Nm auto\n.Nd mdoc\n",
        InputFormat::Auto,
    );
    assert_eq!(mdoc.metadata.macroset, 2);
    assert_eq!(mdoc.sources[0].format, 2);
    assert!(surface(&mdoc).contains("mdoc"));
}

#[test]
fn auto_format_reports_the_resolved_macroset_for_includes() {
    // Pinned read.c::choose_parser selects .TH in the primary input, and
    // mparse_result returns one macroset for the parser and its .so input.
    // The exact root and include were run through fixed CVS -Tutf8
    // -O width=78 before these assertions.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "mant_auto_ref_root.1",
            b".TH AUTO 1\n.so mant_auto_ref_inc.roff\n".to_vec(),
        )
        .unwrap();
    bundle
        .insert("mant_auto_ref_inc.roff", b".SH NAME\nincluded\n".to_vec())
        .unwrap();
    let page = AnnotatedRenderer::default()
        .render_bundle("mant_auto_ref_root.1", &bundle, InputFormat::Auto)
        .unwrap();
    assert_eq!(page.metadata.macroset, 1);
    assert_eq!(page.sources.len(), 2);
    assert!(page.sources.iter().all(|source| source.format == 1));
    assert!(surface(&page).contains("included"));
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

#[test]
fn annotated_stdio_child_render() {
    if std::env::var_os("LIBMANDOC_RS_ANNOTATED_STDIO_CHILD").is_none() {
        return;
    }
    // The exact input was checked with fixed CVS -Tutf8 -O width=78 before
    // asserting that the annotated sink retains it in memory, not stdout.
    let page = render(
        b".TH STDIO 1\n.SH NAME\nANNOTATED-NATIVE-MUST-NOT-LEAK\n",
        InputFormat::Man,
    );
    assert!(surface(&page).contains("ANNOTATED-NATIVE-MUST-NOT-LEAK"));
}

#[test]
fn annotated_rendering_does_not_write_to_process_stdout() {
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", "annotated_stdio_child_render", "--nocapture"])
        .env("LIBMANDOC_RS_ANNOTATED_STDIO_CHILD", "1")
        .output()
        .expect("run isolated annotated renderer child");
    assert!(output.status.success(), "child failed: {output:?}");
    assert!(
        !String::from_utf8_lossy(&output.stdout).contains("ANNOTATED-NATIVE-MUST-NOT-LEAK"),
        "annotated native renderer leaked document content to stdout"
    );
}

#[test]
fn budget_failure_returns_no_page_and_next_call_recovers() {
    // The exact input was checked with fixed CVS -Tutf8 -O width=78. This
    // checks the adapter's per-call failure cleanup, not a formatter rule.
    let mut bundle = SourceBundle::new();
    bundle
        .insert("t.1", b".TH T 1\n.SH D\nbody\n".to_vec())
        .unwrap();
    let limited = AnnotatedRenderer::default()
        .with_max_builder_operations(1)
        .unwrap();
    assert!(
        limited
            .render_bundle("t.1", &bundle, InputFormat::Man)
            .is_err()
    );
    let page = AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle, InputFormat::Man)
        .unwrap();
    assert_eq!(surface(&page), "D\n     body\n");
}

#[test]
fn independent_threads_keep_annotated_sessions_isolated() {
    // Both exact inputs were checked with fixed CVS -Tutf8 -O width=78.
    // The per-call sink/TLS state must never mix their body rows.
    std::thread::scope(|scope| {
        let jobs = [
            (b".TH A 1\n.SH D\nalpha\n".as_slice(), "alpha", "beta"),
            (b".TH B 1\n.SH D\nbeta\n".as_slice(), "beta", "alpha"),
        ];
        let handles = jobs.map(|(input, own, other)| {
            scope.spawn(move || {
                let page = render(input, InputFormat::Man);
                let visible = surface(&page);
                assert!(visible.contains(own));
                assert!(!visible.contains(other));
            })
        });
        for handle in handles {
            handle.join().unwrap();
        }
    });
}
