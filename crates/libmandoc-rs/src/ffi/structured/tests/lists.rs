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
    assert_eq!(document.forms.len(), 5, "{document:#?}");
    assert_eq!(document.items[0].form_count, 2);
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
fn man_ip_ordinals_are_retained_as_native_markers() {
    // Reference output for this source displays 3., 4., and 9. in order.
    // Pinned `man_term.c::pre_IP` passes the marker through the native field;
    // the collector records it as term content instead of recreating spacing.
    let mut bundle = SourceBundle::new();
    bundle
        .insert(
            "ordered.1",
            b".TH ORDERED 1\n.SH STEPS\n.IP 3.\nThird.\n.IP 4.\nFourth.\n.IP 9.\nNinth.\n".to_vec(),
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
    assert_eq!(document.lists.len(), 3);
    assert!(
        document
            .lists
            .iter()
            .all(|list| list.kind == LIST_NATIVE_MARKER)
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
    assert_eq!(markers, ["3.", "4.", "9."]);
}
