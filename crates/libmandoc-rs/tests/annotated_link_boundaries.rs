#![cfg(feature = "annotated")]

//! R05 final-display and provenance boundaries for compatible references.

use libmandoc_rs::annotated::{AnnotatedDocument, AnnotatedRenderer};
use libmandoc_rs::{InputFormat, SourceBundle};

fn render(source: &[u8], format: InputFormat) -> AnnotatedDocument {
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", source.to_vec()).unwrap();
    AnnotatedRenderer::default()
        .render_bundle("t.1", &bundle, format)
        .unwrap()
}

fn label(page: &AnnotatedDocument, key: u32) -> String {
    let mark = &page.marks[(key - 1) as usize];
    let mut label = String::new();
    for part in &page.selection_parts
        [mark.selection_first as usize..(mark.selection_first + mark.selection_count) as usize]
    {
        let run = &page.runs[(part.run - 1) as usize];
        let start = usize::try_from(run.byte_start + part.start_byte).unwrap();
        let end = usize::try_from(run.byte_start + part.end_byte).unwrap();
        label.push_str(&page.text[start..end]);
    }
    label
}

fn clickable_links(page: &AnnotatedDocument) -> Vec<u32> {
    page.marks
        .iter()
        .filter(|mark| mark.kind == 3 && mark.link_target.is_some())
        .map(|mark| mark.key)
        .collect()
}

#[test]
fn authored_include_keeps_its_own_source_key_on_styled_reference() {
    // The exact two files ran on pinned CVS -Tutf8 -O width=78 first.
    // read.c expands the authorized .so input; man_term.c::pre_alternate()
    // emits the joined BR operands from that included source.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "target/r05-link-source-root.1",
            b".TH T 1\n.SH SEE ALSO\n.so target/r05-link-source-inc.roff\n".to_vec(),
        )
        .unwrap();
    bundle
        .insert(
            "target/r05-link-source-inc.roff",
            b".BR printf (3)\n".to_vec(),
        )
        .unwrap();
    let page = AnnotatedRenderer::default()
        .render_bundle("target/r05-link-source-root.1", &bundle, InputFormat::Man)
        .unwrap();
    let keys = clickable_links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(label(&page, keys[0]), "printf(3)");
    let mark = &page.marks[(keys[0] - 1) as usize];
    assert_eq!(page.sources.len(), 2);
    assert_eq!(mark.source, 2);
    assert_eq!(mark.line, 1);
}

#[test]
fn heading_reference_uses_visible_head_glyphs_without_copying_body() {
    // Exact input ran pinned CVS -Tutf8 -O width=78. man_term.c::pre_SH()
    // selects bold heading font, and term.c::term_word() emits the marker's
    // visible <> separately from the preceding manual label.
    let page = render(b".TH T 1\n.SH \"a(1) \\%<>\"\nbody\n", InputFormat::Man);
    let keys = clickable_links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(label(&page, keys[0]), "a(1)");
    assert!(page.text.contains("<>"));
    assert!(page.text.contains("body"));
}

#[test]
fn manual_target_and_link_keep_independent_native_identities() {
    // Exact input ran pinned CVS -Tutf8 -O width=78. tag.c retains the
    // authored .Tg target; mdoc_term.c::termp_sh_pre() and termp_skip_pre()
    // do not turn that zero-width target into link label glyphs.
    let page = render(
        b".Dd September 27, 2026\n.Dt T 1\n.Os\n.Sh SEE ALSO\n.Tg target\n.Xr printf 3\n",
        InputFormat::Mdoc,
    );
    let keys = clickable_links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(label(&page, keys[0]), "printf(3)");
    let anchor = page
        .marks
        .iter()
        .find(|mark| mark.kind == 4 && mark.name.as_deref() == Some("target"))
        .unwrap();
    assert_eq!(anchor.line, 5);
    assert!(anchor.point.is_some());
}

#[test]
fn margin_character_does_not_extend_the_link_label() {
    // Exact input ran pinned CVS -Tutf8 -O width=78. term.c::endline()
    // emits .mc as a separate TERM_COLLECT_MARGIN output after the BR word.
    let page = render(
        b".TH T 1\n.SH SEE ALSO\n.mc |\n.BR printf (3)\n.br\n.mc\n",
        InputFormat::Man,
    );
    let keys = clickable_links(&page);
    assert_eq!(keys.len(), 1);
    assert_eq!(label(&page, keys[0]), "printf(3)");
    assert!(page.text.contains('|'));
}

#[test]
fn explicit_manual_section_zero_is_not_rejected_as_an_inferred_candidate() {
    // Both exact inputs ran pinned CVS -Tutf8 -O width=78. man_term.c::
    // pre_MR() and mdoc_term.c::termp_xr_pre() emit the authored section;
    // the conservative section-0 rule applies only to inferred references.
    for (source, format) in [
        (
            b".TH T 1\n.SH SEE ALSO\n.MR printf 0\n".as_slice(),
            InputFormat::Man,
        ),
        (
            b".Dd September 27, 2026\n.Dt T 1\n.Os\n.Sh SEE ALSO\n.Xr printf 0\n".as_slice(),
            InputFormat::Mdoc,
        ),
    ] {
        let page = render(source, format);
        let keys = clickable_links(&page);
        assert_eq!(keys.len(), 1);
        assert_eq!(label(&page, keys[0]), "printf(0)");
        let target = page.marks[(keys[0] - 1) as usize]
            .link_target
            .as_ref()
            .unwrap();
        assert_eq!(target.primary, "printf");
        assert_eq!(target.secondary.as_deref(), Some("0"));
    }
}
