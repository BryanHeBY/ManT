//! Conservative source classification for visible man list markers.

use super::*;

fn root_text(document: &OwnedStructuredDocument, root: u32) -> String {
    document
        .content_atoms
        .iter()
        .filter(|atom| atom.root == root)
        .map(|atom| atom.text.as_str())
        .collect()
}

fn item_term(document: &OwnedStructuredDocument, item: usize) -> String {
    let owner = document.items[item].owner;
    let root = document
        .content_roots
        .iter()
        .find(|root| root.owner == owner && root.kind == ROOT_TERM)
        .expect("item term root");
    root_text(document, root.key)
}

#[test]
fn marker_prefixes_with_visible_payload_remain_definition_terms() {
    // This exact source was run through the pinned UTF-8/78 reference first.
    // `man_term.c::pre_TP` executes the whole next-line head, and
    // `pre_alternate` joins every BR argument without spacing. The visible
    // labels are therefore `1.LABEL` and `•LABEL`, not pure markers.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "marker-payload.1",
            b".TH X 1\n.SH OPTIONS\n.TP\n.BR 1. LABEL\nBODY\n.TP\n.BR \\(bu LABEL\nSECOND\n"
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "marker-payload.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("mixed marker-like heads remain complete definitions");

    assert_eq!(
        document
            .lists
            .iter()
            .map(|list| list.kind)
            .collect::<Vec<_>>(),
        [LIST_DEFINITION, LIST_DEFINITION]
    );
    assert_eq!(item_term(&document, 0), "1.LABEL");
    assert_eq!(item_term(&document, 1), "•LABEL");
    assert_eq!(document.items[0].form_count, 1);
    assert_eq!(document.items[1].form_count, 1);
}

#[test]
fn alternate_font_arguments_can_prove_one_complete_ordinal_marker() {
    // This exact input was run through the pinned UTF-8/78 reference first.
    // `man_term.c::pre_alternate` renders adjacent BR arguments with
    // TERMP_NOSPACE, so `1` plus `.` is exactly the visible marker `1.`.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "split-marker.1",
            b".TH X 1\n.SH OPTIONS\n.TP\n.BR 1 .\nBODY\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "split-marker.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("the whole alternate-font head proves an ordinal marker");

    assert_eq!(document.lists.len(), 1, "{document:#?}");
    assert_eq!(document.lists[0].kind, LIST_ORDERED);
    assert_eq!(document.lists[0].start, Some(1));
    assert_eq!(item_term(&document, 0), "1.");
}

#[test]
fn zero_width_font_decorations_do_not_impose_a_raw_marker_limit() {
    // This exact input was run through the pinned UTF-8/78 reference first.
    // The repeated `\fB`/`\fR` escapes only change terminal font state, so
    // the complete visible TP head remains the single bullet marker.
    let decorations = "\\fB\\fR".repeat(21);
    let source = format!(".TH X 1\n.SH OPTIONS\n.TP\n{decorations}\\(bu\\fR\nBODY\n");
    let mut bundle = SourceBundle::new();
    bundle
        .insert("decorated-marker.1", source.into_bytes())
        .unwrap();
    let document = render_prelude(
        "decorated-marker.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("zero-width decoration is consumed without a raw byte cap");

    assert_eq!(document.lists.len(), 1, "{document:#?}");
    assert_eq!(document.lists[0].kind, LIST_BULLET);
    assert_eq!(item_term(&document, 0), "•");
}
