#![cfg(feature = "annotated")]

//! R01 link macro identity and final surviving label cells.

use libmandoc_rs::annotated::{AnnotatedDocument, AnnotatedRenderer, AnnotatedTextJoin};
use libmandoc_rs::{InputFormat, SourceBundle};

fn render(source: &[u8], format: InputFormat) -> AnnotatedDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert("x.1", source.to_vec()).unwrap();
    AnnotatedRenderer::default()
        .render_bundle("x.1", &bundle, format)
        .unwrap()
}

fn labels(page: &AnnotatedDocument, key: u32) -> String {
    let mut out = String::new();
    let mark = &page.marks[usize::try_from(key - 1).unwrap()];
    let first = usize::try_from(mark.selection_first).unwrap();
    let count = usize::try_from(mark.selection_count).unwrap();
    for part in &page.selection_parts[first..first + count] {
        let run = &page.runs[usize::try_from(part.run - 1).unwrap()];
        let start = usize::try_from(run.byte_start + part.start_byte).unwrap();
        let end = usize::try_from(run.byte_start + part.end_byte).unwrap();
        out.push_str(&page.text[start..end]);
    }
    out
}

fn links(page: &AnnotatedDocument) -> Vec<u32> {
    page.marks
        .iter()
        .filter(|mark| mark.kind == 3)
        .map(|mark| mark.key)
        .collect()
}

#[test]
fn mr_label_excludes_its_real_suffix_but_keeps_generated_parentheses() {
    // Exact inputs were first run with pinned CVS -Tutf8 and -Thtml.
    // man_term.c::pre_MR prints name(section) before its optional third
    // operand; man_html.c::man_MR_pre closes the anchor before that suffix.
    for (suffix, expected) in [("", "printf(3)"), (" ,", "printf(3)")] {
        let input = format!(".TH T 1\n.SH D\n.MR printf 3{suffix}\n");
        let page = render(input.as_bytes(), InputFormat::Man);
        let keys = links(&page);
        assert_eq!(keys.len(), 1);
        assert_eq!(labels(&page, keys[0]), expected);
        let target = page.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap();
        assert_eq!(target.kind, 4);
        assert_eq!(target.primary, "printf");
        assert_eq!(target.secondary.as_deref(), Some("3"));
    }
}

#[test]
fn xr_label_keeps_generated_parentheses_without_leading_separator() {
    // Exact input was first run with pinned CVS -Tutf8 and -Thtml.
    // mdoc_term.c::termp_xr_pre writes parentheses using term_word(), while
    // mdoc_html.c::mdoc_xr_pre keeps them inside the anchor.
    let page = render(
        b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh SEE ALSO\n.Xr printf 3\n",
        InputFormat::Mdoc,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&page, keys[0]), "printf(3)");
    let target = page.marks[usize::try_from(keys[0] - 1).unwrap()]
        .link_target
        .as_ref()
        .unwrap();
    assert_eq!(target.kind, 4);
    assert_eq!(target.primary, "printf");
    assert_eq!(target.secondary.as_deref(), Some("3"));
}

#[test]
fn ur_body_or_empty_head_is_label_not_generated_target_decoration() {
    // Exact inputs were first run with pinned CVS -Tutf8 and -Thtml.
    // man_term.c::post_UR prints <target> outside the body; man_html.c
    // selects the body, or the head when the body has no children.
    for (body, expected) in [
        ("click here\n", "click here"),
        ("", "https://example.test/a"),
    ] {
        let input = format!(".TH T 1\n.SH D\n.UR https://example.test/a\n{body}.UE\n");
        let page = render(input.as_bytes(), InputFormat::Man);
        let keys = links(&page);
        assert_eq!(keys.len(), 1);
        assert_eq!(labels(&page, keys[0]), expected);
        let target = page.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap();
        assert_eq!(target.primary, "https://example.test/a");
    }
}

#[test]
fn lk_description_is_clickable_without_target_or_trailing_delimiter() {
    // Exact input was first run with pinned CVS -Tutf8/-Thtml.  In
    // mdoc_html.c::mdoc_lk_pre the description is the anchor text, and
    // NODE_DELIMC punctuation is emitted after the closing anchor.
    let page = render(
        b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.Lk https://example.test/a\\-b display text .\n",
        InputFormat::Mdoc,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&page, keys[0]), "display text");
    assert_eq!(
        page.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap()
            .primary,
        "https://example.test/a-b"
    );
}

#[test]
fn nested_link_restores_outer_identity_without_new_macro_mark() {
    // Exact input was first run with pinned CVS -Tutf8/-Thtml.  The inner
    // .MR is one independent macro; man_term.c resumes the outer .UR body.
    let page = render(
        b".TH T 1\n.SH D\n.UR https://outer.test\nbefore\n.MR printf 3 ,\nafter\n.UE\n",
        InputFormat::Man,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 2);
    assert_eq!(labels(&page, keys[0]), "before after");
    assert_eq!(labels(&page, keys[1]), "printf(3)");
}

#[test]
fn paragraph_and_table_close_outer_link_without_stealing_later_text() {
    // Both exact inputs first ran on pinned CVS -Thtml.  man_html.c::
    // man_PP_pre() and tbl_html.c::html_tblopen() call
    // html_close_paragraph(), ending the open A even while terminal
    // traversal remains inside the original UR BODY.
    for middle in [".PP\n", ".TS\ntab(;);\nl l.\na;b\n.TE\n"] {
        let input = format!(".TH T 1\n.SH D\n.UR https://outer.test\nbefore\n{middle}after\n.UE\n");
        let page = render(input.as_bytes(), InputFormat::Man);
        let keys = links(&page);
        assert_eq!(keys.len(), 1);
        assert_eq!(labels(&page, keys[0]), "before");
        assert!(page.text.contains("after"));
    }

    // The exact empty-phrase case also ran through pinned CVS -Thtml:
    // paragraph closure leaves an empty A, not an invented later label.
    let page = render(
        b".TH T 1\n.SH D\n.UR https://outer.test\n.PP\nafter\n.UE\n",
        InputFormat::Man,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&page, keys[0]), "");

    // The paired man .MT/.ME macro uses the same phrase lifetime. Exact
    // input first ran on pinned CVS -Thtml and closed mailto A at PP.
    let page = render(
        b".TH T 1\n.SH D\n.MT a@example.test\nbefore\n.PP\nafter\n.ME\n",
        InputFormat::Man,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&page, keys[0]), "before");
}

#[test]
fn list_and_indent_blocks_close_the_active_link_phrase() {
    // Each exact input first ran on pinned CVS -Thtml.  man_html.c::
    // man_IP_pre() and man_RS_pre() close the paragraph before opening the
    // list/indent container; terminal output after the boundary is visible
    // but no longer part of the earlier A.
    for middle in [".IP tag\n", ".TP\nterm\n", ".RS\n"] {
        let suffix = if middle == ".RS\n" { ".RE\n" } else { "" };
        let input =
            format!(".TH T 1\n.SH D\n.UR https://x.test\nbefore\n{middle}after\n{suffix}.UE\n");
        let page = render(input.as_bytes(), InputFormat::Man);
        let keys = links(&page);
        assert_eq!(keys.len(), 1);
        assert_eq!(labels(&page, keys[0]), "before");
    }
}

#[test]
fn fill_transition_closes_link_but_line_break_and_nofill_sp_do_not() {
    // Each exact case ran on pinned CVS -Thtml. html.c::html_fillmode
    // closes in-phrase tags when switching fi<->nf. man_html.c/roff_html.c
    // keep A open across .br, and roff_html_pre_sp does so inside no-fill.
    let cases: &[(&[u8], bool)] = &[
        (
            b".TH T 1\n.SH D\n.UR https://outer.test\nbefore\n.nf\nafter\n.fi\n.UE\n",
            false,
        ),
        (
            b".TH T 1\n.SH D\n.nf\n.UR https://outer.test\nbefore\n.fi\nafter\n.UE\n",
            false,
        ),
        (
            b".TH T 1\n.SH D\n.UR https://outer.test\nbefore\n.br\nafter\n.UE\n",
            true,
        ),
        (
            b".TH T 1\n.SH D\n.nf\n.UR https://outer.test\nbefore\n.sp\nafter\n.UE\n.fi\n",
            true,
        ),
    ];
    for (input, keeps_after) in cases {
        let page = render(input, InputFormat::Man);
        let keys = links(&page);
        assert_eq!(keys.len(), 1);
        let label = labels(&page, keys[0]);
        assert!(label.contains("before"), "{label:?}");
        assert_eq!(label.contains("after"), *keeps_after, "{label:?}");
    }
}

#[test]
fn a_new_nested_link_does_not_reopen_a_closed_ancestor() {
    // Exact input first ran on pinned CVS -Thtml: PP closes the outer A;
    // MR opens one new A for printf(3), then its terminal parent UR resumes
    // text without regaining the earlier clickable scope.
    let page = render(
        b".TH T 1\n.SH D\n.UR https://outer.test\nbefore\n.PP\n.MR printf 3\nafter\n.UE\n",
        InputFormat::Man,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 2);
    assert_eq!(labels(&page, keys[0]), "before");
    assert_eq!(labels(&page, keys[1]), "printf(3)");
}

#[test]
fn live_link_keeps_native_contact_across_style_runs() {
    // Exact input first ran on pinned CVS -Thtml/-Tutf8.  HTML retains one
    // A around bold alpha and italic beta; term.c::ESCAPE_NOSPACE joins the
    // two device fragments without an authored or generated separator.
    let page = render(
        b".TH T 1\n.SH D\n.UR https://outer.test\n.B alpha\\c\n.I beta\n.UE\n",
        InputFormat::Man,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&page, keys[0]), "alphabeta");
    let mark = &page.marks[usize::try_from(keys[0] - 1).unwrap()];
    let first = usize::try_from(mark.selection_first).unwrap();
    let end = first + usize::try_from(mark.selection_count).unwrap();
    assert!(
        page.selection_parts[first + 1..end]
            .iter()
            .any(|part| part.join_before == AnnotatedTextJoin::DirectContact)
    );
}

#[test]
fn decoded_target_follows_pinned_html_escape_order() {
    // Exact input was first run with pinned CVS -Tutf8/-Thtml.  Its HTML
    // href is https://a.test/Y: html.c::print_encode processes the font
    // change before consuming \z's pending skipped character.
    let page = render(
        b".TH T 1\n.SH D\n.UR https://a.test/\\z\\fBX\\fPY\nlabel\n.UE\n",
        InputFormat::Man,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&page, keys[0]), "label");
    assert_eq!(
        page.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap()
            .primary,
        "https://a.test/Y"
    );
}

#[test]
fn missing_destinations_preserve_valid_native_output_without_inventing_links() {
    // Every exact input was first run with pinned CVS -Tutf8/-Thtml/-Ttree.
    // man_term.c::pre_MR still prints () without operands and
    // man_html.c::man_MR_pre emits a no-href anchor.  A headless .UR prints
    // <> in the terminal but man_html.c::man_UR_pre opens no anchor.
    let mr = render(b".TH T 1\n.SH D\n.MR\n", InputFormat::Man);
    let keys = links(&mr);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&mr, keys[0]), "()");
    assert!(
        mr.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .is_none()
    );

    let ur = render(b".TH T 1\n.SH D\n.UR\n.UE\n", InputFormat::Man);
    assert!(links(&ur).is_empty());
    assert!(ur.text.contains("<>"));

    // mdoc_html.c::mdoc_lk_pre/mdoc_xr_pre return without a child, and an
    // empty .Sx has no visible anchor text.  mdoc_validate.c::post_defaults
    // gives empty .Mt the real authored-path target "~" instead.
    for macro_line in [".Lk", ".Xr", ".Sx"] {
        let input = format!(".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh SEE ALSO\n{macro_line}\n");
        let page = render(input.as_bytes(), InputFormat::Mdoc);
        assert!(links(&page).is_empty());
    }
    let mt = render(
        b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh SEE ALSO\n.Mt\n",
        InputFormat::Mdoc,
    );
    let keys = links(&mt);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&mt, keys[0]), "~");
    assert_eq!(mt.marks[usize::try_from(keys[0] - 1).unwrap()].source, 0);
    assert_eq!(
        mt.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap()
            .primary,
        "~"
    );
}

#[test]
fn mt_operands_are_distinct_link_occurrences_without_shared_separator() {
    // Exact input first checked with pinned CVS -Ttree/-Tutf8/-Thtml.
    // mdoc_html.c::mdoc_mt_pre opens one mailto anchor per direct TEXT
    // operand; the space between them and following delimiter are outside.
    let page = render(
        b".Dd September 23, 2026\n.Dt TEST 1\n.Os\n.Sh DESCRIPTION\n.Mt a@example.test b@example.test .\n",
        InputFormat::Mdoc,
    );
    let keys = links(&page);
    assert_eq!(keys.len(), 2);
    let first = &page.marks[usize::try_from(keys[0] - 1).unwrap()];
    let second = &page.marks[usize::try_from(keys[1] - 1).unwrap()];
    assert_eq!(first.source, second.source);
    assert_eq!(first.line, second.line);
    assert!(first.column < second.column);
    for (key, expected) in keys.into_iter().zip(["a@example.test", "b@example.test"]) {
        assert_eq!(labels(&page, key), expected);
        let target = page.marks[usize::try_from(key - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap();
        assert_eq!(target.kind, 2);
        assert_eq!(target.primary, expected);
    }
}

#[test]
fn explicitly_empty_operand_keeps_label_but_no_invented_destination() {
    // Exact inputs were first run with pinned CVS -Tutf8/-Thtml.  The
    // An authored or decoded empty operand produces href="" for Lk/UR,
    // but a no-href .MR/.Xr anchor; normalization must not invent a target.
    let mr = render(b".TH T 1\n.SH D\n.MR \"\" 3\n", InputFormat::Man);
    let keys = links(&mr);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&mr, keys[0]), "(3)");
    assert!(
        mr.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .is_none()
    );

    // man_term.c::pre_MR still prints foo() when the authored section is
    // empty; man_html.c::man_MR_pre keeps a no-href anchor around it.  Exact
    // input was first run with pinned CVS -Tutf8/-Thtml/-Ttree.
    let empty_section = render(b".TH T 1\n.SH D\n.MR foo \"\"\n", InputFormat::Man);
    let keys = links(&empty_section);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&empty_section, keys[0]), "foo()");
    assert!(
        empty_section.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .is_none()
    );

    // mdoc_term.c::termp_xr_pre and mdoc_html.c::mdoc_xr_pre have the same
    // empty-section boundary.  This exact input was first run with pinned
    // CVS -Tutf8/-Thtml.
    let empty_xr_section = render(
        b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh SEE ALSO\n.Xr foo \"\"\n",
        InputFormat::Mdoc,
    );
    let keys = links(&empty_xr_section);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&empty_xr_section, keys[0]), "foo()");
    assert!(
        empty_xr_section.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .is_none()
    );

    let lk = render(
        b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Lk \"\" label\n",
        InputFormat::Mdoc,
    );
    let keys = links(&lk);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&lk, keys[0]), "label");
    assert_eq!(
        lk.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap()
            .primary,
        ""
    );

    // Each exact escaped operand was first run with pinned CVS utf8/html.
    // html.c::print_encode consumes \& without a scalar, so the decoded
    // destination is empty even though the authored text is not.
    for (operand, expected) in [("foo \\&", "foo()"), ("\\& 3", "(3)")] {
        let input = format!(".TH T 1\n.SH D\n.MR {operand}\n");
        let page = render(input.as_bytes(), InputFormat::Man);
        let keys = links(&page);
        assert_eq!(keys.len(), 1);
        assert_eq!(labels(&page, keys[0]), expected);
        assert!(
            page.marks[usize::try_from(keys[0] - 1).unwrap()]
                .link_target
                .is_none()
        );
    }
    let lk_decoded_empty = render(
        b".Dd September 23, 2026\n.Dt T 1\n.Os\n.Sh D\n.Lk \\& label\n",
        InputFormat::Mdoc,
    );
    let keys = links(&lk_decoded_empty);
    assert_eq!(keys.len(), 1);
    assert_eq!(labels(&lk_decoded_empty, keys[0]), "label");
    assert_eq!(
        lk_decoded_empty.marks[usize::try_from(keys[0] - 1).unwrap()]
            .link_target
            .as_ref()
            .unwrap()
            .primary,
        ""
    );
}
