//! Native term completion across formatter-delayed and zero-width heads.

use super::*;

fn root_text(document: &OwnedStructuredDocument, root: u32) -> String {
    document
        .content_atoms
        .iter()
        .filter(|atom| atom.root == root)
        .map(|atom| atom.text.as_str())
        .collect()
}

#[test]
fn mdoc_inset_and_diag_terms_finish_after_their_delayed_flush() {
    // Both exact inputs were run through the pinned UTF-8/78 reference first.
    // `mdoc_term.c::termp_it_post` flushes inset and diag lists from BODY,
    // after the formatter has already left HEAD.
    for (list_type, expected_term) in [("inset", "-a"), ("diag", "Fl a")] {
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "delayed.1",
                format!(
                    ".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh OPTIONS\n.Bl -{list_type}\n.It Fl a\nBODY\n.El\n"
                )
                .into_bytes(),
            )
            .unwrap();
        let document = render_prelude(
            "delayed.1",
            &bundle,
            InputFormat::Mdoc,
            78,
            &Limits::default(),
        )
        .expect("formatter-delayed term is finalized after rendering");

        assert_eq!(document.items.len(), 1, "{list_type}: {document:#?}");
        assert_eq!(
            document.items[0].form_count, 1,
            "{list_type}: {document:#?}"
        );
        let term = document
            .content_roots
            .iter()
            .find(|root| root.owner == document.items[0].owner && root.kind == ROOT_TERM)
            .expect("term root");
        assert_eq!(root_text(&document, term.key), expected_term);
    }
}

#[test]
fn zero_width_man_and_mdoc_terms_are_completed_without_fake_forms() {
    // Both exact inputs were run through the pinned UTF-8/78 reference first.
    // The zero-width escape reaches the terminal as ASCII_NBRZW and produces
    // no visible atom; that is a completed empty tag, not a damaged result.
    let cases = [
        (
            InputFormat::Man,
            b".TH X 1\n.SH OPTIONS\n.TP\n\\&\nBODY\n".as_slice(),
        ),
        (
            InputFormat::Mdoc,
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh OPTIONS\n.Bl -tag\n.It \\&\nBODY\n.El\n"
                .as_slice(),
        ),
    ];
    for (format, source) in cases {
        let mut bundle = SourceBundle::new();
        bundle.insert("empty.1", source.to_vec()).unwrap();
        let document = render_prelude("empty.1", &bundle, format, 78, &Limits::default())
            .expect("a zero-width tag is a valid completed term");

        assert_eq!(document.items.len(), 1, "{document:#?}");
        assert_eq!(document.items[0].form_count, 0, "{document:#?}");
        let term = document
            .content_roots
            .iter()
            .find(|root| root.owner == document.items[0].owner && root.kind == ROOT_TERM)
            .expect("empty term root remains explicit");
        assert_eq!(root_text(&document, term.key), "");
        assert!(document.content_atoms.iter().any(|atom| {
            document.content_roots[atom.root as usize - 1].owner == document.items[0].owner
                && atom.text.contains("BODY")
        }));
    }
}

#[test]
fn an_empty_tq_head_keeps_the_visible_form_and_its_own_completed_root() {
    // This exact input was run through the pinned UTF-8/78 reference first.
    // `man_term.c::pre_TP` visits both TP/TQ heads, while the zero-width TQ
    // contributes no visible label before the shared body is rendered.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "empty-tq.1",
            b".TH X 1\n.SH OPTIONS\n.TP\n.B --one\n.TQ\n\\&\nBODY\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "empty-tq.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("the completed empty TQ head is valid");

    assert_eq!(document.items.len(), 1, "{document:#?}");
    assert_eq!(document.items[0].form_count, 1, "{document:#?}");
    let terms = document
        .content_roots
        .iter()
        .filter(|root| root.owner == document.items[0].owner && root.kind == ROOT_TERM)
        .map(|root| root_text(&document, root.key))
        .collect::<Vec<_>>();
    assert_eq!(terms, ["--one", ""]);
}

#[test]
fn narrow_inset_terms_finish_after_wrapped_buffer_consumption() {
    // This exact input was run through the pinned UTF-8 reference at width
    // 20 first. Pinned `term.c::term_flushln` may consume a prefix while the
    // inset head remains buffered; finalization waits for the whole render.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "narrow.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh OPTIONS\n.Bl -inset\n.It a-very-long-inset-label\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "narrow.1",
        &bundle,
        InputFormat::Mdoc,
        20,
        &Limits::default(),
    )
    .expect("wrapped inset term completes after buffer consumption");

    assert_eq!(document.items[0].form_count, 1, "{document:#?}");
    let term = document
        .content_roots
        .iter()
        .find(|root| root.owner == document.items[0].owner && root.kind == ROOT_TERM)
        .expect("term root");
    assert_eq!(root_text(&document, term.key), "a-very-long-inset-label");
}
