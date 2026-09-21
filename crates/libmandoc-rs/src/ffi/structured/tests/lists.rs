//! Native list/item ownership and term/body evidence.

use super::*;

fn root_text(document: &OwnedStructuredDocument, root: u32) -> String {
    document
        .content_atoms
        .iter()
        .filter(|atom| atom.root == root)
        .map(|atom| atom.text.as_str())
        .collect()
}

fn ref_text(document: &OwnedStructuredDocument, first: u32, count: u32) -> String {
    document.content_refs[first as usize - 1..first as usize - 1 + count as usize]
        .iter()
        .map(|reference| {
            let atom = &document.content_atoms[reference.atom as usize - 1];
            &atom.text[reference.bytes.start as usize..reference.bytes.end as usize]
        })
        .collect()
}

#[test]
fn man_tp_tq_forms_share_one_owner_without_borrowing_an_empty_body() {
    // Oracle run first with UTF-8 output at width 78. Pinned
    // `man_macro.c::blk_imp` attaches TP/TQ heads and bodies, while
    // `man_term.c::pre_TP` renders each head through the native formatter.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "lists.1",
            br#".TH C03 1
.SH OPTIONS
.TP
.BR --output , " -o=" FILE
.TQ
.B -O
Write file.
.TP
.B --empty
.TP
.B --same
Body A.
.TP
.B --same
Body B.
"#
            .to_vec(),
        )
        .unwrap();
    let document = render_prelude("lists.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("man TP/TQ structure is collected");

    assert_eq!(document.lists.len(), 4, "{document:#?}");
    assert_eq!(document.items.len(), 4, "{document:#?}");
    assert_eq!(document.forms.len(), 6, "{document:#?}");
    assert_eq!(document.items[0].form_count, 3);
    assert_eq!(document.items[1].form_count, 1);
    assert_eq!(document.items[1].owner + 1, document.items[2].owner);
    assert_eq!(document.items[2].owner + 1, document.items[3].owner);

    let first_terms = document
        .content_roots
        .iter()
        .filter(|root| root.owner == document.items[0].owner && root.kind == ROOT_TERM)
        .map(|root| root_text(&document, root.key))
        .collect::<Vec<_>>();
    assert_eq!(first_terms, ["--output, -o=FILE", "-O"]);
    assert_eq!(
        document.forms[..3]
            .iter()
            .map(|form| ref_text(&document, form.first_ref, form.ref_count))
            .collect::<Vec<_>>(),
        ["--output", "-o=FILE", "-O"]
    );
    let empty_bodies = document
        .content_roots
        .iter()
        .filter(|root| root.owner == document.items[1].owner && root.kind == ROOT_BODY)
        .count();
    assert_eq!(empty_bodies, 0, "an empty TP must not borrow the next body");
}

#[test]
fn mdoc_lists_preserve_kind_nesting_ordinals_and_item_targets() {
    // Oracle run first with this exact source. Pinned
    // `mdoc_term.c::termp_bl_pre/termp_it_pre` owns list traversal and
    // `tag.c::tag_move_id` transfers Tg identity to the target item part.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "lists.1",
            br".Dd September 21, 2026
.Dt C03 1
.Os
.Sh OPTIONS
.Bl -tag -compact
.It Fl o Ar file
Write file.
.Tg item-target
.It Fl q
.Bl -enum
.It
Nested one.
.It
Nested two.
.El
.El
"
            .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "lists.1",
        &bundle,
        InputFormat::Mdoc,
        78,
        &Limits::default(),
    )
    .expect("mdoc list structure is collected");

    assert_eq!(document.lists.len(), 2, "{document:#?}");
    assert_eq!(document.lists[0].kind, LIST_DEFINITION);
    assert!(document.lists[0].compact);
    assert_eq!(document.lists[1].kind, LIST_ORDERED);
    assert_eq!(document.lists[1].start, Some(1));
    assert_eq!(document.items.len(), 4);
    assert_eq!(document.items[0].ordinal, 0);
    assert_eq!(document.items[1].ordinal, 1);
    assert_eq!(document.items[2].ordinal, 0);
    assert_eq!(document.items[3].ordinal, 1);
    assert_eq!(document.items[1].target.as_deref(), Some("item-target"));

    let outer_block = document.lists[0].block;
    let nested_block = document.lists[1].block;
    assert_eq!(
        document.blocks[nested_block as usize - 1].parent,
        Some(outer_block)
    );
    assert_eq!(
        document.blocks[nested_block as usize - 1].owner,
        document.items[1].owner
    );
}

#[test]
fn mdoc_empty_and_duplicate_terms_keep_independent_owners() {
    // The exact input was checked with the pinned UTF-8/78 reference first.
    // `mdoc_term.c::termp_it_pre` visits each It owner independently even
    // when a tag is repeated or the preceding body is empty.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "owners.1",
            br".Dd September 21, 2026
.Dt OWNERS 1
.Os
.Sh OPTIONS
.Bl -tag
.It Fl empty
.It Fl same
Body A.
.It Fl same
Body B.
.El
"
            .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "owners.1",
        &bundle,
        InputFormat::Mdoc,
        78,
        &Limits::default(),
    )
    .expect("mdoc empty and duplicate items are collected");
    assert_eq!(document.items.len(), 3);
    assert_eq!(document.items[0].owner + 1, document.items[1].owner);
    assert_eq!(document.items[1].owner + 1, document.items[2].owner);
    assert_eq!(
        document
            .content_roots
            .iter()
            .filter(|root| root.owner == document.items[0].owner && root.kind == ROOT_BODY)
            .count(),
        0
    );
    let duplicate_terms = document.items[1..]
        .iter()
        .map(|item| {
            document
                .content_roots
                .iter()
                .find(|root| root.owner == item.owner && root.kind == ROOT_TERM)
                .map(|root| root_text(&document, root.key))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(duplicate_terms, ["-same", "-same"]);
}

#[test]
fn man_ip_markers_retain_source_classification_and_ordered_boundaries() {
    // Reference output for this source displays 3., 4., and 9. in order.
    // Pinned `man_term.c::pre_IP` passes the marker through the native field;
    // the collector records it as term content instead of recreating spacing.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "ordered.1",
            b".TH ORDERED 1\n.SH STEPS\n.IP \\(bu\nBullet.\n.IP \"*\"\nStar.\n.IP 3.\nThird.\n.IP 4.\nFourth.\n.IP 9.\nNinth.\n.IP \"(1)\"\nParenthesized.\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "ordered.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("man IP markers are collected");
    assert_eq!(
        document
            .lists
            .iter()
            .map(|list| (list.kind, list.start))
            .collect::<Vec<_>>(),
        [
            (LIST_BULLET, None),
            (LIST_DEFINITION, None),
            (LIST_ORDERED, Some(3)),
            (LIST_ORDERED, Some(9)),
            (LIST_ORDERED, Some(1)),
        ]
    );
    let markers = document
        .items
        .iter()
        .map(|item| {
            document
                .content_roots
                .iter()
                .find(|root| root.owner == item.owner && root.kind == ROOT_TERM)
                .map(|root| root_text(&document, root.key))
                .unwrap()
        })
        .collect::<Vec<_>>();
    assert_eq!(markers, ["•", "*", "3.", "4.", "9.", "(1)"]);
}

#[test]
fn man_tp_markers_use_the_same_source_classification_as_ip() {
    // This exact source was run through the pinned reference first.
    // `man_term.c::pre_TP` executes the next-line head through the same
    // visible marker contract as IP.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "tp-markers.1",
            b".TH X 1\n.SH D\n.TP\n\\(bu\nBULLET\n.TP\n2.\nSECOND\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "tp-markers.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("TP marker source is classified natively");
    assert_eq!(
        document
            .lists
            .iter()
            .map(|list| (list.kind, list.start))
            .collect::<Vec<_>>(),
        [(LIST_BULLET, None), (LIST_ORDERED, Some(2))]
    );
}

#[test]
fn tq_continuation_expires_after_body_or_paragraph_boundary() {
    // Both exact inputs were run through the pinned reference first.
    // `man_macro.c::blk_imp` and the local continued-head patch retain only
    // an immediately pending empty TP/TQ head.
    for (body, separator) in [("FIRST BODY\n", ""), ("", ".PP\nBOUNDARY\n")] {
        let mut bundle = SourceBundle::new();
        bundle
            .insert(
                "tq-state.1",
                format!(
                    ".TH STATE 1\n.SH D\n.TP\nFIRST\n{body}{separator}.TQ\nSECOND\nSECOND BODY\n"
                )
                .into_bytes(),
            )
            .unwrap();
        let document = render_prelude(
            "tq-state.1",
            &bundle,
            InputFormat::Man,
            78,
            &Limits::default(),
        )
        .expect("non-contiguous TQ remains an independent owner");
        assert_eq!(document.items.len(), 2, "{document:#?}");
        assert_ne!(document.items[0].owner, document.items[1].owner);
        assert_eq!(document.items[0].form_count, 1);
        assert_eq!(document.items[1].form_count, 1);
    }
}

#[test]
fn tq_continuation_expires_across_relative_indent_scope() {
    // The exact input was run through the pinned reference first. Pinned
    // `man_term.c::pre_RS/post_RS` pushes and restores a distinct scope, so
    // a TQ outside it cannot continue an empty TP inside it.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "tq-rs.1",
            b".TH X 1\n.SH D\n.RS\n.TP\nINNER\n.RE\n.TQ\nOUTER\nBODY\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude("tq-rs.1", &bundle, InputFormat::Man, 78, &Limits::default())
        .expect("TQ outside RS owns a separate item");
    assert_eq!(document.items.len(), 2, "{document:#?}");
    assert_ne!(document.items[0].owner, document.items[1].owner);
}

#[test]
fn literal_separator_terms_retain_their_form() {
    // The exact TP/IP input was run through the pinned reference first; each
    // literal operator is a visible definition label, not an empty separator.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "operators.1",
            b".TH X 1\n.SH D\n.TP\n|\nBODY\n.IP \"|\"\nIPBODY\n".to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "operators.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("literal separators remain native forms");
    assert_eq!(document.forms.len(), 2, "{document:#?}");
    assert!(
        document
            .forms
            .iter()
            .all(|form| { ref_text(&document, form.first_ref, form.ref_count) == "|" })
    );
}

#[test]
fn consecutive_tq_heads_remain_one_definition_item() {
    // This exact source was run through the pinned reference first. The local
    // continued-head patch follows `man_macro.c::blk_imp`: each empty TQ body
    // keeps the next immediately adjacent TQ eligible for the same item.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "tq-chain.1",
            b".TH C03 1\n.SH OPTIONS\n.TP\n.B --one\n.TQ\n.B --two\n.TQ\n.B --three\nBody.\n"
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "tq-chain.1",
        &bundle,
        InputFormat::Man,
        78,
        &Limits::default(),
    )
    .expect("adjacent TQ heads remain a single native definition item");
    assert_eq!(document.items.len(), 1, "{document:#?}");
    assert_eq!(document.items[0].form_count, 3);
    assert_eq!(
        document
            .forms
            .iter()
            .map(|form| ref_text(&document, form.first_ref, form.ref_count))
            .collect::<Vec<_>>(),
        ["--one", "--two", "--three"]
    );
}

#[test]
fn native_check_rejects_a_form_crossing_tq_term_roots() {
    // The exact TP/TQ source was run through the pinned reference first.
    // Each head is a distinct term root even though both belong to one item.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "cross-root.1",
            b".TH X 1\n.SH D\n.TP\nONE\n.TQ\nTWO\nBODY\n".to_vec(),
        )
        .unwrap();
    let limits = Limits::default();
    let storage = InputStorage::new("cross-root.1", &bundle, InputFormat::Man, &limits).unwrap();
    let (status, pointer, failure) = raw_render(&storage.view(78, PROFILE_UTF8), &limits);
    assert_eq!(status, STATUS_OK, "{failure:?}");
    let handle = ResultHandle(NonNull::new(pointer).unwrap());
    let mut view = ResultView::default();
    assert_eq!(
        unsafe { mant_structured_result_view(handle.0.as_ptr(), &raw mut view) },
        STATUS_OK
    );
    let forms = unsafe {
        std::slice::from_raw_parts_mut(
            view.forms.ptr.cast::<FormView>().cast_mut(),
            view.forms.count as usize,
        )
    };
    assert_eq!(forms.len(), 2);
    forms[0].ref_count = 2;
    let mut failure = FailureView::default();
    assert_eq!(
        unsafe { mant_structured_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_RELATION
    );
}

#[test]
fn empty_mdoc_item_receives_preceding_tg_target() {
    // The pinned `mdoc_validate.c::post_tg` keeps Tg on its zero-width node
    // when the following bullet item has no body child; `tag.c` does not move
    // that target into nonexistent content.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "target.1",
            b".Dd September 21, 2026\n.Dt TARGET 1\n.Os\n.Sh D\n.Bl -bullet\n.Tg empty-target\n.It\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "target.1",
        &bundle,
        InputFormat::Mdoc,
        78,
        &Limits::default(),
    )
    .expect("empty native item retains its pending target");
    assert_eq!(document.items[0].target.as_deref(), Some("empty-target"));
}

#[test]
fn mdoc_multiple_labels_produce_independent_forms_and_hints() {
    // The exact source was checked against the pinned reference. Pinned
    // `mdoc_term.c::termp_it_pre` executes both Fl nodes around punctuation;
    // the collector retains two source-marked declaration occurrences.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "labels.1",
            b".Dd September 21, 2026\n.Dt LABELS 1\n.Os\n.Sh D\n.Bl -tag\n.It Fl a , Fl b\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "labels.1",
        &bundle,
        InputFormat::Mdoc,
        78,
        &Limits::default(),
    )
    .expect("mdoc labels retain forms and native hints");
    assert_eq!(document.items[0].form_count, 2);
    assert_eq!(document.name_hints.len(), 2);
    assert_eq!(
        document
            .forms
            .iter()
            .map(|form| ref_text(&document, form.first_ref, form.ref_count))
            .collect::<Vec<_>>(),
        ["-a", "-b"]
    );
}

#[test]
fn one_mdoc_form_retains_each_native_name_hint() {
    // This exact source was run through the pinned reference first.
    // `mdoc_term.c::termp_it_pre` executes both Fl nodes even though an Ar
    // occurrence keeps the complete head in one authored form.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "hint-runs.1",
            b".Dd September 21, 2026\n.Dt X 1\n.Os\n.Sh D\n.Bl -tag\n.It Fl a Ar file Fl b\nBODY\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let document = render_prelude(
        "hint-runs.1",
        &bundle,
        InputFormat::Mdoc,
        78,
        &Limits::default(),
    )
    .expect("one form retains multiple native hint runs");
    assert_eq!(document.forms.len(), 1, "{document:#?}");
    assert_eq!(document.name_hints.len(), 2, "{document:#?}");
    assert_eq!(
        document
            .name_hints
            .iter()
            .map(|hint| ref_text(&document, hint.first_ref, hint.ref_count))
            .collect::<Vec<_>>(),
        ["-a", "-b"]
    );
}

#[test]
fn mdoc_column_lists_fail_closed_until_table_structure_lands() {
    // The exact input was run through the pinned reference first. Pinned
    // `mdoc_term.c::termp_it_pre` has a distinct LIST_column field path, so
    // C03 must not flatten it through ordinary list ownership.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "column.1",
            b".Dd September 21, 2026\n.Dt COLUMN 1\n.Os\n.Sh D\n.Bl -column one\n.It A\n.El\n"
                .to_vec(),
        )
        .unwrap();
    let error = render_prelude(
        "column.1",
        &bundle,
        InputFormat::Mdoc,
        78,
        &Limits::default(),
    )
    .expect_err("column layout belongs to the later table slice");
    assert_eq!(error.status, STATUS_UNSUPPORTED);
}
